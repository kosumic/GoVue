use error::Error;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::Mutex;
use transcript::{Segment, Track, Transcript};
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
mod error;
mod extraction;
mod gallery;
mod identity;
mod language_catalog;
mod languages;
mod media;
mod player;
mod request_lock;
mod speech;
mod trace;
mod transcript;
mod transcript_playback;
mod transcript_reader;
mod transcript_session;
mod transport;
mod vocabulary;

#[cfg(test)]
fn poison<T: Send>(mutex: &Mutex<T>) {
    std::thread::scope(|scope| {
        assert!(
            scope
                .spawn(|| {
                    let _guard = mutex.lock().unwrap();
                    panic!("Poisoned-state fixture.");
                })
                .join()
                .is_err()
        );
    });
    assert!(mutex.is_poisoned());
}

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
                if command["op"] == "chat_notification_visibility" {
                    ai::chat_notification_visibility(&command)
                } else if command["op"] == "chat_notification_foreground" {
                    ai::chat_notification_foreground(command["active"] == true);
                    Ok(Value::Null)
                } else if command["op"]
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
    AiHeader, AiHttpRequest, AiPlatform, AiResponse, AiSecret, ChatNotificationPlatform,
    ai_execute, install_ai_platform, install_chat_notification_platform,
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
    if let Ok(mut slot) = EVENTS.lock() {
        *slot = Some(events);
    }
}
pub(crate) fn changed(channel: &str) {
    if let Ok(events) = EVENTS.lock()
        && let Some(events) = events.as_ref()
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
type Result<T> = std::result::Result<T, Error>;

static REQUEST_LOCK: request_lock::RequestLock = request_lock::RequestLock::new();

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

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Missing {field}.").into())
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
    // These reads take immutable snapshots under their own short lock scopes.
    if op == "restore" {
        return transcript::restore(&command, root);
    }
    if op == "transcript_session_snapshot" {
        let _span = trace::Section::begin(c"GoVue core op: transcript_session_snapshot");
        return transcript_session::snapshot(&command, root);
    }
    let _guard = REQUEST_LOCK
        .lock(c"GoVue core request: execute")
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    if op.starts_with("app_") {
        let _span = trace::Section::begin(c"GoVue core op: app");
        return app::dispatch(&command);
    }
    if op.starts_with("transcript_session_") {
        let _span = trace::Section::begin(c"GoVue core op: transcript_session");
        return transcript_session::dispatch(&command, root);
    }
    if op.starts_with("extract_") {
        let _span = trace::Section::begin(c"GoVue core op: extraction");
        return extraction::dispatch(&command, root);
    }
    if op.starts_with("cast_") {
        let _span = trace::Section::begin(c"GoVue core op: cast");
        return cast::dispatch(&command);
    }
    if matches!(
        op,
        "browser_input" | "browser_identity" | "browser_layout" | "browser_position"
    ) {
        let _span = trace::Section::begin(c"GoVue core op: browser_boundary");
        return browser::boundary(&command);
    }
    if op.starts_with("browser_") {
        let _span = trace::Section::begin(c"GoVue core op: browser");
        return browser::dispatch(&command, root);
    }
    if op.starts_with("player_") {
        let _span = trace::Section::begin(c"GoVue core op: player");
        return player::dispatch(&command, root);
    }
    if op.starts_with("vocabulary_") {
        let _span = trace::Section::begin(c"GoVue core op: vocabulary");
        return vocabulary::dispatch(&command, root);
    }
    if op == "language_settings" {
        let _span = trace::Section::begin(c"GoVue core op: language_settings");
        return languages::settings(&command, root);
    }
    if op == "language_choices" {
        let _span = trace::Section::begin(c"GoVue core op: language_choices");
        return language_catalog::choices(&command);
    }
    if op == "response_speech" {
        let _span = trace::Section::begin(c"GoVue core op: response_speech");
        return speech::reading::plan(&command);
    }
    if op.starts_with("media_") || op.starts_with("sponsor_") || op.starts_with("downloads_") {
        let _span = trace::Section::begin(match op {
            _ if op.starts_with("media_settings") => c"GoVue core op: player_settings",
            _ if op.starts_with("downloads_") => c"GoVue core op: downloads",
            _ if op.starts_with("sponsor_") => c"GoVue core op: sponsor",
            _ => c"GoVue core op: media",
        });
        return media::dispatch(&command, root);
    }
    if op.starts_with("gallery_") {
        let _span = trace::Section::begin(c"GoVue core op: gallery");
        return gallery::dispatch(&command, root);
    }
    if op == "info" {
        let _span = trace::Section::begin(c"GoVue core op: info");
        return Ok(
            json!({"engine": "rust", "version": env!("CARGO_PKG_VERSION"),
            "commit": option_env!("GOVUE_GIT_COMMIT")}),
        );
    }
    let _span = trace::Section::begin(match op {
        "accept" => c"GoVue core op: transcript_accept",
        "restore" => c"GoVue core op: transcript_restore",
        _ => c"GoVue core op: transcript",
    });
    transcript::dispatch(&command, root)
}

/// One JSON boundary keeps the platform adapters identical and small. Work is
/// dispatched off the JavaScript/UI thread by the Expo native module.
#[uniffi::export]
pub fn execute(request: String, storage_path: String) -> String {
    let _span = trace::Section::begin(c"GoVue core: execute");
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
