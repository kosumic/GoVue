//! Standalone YouTube extraction. Rust owns HTTP, protocol state, validation and
//! caches. The platform transports bounded HTTPS bytes; an isolated renderer
//! supplies only BotGuard snapshot/token strings. No YouTube page is executed.
use crate::{MAX_BYTES, Result, Track, Transcript, json, media};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

mod network;
mod page;
mod player_cache;
use network::HTTP;
use player_cache::Player;

const ORIGIN: &str = "https://www.youtube.com";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";
const CACHE_LIFETIME: Duration = Duration::from_secs(90);
const JOB_LIFETIME: Duration = Duration::from_secs(180);
const PREPARATION_REQUEST: &str = "prepare:media";
// On a translated-caption 429, retry once after the guest session has aged.
// Empirical workaround: https://github.com/yt-dlp/yt-dlp/pull/15709.
const TRANSLATION_SESSION_AGE: Duration = Duration::from_secs(75);
#[derive(Clone, Eq, Hash, PartialEq)]
struct RequestKey {
    root: String,
    request: String,
}
impl RequestKey {
    fn new(root: &str, request: &str) -> Self {
        Self {
            root: root.into(),
            request: request.into(),
        }
    }
}
type Jobs = HashMap<RequestKey, Arc<Job>>;
static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
static WORKERS: AtomicUsize = AtomicUsize::new(0);

/// An extraction request or response header, retained only in memory.
#[derive(uniffi::Record)]
pub struct ExtractionHeader {
    pub name: String,
    pub value: String,
}
/// A bounded HTTPS request planned and validated by the Rust extractor.
#[derive(uniffi::Record)]
pub struct ExtractionRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<ExtractionHeader>,
    pub body: String,
    /// Maximum response bytes the adapter may read.
    pub max_bytes: u32,
    /// Allows returning only the bounded prefix instead of requiring the full body.
    pub prefix_only: bool,
}
/// Response bytes and headers for Rust to validate before parsing.
#[derive(uniffi::Record)]
pub struct ExtractionReply {
    /// HTTP status, or zero if no response was obtained.
    pub status: i32,
    pub body: Vec<u8>,
    pub headers: Vec<ExtractionHeader>,
}
/// Blocking native HTTPS effects; extraction state and parsing stay in Rust.
#[uniffi::export(callback_interface)]
pub trait ExtractionHttp: Send + Sync {
    /// Called on a Rust worker. No redirects, automatic cookie jar, logging or disk cache.
    fn request(&self, request: ExtractionRequest) -> ExtractionReply;
}
/// Installs the extraction adapter. The first adapter remains installed for this process.
#[uniffi::export]
pub fn install_extraction_http(backend: Box<dyn ExtractionHttp>) {
    let _ = HTTP.set(backend);
}

#[derive(Clone)]
struct Session {
    cookies: HashMap<String, String>,
    since: Instant,
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum CaptionVerification {
    Unverified,
    Verified,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CaptionKind {
    Original,
    Translated,
}

fn caption_source(
    job: &Job,
    url: &str,
    verification: CaptionVerification,
    kind: CaptionKind,
) -> Result<String> {
    let mut reply = network::fetch_response(job, url, None, false, &[])?;
    if verification == CaptionVerification::Unverified && matches!(reply.status, 403 | 429) {
        // An unmarked caption URL can still require verification. Allow the
        // existing token flow once, without retrying the same request in a loop.
        return Ok(String::new());
    }
    if reply.status == 429 && kind == CaptionKind::Translated {
        let since = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?
            .since;
        let ready_at = since + TRANSLATION_SESSION_AGE;
        if ready_at > Instant::now() {
            job.wait_until(ready_at)?;
            reply = network::fetch_response(job, url, None, false, &[])?;
        }
        if reply.status == 429 {
            return Err("YouTube is temporarily limiting translated captions. Keep watching and try again shortly.".into());
        }
    }
    network::response_text(reply)
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
    let response = json_response(&network::fetch(
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
        network::fetch(job, address.as_str(), None, true, &[])?
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
    let integrity = json_response(&network::fetch(
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
        "mintRefreshThreshold":integrity[2],"websafeFallbackToken":integrity[3],"videoId":player.id.as_str()}))
}
fn caption_address(raw: &str, id: &str, po_token: Option<&str>) -> Result<String> {
    network::allowed_address(raw, false)?;
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
        let saved = crate::transcript::restore(&command, root)?;
        if !saved.is_null() && (track == "default" || saved["trackId"] == track) {
            return Ok(saved);
        }
    }
    let player = player_cache::player(job, root, id, fresh, true)?;
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
    let kind = if selected.translated_from.is_some() {
        CaptionKind::Translated
    } else {
        CaptionKind::Original
    };
    // Protected caption tracks can return HTTP 200 with no bytes even after a
    // standalone token challenge. Let the active player supply its own proof.
    let from_page = page::captions(job, root, id, selected).ok().flatten();
    job.check()?;
    let (segments, caption_metadata) = if let Some(captions) = from_page {
        captions
    } else {
        let result = (|| -> Result<_> {
            let raw = crate::string(&list["captionTracks"][index], "baseUrl")?;
            let address_for = |proof| -> Result<String> {
                let mut address = caption_address(raw, id, proof)?;
                if kind == CaptionKind::Translated {
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
                caption_source(job, &address, CaptionVerification::Unverified, kind)?
            };
            if source.trim().is_empty() {
                let proof = token(job, &player)?;
                source = caption_source(
                    job,
                    &address_for(Some(&proof))?,
                    CaptionVerification::Verified,
                    kind,
                )?;
            }
            if source.trim().is_empty() {
                return Err("YouTube returned no caption text after verification.".into());
            }
            crate::captions::parse(&source)
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
    crate::transcript::normalize(&mut data)?;
    job.check()?;
    let guard = crate::REQUEST_LOCK
        .lock(c"GoVue core request: extraction.persist_transcript")
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    let data = Arc::new(data);
    crate::transcript::remember_shared(root, Arc::clone(&data))?;
    let _ = crate::transcript::save(root, &data);
    drop(guard);
    Ok(json!(data.as_ref()))
}
fn catalog(job: &Job, root: &str, id: &str, fresh: bool) -> Result<Value> {
    let _span = crate::trace::Section::begin(c"GoVue extraction: media");
    if !fresh && let Some(cached) = media::cached_catalog(root, id) {
        return Ok(cached);
    }
    let player = player_cache::player(job, root, id, fresh, false)?;
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
    let cached = player_cache::chapters(root, id, fresh);
    // A prefix may contain the complete caption response but still omit
    // ytInitialData. Caption readiness alone cannot satisfy chapter loading.
    let raw = match cached {
        Some(chapters) => chapters,
        None => player_cache::player(job, root, id, true, true)?.chapters,
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
    let key = RequestKey::new(root, PREPARATION_REQUEST);
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
    let key = RequestKey::new(root, PREPARATION_REQUEST);
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
    jobs.insert(RequestKey::new(root, request), job);
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
            .filter(|id| crate::video_id(id))
    }))
}

fn runtime_tasks(root: &str, from_page: bool) -> Result<Value> {
    let jobs = JOBS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Extractor busy.")?;
    let mut requests = Vec::new();
    let mut tasks = Vec::new();
    for (key, job) in jobs.iter() {
        let store = &key.root;
        let request = &key.request;
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
    let request = crate::string(command, "requestId")?;
    if request.is_empty()
        || request.len() > 150
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err("Invalid extraction request identity.".into());
    }
    let op = crate::string(command, "op")?;
    if request == PREPARATION_REQUEST
        && !matches!(op, "extract_prepare" | "extract_launch" | "extract_release")
    {
        return Err("Reserved extraction request identity.".into());
    }
    Ok(request)
}

pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let op = crate::string(command, "op")?;
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
    let key = RequestKey::new(root, request);
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
                crate::string(command, "videoId")?
            };
            let kind = if preparing {
                "media"
            } else {
                crate::string(command, "kind")?
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
            if !crate::video_id(id)
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
            let result = crate::string(command, "value")?;
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
    getrandom::fill(&mut nonce).map_err(|_| "Could not create extraction identity.")?;
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
    use super::player_cache::PLAYERS;
    use super::*;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;

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
        assert_eq!(
            worker.join().unwrap().unwrap_err().to_string(),
            "Extraction cancelled."
        );
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
            assert!(
                network::allowed_address(address, false).is_err(),
                "{address}"
            );
        }
        assert!(network::allowed_address("https://www.google.com/js/bg/test.js", true).is_ok());
        assert!(network::allowed_address("https://www.google.com/api/private", true).is_err());
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
            .find(|p| p.id.as_str() == ID && p.root == root.to_string_lossy())
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
                .to_string()
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
            let error = wait("translated:error", true).unwrap_err().to_string();
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
                    .find(|p| p.id.as_str() == ID && p.root == root.to_string_lossy())
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
                assert!(result.unwrap_err().to_string().contains("HTTP 500"));
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
