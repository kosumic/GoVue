//! Standalone YouTube extraction. Rust owns HTTP, protocol state, validation and
//! caches. The platform transports bounded HTTPS bytes; an isolated renderer
//! supplies only BotGuard snapshot/token strings. No YouTube page is executed.
use crate::{MAX_BYTES, Result, Track, Transcript, clean, json, media, string, video_id};
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

const ORIGIN: &str = "https://www.youtube.com";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";
const LIFETIME: Duration = Duration::from_secs(90);
static HTTP: OnceLock<Box<dyn ExtractionHttp>> = OnceLock::new();
type Jobs = HashMap<(String, String), Arc<Job>>;
static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static PLAYERS: OnceLock<Mutex<VecDeque<Player>>> = OnceLock::new();

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
struct Player {
    root: String,
    id: String,
    at: Instant,
    data: Value,
    chapters: Vec<Value>,
    cookies: HashMap<String, String>,
}
struct Runtime {
    sequence: u32,
    task: Option<Value>,
    reply: Option<Result<String>>,
}
struct Job {
    at: Instant,
    user_agent: String,
    cookies: Mutex<HashMap<String, String>>,
    cancelled: AtomicBool,
    result: Mutex<Option<Result<Value>>>,
    runtime: Mutex<Runtime>,
    wake: Condvar,
}
impl Job {
    fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Extraction cancelled.".into())
        } else if self.at.elapsed() >= LIFETIME {
            Err("YouTube extraction timed out.".into())
        } else {
            Ok(())
        }
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.wake.notify_all();
    }
    fn evaluate(&self, mut task: Value) -> Result<String> {
        self.check()?;
        let mut state = self
            .runtime
            .lock()
            .map_err(|_| "Token runtime unavailable.")?;
        state.sequence += 1;
        task["taskId"] = json!(state.sequence);
        state.task = Some(task);
        state.reply = None;
        loop {
            self.check()?;
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

fn fetch(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
) -> Result<String> {
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
        let cookies = job
            .cookies
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        if !cookies.is_empty() {
            headers.push(ExtractionHeader {
                name: "Cookie".into(),
                value: cookies
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
        max_bytes: MAX_BYTES as u32,
    };
    let reply = if let Some(http) = HTTP.get() {
        http.request(request)
    } else {
        // Host CLI uses the same bounded contract as Android/iOS.
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(15))
            .build();
        let mut call = agent.request(&request.method, &request.url);
        for header in request.headers {
            call = call.set(&header.name, &header.value);
        }
        let response = if method == "POST" {
            call.send_string(&request.body)
        } else {
            call.call()
        };
        match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => {
                let status = i32::from(response.status());
                let headers = response
                    .all("set-cookie")
                    .into_iter()
                    .map(|value| ExtractionHeader {
                        name: "Set-Cookie".into(),
                        value: value.into(),
                    })
                    .collect();
                let mut body = Vec::new();
                response
                    .into_reader()
                    .take(MAX_BYTES as u64 + 1)
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
        let mut cookies = job
            .cookies
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
                    && (cookies.len() < 20 || cookies.contains_key(name))
                {
                    cookies.insert(name.into(), value.into());
                }
            }
        }
    }
    if reply.body.len() > MAX_BYTES {
        return Err("YouTube extraction response is too large.".into());
    }
    if reply.status != 200 {
        return Err(format!(
            "YouTube extraction request failed (HTTP {}).",
            reply.status
        ));
    }
    String::from_utf8(reply.body).map_err(|_| "Unreadable YouTube extraction response.".into())
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
fn player(job: &Job, root: &str, id: &str, fresh: bool) -> Result<Player> {
    if !fresh {
        let players = PLAYERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Extractor cache unavailable.")?;
        if let Some(cached) = players
            .iter()
            .find(|p| p.root == root && p.id == id && p.at.elapsed() < LIFETIME)
        {
            *job.cookies
                .lock()
                .map_err(|_| "Extraction session unavailable.")? = cached.cookies.clone();
            return Ok(cached.clone());
        }
    }
    // Fetch and parse JSON configuration as data. This does not initialize a
    // browser, run page scripts, click controls, or capture player requests.
    let html = fetch(
        job,
        &format!("{ORIGIN}/watch?v={id}&hl=en"),
        None,
        false,
        &[],
    )?;
    let config = page_json(&html, "ytcfg.set(")
        .ok_or("YouTube did not provide extraction configuration.")?;
    let context = config["INNERTUBE_CONTEXT"].clone();
    let mut client = context["client"].clone();
    if !client.is_object() {
        return Err("YouTube did not provide a client context.".into());
    }
    for (key, value) in json!({"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","userAgent":UA}).as_object().unwrap() {
        client[key] = value.clone();
    }
    let mut native_context = context.clone();
    native_context["client"] = client;
    let mut data = json_response(&fetch(
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
    // Web subtitle addresses require WebPO tokens. Keep the web client's
    // caption metadata paired with that token context, independently of the
    // native client used for direct media streams.
    if let Some(web) = page_json(&html, "var ytInitialPlayerResponse = ")
        .or_else(|| page_json(&html, "ytInitialPlayerResponse="))
        .filter(|web| web["videoDetails"]["videoId"] == id && web["captions"].is_object())
    {
        data["captions"] = web["captions"].clone();
    }
    let initial = page_json(&html, "var ytInitialData = ")
        .or_else(|| page_json(&html, "ytInitialData="))
        .unwrap_or(Value::Null);
    let result = Player {
        root: root.into(),
        id: id.into(),
        at: Instant::now(),
        data,
        chapters: chapters(&initial),
        cookies: job
            .cookies
            .lock()
            .map_err(|_| "Extraction session unavailable.")?
            .clone(),
    };
    job.check()?;
    let mut players = PLAYERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor cache unavailable.")?;
    players.retain(|p| p.at.elapsed() < LIFETIME && !(p.root == root && p.id == id));
    players.push_front(result.clone());
    players.truncate(3);
    Ok(result)
}
fn tracks(player: &Player) -> Vec<Track> {
    player.data["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .enumerate()
        .map(|(index, track)| Track {
            id: track["vssId"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!("{}:{index}", track["languageCode"].as_str().unwrap_or(""))
                }),
            label: clean(&text(&track["name"])),
            language: track["languageCode"].as_str().unwrap_or("").into(),
            automatic: track["kind"] == "asr",
        })
        .collect()
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
            .unwrap()
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
fn transcript(job: &Job, root: &str, id: &str, track: &str, fresh: bool) -> Result<Value> {
    if !fresh {
        let mut command = json!({"op":"restore","videoId":id});
        if track != "default" {
            command["trackId"] = json!(track);
        }
        let saved = crate::dispatch(&command.to_string(), root)?;
        if !saved.is_null() && (track == "default" || saved["trackId"] == track) {
            return Ok(saved);
        }
    }
    let player = player(job, root, id, fresh)?;
    let tracks = tracks(&player);
    let list = &player.data["captions"]["playerCaptionsTracklistRenderer"];
    let index = if track == "default" {
        let audio = list["defaultAudioTrackIndex"].as_u64().unwrap_or(0) as usize;
        list["audioTracks"][audio]["defaultCaptionTrackIndex"]
            .as_u64()
            .unwrap_or(0) as usize
    } else {
        tracks
            .iter()
            .position(|t| t.id == track)
            .ok_or("This caption language is unavailable.")?
    };
    let selected = tracks
        .get(index)
        .ok_or("This video has no caption tracks.")?;
    let raw = string(&list["captionTracks"][index], "baseUrl")?;
    let address = caption_address(raw, id, None)?;
    let requires_token = Url::parse(&address)
        .unwrap()
        .query_pairs()
        .any(|(key, value)| key == "exp" && matches!(value.as_ref(), "xpe" | "xpv"));
    let mut source = if requires_token {
        String::new()
    } else {
        fetch(job, &address, None, false, &[])?
    };
    if source.trim().is_empty() {
        let proof = token(job, &player)?;
        source = fetch(
            job,
            &caption_address(raw, id, Some(&proof))?,
            None,
            false,
            &[],
        )?;
    }
    if source.trim().is_empty() {
        return Err("YouTube returned no caption text after verification.".into());
    }
    let segments = crate::parse_captions(&source)?;
    let mut data = Transcript {
        video_id: id.into(),
        title: text(&player.data["videoDetails"]["title"]),
        language: selected.label.clone(),
        track_id: selected.id.clone(),
        tracks,
        segments,
    };
    crate::normalize(&mut data)?;
    job.check()?;
    let _guard = crate::REQUEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::remember(crate::cache_path(root, id, &data.track_id)?, &data);
    let _ = crate::save(root, &data);
    Ok(json!(data))
}
fn catalog(job: &Job, root: &str, id: &str, fresh: bool) -> Result<Value> {
    if !fresh && let Some(cached) = media::cached_catalog(root, id) {
        return Ok(cached);
    }
    let player = player(job, root, id, fresh)?;
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
                .unwrap()
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

pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let request = string(command, "requestId")?;
    if request.is_empty()
        || request.len() > 150
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err("Invalid extraction request identity.".into());
    }
    let key = (root.to_owned(), request.to_owned());
    let mut jobs = JOBS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor busy.")?;
    jobs.retain(|_, job| {
        if job.at.elapsed() >= LIFETIME {
            job.cancel();
            false
        } else {
            true
        }
    });
    match string(command, "op")? {
        "extract_start" => {
            let id = string(command, "videoId")?;
            let kind = string(command, "kind")?;
            let track = command["trackId"].as_str().unwrap_or("default");
            let user_agent = command["userAgent"]
                .as_str()
                .filter(|agent| !agent.is_empty())
                .unwrap_or(UA);
            if user_agent.len() > 1000 || user_agent.chars().any(char::is_control) {
                return Err("Invalid extraction browser identity.".into());
            }
            if !video_id(id) || !matches!(kind, "media" | "transcript") || track.len() > 512 {
                return Err("Invalid extraction request.".into());
            }
            if jobs.contains_key(&key) || jobs.len() >= 8 {
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
                user_agent: user_agent.into(),
                cookies: Mutex::new(HashMap::new()),
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
            let (root, id, kind, track, fresh) = (
                root.to_owned(),
                id.to_owned(),
                kind.to_owned(),
                track.to_owned(),
                command["fresh"] == true,
            );
            let worker = job.clone();
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
                    let result = if kind == "media" {
                        catalog(&worker, &root, &id, fresh)
                    } else {
                        transcript(&worker, &root, &id, &track, fresh)
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
            let runtime = job
                .runtime
                .lock()
                .map_err(|_| "Token runtime unavailable.")?;
            Ok(json!({"done":false,"task":runtime.task}))
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
            job.wake.notify_all();
            Ok(Value::Null)
        }
        "extract_release" => {
            if let Some(job) = jobs.remove(&key) {
                job.cancel();
            }
            Ok(Value::Null)
        }
        _ => Err("Unsupported extraction operation.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::AtomicUsize};

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

    #[test]
    fn standalone_jobs_extract_media_and_protected_captions_without_page_fallback() {
        const ID: &str = "abcdefghijk";
        const SOURCE: &str = "https://www.youtube.com/api/timedtext?v=abcdefghijk&lang=en&exp=xpe&signature=never-persist";
        struct Http(Arc<AtomicUsize>);
        impl ExtractionHttp for Http {
            fn request(&self, request: ExtractionRequest) -> ExtractionReply {
                self.0.fetch_add(1, Ordering::Relaxed);
                let url = Url::parse(&request.url).unwrap();
                let mut headers = vec![];
                let body = match url.path() {
                    "/watch" => {
                        assert_eq!(request.method, "GET");
                        headers.push(ExtractionHeader {
                            name: "Set-Cookie".into(),
                            value: "YSC=guest-only; Secure; HttpOnly; Path=/".into(),
                        });
                        format!(
                            "ytcfg.set({});var ytInitialPlayerResponse = {};",
                            json!({"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"fixture","visitorData":"never-persist-visitor"}}}),
                            json!({"videoDetails":{"videoId":ID},"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[{"vssId":".en","languageCode":"en","name":{"simpleText":"English"},"baseUrl":SOURCE}]}}})
                        )
                    }
                    "/youtubei/v1/player" => {
                        assert!(request.headers.iter().any(|header| header.name == "Cookie"
                            && header.value.contains("YSC=guest-only")));
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
                        json!([[null, ["interpreter"], [], null, "program", "botguard"]])
                            .to_string()
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
                        assert_eq!(query["pot"], "proof");
                        assert_eq!(query["potc"], "1");
                        assert_eq!(query["c"], "WEB");
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
        let count = Arc::new(AtomicUsize::new(0));
        install_extraction_http(Box::new(Http(count.clone())));
        let root =
            std::env::temp_dir().join(format!("lingoplay-extraction-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let call = |command: Value| -> Value {
            serde_json::from_str(&crate::execute(
                command.to_string(),
                root.to_string_lossy().into(),
            ))
            .unwrap()
        };
        let start = |kind: &str, request: &str, fresh: bool| {
            assert_eq!(
                call(
                    json!({"op":"extract_start","kind":kind,"requestId":request,"videoId":ID,"trackId":".en","fresh":fresh})
                )["ok"],
                true
            );
        };
        let wait = |request: &str, solve: bool| -> Value {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                assert!(Instant::now() < deadline, "Extractor job stalled");
                let result = call(json!({"op":"extract_poll","requestId":request}));
                assert_eq!(result["ok"], true, "{result}");
                let value = &result["value"];
                if value["done"] == true {
                    assert!(value["error"].is_null(), "{result}");
                    return value["value"].clone();
                }
                if !value["task"].is_null() {
                    if !solve {
                        return value["task"].clone();
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
        };
        start("media", "media:1", true);
        let catalog = wait("media:1", true);
        assert_eq!(catalog["streams"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["subtitles"][0]["id"], ".en");
        start("transcript", "captions:1", false);
        let transcript = wait("captions:1", true);
        assert_eq!(transcript["segments"][0]["text"], "Hello there");
        assert_eq!(transcript["segments"][1]["text"], "World");
        let requests = count.load(Ordering::Relaxed);
        start("transcript", "cached:1", false);
        assert_eq!(wait("cached:1", true), transcript);
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
            "captionSource",
            "baseUrl",
            "proof",
            "snapshot",
            "integrity",
        ] {
            assert!(!stored.contains(secret), "Persisted {secret}");
        }
        start("transcript", "cancelled:1", true);
        assert_eq!(wait("cancelled:1", false)["stage"], "snapshot");
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
        for request in ["media:1", "captions:1", "cached:1"] {
            call(json!({"op":"extract_release","requestId":request}));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
