//! Standalone YouTube extraction. Rust owns HTTP, protocol state, validation and
//! caches. The platform transports bounded HTTPS bytes; an isolated renderer
//! supplies only BotGuard snapshot/token strings. No YouTube page is executed.
use crate::{MAX_BYTES, Result, Track, Transcript, json, media, string, video_id};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;

mod page;

const ORIGIN: &str = "https://www.youtube.com";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";
const CACHE_LIFETIME: Duration = Duration::from_secs(90);
const JOB_LIFETIME: Duration = Duration::from_secs(180);
const PREPARATION_REQUEST: &str = "prepare:media";
// On a translated-caption 429, retry once after the guest session has aged.
// Empirical workaround: https://github.com/yt-dlp/yt-dlp/pull/15709.
const TRANSLATION_SESSION_AGE: Duration = Duration::from_secs(75);
static HTTP: OnceLock<Box<dyn ExtractionHttp>> = OnceLock::new();
type Jobs = HashMap<(String, String), Arc<Job>>;
static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static PLAYERS: OnceLock<Mutex<VecDeque<Player>>> = OnceLock::new();
static CONFIGURATIONS: OnceLock<Mutex<VecDeque<Configuration>>> = OnceLock::new();

#[derive(uniffi::Record)]
pub struct ExtractionHeader {
    pub name: String,
    pub value: String,
}
#[derive(uniffi::Record)]
pub struct ExtractionRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<ExtractionHeader>,
    pub body: String,
    pub max_bytes: u32,
    pub prefix_only: bool,
}
#[derive(uniffi::Record)]
pub struct ExtractionReply {
    pub status: i32,
    pub body: Vec<u8>,
    pub headers: Vec<ExtractionHeader>,
}
#[uniffi::export(callback_interface)]
pub trait ExtractionHttp: Send + Sync {
    /// Called on a Rust worker. No redirects, automatic cookie jar, logging or disk cache.
    fn request(&self, request: ExtractionRequest) -> ExtractionReply;
}
#[uniffi::export]
pub fn install_extraction_http(backend: Box<dyn ExtractionHttp>) {
    let _ = HTTP.set(backend);
}

#[derive(Clone)]
struct Session {
    cookies: HashMap<String, String>,
    since: Instant,
}
#[derive(Clone)]
struct Player {
    root: String,
    id: String,
    at: Instant,
    data: Value,
    chapters: Vec<Value>,
    session: Session,
    web_loaded: bool,
    chapters_loaded: bool,
}
#[derive(Clone)]
struct Configuration {
    root: String,
    at: Instant,
    context: Value,
    session: Session,
}
struct Runtime {
    sequence: u32,
    task: Option<Value>,
    reply: Option<Result<String>>,
}
struct Job {
    at: Instant,
    prepared_video: Option<String>,
    user_agent: String,
    session: Mutex<Session>,
    cancelled: AtomicBool,
    result: Mutex<Option<Result<Value>>>,
    runtime: Mutex<Runtime>,
    wake: Condvar,
}
impl Job {
    fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Extraction cancelled.".into())
        } else if self.at.elapsed() >= JOB_LIFETIME {
            Err("YouTube extraction timed out.".into())
        } else {
            Ok(())
        }
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.wake.notify_all();
    }
    fn wait_until(&self, until: Instant) -> Result<()> {
        let mut state = self
            .runtime
            .lock()
            .map_err(|_| "Token runtime unavailable.")?;
        loop {
            self.check()?;
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            state = self
                .wake
                .wait_timeout(state, remaining.min(Duration::from_millis(250)))
                .map_err(|_| "Token runtime unavailable.")?
                .0;
        }
    }
    fn evaluate(&self, mut task: Value) -> Result<String> {
        self.check()?;
        let from_page = task["stage"] == page::STAGE;
        let deadline = Instant::now() + page::TIMEOUT;
        let mut state = self
            .runtime
            .lock()
            .map_err(|_| "Token runtime unavailable.")?;
        state.sequence += 1;
        task["taskId"] = json!(state.sequence);
        state.task = Some(task);
        state.reply = None;
        crate::changed(if from_page {
            page::CHANNEL
        } else {
            "extraction-token"
        });
        loop {
            self.check()?;
            if from_page && Instant::now() >= deadline {
                state.task = None;
                return Err("YouTube caption page timed out.".into());
            }
            if let Some(reply) = state.reply.take() {
                return reply;
            }
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(250))
                .map_err(|_| "Token runtime unavailable.")?
                .0;
        }
    }
}

fn allowed_address(raw: &str, interpreter: bool) -> Result<()> {
    let url = Url::parse(raw).map_err(|_| "Invalid extraction address.")?;
    if raw.len() > 30_000
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || raw.chars().any(char::is_control)
    {
        return Err("Unsupported extraction address.".into());
    }
    let host = url.host_str().unwrap_or("");
    let allowed = if interpreter {
        [
            "www.youtube.com",
            "m.youtube.com",
            "www.google.com",
            "www.gstatic.com",
        ]
        .contains(&host)
            && (url.path().starts_with("/js/") || url.path().starts_with("/s/"))
    } else {
        ["www.youtube.com", "m.youtube.com", "youtube.com"].contains(&host)
            && matches!(
                url.path(),
                "/watch"
                    | "/youtubei/v1/player"
                    | "/youtubei/v1/att/get"
                    | "/api/timedtext"
                    | "/api/jnn/v1/GenerateIT"
                    | "/api/jnn/v1/Create"
            )
    };
    if allowed {
        Ok(())
    } else {
        Err("Unsupported extraction address.".into())
    }
}

fn fetch_response_bounded(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
    limit: usize,
) -> Result<ExtractionReply> {
    if limit == 0 || limit > MAX_BYTES {
        return Err("Invalid extraction size limit.".into());
    }
    job.check()?;
    allowed_address(url, interpreter)?;
    let browser_request = interpreter
        || url.contains("/api/jnn/v1/")
        || url.contains("/api/timedtext")
        || url.contains("/youtubei/v1/att/get");
    let mut headers = vec![
        ExtractionHeader {
            name: "User-Agent".into(),
            value: if browser_request {
                job.user_agent.clone()
            } else {
                UA.into()
            },
        },
        ExtractionHeader {
            name: "Referer".into(),
            value: format!("{ORIGIN}/"),
        },
    ];
    for (name, value) in extra {
        headers.push(ExtractionHeader {
            name: (*name).into(),
            value: (*value).into(),
        });
    }
    let method = if body.is_some() { "POST" } else { "GET" };
    if !interpreter {
        let session = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        if !session.cookies.is_empty() {
            headers.push(ExtractionHeader {
                name: "Cookie".into(),
                value: session
                    .cookies
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
    }
    if body.is_some()
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
    {
        headers.push(ExtractionHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        });
    }
    let request = ExtractionRequest {
        url: url.into(),
        method: method.into(),
        headers,
        body: body.map_or_else(String::new, |b| b.to_string()),
        max_bytes: u32::try_from(limit).map_err(|_| "Invalid extraction size limit.")?,
        prefix_only: limit < MAX_BYTES,
    };
    let reply = if let Some(http) = HTTP.get() {
        http.request(request)
    } else {
        // Host CLI uses the same bounded contract as Android/iOS.
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .new_agent();
        let mut call = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(&request.url);
        for header in request.headers {
            call = call.header(&header.name, &header.value);
        }
        let call = call
            .body(request.body.as_bytes())
            .map_err(|_| "Invalid extraction request.")?;
        match agent.run(call) {
            Ok(response) => {
                let status = i32::from(response.status().as_u16());
                let headers = response
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .filter_map(|value| value.to_str().ok())
                    .map(|value| ExtractionHeader {
                        name: "Set-Cookie".into(),
                        value: value.into(),
                    })
                    .collect();
                let mut body = Vec::new();
                response
                    .into_body()
                    .into_reader()
                    .take(u64::from(request.max_bytes) + u64::from(!request.prefix_only))
                    .read_to_end(&mut body)
                    .map_err(|_| "YouTube extraction connection failed.")?;
                ExtractionReply {
                    status,
                    body,
                    headers,
                }
            }
            Err(_) => return Err("YouTube extraction connection failed.".into()),
        }
    };
    job.check()?;
    if !interpreter {
        let mut session = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        for header in reply
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("Set-Cookie"))
        {
            for cookie in header.value.split(',') {
                if let Some((name, value)) = cookie
                    .trim()
                    .split(';')
                    .next()
                    .and_then(|cookie| cookie.split_once('='))
                    && name.len() <= 100
                    && value.len() <= 8192
                    && !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    && !value.chars().any(char::is_control)
                    && (session.cookies.len() < 20 || session.cookies.contains_key(name))
                {
                    if name == "VISITOR_INFO1_LIVE"
                        && session
                            .cookies
                            .get(name)
                            .is_none_or(|previous| previous != value)
                    {
                        session.since = Instant::now();
                    }
                    session.cookies.insert(name.into(), value.into());
                }
            }
        }
    }
    if reply.body.len() > limit + usize::from(limit == MAX_BYTES) {
        return Err("YouTube extraction response is too large.".into());
    }
    Ok(reply)
}
fn fetch_response(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
) -> Result<ExtractionReply> {
    let reply = fetch_response_bounded(job, url, body, interpreter, extra, MAX_BYTES)?;
    if reply.body.len() > MAX_BYTES {
        return Err("YouTube extraction response is too large.".into());
    }
    Ok(reply)
}
fn response_text(reply: ExtractionReply) -> Result<String> {
    if reply.status != 200 {
        return Err(format!(
            "YouTube extraction request failed (HTTP {}).",
            reply.status
        ));
    }
    String::from_utf8(reply.body).map_err(|_| "Unreadable YouTube extraction response.".into())
}
fn fetch(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
) -> Result<String> {
    response_text(fetch_response(job, url, body, interpreter, extra)?)
}
fn caption_source(job: &Job, url: &str, verified: bool, translated: bool) -> Result<String> {
    let mut reply = fetch_response(job, url, None, false, &[])?;
    if !verified && matches!(reply.status, 403 | 429) {
        // An unmarked caption URL can still require verification. Allow the
        // existing token flow once, without retrying the same request in a loop.
        return Ok(String::new());
    }
    if reply.status == 429 && translated {
        let since = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?
            .since;
        let ready_at = since + TRANSLATION_SESSION_AGE;
        if ready_at > Instant::now() {
            job.wait_until(ready_at)?;
            reply = fetch_response(job, url, None, false, &[])?;
        }
        if reply.status == 429 {
            return Err("YouTube is temporarily limiting translated captions. Keep watching and try again shortly.".into());
        }
    }
    response_text(reply)
}
fn json_response(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|_| "Unreadable YouTube extraction data.".into())
}
fn page_json(html: &str, marker: &str) -> Option<Value> {
    html.match_indices(marker).take(32).find_map(|(offset, _)| {
        serde_json::Deserializer::from_str(html[offset + marker.len()..].trim_start())
            .into_iter::<Value>()
            .next()?
            .ok()
            .filter(|value| {
                value.is_object()
                    && (marker != "ytcfg.set(" || value["INNERTUBE_CONTEXT"].is_object())
            })
    })
}
fn text(value: &Value) -> String {
    value
        .as_str()
        .or_else(|| value["simpleText"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            value["runs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|run| run["text"].as_str())
                .collect()
        })
}
fn chapters(data: &Value) -> Vec<Value> {
    let mut queue = vec![data];
    let mut result = Vec::new();
    let mut visited = 0;
    while let Some(value) = queue.pop() {
        visited += 1;
        if visited > 100_000 || result.len() >= 300 {
            break;
        }
        if let Some(chapter) = value.get("chapterRenderer")
            && let Some(start) = chapter["timeRangeStartMillis"]
                .as_f64()
                .or_else(|| chapter["timeRangeStartMillis"].as_str()?.parse().ok())
        {
            result.push(json!({"start": start / 1000.0, "title": text(&chapter["title"]),
                    "previewUrl":chapter["thumbnail"]["thumbnails"].as_array().and_then(|v|v.last()).and_then(|v|v["url"].as_str())}));
        }
        if let Some(chapter) = value.get("macroMarkersListItemRenderer") {
            result.push(json!({"timestamp":text(&chapter["timeDescription"]),"title":text(&chapter["title"]),
                "previewUrl":chapter["thumbnail"]["thumbnails"].as_array().and_then(|v|v.last()).and_then(|v|v["url"].as_str())}));
        }
        match value {
            Value::Object(values) => queue.extend(values.values()),
            Value::Array(values) => queue.extend(values),
            _ => {}
        }
    }
    result
}
fn configuration_context(html: &str) -> Option<Value> {
    page_json(html, "ytcfg.set(")
        .map(|config| config["INNERTUBE_CONTEXT"].clone())
        .filter(|context| context["client"].is_object())
        .or_else(|| {
            page_json(html, "\"INNERTUBE_CONTEXT\":")
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
        let reply = fetch_response_bounded(job, &address, None, false, &[], 65_536)?;
        if reply.status != 200 {
            return Err(format!(
                "YouTube extraction request failed (HTTP {}).",
                reply.status
            ));
        }
        // The trailing UTF-8 code point may be cut off. Only a complete JSON
        // context before that boundary is accepted; no page code is evaluated.
        html = String::from_utf8_lossy(&reply.body).into_owned();
    }
    if need_page || configuration_context(&html).is_none() {
        html = fetch(job, &address, None, false, &[])?;
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
    let data = json_response(&fetch(
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

fn player(job: &Job, root: &str, id: &str, fresh: bool, need_page: bool) -> Result<Player> {
    {
        let players = PLAYERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Extractor cache unavailable.")?;
        if let Some(cached) = players.iter().find(|p| p.root == root && p.id == id) {
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
    let web = page_json(html, "var ytInitialPlayerResponse = ")
        .or_else(|| page_json(html, "ytInitialPlayerResponse="))
        .filter(|web| web["videoDetails"]["videoId"] == id);
    let web_loaded = need_page || web.is_some();
    if let Some(web) = web.filter(|web| web["captions"].is_object()) {
        data["captions"] = web["captions"].clone();
    }
    let initial = page_json(html, "var ytInitialData = ")
        .or_else(|| page_json(html, "ytInitialData="))
        .unwrap_or(Value::Null);
    let result = Player {
        root: root.into(),
        id: id.into(),
        at: Instant::now(),
        data,
        web_loaded,
        chapters_loaded: initial.is_object(),
        chapters: chapters(&initial),
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
    players.retain(|p| p.at.elapsed() < CACHE_LIFETIME && !(p.root == root && p.id == id));
    players.push_front(result.clone());
    players.truncate(3);
    drop(players);
    Ok(result)
}
fn tracks(player: &Player) -> Vec<Track> {
    crate::caption_tracks::extract(&player.data, "").0
}
fn parse_challenge(response: &Value) -> Result<Value> {
    if let Some(encoded) = response[1].as_str() {
        let bytes = URL_SAFE
            .decode(encoded)
            .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
            .or_else(|_| STANDARD.decode(encoded))
            .map_err(|_| "Unreadable YouTube token challenge.")?;
        let decoded: Vec<_> = bytes.into_iter().map(|b| b.wrapping_add(97)).collect();
        serde_json::from_slice(&decoded).map_err(|_| "Unreadable YouTube token challenge.".into())
    } else if response[0].is_array() {
        Ok(response[0].clone())
    } else {
        Err("Missing YouTube token challenge.".into())
    }
}
fn token(job: &Job, player: &Player) -> Result<String> {
    let response = json_response(&fetch(
        job,
        &format!("{ORIGIN}/api/jnn/v1/Create"),
        Some(json!([REQUEST_KEY])),
        false,
        &[
            ("Content-Type", "application/json+protobuf"),
            ("X-Goog-Api-Key", "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw"),
            ("X-User-Agent", "grpc-web-javascript/0.1"),
        ],
    )?)?;
    let challenge = parse_challenge(&response)?;
    let program = challenge[4]
        .as_str()
        .ok_or("Missing YouTube token program.")?;
    let name = challenge[5]
        .as_str()
        .ok_or("Missing YouTube token interpreter name.")?;
    if program.len() > 2_000_000
        || name.len() > 200
        || name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_$".contains(&b))
    {
        return Err("Invalid YouTube token challenge.".into());
    }
    let interpreter = if let Some(address) = challenge[2]
        .as_array()
        .and_then(|values| values.iter().find_map(Value::as_str))
    {
        let address = Url::parse(ORIGIN)
            .map_err(|_| "Invalid extraction origin.")?
            .join(address)
            .map_err(|_| "Invalid token interpreter address.")?;
        fetch(job, address.as_str(), None, true, &[])?
    } else {
        challenge[1]
            .as_array()
            .and_then(|values| values.iter().find_map(Value::as_str))
            .filter(|source| source.len() <= 2_000_000)
            .ok_or("Missing YouTube token interpreter.")?
            .to_owned()
    };
    let snapshot = job.evaluate(
        json!({"stage":"snapshot","program":program,"globalName":name,"interpreter":interpreter}),
    )?;
    let integrity = json_response(&fetch(
        job,
        &format!("{ORIGIN}/api/jnn/v1/GenerateIT"),
        Some(json!([REQUEST_KEY, snapshot])),
        false,
        &[
            ("Content-Type", "application/json+protobuf"),
            ("X-Goog-Api-Key", "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw"),
            ("X-User-Agent", "grpc-web-javascript/0.1"),
        ],
    )?)?;
    let ttl = integrity[1]
        .as_f64()
        .or_else(|| integrity[1].as_str()?.parse().ok())
        .unwrap_or(0.0);
    if !integrity[0].is_string() || ttl <= 0.0 {
        return Err("YouTube could not verify this extraction session.".into());
    }
    job.evaluate(json!({"stage":"mint","integrityToken":integrity[0],"estimatedTtlSecs":integrity[1],
        "mintRefreshThreshold":integrity[2],"websafeFallbackToken":integrity[3],"videoId":player.id}))
}
fn caption_address(raw: &str, id: &str, po_token: Option<&str>) -> Result<String> {
    allowed_address(raw, false)?;
    let url = Url::parse(raw).map_err(|_| "Invalid caption address.")?;
    if url.path() != "/api/timedtext"
        || url
            .query_pairs()
            .find(|(k, _)| k == "v")
            .is_none_or(|(_, v)| v != id)
        || url.query_pairs().any(|(k, _)| k == "tlang")
    {
        return Err("Caption address belongs to another video or language.".into());
    }
    // Preserve signed query bytes. Only replace unsigned format/token fields.
    let (base, query) = raw.split_once('?').ok_or("Missing caption query.")?;
    let pairs: Vec<_> = query
        .split('&')
        .filter(|pair| {
            !url::form_urlencoded::parse(pair.as_bytes())
                .next()
                .is_some_and(|(key, _)| {
                    matches!(key.as_ref(), "fmt" | "xosf")
                        || po_token.is_some() && matches!(key.as_ref(), "pot" | "potc" | "c")
                })
        })
        .collect();
    let mut result = format!("{base}?{}&fmt=json3", pairs.join("&"));
    if let Some(token) = po_token {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("pot", token)
            .append_pair("potc", "1")
            .append_pair("c", "WEB")
            .finish();
        result.push('&');
        result.push_str(&query);
    }
    Ok(result)
}
fn transcript(
    job: &Job,
    root: &str,
    id: &str,
    track: &str,
    audio_key: &str,
    fresh: bool,
) -> Result<Value> {
    if !fresh {
        let mut command = json!({"op":"restore","videoId":id,"audioKey":audio_key});
        if track != "default" {
            command["trackId"] = json!(track);
        }
        let saved = crate::dispatch(&command.to_string(), root)?;
        if !saved.is_null() && (track == "default" || saved["trackId"] == track) {
            return Ok(saved);
        }
    }
    let player = player(job, root, id, fresh, true)?;
    let source_id = track
        .strip_prefix("translate:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, source)| source)
        .or_else(|| (track != "default").then_some(track));
    let (tracks, track_metadata) =
        crate::caption_tracks::extract_for_source(&player.data, audio_key, source_id);
    let list = &player.data["captions"]["playerCaptionsTracklistRenderer"];
    let selected = if track == "default" {
        crate::caption_tracks::default_track(&tracks, &track_metadata)
    } else {
        tracks.iter().find(|t| t.id == track)
    }
    .ok_or(if tracks.is_empty() {
        "This video has no caption tracks."
    } else {
        "This caption language is unavailable."
    })?;
    let source_id = selected.translated_from.as_deref().unwrap_or(&selected.id);
    let index = tracks
        .iter()
        .position(|t| t.id == source_id)
        .ok_or("This caption source is unavailable.")?;
    let translated = selected.translated_from.is_some();
    // Protected caption tracks can return HTTP 200 with no bytes even after a
    // standalone token challenge. Let the active player supply its own proof.
    let from_page = page::captions(job, root, id, selected).ok().flatten();
    job.check()?;
    let (segments, caption_metadata) = if let Some(captions) = from_page {
        captions
    } else {
        let result = (|| -> Result<_> {
            let raw = string(&list["captionTracks"][index], "baseUrl")?;
            let address_for = |proof| -> Result<String> {
                let mut address = caption_address(raw, id, proof)?;
                if translated {
                    address.push('&');
                    address.push_str(
                        &url::form_urlencoded::Serializer::new(String::new())
                            .append_pair("tlang", &selected.language)
                            .finish(),
                    );
                }
                Ok(address)
            };
            let address = address_for(None)?;
            let requires_token = Url::parse(&address)
                .map_err(|_| "Invalid caption address.")?
                .query_pairs()
                .any(|(key, value)| key == "exp" && matches!(value.as_ref(), "xpe" | "xpv"));
            let mut source = if requires_token {
                String::new()
            } else {
                caption_source(job, &address, false, translated)?
            };
            if source.trim().is_empty() {
                let proof = token(job, &player)?;
                source = caption_source(job, &address_for(Some(&proof))?, true, translated)?;
            }
            if source.trim().is_empty() {
                return Err("YouTube returned no caption text after verification.".into());
            }
            crate::parse_captions(&source)
        })();
        match result {
            Err(error) => {
                // A page that was missing or still loading can become ready while
                // standalone verification runs. Retry its transport once.
                job.check()?;
                page::captions(job, root, id, selected)
                    .ok()
                    .flatten()
                    .ok_or(error)?
            }
            Ok(result) => result,
        }
    };
    let mut data = Transcript {
        video_id: id.into(),
        title: text(&player.data["videoDetails"]["title"]),
        language: selected.label.clone(),
        track_id: selected.id.clone(),
        tracks,
        segments,
        caption_metadata: Some(caption_metadata),
        track_metadata: Some(track_metadata),
    };
    crate::normalize(&mut data)?;
    job.check()?;
    let _guard = crate::REQUEST_LOCK
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    crate::remember(crate::cache_path(root, id, &data.track_id)?, &data)?;
    let _ = crate::save(root, &data);
    Ok(json!(data))
}
fn catalog(job: &Job, root: &str, id: &str, fresh: bool) -> Result<Value> {
    let _span = crate::trace::Section::begin(c"GoVue extraction: media");
    if !fresh && let Some(cached) = media::cached_catalog(root, id) {
        return Ok(cached);
    }
    let player = player(job, root, id, fresh, false)?;
    let data = &player.data;
    let mut formats = Vec::new();
    for list in ["formats", "adaptiveFormats"] {
        for raw in data["streamingData"][list]
            .as_array()
            .into_iter()
            .flatten()
            .take(500)
        {
            let Some(address) = raw["url"].as_str().and_then(media::media_url) else {
                continue;
            };
            if Url::parse(&address)
                .map_err(|_| "Invalid media address.")?
                .query_pairs()
                .any(|(k, _)| k == "n")
                || raw.get("drmFamilies").is_some()
            {
                continue;
            }
            let mut format = raw.clone();
            format["url"] = json!(address);
            format["userAgent"] = json!(UA);
            formats.push(format);
        }
    }
    let hls = data["streamingData"]["hlsManifestUrl"]
        .as_str()
        .and_then(media::media_url);
    if formats.is_empty() && hls.is_none() {
        return Err("YouTube returned no direct media formats supported by this extractor.".into());
    }
    let details = &data["videoDetails"];
    let raw = json!({"type":"media","requestId":"rust-extraction","videoId":id,"ok":true,"media":{
        "title":text(&details["title"]),"author":text(&details["author"]),"duration":details["lengthSeconds"].as_str().and_then(|n|n.parse::<f64>().ok()),
        "position":0,"live":details["isLive"]==true || data["playabilityStatus"]["liveStreamability"].is_object(),
        "formats":formats,"hlsManifestUrl":hls,"hlsUserAgent":UA,"subtitles":tracks(&player),"chapters":player.chapters}});
    let result = media::catalog(
        &json!({"raw":raw.to_string(),"origin":ORIGIN,"requestId":"rust-extraction","videoId":id}),
    )?;
    job.check()?;
    media::cache_catalog(root, &result);
    Ok(result)
}

fn chapter_catalog(job: &Job, root: &str, id: &str, fresh: bool) -> Result<Value> {
    let _span = crate::trace::Section::begin(c"GoVue extraction: chapters");
    // Playback intentionally fetches only a configuration prefix. Load the
    // complete, video-correlated metadata in a separate cancellable job.
    let cached = PLAYERS
        .get()
        .and_then(|players| players.lock().ok())
        .and_then(|players| {
            players
                .iter()
                .find(|player| {
                    !fresh
                        && player.root == root
                        && player.id == id
                        && player.at.elapsed() < CACHE_LIFETIME
                        && player.chapters_loaded
                })
                .map(|player| player.chapters.clone())
        });
    // A prefix may contain the complete caption response but still omit
    // ytInitialData. Caption readiness alone cannot satisfy chapter loading.
    let raw = match cached {
        Some(chapters) => chapters,
        None => player(job, root, id, true, true)?.chapters,
    };
    let chapters = json!(media::chapter_list(&json!(raw)));
    job.check()?;
    media::cache_chapters(root, id, &chapters);
    Ok(chapters)
}

/// Navigation keeps only work for the selected video; a newer click replaces it.
pub(super) fn select_preparation(root: &str, video: Option<&str>) {
    let Some(mut jobs) = JOBS.get().and_then(|jobs| jobs.lock().ok()) else {
        return;
    };
    let key = (root.to_owned(), PREPARATION_REQUEST.to_owned());
    if jobs
        .get(&key)
        .is_some_and(|job| job.prepared_video.as_deref() != video)
        && let Some(job) = jobs.remove(&key)
    {
        let _span = crate::trace::Section::begin(c"GoVue extraction: discard preparation");
        job.cancel();
    }
}

fn claim_preparation(jobs: &mut Jobs, root: &str, id: &str, request: &str, fresh: bool) -> bool {
    let key = (root.to_owned(), PREPARATION_REQUEST.to_owned());
    if jobs
        .get(&key)
        .is_none_or(|job| job.prepared_video.as_deref() != Some(id))
    {
        return false;
    }
    let Some(job) = jobs.remove(&key) else {
        return false;
    };
    let failed = job
        .result
        .lock()
        .map_or(true, |result| result.as_ref().is_some_and(Result::is_err));
    if fresh || failed || job.check().is_err() || job.at.elapsed() >= CACHE_LIFETIME {
        job.cancel();
        return false;
    }
    let _span = crate::trace::Section::begin(c"GoVue extraction: claim preparation");
    jobs.insert((root.into(), request.into()), job);
    true
}

// OS launches have no page document yet. Canonicalize their shared text in
// Rust and use the same bounded, video-correlated slot as page preparation.
fn launch_video(command: &Value, root: &str) -> Result<Option<String>> {
    let settings = crate::player::preferences::settings(&json!({}), root)?;
    if !["adBlock", "backgroundPlayback", "autoPip"]
        .iter()
        .any(|key| settings[key] == true)
    {
        return Ok(None);
    }
    Ok(crate::browser::dispatch(
        &json!({"op":"browser_shared_url","text":command["text"]}),
        root,
    )
    .ok()
    .and_then(|target| Url::parse(target.as_str()?).ok())
    .filter(|target| target.path() == "/watch")
    .and_then(|target| {
        target
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, id)| id.into_owned())
            .filter(|id| video_id(id))
    }))
}

fn runtime_tasks(root: &str, from_page: bool) -> Result<Value> {
    let jobs = JOBS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor busy.")?;
    let mut requests = Vec::new();
    let mut tasks = Vec::new();
    for ((store, request), job) in jobs.iter() {
        if store != root || !from_page && !request.starts_with("worker:") || job.check().is_err() {
            continue;
        }
        requests.push(request.clone());
        if let Some(task) = job
            .runtime
            .lock()
            .map_err(|_| "Token runtime unavailable.")?
            .task
            .as_ref()
            .filter(|task| (task["stage"] == page::STAGE) == from_page)
        {
            tasks.push(json!({"requestId":request,"task":task}));
        }
    }
    drop(jobs);
    Ok(json!({"requests":requests,"tasks":tasks}))
}

fn request_id(command: &Value) -> Result<&str> {
    let request = string(command, "requestId")?;
    if request.is_empty()
        || request.len() > 150
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err("Invalid extraction request identity.".into());
    }
    let op = string(command, "op")?;
    if request == PREPARATION_REQUEST
        && !matches!(op, "extract_prepare" | "extract_launch" | "extract_release")
    {
        return Err("Reserved extraction request identity.".into());
    }
    Ok(request)
}

pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let op = string(command, "op")?;
    if op == "extract_page_register" {
        return page::register(command, root);
    }
    if matches!(op, "extract_runtime_tasks" | "extract_page_tasks") {
        return runtime_tasks(root, op == "extract_page_tasks");
    }
    let request = request_id(command)?;
    let selected = if matches!(op, "extract_prepare" | "extract_launch") {
        let _span = crate::trace::Section::begin(c"GoVue extraction: prepare media");
        if request != PREPARATION_REQUEST {
            return Err("Invalid media preparation identity.".into());
        }
        let id = if op == "extract_launch" {
            launch_video(command, root)?
        } else {
            crate::browser::preparation_video(command, root)
        };
        let Some(id) = id else {
            return Ok(json!({"prepared":false}));
        };
        Some(id)
    } else {
        None
    };
    let key = (root.to_owned(), request.to_owned());
    let mut jobs = JOBS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor busy.")?;
    jobs.retain(|_, job| {
        if job.at.elapsed() >= JOB_LIFETIME {
            job.cancel();
            false
        } else {
            true
        }
    });
    match op {
        "extract_start" | "extract_prepare" | "extract_launch" => {
            let preparing = selected.is_some();
            let id = if let Some(id) = &selected {
                id.as_str()
            } else {
                string(command, "videoId")?
            };
            let kind = if preparing {
                "media"
            } else {
                string(command, "kind")?
            };
            let track = command["trackId"].as_str().unwrap_or("default");
            let audio_key = command["audioKey"].as_str().unwrap_or("");
            let user_agent = command["userAgent"]
                .as_str()
                .filter(|agent| !agent.is_empty())
                .unwrap_or(UA);
            if user_agent.len() > 1000 || user_agent.chars().any(char::is_control) {
                return Err("Invalid extraction browser identity.".into());
            }
            if !video_id(id)
                || !matches!(kind, "media" | "transcript" | "chapters")
                || track.len() > 512
                || audio_key.len() > 512
            {
                return Err("Invalid extraction request.".into());
            }
            if !preparing && jobs.contains_key(&key) {
                return Err("Too many active YouTube extractions.".into());
            }
            if kind == "media"
                && claim_preparation(&mut jobs, root, id, request, command["fresh"] == true)
            {
                return Ok(json!({"done":false,"prepared":true}));
            }
            if preparing && let Some(previous) = jobs.remove(&key) {
                previous.cancel();
            }
            if jobs.len() >= 8 {
                return Err("Too many active YouTube extractions.".into());
            }
            if WORKERS
                .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < 4).then_some(count + 1)
                })
                .is_err()
            {
                return Err("Too many active YouTube extractions.".into());
            }
            let job = Arc::new(Job {
                at: Instant::now(),
                prepared_video: selected.clone(),
                user_agent: user_agent.into(),
                session: Mutex::new(Session {
                    cookies: HashMap::new(),
                    since: Instant::now(),
                }),
                cancelled: AtomicBool::new(false),
                result: Mutex::new(None),
                runtime: Mutex::new(Runtime {
                    sequence: 0,
                    task: None,
                    reply: None,
                }),
                wake: Condvar::new(),
            });
            jobs.insert(key.clone(), job.clone());
            let (root, id, kind, track, audio_key, fresh) = (
                root.to_owned(),
                id.to_owned(),
                kind.to_owned(),
                track.to_owned(),
                audio_key.to_owned(),
                command["fresh"] == true,
            );
            let worker = job;
            if thread::Builder::new()
                .name("youtube-extraction".into())
                .spawn(move || {
                    struct Running;
                    impl Drop for Running {
                        fn drop(&mut self) {
                            WORKERS.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    let _running = Running;
                    let result = match kind.as_str() {
                        "media" => catalog(&worker, &root, &id, fresh),
                        "chapters" => chapter_catalog(&worker, &root, &id, fresh),
                        _ => transcript(&worker, &root, &id, &track, &audio_key, fresh),
                    };
                    if let Ok(mut output) = worker.result.lock() {
                        *output = Some(result);
                    }
                })
                .is_err()
            {
                WORKERS.fetch_sub(1, Ordering::AcqRel);
                jobs.remove(&key);
                return Err("Could not start YouTube extraction.".into());
            }
            Ok(json!({"done":false}))
        }
        "extract_poll" => {
            let job = jobs.get(&key).ok_or("This extraction has expired.")?;
            job.check()?;
            let result = job.result.lock().map_err(|_| "Extractor busy.")?;
            if let Some(result) = result.as_ref() {
                return Ok(match result {
                    Ok(value) => json!({"done":true,"value":value}),
                    Err(error) => json!({"done":true,"error":error}),
                });
            }
            drop(result);
            let runtime = job
                .runtime
                .lock()
                .map_err(|_| "Token runtime unavailable.")?;
            Ok(
                json!({"done":false,"task":runtime.task.as_ref().filter(|task| task["stage"] != page::STAGE)}),
            )
        }
        "extract_page" => {
            let job = jobs.get(&key).ok_or("This extraction has expired.")?;
            job.check()?;
            page::reply(command, root, job)
        }
        "extract_token" => {
            let job = jobs.get(&key).ok_or("This extraction has expired.")?;
            job.check()?;
            let mut runtime = job
                .runtime
                .lock()
                .map_err(|_| "Token runtime unavailable.")?;
            if runtime
                .task
                .as_ref()
                .is_none_or(|task| task["taskId"] != command["taskId"])
            {
                return Err("The extraction token request changed.".into());
            }
            let result = string(command, "value")?;
            if result.len() > 2_000_000 || result.is_empty() || result.chars().any(char::is_control)
            {
                return Err("Invalid extraction token response.".into());
            }
            runtime.task = None;
            runtime.reply = Some(Ok(result.into()));
            drop(runtime);
            job.wake.notify_all();
            Ok(Value::Null)
        }
        "extract_release" => {
            if let Some(job) = jobs.remove(&key) {
                job.cancel();
            }
            drop(jobs);
            crate::changed("extraction-token");
            Ok(Value::Null)
        }
        _ => Err("Unsupported extraction operation.".into()),
    }
}

/// Shared workflows own extraction and cancellation; JS only executes token tasks.
pub(crate) fn resolve(
    root: &str,
    kind: &str,
    id: &str,
    track: &str,
    audio: &str,
    fresh: bool,
    check: &impl Fn() -> Result<()>,
) -> Result<Value> {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(|_| "Could not create extraction identity.")?;
    let request = format!("worker:{}", crate::hex(&nonce));
    check()?;
    crate::request(
        &json!({"op":"extract_start","requestId":request,"kind":kind,
        "videoId":id,"trackId":track,"audioKey":audio,"fresh":fresh}),
        root,
    )?;
    let result = (|| {
        loop {
            check()?;
            let result = crate::request(&json!({"op":"extract_poll","requestId":request}), root)?;
            if result["done"] == true {
                return if let Some(error) = result["error"].as_str() {
                    Err(error.into())
                } else {
                    Ok(result["value"].clone())
                };
            }
            thread::sleep(Duration::from_millis(100));
        }
    })();
    let _ = crate::request(&json!({"op":"extract_release","requestId":request}), root);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, sync::atomic::AtomicUsize};

    #[test]
    fn translation_wait_is_cancellable_and_has_its_own_job_budget() {
        let job = Arc::new(Job {
            at: Instant::now()
                .checked_sub(Duration::from_secs(100))
                .unwrap(),
            prepared_video: None,
            user_agent: UA.into(),
            session: Mutex::new(Session {
                cookies: HashMap::new(),
                since: Instant::now(),
            }),
            cancelled: AtomicBool::new(false),
            result: Mutex::new(None),
            runtime: Mutex::new(Runtime {
                sequence: 0,
                task: None,
                reply: None,
            }),
            wake: Condvar::new(),
        });
        assert!(job.check().is_ok());
        assert!(job.wait_until(Instant::now()).is_ok());
        let waiting = job.clone();
        let worker =
            thread::spawn(move || waiting.wait_until(Instant::now() + TRANSLATION_SESSION_AGE));
        thread::sleep(Duration::from_millis(10));
        job.cancel();
        assert_eq!(worker.join().unwrap().unwrap_err(), "Extraction cancelled.");
    }

    #[test]
    fn caption_addresses_preserve_signatures_and_enforce_video_and_origin() {
        let raw = "https://www.youtube.com/api/timedtext?v=abcdefghijk&signature=A%2fb+C&lang=en&fmt=srv3&xosf=1";
        let address = caption_address(raw, "abcdefghijk", Some("proof+/=")).unwrap();
        assert!(address.contains("signature=A%2fb+C&lang=en"));
        assert!(!address.contains("xosf="));
        let url = Url::parse(&address).unwrap();
        let query: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["fmt"], "json3");
        assert_eq!(query["pot"], "proof+/=");
        assert_eq!(query["c"], "WEB");
        assert!(caption_address(raw, "0123456789a", None).is_err());
        assert!(caption_address(&format!("{raw}&tlang=fr"), "abcdefghijk", None).is_err());
        for address in [
            "http://www.youtube.com/api/timedtext",
            "https://youtube.com.evil.test/api/timedtext",
            "https://www.youtube.com:444/api/timedtext",
            "https://user@www.youtube.com/api/timedtext",
            "https://127.0.0.1/watch",
        ] {
            assert!(allowed_address(address, false).is_err(), "{address}");
        }
        assert!(allowed_address("https://www.google.com/js/bg/test.js", true).is_ok());
        assert!(allowed_address("https://www.google.com/api/private", true).is_err());
    }

    #[test]
    fn token_challenges_decode_scrambled_and_inline_responses() {
        let challenge = json!([null, ["interpreter"], [], null, "program", "botguard"]);
        assert_eq!(parse_challenge(&json!([challenge])).unwrap(), challenge);
        let encoded = URL_SAFE_NO_PAD.encode(
            challenge
                .to_string()
                .bytes()
                .map(|b| b.wrapping_sub(97))
                .collect::<Vec<_>>(),
        );
        assert_eq!(parse_challenge(&json!([null, encoded])).unwrap(), challenge);
        assert!(parse_challenge(&json!([null, "!bad"])).is_err());
        assert!(parse_challenge(&json!({})).is_err());
    }

    const ID: &str = "abcdefghijk";
    const SOURCE: &str = "https://www.youtube.com/api/timedtext?v=abcdefghijk&lang=en&exp=xpe&signature=never-persist";
    struct Http {
        count: Arc<AtomicUsize>,
        unverified_status: Arc<AtomicUsize>,
        translated_status: Arc<Mutex<VecDeque<usize>>>,
    }
    impl ExtractionHttp for Http {
        fn request(&self, request: ExtractionRequest) -> ExtractionReply {
            self.count.fetch_add(1, Ordering::Relaxed);
            let url = Url::parse(&request.url).unwrap();
            let mut headers = vec![];
            let body = match url.path() {
                "/watch" => {
                    assert_eq!(request.method, "GET");
                    headers.push(ExtractionHeader {
                        name: "Set-Cookie".into(),
                        value: "YSC=guest-only; Secure; HttpOnly; Path=/".into(),
                    });
                    headers.push(ExtractionHeader {
                        name: "Set-Cookie".into(),
                        value: "VISITOR_INFO1_LIVE=guest-visitor; Secure; HttpOnly; Path=/".into(),
                    });
                    format!(
                        "ytcfg.set({});var ytInitialPlayerResponse = {};",
                        json!({"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"fixture","visitorData":"never-persist-visitor"}}}),
                        json!({"videoDetails":{"videoId":ID},"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[{"vssId":".en","languageCode":"en","name":{"simpleText":"English"},"baseUrl":SOURCE,"isTranslatable":true}],
                                "audioTracks":[{"audioTrackId":"en.4","captionTrackIndices":[0],"defaultCaptionTrackIndex":0}],
                                "translationLanguages":[{"languageCode":"es","languageName":{"simpleText":"Spanish"}}]}}})
                    )
                }
                "/youtubei/v1/player" => {
                    assert!(
                        request.headers.iter().any(|header| header.name == "Cookie"
                            && header.value.contains("YSC=guest-only"))
                    );
                    let input: Value = serde_json::from_str(&request.body).unwrap();
                    assert_eq!(input["videoId"], ID);
                    assert_eq!(input["context"]["client"]["clientName"], "VISIONOS");
                    json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":ID,"title":"Fixture","author":"Author","lengthSeconds":"10"},
                            "streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?sig=never-persist","contentLength":"2048","bitrate":64000},
                            {"itag":141,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?n=unsolved","contentLength":"2048"}]}}).to_string()
                }
                "/api/jnn/v1/Create" => {
                    assert_eq!(
                        serde_json::from_str::<Value>(&request.body).unwrap(),
                        json!([REQUEST_KEY])
                    );
                    json!([[null, ["interpreter"], [], null, "program", "botguard"]]).to_string()
                }
                "/api/jnn/v1/GenerateIT" => {
                    assert_eq!(
                        serde_json::from_str::<Value>(&request.body).unwrap(),
                        json!([REQUEST_KEY, "snapshot"])
                    );
                    json!(["integrity", "43200", null, null]).to_string()
                }
                "/api/timedtext" => {
                    let query: HashMap<_, _> = url.query_pairs().collect();
                    let status = if !query.contains_key("pot") {
                        self.unverified_status.load(Ordering::Relaxed)
                    } else if query.contains_key("tlang") {
                        self.translated_status
                            .lock()
                            .unwrap()
                            .pop_front()
                            .unwrap_or(200)
                    } else {
                        200
                    };
                    if status != 200 {
                        return ExtractionReply {
                            status: i32::try_from(status).unwrap(),
                            headers,
                            body: Vec::new(),
                        };
                    }
                    assert_eq!(query["pot"], "proof");
                    assert_eq!(query["potc"], "1");
                    assert_eq!(query["c"], "WEB");
                    assert_eq!(query["signature"], "never-persist");
                    if let Some(language) = query.get("tlang") {
                        assert_eq!(language, "es");
                        return ExtractionReply { status:200, headers, body:json!({"events":[{"tStartMs":0,"dDurationMs":2000,"segs":[{"utf8":"Hola","tOffsetMs":0,"acAsrConf":0}]}]}).to_string().into_bytes() };
                    }
                    json!({"events":[{"tStartMs":3000,"dDurationMs":1000,"segs":[{"utf8":" Ｗorld "}]},{"tStartMs":0,"dDurationMs":2000,"segs":[{"utf8":"Hello\n there"}]}]}).to_string()
                }
                path => panic!("Unexpected request / page fallback: {path}"),
            };
            ExtractionReply {
                status: 200,
                body: body.into_bytes(),
                headers,
            }
        }
    }

    fn test_execute(root: &Path, command: Value) -> Value {
        serde_json::from_str(&crate::execute(
            command.to_string(),
            root.to_string_lossy().into(),
        ))
        .unwrap()
    }

    fn await_job(root: &Path, request: &str, solve: bool) -> Result<Value> {
        let call = |command| test_execute(root, command);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "Extractor job stalled");
            let result = call(json!({"op":"extract_poll","requestId":request}));
            assert_eq!(result["ok"], true, "{result}");
            let value = &result["value"];
            if value["done"] == true {
                return match value["error"].as_str() {
                    Some(error) => Err(error.into()),
                    None => Ok(value["value"].clone()),
                };
            }
            if !value["task"].is_null() {
                if !solve {
                    return Ok(value["task"].clone());
                }
                let task = &value["task"];
                assert_eq!(
                    call(
                        json!({"op":"extract_token","requestId":request,"taskId":999,"value":"stale"})
                    )["ok"],
                    false
                );
                let reply = if task["stage"] == "snapshot" {
                    "snapshot"
                } else {
                    assert_eq!(task["videoId"], ID);
                    "proof"
                };
                assert_eq!(
                    call(
                        json!({"op":"extract_token","requestId":request,"taskId":task["taskId"],"value":reply})
                    )["ok"],
                    true
                );
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn set_translation_ready(root: &Path, remaining: Duration) -> Instant {
        let ready_at = Instant::now() + remaining;
        let mut players = PLAYERS.get().unwrap().lock().unwrap();
        let cached = players
            .iter_mut()
            .find(|p| p.id == ID && p.root == root.to_string_lossy())
            .unwrap();
        cached.session.since = ready_at.checked_sub(TRANSLATION_SESSION_AGE).unwrap();
        drop(players);
        ready_at
    }

    #[test]
    fn standalone_jobs_extract_media_and_protected_captions_without_page_fallback() {
        let count = Arc::new(AtomicUsize::new(0));
        let unverified_status = Arc::new(AtomicUsize::new(403));
        let translated_status = Arc::new(Mutex::new(VecDeque::new()));
        install_extraction_http(Box::new(Http {
            count: count.clone(),
            unverified_status: unverified_status.clone(),
            translated_status: translated_status.clone(),
        }));
        let root = std::env::temp_dir().join(format!("govue-extraction-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let call = |command| test_execute(&root, command);
        let start = |kind: &str, request: &str, fresh: bool| {
            assert_eq!(
                call(
                    json!({"op":"extract_start","kind":kind,"requestId":request,"videoId":ID,"trackId":".en","fresh":fresh})
                )["ok"],
                true
            );
        };
        let wait = |request, solve| await_job(&root, request, solve);
        start("media", "media:1", true);
        let catalog = wait("media:1", true).unwrap();
        assert_eq!(catalog["streams"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["subtitles"][0]["id"], ".en");
        start("transcript", "captions:1", false);
        let transcript = wait("captions:1", true).unwrap();
        assert_eq!(transcript["segments"][0]["text"], "Hello\nthere");
        assert_eq!(transcript["segments"][1]["text"], "World");
        let requests = count.load(Ordering::Relaxed);
        start("transcript", "cached:1", false);
        assert_eq!(wait("cached:1", true).unwrap(), transcript);
        assert_eq!(count.load(Ordering::Relaxed), requests);
        let files = fs::read_dir(&root)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        let stored = fs::read_to_string(files[0].path()).unwrap();
        for secret in [
            "never-persist",
            "guest-only",
            "guest-visitor",
            "captionSource",
            "baseUrl",
            "proof",
            "snapshot",
            "integrity",
        ] {
            assert!(!stored.contains(secret), "Persisted {secret}");
        }
        // A successful translation from a brand-new session must complete
        // within the fixture's 5s deadline, without the former 75s wait.
        set_translation_ready(&root, TRANSLATION_SESSION_AGE);
        let before_immediate = count.load(Ordering::Relaxed);
        assert_eq!(
            call(
                json!({"op":"extract_start","kind":"transcript","requestId":"translated:immediate","videoId":ID,"trackId":"translate:es:.en"})
            )["ok"],
            true
        );
        assert_eq!(
            wait("translated:immediate", true).unwrap()["segments"][0]["text"],
            "Hola"
        );
        assert_eq!(count.load(Ordering::Relaxed) - before_immediate, 3);
        call(json!({"op":"extract_release","requestId":"translated:immediate"}));

        // Retry a young session's 429 once, then report a repeated limit.
        let ready_at = set_translation_ready(&root, Duration::from_millis(500));
        *translated_status.lock().unwrap() = VecDeque::from([429, 429]);
        let before_translation = count.load(Ordering::Relaxed);
        assert_eq!(
            call(
                json!({"op":"extract_start","kind":"transcript","requestId":"translated:limited","videoId":ID,"trackId":"translate:es:.en","audioKey":"en.4","fresh":true})
            )["ok"],
            true
        );
        assert!(
            wait("translated:limited", true)
                .unwrap_err()
                .contains("temporarily limiting translated captions")
        );
        assert!(Instant::now() >= ready_at);
        assert_eq!(count.load(Ordering::Relaxed) - before_translation, 6);
        assert_eq!(
            call(json!({"op":"restore","videoId":ID,"trackId":".en"}))["value"],
            transcript
        );
        call(json!({"op":"extract_release","requestId":"translated:limited"}));
        // Recover after a 429, preserving session age across a refresh even
        // when the watch document sends the same visitor cookie again.
        let ready_at = set_translation_ready(&root, Duration::from_millis(500));
        *translated_status.lock().unwrap() = VecDeque::from([429, 200]);
        let before_recovery = count.load(Ordering::Relaxed);
        assert_eq!(
            call(
                json!({"op":"extract_start","kind":"transcript","requestId":"translated:1","videoId":ID,"trackId":"translate:es:.en","audioKey":"en.4","fresh":true})
            )["ok"],
            true
        );
        let translated = wait("translated:1", true).unwrap();
        assert!(Instant::now() >= ready_at);
        assert_eq!(count.load(Ordering::Relaxed) - before_recovery, 6);
        assert_eq!(translated["segments"][0]["text"], "Hola");
        assert_eq!(translated["trackId"], "translate:es:.en");
        assert_eq!(translated["trackMetadata"]["defaultAudioTrackId"], "en.4");
        assert_eq!(translated["segments"][0]["words"][0]["start"], 0.0);
        assert_eq!(
            call(json!({"op":"restore","videoId":ID,"trackId":"translate:es:.en"}))["value"],
            translated
        );
        // Other errors never wait. A mature session's 429 does not trigger an
        // identical request with no age left to wait for.
        for status in [429, 403, 500] {
            set_translation_ready(
                &root,
                if status == 429 {
                    Duration::ZERO
                } else {
                    TRANSLATION_SESSION_AGE
                },
            );
            *translated_status.lock().unwrap() = VecDeque::from([status]);
            let before = count.load(Ordering::Relaxed);
            assert_eq!(
                call(
                    json!({"op":"extract_start","kind":"transcript","requestId":"translated:error","videoId":ID,"trackId":"translate:es:.en","fresh":true})
                )["ok"],
                true
            );
            let error = wait("translated:error", true).unwrap_err();
            assert!(error.contains(if status == 429 {
                "temporarily limiting"
            } else if status == 403 {
                "HTTP 403"
            } else {
                "HTTP 500"
            }));
            assert_eq!(count.load(Ordering::Relaxed) - before, 5);
            call(json!({"op":"extract_release","requestId":"translated:error"}));
        }
        start("transcript", "cancelled:1", true);
        assert_eq!(wait("cancelled:1", false).unwrap()["stage"], "snapshot");
        assert_eq!(call(json!({"op":"info"}))["ok"], true);
        assert_eq!(
            call(json!({"op":"extract_release","requestId":"cancelled:1"}))["ok"],
            true
        );
        assert_eq!(
            call(json!({"op":"extract_token","requestId":"cancelled:1","taskId":1,"value":"late"}))
                ["ok"],
            false
        );
        for request in ["media:1", "captions:1", "cached:1", "translated:1"] {
            call(json!({"op":"extract_release","requestId":request}));
        }
        // Missing exp hints must still allow exactly one verification attempt;
        // unrelated server failures must not start a challenge.
        for status in [403, 429, 500] {
            let track = format!(".probe{status}");
            {
                let mut players = PLAYERS.get().unwrap().lock().unwrap();
                let cached = players
                    .iter_mut()
                    .find(|p| p.id == ID && p.root == root.to_string_lossy())
                    .unwrap();
                cached.data["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"][0]["vssId"] =
                    json!(track);
                cached.data["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"][0]["baseUrl"] =
                    json!(SOURCE.replace("&exp=xpe", ""));
                drop(players);
            }
            unverified_status.store(status, Ordering::Relaxed);
            let before = count.load(Ordering::Relaxed);
            assert_eq!(
                call(
                    json!({"op":"extract_start","kind":"transcript","requestId":"unmarked","videoId":ID,"trackId":track})
                )["ok"],
                true
            );
            let result = wait("unmarked", true);
            if status == 500 {
                assert!(result.unwrap_err().contains("HTTP 500"));
                assert_eq!(count.load(Ordering::Relaxed) - before, 1);
            } else {
                assert_eq!(result.unwrap()["segments"][0]["text"], "Hello\nthere");
                assert_eq!(count.load(Ordering::Relaxed) - before, 4);
            }
            call(json!({"op":"extract_release","requestId":"unmarked"}));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
