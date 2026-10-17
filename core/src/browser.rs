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
    {
        return Err("Unsupported page address.".into());
    }
    if url.host_str() == Some("youtu.be") {
        let id = url.path().trim_matches('/');
        if !crate::video_id(id) {
            return Err("Invalid shared video link.".into());
        }
        let mut watch = Url::parse("https://m.youtube.com/watch").unwrap();
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

fn page_origin(command: &Value) -> bool {
    let parse = |key| {
        command[key]
            .as_str()
            .and_then(|v| address(v).ok())
            .and_then(|(v, _)| Url::parse(&v).ok())
    };
    let (Some(origin), Some(document), Some(known)) =
        (parse("origin"), parse("documentUrl"), parse("knownUrl"))
    else {
        return false;
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
        return false;
    }
    let bare = origin.path() == "/" && origin.query().is_none() && origin.fragment().is_none();
    video(&document) == video(if bare { &known } else { &origin })
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
        .unwrap_or_else(|e| e.into_inner());
    let browser = browsers.entry(root.into()).or_default();
    let mut target = Value::Null;
    let op = string(command, "op")?;
    match op {
        "browser_state" => {}
        "browser_open" => {
            let (url, kind) = address(string(command, "url")?)?;
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
                tab.url = url.clone();
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
                let id = browser.stack.pop().unwrap();
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
    if op != "browser_state" {
        browser.revision += 1;
    }
    Ok(
        json!({"tabs":browser.tabs,"activeId":browser.active,"canGoBack":browser.stack.len()>1,"target":target,"revision":browser.revision,"suspendedWatchId":browser.suspended_watch,"transient":browser.tabs.iter().find(|t|t.id==browser.active).map(|t|t.transient.as_str()).unwrap_or("none")}),
    )
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
