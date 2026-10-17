//! YouTube file streams require bounded byte ranges. This loopback transport
//! adapts those responses to the ordinary HTTP ranges used by AVPlayer/Media3.
use crate::{
    Result,
    media::{container, dash, media_url, playback},
    string,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const CHUNK: u64 = 1_048_576;
pub(crate) struct Source {
    url: String,
    length: u64,
    user_agent: String,
    origin: String,
    mime: String,
    post: AtomicBool,
}
impl Source {
    pub(crate) fn length(&self) -> u64 {
        self.length
    }
    pub(crate) fn mime(&self) -> &str {
        &self.mime
    }
}
struct Proxy {
    port: u16,
    sources: Arc<Mutex<HashMap<String, Arc<Source>>>>,
    playlists: Arc<Mutex<HashMap<String, Arc<Playlist>>>>,
}
enum Playlist {
    Master(String),
    Dash(String),
    Indexed {
        source: Arc<Source>,
        stream: Value,
        uri: String,
        text: OnceLock<Result<String>>,
    },
}
impl Playlist {
    fn text(&self) -> Result<String> {
        match self {
            Self::Master(text) | Self::Dash(text) => Ok(text.clone()),
            Self::Indexed {
                source,
                stream,
                uri,
                text,
            } => text
                .get_or_init(|| {
                    let (start, end) = playback::range(&stream["indexRange"], source.length)
                        .ok_or("Missing MP4 segment index.")?;
                    let bytes = range_bytes(source, start, end)
                        .map_err(|_| "Could not load the video segment index.")?;
                    playback::playlist(stream, &bytes, uri)
                })
                .clone(),
        }
    }
}
static PROXY: OnceLock<Proxy> = OnceLock::new();
static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
static CLIENTS: AtomicUsize = AtomicUsize::new(0);
static REGISTRATIONS: AtomicUsize = AtomicUsize::new(0);
static REQUESTS: AtomicUsize = AtomicUsize::new(0);
static HTTP: OnceLock<Box<dyn crate::MediaHttp>> = OnceLock::new();
pub(super) fn install_http(backend: Box<dyn crate::MediaHttp>) {
    let _ = HTTP.set(backend);
}
static STATUS: OnceLock<Mutex<Value>> = OnceLock::new();
fn note(key: &str, value: Value) {
    STATUS
        .get_or_init(|| Mutex::new(json!({})))
        .lock()
        .unwrap_or_else(|e| e.into_inner())[key] = value;
}
pub(super) fn status() -> Value {
    STATUS
        .get_or_init(|| Mutex::new(json!({})))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}
pub(crate) fn range_bytes(source: &Source, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let agent = AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout_read(Duration::from_secs(15))
            .redirects(0)
            .build()
    });
    let mut last = "Media range unavailable.".to_string();
    let mut post = source.post.load(Ordering::Relaxed);
    for attempt in 0..3 {
        note("upstreamStart", json!(start));
        if let Some(backend) = HTTP.get() {
            let reply = backend.fetch_range(
                source.url.clone(),
                source.user_agent.clone(),
                source.origin.clone(),
                if post { "POST" } else { "GET" }.into(),
                start,
                end,
            );
            if matches!(reply.status, 200 | 206) && reply.body.len() as u64 == end - start + 1 {
                note("received", json!(end + 1));
                note("lastError", Value::Null);
                source.post.store(post, Ordering::Relaxed);
                return Ok(reply.body);
            }
            last = format!("Media HTTP {} ({} bytes).", reply.status, reply.body.len());
            note("lastError", json!(last));
            if reply.status == 403 {
                post = true;
            }
            thread::sleep(Duration::from_millis(150 * (attempt + 1)));
            continue;
        }
        let address = format!("{}&range={start}-{end}", source.url);
        let request = agent
            .request(if post { "POST" } else { "GET" }, &address)
            .set("User-Agent", &source.user_agent)
            .set("Accept-Encoding", "identity")
            .set("Referer", &format!("{}/", source.origin))
            .set("Origin", &source.origin);
        match if post {
            request.send_bytes(&[0x78, 0])
        } else {
            request.call()
        } {
            Ok(response) => {
                let mut bytes = Vec::with_capacity((end - start + 1) as usize);
                response
                    .into_reader()
                    .take(CHUNK + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 == end - start + 1 {
                    note("received", json!(end + 1));
                    note("lastError", Value::Null);
                    source.post.store(post, Ordering::Relaxed);
                    return Ok(bytes);
                }
                last = "Incomplete media range.".into();
            }
            Err(error) => {
                last = match error {
                    ureq::Error::Status(status, _) => {
                        if status == 403 {
                            post = true;
                        }
                        format!("Media HTTP {status}.")
                    }
                    ureq::Error::Transport(error) => {
                        let detail = error.message().unwrap_or("");
                        format!(
                            "Media {:?}: {}",
                            error.kind(),
                            if detail.contains("://") {
                                "connection failed"
                            } else {
                                detail
                            }
                        )
                    }
                };
                note("lastError", json!(last));
            }
        }
        thread::sleep(Duration::from_millis(150 * (attempt + 1)));
    }
    Err(io::Error::other(last))
}

/// A download owns one source across its parallel bounded requests. Addresses
/// remain inside Rust; the adapter receives only sandbox file paths.
pub(crate) struct DownloadSource(Source);
impl DownloadSource {
    pub(crate) fn new(stream: &Value, user_agent: &str, origin: &str) -> Result<Self> {
        let url = stream["url"]
            .as_str()
            .and_then(media_url)
            .ok_or("Invalid download source.")?;
        let length = stream["bytes"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 100_000_000_000)
            .ok_or("Missing media length.")?;
        if !matches!(
            origin,
            "https://youtube.com"
                | "https://www.youtube.com"
                | "https://m.youtube.com"
                | "https://music.youtube.com"
        ) || user_agent.len() > 1000
            || user_agent.contains(['\r', '\n'])
        {
            return Err("Invalid media context.".into());
        }
        Ok(Self(Source {
            url,
            length,
            user_agent: stream["userAgent"]
                .as_str()
                .filter(|s| s.len() <= 1000 && !s.contains(['\r', '\n']))
                .unwrap_or(user_agent)
                .into(),
            origin: origin.into(),
            mime: container(stream)
                .unwrap_or("application/octet-stream")
                .into(),
            post: AtomicBool::new(false),
        }))
    }
    pub(crate) fn bytes(&self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        if start > end || end >= self.0.length || end - start >= CHUNK {
            return Err(io::Error::other("Invalid download range."));
        }
        range_bytes(&self.0, start, end)
    }
}

struct Body {
    source: Arc<Source>,
    next: u64,
    end: u64,
    bytes: io::Cursor<Vec<u8>>,
}
impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let ready = self.bytes.read(buffer)?;
        if ready > 0 {
            return Ok(ready);
        }
        if self.next > self.end {
            return Ok(0);
        }
        let end = (self.next + CHUNK - 1).min(self.end);
        self.bytes = io::Cursor::new(range_bytes(&self.source, self.next, end)?);
        self.next = end + 1;
        self.bytes.read(buffer)
    }
}

fn serve(
    request: tiny_http::Request,
    sources: &Mutex<HashMap<String, Arc<Source>>>,
    playlists: &Mutex<HashMap<String, Arc<Playlist>>>,
) {
    note(
        "requests",
        json!(REQUESTS.fetch_add(1, Ordering::Relaxed) + 1),
    );
    if request.method() != &Method::Get && request.method() != &Method::Head {
        let _ = request.respond(Response::empty(StatusCode(405)));
        return;
    }
    let path = request.url().trim_start_matches('/');
    if let Some(key) = path
        .strip_suffix(".m3u8")
        .or_else(|| path.strip_suffix(".mpd"))
    {
        let playlist = playlists
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned();
        let Some(playlist) = playlist else {
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        };
        let response = match playlist.text() {
            Ok(text) => Response::from_string(text)
                .with_header(header(
                    "Content-Type",
                    if matches!(&*playlist, Playlist::Dash(_)) {
                        "application/dash+xml"
                    } else {
                        "application/vnd.apple.mpegurl"
                    },
                ))
                .with_header(header("Cache-Control", "no-store")),
            Err(_) => Response::from_string("Media playlist unavailable.")
                .with_status_code(StatusCode(502)),
        };
        let _ = request.respond(response);
        return;
    }
    let key = request
        .url()
        .trim_start_matches('/')
        .strip_suffix(".mp4")
        .or_else(|| path.strip_suffix(".webm"))
        .unwrap_or("");
    let source = sources
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned();
    let Some(source) = source else {
        let _ = request.respond(Response::empty(StatusCode(404)));
        return;
    };
    if request.method() != &Method::Get && request.method() != &Method::Head {
        let _ = request.respond(Response::empty(StatusCode(405)));
        return;
    }
    let requested = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_owned());
    note(
        "lastRequest",
        json!({"length":source.length,"range":requested,"method":request.method().as_str()}),
    );
    let bounds = requested
        .as_deref()
        .and_then(|range| range.strip_prefix("bytes="))
        .and_then(|value| {
            let (start, end) = value.split_once('-')?;
            if start.is_empty() {
                let tail = end.parse::<u64>().ok()?;
                if tail == 0 {
                    return None;
                }
                Some((source.length.saturating_sub(tail), source.length - 1))
            } else {
                Some((
                    start.parse::<u64>().ok()?,
                    if end.is_empty() {
                        source.length - 1
                    } else {
                        end.parse::<u64>().ok()?.min(source.length - 1)
                    },
                ))
            }
        });
    if requested.is_some()
        && (bounds.is_none()
            || bounds.is_some_and(|(start, end)| start > end || start >= source.length))
    {
        let _ = request.respond(Response::empty(StatusCode(416)).with_header(header(
            "Content-Range",
            &format!("bytes */{}", source.length),
        )));
        return;
    }
    let (start, end) = bounds.unwrap_or((0, source.length - 1));
    let mut headers = vec![
        header("Content-Type", &source.mime),
        header("Accept-Ranges", "bytes"),
        header("Cache-Control", "no-store"),
        header("Connection", "close"),
    ];
    if requested.is_some() {
        headers.push(header(
            "Content-Range",
            &format!("bytes {start}-{end}/{}", source.length),
        ));
    }
    let body = Body {
        source,
        next: start,
        end,
        bytes: io::Cursor::new(Vec::new()),
    };
    let head = request.method() == &Method::Head;
    let mut writer = request.into_writer();
    let status = if requested.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    if write!(
        writer,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n",
        end - start + 1
    )
    .is_err()
    {
        return;
    }
    for field in headers {
        if write!(writer, "{field}\r\n").is_err() {
            return;
        }
    }
    if write!(writer, "\r\n").and_then(|_| writer.flush()).is_err() {
        return;
    }
    if !head {
        let _ = io::copy(&mut { body }, &mut writer);
    }
    let _ = writer.flush();
}

pub(crate) fn source(command: &Value) -> Result<Arc<Source>> {
    let url = media_url(string(command, "url")?).ok_or("Untrusted media source.")?;
    let length = command["length"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 100_000_000_000)
        .ok_or("Missing media length.")?;
    let user_agent = string(command, "userAgent")?;
    let origin = string(command, "origin")?;
    if !matches!(
        origin,
        "https://www.youtube.com"
            | "https://m.youtube.com"
            | "https://youtube.com"
            | "https://music.youtube.com"
    ) {
        return Err("Invalid media origin.".into());
    }
    if user_agent.len() > 1000 || user_agent.contains(['\r', '\n']) {
        return Err("Invalid media agent.".into());
    }
    Ok(Arc::new(Source {
        url,
        length,
        user_agent: user_agent.into(),
        origin: origin.into(),
        mime: container(command).unwrap_or("video/mp4").into(),
        post: AtomicBool::new(false),
    }))
}

fn server() -> Result<&'static Proxy> {
    if PROXY.get().is_none() {
        let server = Server::http("127.0.0.1:0").map_err(|_| "Could not start media transport.")?;
        let port = server
            .server_addr()
            .to_ip()
            .ok_or("Invalid local transport.")?
            .port();
        let sources = Arc::new(Mutex::new(HashMap::new()));
        let playlists = Arc::new(Mutex::new(HashMap::new()));
        let source_copy = sources.clone();
        let playlist_copy = playlists.clone();
        PROXY
            .set(Proxy {
                port,
                sources,
                playlists,
            })
            .map_err(|_| "Media transport already started.")?;
        thread::spawn(move || {
            for request in server.incoming_requests() {
                if CLIENTS.fetch_add(1, Ordering::SeqCst) >= 12 {
                    CLIENTS.fetch_sub(1, Ordering::SeqCst);
                    let _ = request.respond(Response::empty(StatusCode(503)));
                    continue;
                }
                let sources = source_copy.clone();
                let playlists = playlist_copy.clone();
                thread::spawn(move || {
                    serve(request, &sources, &playlists);
                    CLIENTS.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
    }
    Ok(PROXY.get().unwrap())
}

fn token(url: &str) -> String {
    // Signed source URLs contain high-entropy session material; their hash is
    // an opaque capability. Never expose the signed URL on the loopback route.
    let unique = format!("{url}:{}", REGISTRATIONS.fetch_add(1, Ordering::SeqCst));
    format!("{:x}", Sha256::digest(unique.as_bytes()))
}

pub(super) fn proxy(command: &Value) -> Result<Value> {
    let source = source(command)?;
    let server = server()?;
    let token = token(&source.url);
    let mut sources = server.sources.lock().unwrap_or_else(|e| e.into_inner());
    if sources.len() >= 32 {
        return Err("Too many active media streams. Close the player and try again.".into());
    }
    sources.insert(token.clone(), source);
    Ok(json!(format!(
        "http://127.0.0.1:{}/{}.{}",
        server.port,
        token,
        if container(command).is_some_and(|mime| mime.ends_with("/webm")) {
            "webm"
        } else {
            "mp4"
        }
    )))
}

pub(super) fn playback(command: &Value) -> Result<Value> {
    let catalog = &command["catalog"];
    if catalog["live"] == true {
        let url = catalog["hlsUrl"]
            .as_str()
            .and_then(media_url)
            .ok_or("No playable live manifest is available. Refresh the video.")?;
        let agent = catalog["hlsUserAgent"]
            .as_str()
            .filter(|agent| !agent.is_empty())
            .or_else(|| command["userAgent"].as_str())
            .filter(|s| s.len() <= 1000 && !s.contains(['\r', '\n']))
            .unwrap_or("");
        return Ok(
            json!({"uri":url,"contentType":"hls","qualityHeight":0,"remote":true,"headers":{"User-Agent":agent,"Referer":format!("{}/",catalog["origin"].as_str().unwrap_or("https://m.youtube.com")),"Origin":catalog["origin"]}}),
        );
    }
    let videos = playback::videos(catalog)?;
    let fps = command["fps"].as_u64().unwrap_or(0);
    let mut requested = command["height"]
        .as_u64()
        .filter(|n| *n <= 8640)
        .ok_or("Invalid playback resolution.")?;
    if requested != 0
        && command["allowFallback"] == true
        && !videos.iter().any(|s| s["height"] == requested)
    {
        requested = 0;
    }
    let selected = if requested == 0 {
        videos[0]
    } else {
        videos
            .iter()
            .copied()
            .find(|s| s["height"] == requested && (fps == 0 || s["fps"] == fps))
            .or_else(|| {
                (command["allowFallback"] == true)
                    .then(|| videos.iter().copied().find(|s| s["height"] == requested))
                    .flatten()
            })
            .ok_or("Selected resolution is unavailable. Refresh the video.")?
    };
    let file_source = |stream: &Value| {
        source(&json!({"url":stream["url"],"length":stream["bytes"],
        "mimeType":stream["mimeType"],
        "origin":catalog["origin"],"userAgent":stream["userAgent"].as_str().filter(|s| !s.is_empty())
            .unwrap_or(command["userAgent"].as_str().unwrap_or(""))}))
    };
    if selected["kind"] == "combined" {
        let stream = file_source(selected)?;
        let uri = proxy(
            &json!({"url":stream.url,"length":stream.length,"origin":stream.origin,"userAgent":stream.user_agent,"mimeType":stream.mime}),
        )?;
        return Ok(
            json!({"uri":uri,"contentType":"progressive","qualityHeight":requested,"qualityFps":selected["fps"].as_u64().unwrap_or(0)}),
        );
    }
    let audio = playback::audio_for(catalog, command["audioKey"].as_str().unwrap_or(""))
        .or_else(|| {
            (command["allowFallback"] == true)
                .then(|| playback::audio(catalog))
                .flatten()
        })
        .ok_or("No compatible audio track is available.")?;
    let server = server()?;
    let root = token(selected["url"].as_str().unwrap_or(""));
    let mut files = Vec::new();
    let mut lists = Vec::new();
    if command["manifest"] == "dash" {
        let variants: Vec<_> = if requested == 0 {
            videos
                .into_iter()
                .filter(|s| s["kind"] == "video")
                .collect()
        } else {
            vec![selected]
        };
        let mut tracks = Vec::new();
        for (index, stream) in std::iter::once(audio).chain(variants).enumerate() {
            let key = format!("{root}-{index}");
            let extension = if container(stream).is_some_and(|mime| mime.ends_with("/webm")) {
                "webm"
            } else {
                "mp4"
            };
            tracks.push((stream, format!("{key}.{extension}")));
            files.push((key, file_source(stream)?));
        }
        let text = dash::manifest(catalog["duration"].as_f64().unwrap_or(0.0), &tracks)?;
        let mut sources = server.sources.lock().unwrap_or_else(|e| e.into_inner());
        let mut playlists = server.playlists.lock().unwrap_or_else(|e| e.into_inner());
        if sources.len() + files.len() > 96 || playlists.len() >= 100 {
            return Err("Too many active media streams. Close the player and try again.".into());
        }
        sources.extend(files);
        playlists.insert(root.clone(), Arc::new(Playlist::Dash(text)));
        return Ok(
            json!({"uri":format!("http://127.0.0.1:{}/{root}.mpd",server.port),"contentType":"dash","qualityHeight":requested,"qualityFps":selected["fps"].as_u64().unwrap_or(0)}),
        );
    }
    if container(audio) != Some("audio/mp4") || container(selected) != Some("video/mp4") {
        return Err("This playback format requires DASH support.".into());
    }
    let mut add_track = |stream: &Value, name: &str| -> Result<String> {
        let key = format!("{root}-{name}");
        let file = file_source(stream)?;
        lists.push((
            key.clone(),
            Arc::new(Playlist::Indexed {
                source: file.clone(),
                stream: stream.clone(),
                uri: format!("{key}.mp4"),
                text: OnceLock::new(),
            }),
        ));
        files.push((key.clone(), file));
        Ok(format!("{key}.m3u8"))
    };
    let audio_uri = add_track(audio, "audio")?;
    let mut master = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"Audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"{audio_uri}\"\n"
    );
    let variants: Vec<_> = if requested == 0 {
        videos
            .into_iter()
            .filter(|s| s["kind"] == "video" && container(s) == Some("video/mp4"))
            .collect()
    } else {
        vec![selected]
    };
    for stream in variants {
        let uri = add_track(
            stream,
            &format!(
                "video-{}-{}",
                stream["height"],
                stream["fps"].as_u64().unwrap_or(0)
            ),
        )?;
        let bandwidth = stream["bitrate"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(audio["bitrate"].as_u64().unwrap_or(0))
            .max(1);
        let width = stream["width"].as_u64().filter(|n| *n > 0 && *n <= 15360);
        let resolution = width
            .map(|w| format!(",RESOLUTION={w}x{}", stream["height"]))
            .unwrap_or_default();
        master.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH={bandwidth},CODECS=\"{},{}\",AUDIO=\"audio\"{resolution}\n{uri}\n", playback::codecs(stream)?, playback::codecs(audio)?));
    }
    lists.push((root.clone(), Arc::new(Playlist::Master(master))));
    let mut sources = server.sources.lock().unwrap_or_else(|e| e.into_inner());
    let mut playlists = server.playlists.lock().unwrap_or_else(|e| e.into_inner());
    if sources.len() + files.len() > 96 || playlists.len() + lists.len() > 100 {
        return Err("Too many active media streams. Close the player and try again.".into());
    }
    sources.extend(files);
    playlists.extend(lists);
    Ok(
        json!({"uri":format!("http://127.0.0.1:{}/{root}.m3u8",server.port),"contentType":"hls","qualityHeight":requested,"qualityFps":selected["fps"].as_u64().unwrap_or(0)}),
    )
}

pub(super) fn release(command: &Value) -> Result<Value> {
    if let Some(server) = PROXY.get() {
        let uri =
            url::Url::parse(string(command, "uri")?).map_err(|_| "Invalid local media address.")?;
        if uri.scheme() != "http"
            || uri.host_str() != Some("127.0.0.1")
            || uri.port() != Some(server.port)
        {
            return Err("Invalid local media address.".into());
        }
        if let Some(key) = uri
            .path()
            .trim_start_matches('/')
            .strip_suffix(".mp4")
            .or_else(|| uri.path().trim_start_matches('/').strip_suffix(".webm"))
            .or_else(|| uri.path().trim_start_matches('/').strip_suffix(".mpd"))
            .or_else(|| uri.path().trim_start_matches('/').strip_suffix(".m3u8"))
        {
            let prefix = format!("{key}-");
            server
                .sources
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|name, _| name != key && !name.starts_with(&prefix));
            server
                .playlists
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|name, _| name != key && !name.starts_with(&prefix));
        }
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::playback::tests::{index, stream};

    struct FixtureHttp;
    impl crate::MediaHttp for FixtureHttp {
        fn fetch_range(
            &self,
            _url: String,
            agent: String,
            origin: String,
            _method: String,
            start: u64,
            end: u64,
        ) -> crate::MediaRangeReply {
            assert_eq!(agent, "test-client");
            assert_eq!(origin, "https://m.youtube.com");
            let body = if (start, end) == (100, 155) {
                index(0)
            } else {
                (start..=end).map(|n| n as u8).collect()
            };
            crate::MediaRangeReply { status: 206, body }
        }
    }

    #[test]
    fn hls_routes_stream_ranges_switch_quality_and_revoke_together() {
        install_http(Box::new(FixtureHttp));
        let catalog = json!({"origin":"https://m.youtube.com","streams":[stream("video",1080),stream("video",360),stream("audio",0)]});
        let mut command = json!({"catalog":catalog,"height":2160,"userAgent":"fallback"});
        assert!(playback(&command).is_err());
        command["allowFallback"] = json!(true);
        let source = playback(&command).unwrap();
        assert_eq!(source["contentType"], "hls");
        assert_eq!(source["qualityHeight"], 0);
        let uri = source["uri"].as_str().unwrap();
        let master = ureq::get(uri).call().unwrap().into_string().unwrap();
        assert!(master.contains("RESOLUTION=2160x1080"));
        assert!(master.contains("RESOLUTION=720x360"));
        assert!(!master.contains("private"));
        assert!(!master.contains("googlevideo"));
        let audio = master
            .lines()
            .find(|line| line.starts_with("#EXT-X-MEDIA:"))
            .unwrap()
            .split("URI=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let base = url::Url::parse(uri).unwrap();
        let audio = base.join(audio).unwrap();
        let playlist = ureq::get(audio.as_str())
            .call()
            .unwrap()
            .into_string()
            .unwrap();
        assert!(playlist.contains("#EXT-X-BYTERANGE:200@156"));
        let file = base
            .join(
                playlist
                    .lines()
                    .find(|line| line.ends_with(".mp4"))
                    .unwrap(),
            )
            .unwrap();
        let response = ureq::get(file.as_str())
            .set("Range", "bytes=10-19")
            .call()
            .unwrap();
        assert_eq!(response.status(), 206);
        assert_eq!(response.header("Content-Range"), Some("bytes 10-19/556"));
        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, (10u8..20).collect::<Vec<_>>());
        command["height"] = json!(360);
        let pinned = playback(&command).unwrap();
        assert_eq!(pinned["qualityHeight"], 360);
        let pinned_master = ureq::get(pinned["uri"].as_str().unwrap())
            .call()
            .unwrap()
            .into_string()
            .unwrap();
        assert!(pinned_master.contains("RESOLUTION=720x360"));
        assert!(!pinned_master.contains("RESOLUTION=2160x1080"));
        release(&json!({"uri":uri})).unwrap();
        for revoked in [uri, audio.as_str(), file.as_str()] {
            assert!(matches!(
                ureq::get(revoked).call(),
                Err(ureq::Error::Status(404, _))
            ));
        }
        assert!(ureq::get(pinned["uri"].as_str().unwrap()).call().is_ok());
        release(&json!({"uri":pinned["uri"]})).unwrap();
    }
}
