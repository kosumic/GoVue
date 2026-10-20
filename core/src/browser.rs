//! Navigation stacks keep the watch document alive while another page is shown.
use crate::{Result, string};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use url::Url;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tab {
    id: u64,
    url: String,
    kind: String,
    generation: u64,
    transient: String,
}
struct Browser {
    tabs: Vec<Tab>,
    active: u64,
    next: u64,
    stack: Vec<u64>,
    suspended_watch: Option<u64>,
    revision: u64,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            tabs: vec![Tab {
                id: 1,
                url: "https://m.youtube.com/".into(),
                kind: "home".into(),
                generation: 0,
                transient: "none".into(),
            }],
            active: 1,
            next: 2,
            stack: vec![1],
            suspended_watch: None,
            revision: 0,
        }
    }
}
static BROWSERS: OnceLock<Mutex<HashMap<String, Browser>>> = OnceLock::new();
fn address(raw: &str) -> Result<(String, String)> {
    let mut url = Url::parse(raw).map_err(|_| "Invalid page address.")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || raw.len() > 8192
        || raw.chars().any(char::is_control)
    {
        return Err("Unsupported page address.".into());
    }
    if url.host_str() == Some("youtu.be") {
        let id = url.path().trim_matches('/');
        if !crate::video_id(id) {
            return Err("Invalid shared video link.".into());
        }
        let mut watch =
            Url::parse("https://m.youtube.com/watch").map_err(|_| "Invalid watch page address.")?;
        watch.query_pairs_mut().append_pair("v", id);
        for (key, value) in url.query_pairs() {
            if matches!(key.as_ref(), "t" | "start" | "list" | "index") && value.len() <= 200 {
                watch.query_pairs_mut().append_pair(&key, &value);
            }
        }
        url = watch;
    }
    let host = url.host_str().ok_or("Missing page host.")?;
    let kind = if host == "youtube.com" || host.ends_with(".youtube.com") {
        let path = url.path().trim_matches('/');
        let mut segments = path.split('/');
        match segments.next().unwrap_or("").to_ascii_lowercase().as_str() {
            "" => "home".into(),
            "watch" | "live" | "embed" => "watch".into(),
            "shorts" => "shorts".into(),
            "results" => "searching".into(),
            "channel" => "channel".into(),
            "gaming" => "gaming".into(),
            "select_site" => "select_site".into(),
            first if first.starts_with('@') => "@".into(),
            "feed" => match segments.next().unwrap_or("").to_ascii_lowercase().as_str() {
                kind @ ("subscriptions" | "library" | "history" | "channels" | "playlists") => {
                    kind.into()
                }
                _ => path.into(),
            },
            _ => path.into(),
        }
    } else {
        "external".into()
    };
    Ok((url.to_string(), kind))
}

pub(super) fn url_policy(raw: &str) -> &'static str {
    if address(raw).is_ok() {
        return "internal";
    }
    if raw.len() <= 8192
        && !raw.chars().any(char::is_control)
        && Url::parse(raw).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
    {
        "external"
    } else {
        "blocked"
    }
}

fn nav(kind: &str) -> bool {
    matches!(kind, "home" | "shorts" | "subscriptions" | "library")
}

fn video(url: &Url) -> Option<String> {
    let id = if url.path() == "/watch" {
        url.query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
    } else {
        let mut segments = url.path().trim_matches('/').split('/');
        match segments.next() {
            Some("shorts" | "live" | "embed") => segments.next().map(str::to_owned),
            _ => None,
        }
    };
    id.filter(|id| crate::video_id(id))
}

fn page_context(command: &Value) -> Option<(Url, Url, Url)> {
    let parse = |key| {
        command[key]
            .as_str()
            .and_then(|v| address(v).ok())
            .and_then(|(v, _)| Url::parse(&v).ok())
    };
    let (Some(origin), Some(document), Some(known)) =
        (parse("origin"), parse("documentUrl"), parse("knownUrl"))
    else {
        return None;
    };
    let youtube = |url: &Url| {
        matches!(
            url.host_str(),
            Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com")
        )
    };
    if !youtube(&origin)
        || !youtube(&document)
        || origin.origin() != document.origin()
        || known.origin() != document.origin()
    {
        return None;
    }
    Some((origin, document, known))
}

fn page_origin(command: &Value) -> bool {
    let Some((origin, document, known)) = page_context(command) else {
        return false;
    };
    let bare = origin.path() == "/" && origin.query().is_none() && origin.fragment().is_none();
    video(&document) == video(if bare { &known } else { &origin })
}

/// Only the current visible document may prepare the selected watch link.
pub(super) fn preparation_video(command: &Value, root: &str) -> Option<String> {
    let browsers = BROWSERS.get()?.lock().ok()?;
    let browser = browsers.get(root)?;
    let tab = browser
        .tabs
        .iter()
        .find(|tab| Some(tab.id) == command["id"].as_u64() && tab.id == browser.active)
        .cloned();
    drop(browsers);
    let Some(tab) = tab else {
        let _span = crate::trace::Section::begin(c"GoVue extraction: inactive preparation tab");
        return None;
    };
    if tab.generation == 0 || command["generation"].as_u64() != Some(tab.generation) {
        let _span = crate::trace::Section::begin(c"GoVue extraction: stale preparation document");
        return None;
    }
    // The selected link can commit its SPA URL before the click message is
    // delivered. Bind preparation to the document and origin, then correlate
    // the selected video separately when playback claims the job.
    if page_context(
            &json!({"origin":command["origin"],"documentUrl":command["documentUrl"],"knownUrl":tab.url}),
        ).is_none()
    {
        let _span = crate::trace::Section::begin(c"GoVue extraction: invalid preparation origin");
        return None;
    }
    let (target, kind) = address(command["url"].as_str()?).ok()?;
    let target = Url::parse(&target).ok()?;
    if kind != "watch"
        || !matches!(
            target.host_str(),
            Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com")
        )
    {
        return None;
    }
    video(&target)
}

fn shared_url(text: &str) -> Result<Value> {
    if text.len() > 100_000 {
        return Err("Shared text is too large.".into());
    }
    for word in text.split_whitespace() {
        let candidate = word.trim_matches(|c: char| {
            matches!(
                c,
                '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | '.' | ';' | '!' | '?'
            )
        });
        if let Ok((url, kind)) = address(candidate)
            && kind != "external"
        {
            return Ok(json!(url));
        }
    }
    Err("No supported YouTube link was found.".into())
}
fn open_address(browser: &Browser, command: &Value) -> Result<(String, String)> {
    let Some(value) = command.get("videoId") else {
        return address(string(command, "url")?);
    };
    let id = value
        .as_str()
        .filter(|id| crate::video_id(id))
        .ok_or("Invalid video identity.")?;
    if let Some(start) = command.get("start") {
        let start = start
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=31_536_000.0).contains(n))
            .ok_or("Invalid video timestamp.")?;
        return address(&format!(
            "https://m.youtube.com/watch?v={id}&t={}s",
            start.floor()
        ));
    }
    let retained = browser.tabs.iter().find(|tab| {
        address(&tab.url).is_ok_and(|(_, kind)| matches!(kind.as_str(), "watch" | "shorts"))
            && Url::parse(&tab.url)
                .ok()
                .and_then(|url| video(&url))
                .as_deref()
                == Some(id)
    });
    address(
        &retained
            .map(|tab| tab.url.clone())
            .unwrap_or_else(|| format!("https://m.youtube.com/watch?v={id}")),
    )
}
pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    if command["op"] == "browser_shared_url" {
        return shared_url(string(command, "text")?);
    }
    if command["op"] == "browser_message_origin" {
        return Ok(json!(page_origin(command)));
    }
    if command["op"] == "browser_user_agent" {
        let agent = string(command, "agent")?;
        if agent.len() > 1000 || agent.chars().any(char::is_control) {
            return Err("Invalid browser identity.".into());
        }
        let mut agent = agent
            .replace("; wv", "")
            .split_whitespace()
            .filter(|part| !part.starts_with("Version/"))
            .collect::<Vec<_>>()
            .join(" ");
        if agent.contains("Chrome/") && !agent.contains("Mobile") {
            agent = agent.replace(" Safari/", " Mobile Safari/");
        }
        return Ok(json!(agent));
    }
    let mut browsers = BROWSERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let browser = browsers.entry(root.into()).or_default();
    let previous = (
        browser.active,
        browser
            .tabs
            .iter()
            .find(|tab| tab.id == browser.active)
            .map(|tab| tab.url.clone()),
    );
    let mut target = Value::Null;
    let op = string(command, "op")?;
    match op {
        "browser_state" => {}
        "browser_open" => {
            let (url, kind) = open_address(browser, command)?;
            if command["suspendWatch"] == true
                && kind != "watch"
                && browser
                    .tabs
                    .iter()
                    .any(|t| t.id == browser.active && t.kind == "watch")
            {
                browser.suspended_watch = Some(browser.active);
                browser.stack.retain(|id| *id != browser.active);
            }
            let reusable = kind == "watch" || nav(&kind);
            let existing = reusable
                .then(|| browser.tabs.iter().find(|t| t.kind == kind).map(|t| t.id))
                .flatten();
            // Bottom navigation discards intermediate pages; its home and the
            // suspended watch renderer remain mounted, as in TabController.
            if nav(&kind) {
                browser.tabs.retain(|t| {
                    t.kind == "home"
                        || Some(t.id) == existing
                        || Some(t.id) == browser.suspended_watch
                });
                browser.stack.retain(|id| {
                    browser.tabs.iter().any(|t| t.id == *id) && Some(*id) != browser.suspended_watch
                });
            }
            let mut load = false;
            let id = if let Some(tab) = browser.tabs.iter_mut().find(|t| Some(t.id) == existing) {
                load = tab.url != url;
                if load {
                    tab.transient = "none".into();
                    tab.generation = tab.generation.saturating_add(1);
                }
                tab.url.clone_from(&url);
                tab.id
            } else {
                if browser.tabs.len() >= 64 {
                    return Err("Too many open pages. Go back before opening another page.".into());
                }
                let id = browser.next;
                browser.next += 1;
                browser.tabs.push(Tab {
                    id,
                    url: url.clone(),
                    kind: kind.clone(),
                    generation: 0,
                    transient: "none".into(),
                });
                id
            };
            browser.stack.retain(|v| *v != id);
            browser.stack.push(id);
            if kind == "watch" {
                browser.suspended_watch = None;
            }
            browser.active = id;
            target = json!({"id":id,"url":url,"load":load});
        }
        "browser_invalidate" => {
            let id = command["id"].as_u64().ok_or("Missing browser tab.")?;
            if let Some(tab) = browser.tabs.iter_mut().find(|t| t.id == id) {
                tab.generation = tab.generation.saturating_add(1);
                tab.transient = "none".into();
            }
        }
        "browser_document" => {
            let id = command["id"].as_u64().ok_or("Missing browser tab.")?;
            let generation = command["generation"]
                .as_u64()
                .filter(|g| *g > 0 && *g <= 9_007_199_254_740_991)
                .ok_or("Invalid browser document.")?;
            let (url, _) = address(string(command, "url")?)?;
            if let Some(tab) = browser.tabs.iter_mut().find(|t| t.id == id)
                && generation > tab.generation
            {
                tab.generation = generation;
                tab.transient = "none".into();
                tab.url = url;
            }
        }
        "browser_transient" => {
            let id = command["id"].as_u64().ok_or("Missing browser tab.")?;
            let kind = string(command, "kind")?;
            if !matches!(kind, "none" | "search" | "media_menu") {
                return Err("Invalid page overlay.".into());
            }
            if let Some(tab) = browser
                .tabs
                .iter_mut()
                .find(|t| t.id == id && t.id == browser.active)
                && command["generation"].as_u64() == Some(tab.generation)
                && tab.generation > 0
                && page_origin(
                    &json!({"origin":command["origin"],"documentUrl":command["documentUrl"],"knownUrl":tab.url}),
                )
            {
                tab.transient = kind.into();
            }
        }
        "browser_dismiss" => {
            if let Some(tab) = browser.tabs.iter_mut().find(|t| t.id == browser.active)
                && tab.transient != "none"
            {
                target = json!({"id":tab.id,"url":tab.url,"load":false,"dismiss":true});
                tab.transient = "none".into();
            }
        }
        "browser_update" => {
            let id = command["id"].as_u64().ok_or("Missing browser tab.")?;
            let (url, kind) = address(string(command, "url")?)?;
            let duplicate = browser.tabs.iter().any(|t| t.id != id && t.kind == kind);
            if let Some(tab) = browser.tabs.iter_mut().find(|t| t.id == id) {
                tab.url = url;
                if tab.kind != "home" && !duplicate && kind != "external" {
                    tab.kind = kind;
                }
            }
        }
        "browser_back" => {
            if browser.stack.len() > 1 {
                let id = browser.stack.pop().ok_or("Missing browser history.")?;
                if command["suspendWatch"] == true
                    && browser.tabs.iter().any(|t| t.id == id && t.kind == "watch")
                {
                    browser.suspended_watch = Some(id);
                } else {
                    browser.tabs.retain(|t| t.id != id);
                }
            }
            browser.active = *browser.stack.last().unwrap_or(&1);
        }
        "browser_restore_watch" => {
            if let Some(tab) = browser.tabs.iter().find(|t| t.kind == "watch") {
                browser.active = tab.id;
                browser.stack.retain(|i| *i != tab.id);
                browser.stack.push(tab.id);
                browser.suspended_watch = None;
            }
        }
        "browser_drop_watch" => {
            if let Some(id) = browser.suspended_watch.take() {
                browser.tabs.retain(|t| t.id != id);
            }
        }
        _ => return Err("Unknown browser action.".into()),
    }
    let selected = browser.tabs.iter().find(|tab| tab.id == browser.active);
    if previous != (browser.active, selected.map(|tab| tab.url.clone()))
        || (op == "browser_invalidate" && command["id"].as_u64() == Some(browser.active))
    {
        let video = selected
            .and_then(|tab| address(&tab.url).ok())
            .filter(|(_, kind)| kind == "watch")
            .and_then(|(url, _)| Url::parse(&url).ok())
            .and_then(|url| video(&url));
        crate::extraction::select_preparation(root, video.as_deref());
    }
    if op != "browser_state" {
        browser.revision += 1;
    }
    let result = Ok(
        json!({"tabs":browser.tabs,"activeId":browser.active,"canGoBack":browser.stack.len()>1,"target":target,"revision":browser.revision,"suspendedWatchId":browser.suspended_watch,"transient":browser.tabs.iter().find(|t|t.id==browser.active).map(|t|t.transient.as_str()).unwrap_or("none")}),
    );
    drop(browsers);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_links_select_youtube_and_preserve_safe_watch_context() {
        assert_eq!(shared_url("See https://example.org/source then https://youtu.be/abcdefghijk?t=12s&list=PLexample&si=tracking.").unwrap(), "https://m.youtube.com/watch?v=abcdefghijk&t=12s&list=PLexample");
        assert_eq!(
            shared_url("https://m.youtube.com/@NASA/posts").unwrap(),
            "https://m.youtube.com/@NASA/posts"
        );
        for text in [
            "No link",
            "https://youtube.com.evil.org/watch?v=abcdefghijk",
            "http://m.youtube.com/",
            "https://user:secret@youtube.com/",
            "https://youtu.be/invalid",
        ] {
            assert!(shared_url(text).is_err());
        }
    }

    #[test]
    fn page_announcements_accept_android_origins_and_correlate_document_video() {
        let mut command = json!({"origin":"https://m.youtube.com","documentUrl":"https://m.youtube.com/watch?v=abcdefghijk","knownUrl":"https://m.youtube.com/watch?v=abcdefghijk"});
        assert!(page_origin(&command));
        command["origin"] = command["documentUrl"].clone();
        assert!(page_origin(&command));
        command["documentUrl"] = json!("https://m.youtube.com/watch?v=0123456789a");
        assert!(!page_origin(&command));
        command["origin"] = json!("https://m.youtube.com");
        assert!(!page_origin(&command));
        command["origin"] = json!("https://youtube.com.evil.org");
        assert!(!page_origin(&command));
    }
    #[test]
    fn navigation_keeps_watch_and_home_and_orders_snapshots() {
        let root = format!(
            "browser-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let first = dispatch(&json!({"op":"browser_state"}), &root).unwrap();
        let watch = dispatch(
            &json!({"op":"browser_open","url":"https://m.youtube.com/watch?v=abcdefghijk"}),
            &root,
        )
        .unwrap();
        let watch_id = watch["activeId"].as_u64().unwrap();
        assert_eq!(
            watch["target"]["load"], false,
            "a new WebView loads its source once"
        );
        assert_ne!(watch_id, first["activeId"]);
        assert_eq!(watch["tabs"].as_array().unwrap().len(), 2);
        let back = dispatch(&json!({"op":"browser_back","suspendWatch":true}), &root).unwrap();
        assert_eq!(back["activeId"], first["activeId"]);
        assert!(
            back["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tab| tab["id"] == watch_id)
        );
        assert!(back["revision"].as_u64().unwrap() > watch["revision"].as_u64().unwrap());
        let restored = dispatch(&json!({"op":"browser_restore_watch"}), &root).unwrap();
        assert_eq!(restored["activeId"], watch_id);
        let same = dispatch(
            &json!({"op":"browser_open","url":"https://m.youtube.com/watch?v=abcdefghijk"}),
            &root,
        )
        .unwrap();
        assert_eq!(
            same["target"]["load"], false,
            "restoring a retained document does not reload it"
        );
        let changed = dispatch(
            &json!({"op":"browser_open","url":"https://m.youtube.com/watch?v=0123456789a"}),
            &root,
        )
        .unwrap();
        assert_eq!(changed["activeId"], watch_id);
        assert_eq!(
            changed["target"]["load"], true,
            "a reused watch tab loads the new video"
        );
        for url in [
            "http://m.youtube.com/",
            "https://user:secret@m.youtube.com/",
            "https://m.youtube.com:8443/",
        ] {
            assert!(dispatch(&json!({"op":"browser_open","url":url}), &root).is_err());
        }
    }
}
