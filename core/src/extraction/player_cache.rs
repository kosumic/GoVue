//! Short-lived player responses and guest configurations, scoped by storage root.
use crate::extraction;
use crate::extraction::network;

use super::{CACHE_LIFETIME, Job, ORIGIN, Session, UA};
use crate::identity::VideoId;
use crate::{Result, json};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub(super) static PLAYERS: OnceLock<Mutex<VecDeque<Player>>> = OnceLock::new();
static CONFIGURATIONS: OnceLock<Mutex<VecDeque<Configuration>>> = OnceLock::new();

#[derive(Clone)]
pub(super) struct Player {
    pub(super) root: String,
    pub(super) id: VideoId,
    pub(super) at: Instant,
    pub(super) data: Value,
    pub(super) chapters: Vec<Value>,
    pub(super) session: Session,
    pub(super) web_loaded: bool,
    pub(super) chapters_loaded: bool,
}
#[derive(Clone)]
struct Configuration {
    root: String,
    at: Instant,
    context: Value,
    session: Session,
}
fn configuration_context(html: &str) -> Option<Value> {
    extraction::page_json(html, "ytcfg.set(")
        .map(|config| config["INNERTUBE_CONTEXT"].clone())
        .filter(|context| context["client"].is_object())
        .or_else(|| {
            extraction::page_json(html, "\"INNERTUBE_CONTEXT\":")
                .filter(|context| context["client"].is_object())
        })
}

fn configuration(
    job: &Job,
    root: &str,
    id: &str,
    fresh: bool,
    need_page: bool,
) -> Result<(Value, Option<String>)> {
    let cached = CONFIGURATIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor configuration unavailable.")?
        .iter()
        .find(|config| config.root == root)
        .cloned();
    if let Some(cached) = cached {
        let mut session = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        // A player refresh may already have restored newer cookies and session
        // age. Keep them so configuration reuse cannot reset caption retries.
        if session.cookies.is_empty() {
            session.clone_from(&cached.session);
        }
        drop(session);
        if !fresh && !need_page && cached.at.elapsed() < CACHE_LIFETIME {
            let _span = crate::trace::Section::begin(c"GoVue extraction: reuse configuration");
            return Ok((cached.context, None));
        }
    }
    let address = format!("{ORIGIN}/watch?v={id}&hl=en");
    let mut html = String::new();
    if !need_page {
        let _span = crate::trace::Section::begin(c"GoVue extraction: configuration prefix");
        let reply = network::fetch_response_bounded(job, &address, None, false, &[], 65_536)?;
        if reply.status != 200 {
            return Err(
                format!("YouTube extraction request failed (HTTP {}).", reply.status).into(),
            );
        }
        // The trailing UTF-8 code point may be cut off. Only a complete JSON
        // context before that boundary is accepted; no page code is evaluated.
        html = String::from_utf8_lossy(&reply.body).into_owned();
    }
    if need_page || configuration_context(&html).is_none() {
        html = network::fetch(job, &address, None, false, &[])?;
    }
    let context =
        configuration_context(&html).ok_or("YouTube did not provide extraction configuration.")?;
    let config = Configuration {
        root: root.into(),
        at: Instant::now(),
        context: context.clone(),
        session: job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?
            .clone(),
    };
    job.check()?;
    let mut configurations = CONFIGURATIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor configuration unavailable.")?;
    configurations.retain(|config| config.root != root && config.at.elapsed() < CACHE_LIFETIME);
    configurations.push_front(config);
    configurations.truncate(3);
    drop(configurations);
    Ok((context, Some(html)))
}

fn native_player(job: &Job, id: &str, context: Value) -> Result<Value> {
    let mut client = context["client"].clone();
    if !client.is_object() {
        return Err("YouTube did not provide a client context.".into());
    }
    for (key, value) in [
        ("clientName", "VISIONOS"),
        ("clientVersion", "1.02"),
        ("deviceMake", "Apple"),
        ("deviceModel", "RealityDevice17,1"),
        ("osName", "visionOS"),
        ("osVersion", "26.5.23O471"),
        ("userAgent", UA),
    ] {
        client[key] = json!(value);
    }
    let mut native_context = context;
    native_context["client"] = client;
    let data = extraction::json_response(&network::fetch(
        job,
        &format!("{ORIGIN}/youtubei/v1/player?prettyPrint=false"),
        Some(
            json!({"context":native_context,"videoId":id,"contentCheckOk":true,"racyCheckOk":true}),
        ),
        false,
        &[
            ("X-Youtube-Client-Name", "101"),
            ("X-Youtube-Client-Version", "1.02"),
        ],
    )?)?;
    match data["playabilityStatus"]["status"].as_str().unwrap_or("") {
        "OK" => {},
        "LOGIN_REQUIRED" => return Err("YouTube requires sign-in or verification for this video. Standalone extraction cannot access it yet.".into()),
        "LIVE_STREAM_OFFLINE" => return Err("This live stream is offline.".into()),
        _ => return Err("This video is unavailable to the standalone YouTube extractor.".into()),
    }
    if data["videoDetails"]["videoId"] != id {
        return Err("YouTube returned another video's metadata.".into());
    }
    Ok(data)
}

pub(super) fn player(
    job: &Job,
    root: &str,
    id: &str,
    fresh: bool,
    need_page: bool,
) -> Result<Player> {
    {
        let players = PLAYERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Extractor cache unavailable.")?;
        if let Some(cached) = players
            .iter()
            .find(|p| p.root == root && p.id.as_str() == id)
        {
            job.session
                .lock()
                .map_err(|_| "Extraction session unavailable.")?
                .clone_from(&cached.session);
            if !fresh && cached.at.elapsed() < CACHE_LIFETIME && (!need_page || cached.web_loaded) {
                return Ok(cached.clone());
            }
        }
    }
    let (context, html) = configuration(job, root, id, fresh, need_page)?;
    let mut data = match native_player(job, id, context) {
        Err(_) if html.is_none() => return player(job, root, id, true, need_page),
        result => result?,
    };
    let html = html.as_deref().unwrap_or("");
    // Web subtitle addresses require WebPO tokens. Keep the web client's
    // caption metadata paired with that token context, independently of the
    // native client used for direct media streams.
    let web = extraction::page_json(html, "var ytInitialPlayerResponse = ")
        .or_else(|| extraction::page_json(html, "ytInitialPlayerResponse="))
        .filter(|web| web["videoDetails"]["videoId"] == id);
    let web_loaded = need_page || web.is_some();
    if let Some(web) = web.filter(|web| web["captions"].is_object()) {
        data["captions"] = web["captions"].clone();
    }
    let initial = extraction::page_json(html, "var ytInitialData = ")
        .or_else(|| extraction::page_json(html, "ytInitialData="))
        .unwrap_or(Value::Null);
    let result = Player {
        root: root.into(),
        id: VideoId::parse(id)?,
        at: Instant::now(),
        data,
        web_loaded,
        chapters_loaded: initial.is_object(),
        chapters: super::chapters(&initial),
        session: job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?
            .clone(),
    };
    job.check()?;
    let mut players = PLAYERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor cache unavailable.")?;
    players.retain(|p| p.at.elapsed() < CACHE_LIFETIME && !(p.root == root && p.id.as_str() == id));
    players.push_front(result.clone());
    players.truncate(3);
    drop(players);
    Ok(result)
}

pub(super) fn chapters(root: &str, id: &str, fresh: bool) -> Option<Vec<Value>> {
    PLAYERS
        .get()
        .and_then(|players| players.lock().ok())
        .and_then(|players| {
            players
                .iter()
                .find(|player| {
                    !fresh
                        && player.root == root
                        && player.id.as_str() == id
                        && player.at.elapsed() < CACHE_LIFETIME
                        && player.chapters_loaded
                })
                .map(|player| player.chapters.clone())
        })
}
