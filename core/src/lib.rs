use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use unicode_normalization::UnicodeNormalization;
use url::Url;

uniffi::setup_scaffolding!();

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
}

#[derive(Clone, Serialize, Deserialize)]
struct Segment {
    start: f64,
    duration: f64,
    text: String,
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

fn parse_captions(source: &str) -> Result<Vec<Segment>> {
    if source.len() > MAX_BYTES {
        return Err("Caption response is too large.".into());
    }
    let mut segments = Vec::new();
    if source.trim_start().starts_with('{') {
        let data: Value = serde_json::from_str(source).map_err(|_| "Unreadable JSON captions.")?;
        let events = data["events"].as_array().ok_or("Missing caption events.")?;
        if events.len() > MAX_SEGMENTS * 4 {
            return Err("Too many caption events.".into());
        }
        for event in events {
            let Some(parts) = event["segs"].as_array() else {
                continue;
            };
            let text = clean(
                &parts
                    .iter()
                    .filter_map(|s| s["utf8"].as_str())
                    .collect::<String>(),
            );
            let Some(start) = number(&event["tStartMs"]) else {
                continue;
            };
            let duration = number(&event["dDurationMs"]).unwrap_or(0.0).max(0.0);
            if !text.is_empty() {
                segments.push(Segment {
                    start: start / 1000.0,
                    duration: duration / 1000.0,
                    text,
                });
            }
        }
    } else {
        let xml = roxmltree::Document::parse(source).map_err(|_| "Unreadable XML captions.")?;
        for node in xml
            .descendants()
            .filter(|n| n.has_tag_name("text") || n.has_tag_name("p"))
        {
            let legacy = node.has_tag_name("text");
            let unit = if legacy { 1.0 } else { 1000.0 };
            let Some(start) = node
                .attribute(if legacy { "start" } else { "t" })
                .and_then(|s| s.parse::<f64>().ok())
            else {
                continue;
            };
            let duration = node
                .attribute(if legacy { "dur" } else { "d" })
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
                .max(0.0);
            let content = node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>();
            let text = clean(&html_escape::decode_html_entities(&content));
            if !text.is_empty() {
                segments.push(Segment {
                    start: start / unit,
                    duration: duration / unit,
                    text,
                });
            }
            if segments.len() > MAX_SEGMENTS {
                return Err("Too many caption lines.".into());
            }
        }
    }
    segments.retain(|s| s.start.is_finite() && s.start >= 0.0 && s.duration.is_finite());
    Ok(segments)
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
        segment.text = clean(&segment.text);
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
        .unwrap_or_else(|e| e.into_inner());
    docs.retain(|(p, _)| p != &path);
    docs.push_back((path, data.clone()));
    while docs.len() > 3 {
        docs.pop_front();
    }
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
            .unwrap_or_else(|e| e.into_inner());
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

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Missing {field}."))
}

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
    if op == "info" {
        return Ok(json!({"engine": "rust", "version": env!("CARGO_PKG_VERSION")}));
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
        let reply: Value =
            serde_json::from_str(raw).map_err(|_| "Unreadable transcript response.")?;
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
        let mut data: Transcript = serde_json::from_value(reply["transcript"].clone())
            .map_err(|_| "Invalid transcript data.")?;
        if data.video_id != id {
            return Err("Transcript belongs to another video.".into());
        }
        if let Some(source) = reply["transcript"]["captionSource"].as_str() {
            data.segments = parse_captions(source)?;
        }
        normalize(&mut data)?;
        // Captions remain usable when device storage is full or unavailable.
        let path = cache_path(root, &data.video_id, &data.track_id)?;
        remember(path, &data);
        let cache_saved = save(root, &data).is_ok();
        return Ok(
            json!({"type":"transcript","requestId":request_id,"videoId":id,"ok":true,"transcript":data,"cacheSaved":cache_saved}),
        );
    }
    if op == "restore" {
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
            if data.video_id == id && normalize(&mut data).is_ok() {
                remember(cache_path(root, id, &data.track_id)?, &data);
                return Ok(json!(data));
            }
        }
        return Ok(Value::Null);
    }
    let data = load(root, id, string(&command, "trackId")?)?;
    match op {
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
                    (query.is_empty() || folded(&s.text).contains(&query)).then_some(i)
                })
                .collect();
            Ok(json!(indices))
        }
        "export" => {
            let lines: Vec<_> = data
                .segments
                .iter()
                .map(|s| format!("{}  {}", timestamp(s.start), s.text))
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
    let _guard = REQUEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match dispatch(&request, &storage_path) {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}
