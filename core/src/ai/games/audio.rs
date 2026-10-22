//! Caption clips cache fMP4 initialization and indexed audio fragments lazily.
//! The generated document never receives network addresses or filesystem paths.
use super::Cue;
use crate::Result;
use crate::ai::{now, random, valid_id};
use crate::{
    media::{container, playback},
    transport::{DownloadSource, header},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::{fs, thread};
use tiny_http::{Method, Response, Server, StatusCode};

const MAX_CLIP: u64 = 12_000_000;
const MAX_CACHE: u64 = 64_000_000;
static BUILDS: Mutex<()> = Mutex::new(());
static SERVER: OnceLock<Result<ClipServer>> = OnceLock::new();

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    start: f64,
    end: f64,
    durations: Vec<f64>,
    sizes: Vec<u64>,
    used_at: u64,
}
struct Clip {
    directory: PathBuf,
    manifest: String,
}
struct ClipServer {
    port: u16,
    clips: Arc<Mutex<HashMap<String, Arc<Clip>>>>,
}
fn server() -> Result<&'static ClipServer> {
    SERVER
        .get_or_init(|| {
            let server =
                Server::http("127.0.0.1:0").map_err(|_| "Could not start game audio playback.")?;
            let port = server
                .server_addr()
                .to_ip()
                .ok_or("Invalid audio listener.")?
                .port();
            let clips = Arc::new(Mutex::new(HashMap::<String, Arc<Clip>>::new()));
            let worker = clips.clone();
            thread::spawn(move || -> Result<()> {
                for request in server.incoming_requests() {
                    if !matches!(request.method(), Method::Get | Method::Head) {
                        let _ = request.respond(Response::empty(StatusCode(405)));
                        continue;
                    }
                    let route = request
                        .url()
                        .trim_start_matches('/')
                        .split('/')
                        .collect::<Vec<_>>();
                    let clip = if route.len() == 2 {
                        worker
                            .lock()
                            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                            .get(route[0])
                            .cloned()
                    } else {
                        None
                    };
                    let Some(clip) = clip else {
                        let _ = request.respond(Response::empty(StatusCode(404)));
                        continue;
                    };
                    let (bytes, mime) = if route[1] == "index.m3u8" {
                        (
                            Ok(clip.manifest.as_bytes().to_vec()),
                            "application/vnd.apple.mpegurl",
                        )
                    } else if route[1] == "init.mp4"
                        || route[1]
                            .strip_prefix("segment-")
                            .and_then(|v| v.strip_suffix(".m4s"))
                            .and_then(|v| v.parse::<usize>().ok())
                            .is_some_and(|i| i < 64)
                    {
                        (fs::read(clip.directory.join(route[1])), "audio/mp4")
                    } else {
                        (
                            Err(std::io::Error::other("Missing audio segment.")),
                            "audio/mp4",
                        )
                    };
                    let Ok(bytes) = bytes else {
                        let _ = request.respond(Response::empty(StatusCode(404)));
                        continue;
                    };
                    let length = bytes.len();
                    let range = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Range"))
                        .and_then(|h| h.value.as_str().strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .and_then(|(a, b)| {
                            Some((
                                a.parse::<usize>().ok()?,
                                if b.is_empty() {
                                    length.checked_sub(1)?
                                } else {
                                    b.parse::<usize>().ok()?
                                },
                            ))
                        })
                        .filter(|(a, b)| a <= b && *b < length);
                    let (data, status) = if let Some((a, b)) = range {
                        (bytes[a..=b].to_vec(), 206)
                    } else {
                        (bytes, 200)
                    };
                    let data_length = data.len();
                    let head = request.method() == &Method::Head;
                    let mut response = Response::from_data(if head { Vec::new() } else { data })
                        .with_status_code(StatusCode(status))
                        .with_header(header("Content-Type", mime)?)
                        .with_header(header("Cache-Control", "no-store")?)
                        .with_header(header("Accept-Ranges", "bytes")?)
                        .with_header(header("Content-Length", &data_length.to_string())?);
                    if let Some((a, b)) = range {
                        response.add_header(header(
                            "Content-Range",
                            &format!("bytes {a}-{b}/{length}"),
                        )?);
                    }
                    let _ = request.respond(response);
                }
                Ok(())
            });
            Ok(ClipServer { port, clips })
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(in crate::ai) fn release(token: &str) -> Result<Value> {
    if !valid_id(token) {
        return Err("Invalid game audio session.".into());
    }
    if let Some(Ok(server)) = SERVER.get() {
        server
            .clips
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .remove(token);
    }
    Ok(Value::Null)
}

fn read_metadata(directory: &Path) -> Option<Metadata> {
    let file = fs::File::open(directory.join("clip.json")).ok()?;
    let mut bytes = Vec::new();
    file.take(16_001).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 16_000 {
        return None;
    }
    let data: Metadata = serde_json::from_slice(&bytes).ok()?;
    if !data.start.is_finite()
        || data.start < 0.0
        || !data.end.is_finite()
        || data.end <= data.start
        || data.end - data.start > 30.0
        || data.durations.is_empty()
        || data.durations.len() > 64
        || data.sizes.len() != data.durations.len() + 1
        || data
            .sizes
            .iter()
            .try_fold(0u64, |total, size| total.checked_add(*size))
            .is_none_or(|total| total > MAX_CLIP)
        || data
            .durations
            .iter()
            .any(|d| !d.is_finite() || *d <= 0.0 || *d > 60.0)
    {
        return None;
    }
    for (i, size) in data.sizes.iter().enumerate() {
        let name = if i == 0 {
            "init.mp4".to_owned()
        } else {
            format!("segment-{}.m4s", i - 1)
        };
        if *size == 0 || fs::metadata(directory.join(name)).ok()?.len() != *size {
            return None;
        }
    }
    Some(data)
}
fn cache_key(video_id: &str, cue: &Cue) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "game-audio-v1-original-aac\0{video_id}\0{}\0{}",
            cue.start, cue.duration
        ))
    )
}
fn register(directory: PathBuf, data: &mut Metadata) -> Result<Value> {
    let server = server()?;
    let token = random()?;
    let mut clips = server
        .clips
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    if clips.len() >= 16 {
        return Err("Too many game audio sessions are active.".into());
    }
    let target = data.durations.iter().copied().fold(1.0, f64::max).ceil();
    let mut manifest = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target:.0}\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"init.mp4\"\n"
    );
    for (i, duration) in data.durations.iter().enumerate() {
        let _ = write!(manifest, "#EXTINF:{duration:.6},\nsegment-{i}.m4s\n");
    }
    manifest.push_str("#EXT-X-ENDLIST\n");
    data.used_at = now();
    let _ = fs::write(
        directory.join("clip.json"),
        serde_json::to_vec(data).unwrap_or_default(),
    );
    clips.insert(
        token.clone(),
        Arc::new(Clip {
            directory,
            manifest,
        }),
    );
    drop(clips);
    Ok(
        json!({"uri":format!("http://127.0.0.1:{}/{token}/index.m3u8",server.port),"token":token,"start":data.start,"end":data.end,"needsMedia":false}),
    )
}
fn evict(root: &Path, protected: &Path) -> Result<()> {
    let pinned = if let Some(Ok(server)) = SERVER.get() {
        server
            .clips
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .values()
            .map(|c| c.directory.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut entries = Vec::new();
    for item in fs::read_dir(root)
        .map_err(|_| "Could not read the game audio cache.")?
        .flatten()
        .take(1024)
    {
        let path = item.path();
        if !item
            .file_name()
            .to_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            continue;
        }
        if let Some(data) = read_metadata(&path) {
            entries.push((data.used_at, data.sizes.iter().sum::<u64>(), path));
        } else if path != protected && !pinned.contains(&path) {
            let _ = fs::remove_dir_all(&path);
        }
    }
    entries.sort_by_key(|e| e.0);
    let mut total = entries.iter().map(|e| e.1).sum::<u64>();
    for (_, size, path) in entries {
        if total <= MAX_CACHE - MAX_CLIP {
            break;
        }
        if path != protected && !pinned.contains(&path) {
            fs::remove_dir_all(path).map_err(|_| "Could not free the game audio cache.")?;
            total = total.saturating_sub(size);
        }
    }
    if total > MAX_CACHE - MAX_CLIP {
        return Err("The game audio cache is busy. Stop another clip and try again.".into());
    }
    Ok(())
}
fn bytes(source: &DownloadSource, start: u64, end: u64) -> Result<Vec<u8>> {
    if end < start || end - start >= MAX_CLIP {
        return Err("Audio segment exceeds the clip limit.".into());
    }
    let mut data = Vec::new();
    let mut next = start;
    while next <= end {
        let last = end.min(next + 1_048_575);
        data.extend(
            source
                .bytes(next, last)
                .map_err(|_| "Could not fetch game audio. Refresh the video and retry.")?,
        );
        next = last + 1;
    }
    Ok(data)
}

pub(in crate::ai) fn prepare(
    root: &Path,
    video_id: &str,
    cue: &Cue,
    catalog: &Value,
) -> Result<Value> {
    prepare_clip(root, video_id, cue, catalog, false)
}
pub(in crate::ai) fn prefetch(
    root: &Path,
    video_id: &str,
    cue: &Cue,
    catalog: &Value,
) -> Result<Value> {
    prepare_clip(root, video_id, cue, catalog, true)
}
fn prepared(directory: PathBuf, data: &mut Metadata, cache_only: bool) -> Result<Value> {
    if !cache_only {
        return register(directory, data);
    }
    data.used_at = now();
    fs::write(
        directory.join("clip.json"),
        serde_json::to_vec(data).map_err(|_| "Could not encode audio cache metadata.")?,
    )
    .map_err(|_| "Could not update the audio cache.")?;
    Ok(json!({"needsMedia":false}))
}
fn prepare_clip(
    root: &Path,
    video_id: &str,
    cue: &Cue,
    catalog: &Value,
    cache_only: bool,
) -> Result<Value> {
    let _build = BUILDS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let cache = root.join("ai/game-audio");
    let directory = cache.join(cache_key(video_id, cue));
    if let Some(mut data) = read_metadata(&directory) {
        return prepared(directory, &mut data, cache_only);
    }
    if catalog.is_null() {
        return Ok(json!({"needsMedia":true,"videoId":video_id}));
    }
    if catalog["videoId"] != video_id || catalog["live"] == true {
        return Err("Game audio requires the matching recorded video.".into());
    }
    let streams = catalog["streams"]
        .as_array()
        .ok_or("Invalid audio catalog.")?;
    let stream = streams
        .iter()
        .filter(|s| {
            s["kind"] == "audio"
                && container(s) == Some("audio/mp4")
                && s["mimeType"].as_str().is_some_and(|m| m.contains("mp4a"))
                && playback::indexed(s)
        })
        .max_by_key(|s| {
            (
                s["audioOriginal"] == true,
                s["audioDefault"] == true,
                s["bitrate"].as_u64().unwrap_or(0),
            )
        })
        .ok_or("This video has no indexed AAC audio for games.")?;
    let length = stream["bytes"].as_u64().ok_or("Missing audio length.")?;
    let source = DownloadSource::new(
        stream,
        "",
        catalog["origin"].as_str().ok_or("Missing audio origin.")?,
    )?;
    let (index_start, index_end) =
        playback::range(&stream["indexRange"], length).ok_or("Missing audio index.")?;
    if index_end - index_start > 1_048_575 {
        return Err("Audio index exceeds the clip limit.".into());
    }
    let index = bytes(&source, index_start, index_end)?;
    let segments = playback::segment_index(stream, &index)?
        .into_iter()
        .filter(|s| s.start < cue.start + cue.duration && s.start + s.duration > cue.start)
        .collect::<Vec<_>>();
    let first = segments.first().ok_or("No audio covers this caption.")?;
    let last = segments.last().ok_or("Missing audio segments.")?;
    if segments.len() > 64
        || first.start > cue.start
        || last.start + last.duration + 0.01 < cue.start + cue.duration
    {
        return Err("The audio does not cover this caption completely.".into());
    }
    let (init_start, init_end) =
        playback::range(&stream["initRange"], length).ok_or("Missing audio initialization.")?;
    let total = init_end - init_start + 1 + segments.iter().map(|s| s.size).sum::<u64>();
    if total > MAX_CLIP {
        return Err("This caption's audio is too large to cache.".into());
    }
    fs::create_dir_all(&cache).map_err(|_| "Could not create the game audio cache.")?;
    evict(&cache, &directory)?;
    if directory.exists() {
        fs::remove_dir_all(&directory)
            .map_err(|_| "Could not replace the incomplete audio clip.")?;
    }
    fs::create_dir(&directory).map_err(|_| "Could not create the audio clip.")?;
    let result = (|| {
        fs::write(
            directory.join("init.mp4"),
            bytes(&source, init_start, init_end)?,
        )
        .map_err(|_| "Could not cache audio initialization.")?;
        for (i, segment) in segments.iter().enumerate() {
            fs::write(
                directory.join(format!("segment-{i}.m4s")),
                bytes(&source, segment.offset, segment.offset + segment.size - 1)?,
            )
            .map_err(|_| "Could not cache an audio segment.")?;
        }
        let mut sizes = vec![init_end - init_start + 1];
        sizes.extend(segments.iter().map(|s| s.size));
        let data = Metadata {
            start: cue.start - first.start,
            end: cue.start + cue.duration - first.start,
            durations: segments.iter().map(|s| s.duration).collect(),
            sizes,
            used_at: now(),
        };
        fs::write(
            directory.join("clip.json"),
            serde_json::to_vec(&data).map_err(|_| "Could not encode the audio clip.")?,
        )
        .map_err(|_| "Could not finish caching audio.")?;
        Ok::<_, crate::Error>(data)
    })();
    match result {
        Ok(mut data) => prepared(directory, &mut data, cache_only),
        Err(error) => {
            let _ = fs::remove_dir_all(directory);
            Err(error)
        }
    }
}
