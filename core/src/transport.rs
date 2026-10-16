//! YouTube file streams require bounded byte ranges. This loopback transport
//! adapts those responses to the ordinary HTTP ranges used by AVPlayer/Media3.
use crate::{media::media_url, string, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const CHUNK: u64 = 1_048_576;
struct Source {
    url: String,
    length: u64,
    user_agent: String,
    origin: String,
    post: AtomicBool,
}
struct Proxy {
    port: u16,
    sources: Arc<Mutex<HashMap<String, Arc<Source>>>>,
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
fn range_bytes(source: &Source, start: u64, end: u64) -> io::Result<Vec<u8>> {
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

fn serve(request: tiny_http::Request, sources: &Mutex<HashMap<String, Arc<Source>>>) {
    note(
        "requests",
        json!(REQUESTS.fetch_add(1, Ordering::Relaxed) + 1),
    );
    let key = request
        .url()
        .trim_start_matches('/')
        .strip_suffix(".mp4")
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
        header("Content-Type", "video/mp4"),
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

pub(super) fn proxy(command: &Value) -> Result<Value> {
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
    if PROXY.get().is_none() {
        let server = Server::http("127.0.0.1:0").map_err(|_| "Could not start media transport.")?;
        let port = server
            .server_addr()
            .to_ip()
            .ok_or("Invalid local transport.")?
            .port();
        let sources = Arc::new(Mutex::new(HashMap::new()));
        let source_copy = sources.clone();
        PROXY
            .set(Proxy { port, sources })
            .map_err(|_| "Media transport already started.")?;
        thread::spawn(move || {
            for request in server.incoming_requests() {
                if CLIENTS.fetch_add(1, Ordering::SeqCst) >= 12 {
                    CLIENTS.fetch_sub(1, Ordering::SeqCst);
                    let _ = request.respond(Response::empty(StatusCode(503)));
                    continue;
                }
                let sources = source_copy.clone();
                thread::spawn(move || {
                    serve(request, &sources);
                    CLIENTS.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
    }
    let server = PROXY.get().unwrap();
    // Signed source URLs contain high-entropy session material; their hash is
    // an opaque capability. Never expose the signed URL on the loopback route.
    let unique = format!("{url}:{}", REGISTRATIONS.fetch_add(1, Ordering::SeqCst));
    let token = format!("{:x}", Sha256::digest(unique.as_bytes()));
    let mut sources = server.sources.lock().unwrap_or_else(|e| e.into_inner());
    if sources.len() >= 32 {
        return Err("Too many active media streams. Close the player and try again.".into());
    }
    sources.insert(
        token.clone(),
        Arc::new(Source {
            url,
            length,
            user_agent: user_agent.into(),
            origin: origin.into(),
            post: AtomicBool::new(false),
        }),
    );
    Ok(json!(format!(
        "http://127.0.0.1:{}/{}.mp4",
        server.port, token
    )))
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
        if let Some(key) = uri.path().trim_start_matches('/').strip_suffix(".mp4") {
            server
                .sources
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(key);
        }
    }
    Ok(Value::Null)
}
