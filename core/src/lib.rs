use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use unicode_normalization::UnicodeNormalization;
use url::Url;

uniffi::setup_scaffolding!();
mod ai;
mod app;
mod browser;
mod caption_tracks;
mod caption_words;
mod captions;
mod cast;
mod downloads;
mod extraction;
mod gallery;
mod language_catalog;
mod languages;
mod media;
mod player;
mod speech;
mod trace;
mod transcript_playback;
mod transcript_reader;
mod transcript_session;
mod transport;
mod vocabulary;

/// The same synchronous URL policy gates WebView loads and Rust tab state.
#[uniffi::export]
pub fn browser_url_policy(url: String) -> String {
    browser::url_policy(&url).into()
}
/// Small synchronous checks for UI callbacks; no storage or network access.
#[uniffi::export]
pub fn state_execute(request: String) -> String {
    let result = if request.len() > 100_000 {
        Err("Browser message is too large.".into())
    } else {
        serde_json::from_str(&request)
            .map_err(|_| "Unreadable browser request.".into())
            .and_then(|command: Value| {
                if command["op"]
                    .as_str()
                    .is_some_and(|op| op.starts_with("player_session_"))
                {
                    player::session::dispatch(&command)
                } else {
                    browser::boundary(&command)
                }
            })
    };
    match result {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}
pub use ai::{
    AiHeader, AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, install_ai_platform,
};
pub use extraction::{
    ExtractionHeader, ExtractionHttp, ExtractionReply, ExtractionRequest, install_extraction_http,
};

/// Notifications carry no application data. Consumers fetch the current Rust snapshot.
#[uniffi::export(callback_interface)]
pub trait CoreEvents: Send + Sync {
    fn changed(&self, channel: String);
}
static EVENTS: Mutex<Option<Box<dyn CoreEvents>>> = Mutex::new(None);
#[uniffi::export]
pub fn install_core_events(events: Box<dyn CoreEvents>) {
    *EVENTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(events);
}
pub(crate) fn changed(channel: &str) {
    if let Some(events) = EVENTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        events.changed(channel.into());
    }
}

/// Blocking platform effects run only on Rust workers, outside the core/storage locks.
#[uniffi::export(callback_interface)]
pub trait DownloadPlatform: Send + Sync {
    fn directory(&self) -> String;
    fn export_directory(&self) -> String;
    fn supported_formats(&self, formats: String) -> String;
    fn mux(&self, video: String, audio: String, output: String) -> bool;
    /// -1: failed, 0: platform does not publish, 1: published and verified.
    fn publish(&self, id: String) -> i32;
    fn delete_published(&self, uri: String) -> bool;
    fn activity(&self, count: u32, title: String, received: u64, total: u64);
}
#[uniffi::export]
pub fn install_download_platform(platform: Box<dyn DownloadPlatform>) {
    downloads::runner::install(platform);
}

#[derive(uniffi::Record)]
pub struct MediaRangeReply {
    pub status: i32,
    pub body: Vec<u8>,
    pub location: Option<String>,
}

#[uniffi::export(callback_interface)]
pub trait MediaHttp: Send + Sync {
    fn fetch_range(
        &self,
        url: String,
        user_agent: String,
        origin: String,
        method: String,
        start: u64,
        end: u64,
    ) -> MediaRangeReply;
}

#[uniffi::export]
pub fn install_media_http(backend: Box<dyn MediaHttp>) {
    transport::install_http(backend);
}

#[derive(uniffi::Record)]
pub struct DownloadRange {
    pub start: u64,
    pub end: u64,
}

#[derive(uniffi::Record)]
pub struct DownloadRequest {
    pub url: String,
    pub user_agent: String,
    pub origin: String,
    pub method: String,
    pub range: Option<DownloadRange>,
    pub wifi_only: bool,
}

/// Native connectivity and network-restricted HTTPS, independent of JS timers.
#[uniffi::export(callback_interface)]
pub trait DownloadHttp: Send + Sync {
    fn wifi_connected(&self) -> bool;
    /// Status -1 means the allowed network disappeared; retry without failing.
    fn fetch(&self, request: DownloadRequest) -> MediaRangeReply;
}

#[uniffi::export]
pub fn install_download_http(backend: Box<dyn DownloadHttp>) {
    downloads::transfer::install_http(backend);
}

/// Incremental HTTPS byte adapter. Rust owns ranges, redirects and bounds.
#[derive(uniffi::Record)]
pub struct MediaStreamReply {
    pub status: i32,
    pub handle: u64,
    pub length: i64,
    pub location: Option<String>,
}
#[uniffi::export(callback_interface)]
pub trait MediaStreamHttp: Send + Sync {
    fn open_range(
        &self,
        url: String,
        user_agent: String,
        origin: String,
        method: String,
        start: u64,
        end: u64,
    ) -> MediaStreamReply;
    fn read(&self, handle: u64, max_bytes: u32) -> Vec<u8>;
    fn close(&self, handle: u64);
}
#[uniffi::export]
pub fn install_media_stream_http(backend: Box<dyn MediaStreamHttp>) {
    transport::streaming::install(backend);
}

/// OS TLS and Wi-Fi access only. Protocol, routes and session ownership stay in Rust.
#[uniffi::export(callback_interface)]
pub trait CastPlatform: Send + Sync {
    fn wifi_address(&self) -> String;
    fn bind_wifi_socket(&self, socket_fd: u32, address: String) -> bool;
    // Avoid Swift keywords: UniFFI also uses this name in its C callback table.
    fn connect(&self, host: String, port: u16) -> String;
    fn read(&self, socket: String) -> Vec<u8>;
    fn write(&self, socket: String, frame: Vec<u8>) -> bool;
    fn close(&self, socket: String);
    fn activity(&self, active: bool) -> bool;
}

#[uniffi::export]
pub fn install_cast_platform(backend: Box<dyn CastPlatform>) {
    cast::install(backend);
}

const MAX_BYTES: usize = 8_000_000;
const MAX_SEGMENTS: usize = 50_000;
const CACHE_BYTES: u64 = 24_000_000;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Track {
    id: String,
    label: String,
    language: String,
    automatic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    translatable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    audio_track_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    translated_from: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Segment {
    start: f64,
    duration: f64,
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    words: Vec<caption_words::Word>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    runs: Vec<captions::Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "windowId")]
    window_id: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Transcript {
    video_id: String,
    title: String,
    language: String,
    track_id: String,
    tracks: Vec<Track>,
    segments: Vec<Segment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    caption_metadata: Option<captions::Metadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    track_metadata: Option<caption_tracks::Metadata>,
}

// At most three documents remain in memory. Disk storage is bounded separately.
static DOCUMENTS: OnceLock<Mutex<VecDeque<(PathBuf, Transcript)>>> = OnceLock::new();
static REQUEST_LOCK: Mutex<()> = Mutex::new(());

fn clean(text: &str) -> String {
    text.nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn folded(text: &str) -> String {
    clean(text).to_lowercase()
}
fn video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn trusted_origin(origin: &str, id: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(
            url.host_str(),
            Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com")
        )
    {
        return false;
    }
    if url.path() == "/" && url.query().is_none() && url.fragment().is_none() {
        return true;
    }
    if url.path() == "/watch" {
        return url
            .query_pairs()
            .any(|(key, value)| key == "v" && value == id);
    }
    let parts: Vec<_> = url.path().split('/').collect();
    parts.len() >= 3 && matches!(parts[1], "shorts" | "live" | "embed") && parts[2] == id
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn parse_captions(source: &str) -> Result<(Vec<Segment>, captions::Metadata)> {
    captions::parse(source)
}

fn normalize(data: &mut Transcript) -> Result<()> {
    if !video_id(&data.video_id)
        || data.title.len() > 10_000
        || data.language.len() > 500
        || data.track_id.len() > 1_000
        || data.tracks.is_empty()
        || data.tracks.len() > 300
        || data.segments.is_empty()
        || data.segments.len() > MAX_SEGMENTS
    {
        return Err("Invalid transcript metadata or line count.".into());
    }
    caption_tracks::validate(data)?;
    if data
        .tracks
        .iter()
        .any(|t| t.id.len() > 1_000 || t.label.len() > 1_000 || t.language.len() > 100)
        || !data.tracks.iter().any(|t| t.id == data.track_id)
    {
        return Err("Invalid caption track.".into());
    }
    if data.segments.iter().any(|s| {
        !s.start.is_finite()
            || s.start < 0.0
            || !s.duration.is_finite()
            || s.duration < 0.0
            || s.text.len() > 50_000
    }) {
        return Err("Invalid caption line.".into());
    }
    for segment in &mut data.segments {
        segment.text = captions::clean_text(&segment.text);
        if !segment.runs.is_empty() {
            let text = segment.text.clone();
            if segment.runs.len() > 50_000
                || segment.runs.iter().any(|run| {
                    run.text.len() > 50_000
                        || run
                            .start
                            .is_some_and(|n| !n.is_finite() || n < segment.start)
                        || run.reading.as_ref().is_some_and(|s| s.len() > 50_000)
                })
            {
                return Err("Invalid caption formatting.".into());
            }
            captions::refresh(segment);
            if segment.text != text {
                return Err("Caption formatting does not match its text.".into());
            }
        }
        if !segment.words.is_empty() && !caption_words::valid(segment) {
            segment.words.clear();
        }
    }
    if data
        .caption_metadata
        .as_ref()
        .is_some_and(|metadata| !captions::validate(metadata))
    {
        return Err("Invalid caption structure.".into());
    }
    data.segments.retain(|s| !s.text.is_empty());
    data.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    if data.segments.is_empty() {
        return Err("This video has no caption text available.".into());
    }
    data.title = clean(&data.title);
    Ok(())
}

fn cache_path(root: &str, id: &str, track: &str) -> Result<PathBuf> {
    if !video_id(id) || track.len() > 1_000 {
        return Err("Invalid transcript identity.".into());
    }
    let key = format!("{id}\0{track}");
    let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    Ok(Path::new(root).join(format!("{hash}.json")))
}

fn remember(path: PathBuf, data: &Transcript) {
    let mut docs = DOCUMENTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    docs.retain(|(p, _)| p != &path);
    docs.push_back((path, data.clone()));
    while docs.len() > 3 {
        docs.pop_front();
    }
    drop(docs);
}

fn save(root: &str, data: &Transcript) -> Result<()> {
    fs::create_dir_all(root).map_err(|_| "Could not create transcript cache.")?;
    let path = cache_path(root, &data.video_id, &data.track_id)?;
    let bytes = serde_json::to_vec(data).map_err(|_| "Could not encode transcript.")?;
    if bytes.len() > MAX_BYTES {
        return Err("Transcript is too large to cache.".into());
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)
        .and_then(|_| fs::rename(&temporary, &path))
        .map_err(|_| "Could not save transcript cache.")?;
    remember(path.clone(), data);
    let mut files: Vec<_> = fs::read_dir(root)
        .map_err(|_| "Could not read transcript cache.")?
        .filter_map(|e| {
            let entry = e.ok()?;
            let meta = entry.metadata().ok()?;
            (entry.path().extension()?.to_str()? == "json" && meta.is_file()).then_some((
                entry.path(),
                meta.len(),
                meta.modified().ok()?,
            ))
        })
        .collect();
    files.sort_by_key(|f| f.2);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut count = files.len();
    for (file, bytes, _) in files {
        if count <= 20 && total <= CACHE_BYTES {
            break;
        }
        if file != path && fs::remove_file(file).is_ok() {
            total -= bytes;
            count -= 1;
        }
    }
    Ok(())
}

fn load(root: &str, id: &str, track: &str) -> Result<Transcript> {
    let path = cache_path(root, id, track)?;
    {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, data)) = docs.iter().find(|(p, _)| p == &path) {
            return Ok(data.clone());
        }
    }
    if fs::metadata(&path)
        .map_err(|_| "Transcript is not cached yet.")?
        .len()
        > MAX_BYTES as u64
    {
        return Err("Cached transcript is too large.".into());
    }
    let bytes = fs::read(&path).map_err(|_| "Could not read cached transcript.")?;
    let mut data: Transcript =
        serde_json::from_slice(&bytes).map_err(|_| "Unreadable cached transcript.")?;
    if data.video_id != id || data.track_id != track {
        return Err("Cached transcript identity changed.".into());
    }
    normalize(&mut data)?;
    remember(path, &data);
    Ok(data)
}

/// Caption sampling borrows the bounded cache without cloning every cue.
fn with_transcript<T>(
    root: &str,
    id: &str,
    track: &str,
    read: impl FnOnce(&Transcript) -> T,
) -> Result<T> {
    let path = cache_path(root, id, track)?;
    {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, data)) = docs.iter().find(|(p, _)| p == &path) {
            return Ok(read(data));
        }
    }
    Ok(read(&load(root, id, track)?))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Missing {field}."))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Display timestamps intentionally floor nonnegative seconds and saturate out-of-range values."
)]
fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0).floor() as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn dispatch(request: &str, root: &str) -> Result<Value> {
    if request.len() > MAX_BYTES * 2 {
        return Err("Core request is too large.".into());
    }
    let command: Value = serde_json::from_str(request).map_err(|_| "Unreadable core request.")?;
    let op = string(&command, "op")?;
    if op.starts_with("app_") {
        return app::dispatch(&command);
    }
    if op.starts_with("transcript_session_") {
        return transcript_session::dispatch(&command, root);
    }
    if op.starts_with("extract_") {
        return extraction::dispatch(&command, root);
    }
    if op.starts_with("cast_") {
        return cast::dispatch(&command);
    }
    if matches!(
        op,
        "browser_input" | "browser_identity" | "browser_layout" | "browser_position"
    ) {
        return browser::boundary(&command);
    }
    if op.starts_with("browser_") {
        return browser::dispatch(&command, root);
    }
    if op.starts_with("player_") {
        return player::dispatch(&command, root);
    }
    if op.starts_with("vocabulary_") {
        return vocabulary::dispatch(&command, root);
    }
    if op == "language_settings" {
        return languages::settings(&command, root);
    }
    if op == "language_choices" {
        return language_catalog::choices(&command);
    }
    if op == "response_speech" {
        return speech::reading::plan(&command);
    }
    if op.starts_with("media_") || op.starts_with("sponsor_") || op.starts_with("downloads_") {
        return media::dispatch(&command, root);
    }
    if op.starts_with("gallery_") {
        return gallery::dispatch(&command, root);
    }
    if op == "info" {
        return Ok(
            json!({"engine": "rust", "version": env!("CARGO_PKG_VERSION"),
            "commit": option_env!("GOVUE_GIT_COMMIT")}),
        );
    }
    let id = string(&command, "videoId")?;
    if !video_id(id) {
        return Err("Invalid video identity.".into());
    }
    if op == "accept" {
        let origin = string(&command, "origin")?;
        let request_id = string(&command, "requestId")?;
        let raw = string(&command, "raw")?;
        if raw.len() > MAX_BYTES || request_id.len() > 200 || !trusted_origin(origin, id) {
            return Err("Untrusted transcript response.".into());
        }
        let reply: Value = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.reply_parse");
            serde_json::from_str(raw).map_err(|_| "Unreadable transcript response.")?
        };
        if reply["type"] != "transcript"
            || reply["requestId"] != request_id
            || reply["videoId"] != id
        {
            return Err("Transcript response belongs to another request.".into());
        }
        if reply["ok"] == false {
            let error = string(&reply, "error")?
                .chars()
                .take(500)
                .collect::<String>();
            return Ok(
                json!({"type":"transcript","requestId":request_id,"videoId":id,"ok":false,"error":error}),
            );
        }
        if reply["ok"] != true {
            return Err("Invalid transcript response.".into());
        }
        let mut data: Transcript = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.decode");
            serde_json::from_value(reply["transcript"].clone())
                .map_err(|_| "Invalid transcript data.")?
        };
        if data.video_id != id {
            return Err("Transcript belongs to another video.".into());
        }
        if let Some(source) = reply["transcript"]["captionSource"].as_str() {
            let _span = trace::Section::begin(c"GoVue transcript: rust.caption_parse");
            let (segments, metadata) = parse_captions(source)?;
            data.segments = segments;
            data.caption_metadata = Some(metadata);
        }
        {
            let _span = trace::Section::begin(c"GoVue transcript: rust.normalize");
            normalize(&mut data)?;
        }
        // Captions remain usable when device storage is full or unavailable.
        let path = cache_path(root, &data.video_id, &data.track_id)?;
        remember(path, &data);
        let cache_saved = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.cache_save");
            save(root, &data).is_ok()
        };
        return Ok(
            json!({"type":"transcript","requestId":request_id,"videoId":id,"ok":true,"transcript":data,"cacheSaved":cache_saved}),
        );
    }
    if op == "restore" {
        if let Some(track) = command["trackId"].as_str() {
            return Ok(load(root, id, track).map_or(Value::Null, |data| json!(data)));
        }
        let _span = trace::Section::begin(c"GoVue transcript: rust.cache_restore");
        let Ok(entries) = fs::read_dir(root) else {
            return Ok(Value::Null);
        };
        let mut files: Vec<_> = entries
            .filter_map(|e| {
                let entry = e.ok()?;
                let meta = entry.metadata().ok()?;
                (entry.path().extension()?.to_str()? == "json"
                    && meta.is_file()
                    && meta.len() <= MAX_BYTES as u64)
                    .then_some((entry.path(), meta.modified().ok()?))
            })
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.1));
        for (path, _) in files.into_iter().take(20) {
            let Some(mut data) = fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Transcript>(&bytes).ok())
            else {
                continue;
            };
            if data.video_id == id
                && normalize(&mut data).is_ok()
                && caption_tracks::matches_audio(&data, command["audioKey"].as_str().unwrap_or(""))
            {
                remember(cache_path(root, id, &data.track_id)?, &data);
                return Ok(json!(data));
            }
        }
        return Ok(Value::Null);
    }
    let track = string(&command, "trackId")?;
    if op == "transcript_playback" {
        // Position samples are frequent. Borrow the cached document rather
        // than cloning every caption string for each playback update.
        let path = cache_path(root, id, track)?;
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, data)) = docs.iter().find(|(p, _)| p == &path) {
            return transcript_playback::step(data, &command);
        }
    }
    let data = load(root, id, track)?;
    match op {
        "transcript_select" | "transcript_reader" | "transcript_export" => {
            transcript_reader::dispatch(op, &data, &command, root)
        }
        "transcript_playback" => transcript_playback::step(&data, &command),
        "load" => {
            Ok(serde_json::to_value(data).map_err(|_| "Could not encode cached transcript.")?)
        }
        "search" => {
            let query = string(&command, "query")?;
            if query.len() > 10_000 {
                return Err("Search query is too long.".into());
            }
            let query = folded(query);
            let indices: Vec<usize> = data
                .segments
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    (query.is_empty() || captions::matches(s, &query)).then_some(i)
                })
                .collect();
            Ok(json!(indices))
        }
        "export" => {
            let lines: Vec<_> = data
                .segments
                .iter()
                .map(|s| format!("{}  {}", timestamp(s.start), captions::readable_text(s)))
                .collect();
            Ok(json!(format!(
                "{}\nhttps://www.youtube.com/watch?v={}\n\n{}",
                data.title,
                data.video_id,
                lines.join("\n")
            )))
        }
        _ => Err("Unsupported core operation.".into()),
    }
}

/// One JSON boundary keeps the platform adapters identical and small. Work is
/// dispatched off the JavaScript/UI thread by the Expo native module.
#[uniffi::export]
pub fn execute(request: String, storage_path: String) -> String {
    let _span = trace::Section::begin(c"GoVue core: execute");
    let _guard = {
        let _span = trace::Section::begin(c"GoVue core: lock_wait");
        REQUEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    match dispatch(&request, &storage_path) {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}

/// Dictation never touches storage or transport. Its small, synchronous JSON
/// boundary makes final-result edits atomic with the JavaScript draft read.
#[uniffi::export]
pub fn dictation_execute(request: String) -> String {
    match speech::dispatch(&request) {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}

pub(crate) fn request(command: &Value, root: &str) -> Result<Value> {
    let reply: Value = serde_json::from_str(&execute(command.to_string(), root.into()))
        .map_err(|_| "Unreadable core response.")?;
    if reply["ok"] == true {
        Ok(reply["value"].clone())
    } else {
        Err(reply["error"]
            .as_str()
            .unwrap_or("Core operation failed.")
            .into())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}
