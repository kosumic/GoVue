//! Incremental HTTPS bodies use bounded byte reads and opaque response handles.
//! Signed addresses remain in native memory, never in persistent storage.
use super::{CHUNK, Source};
use crate::MediaStreamHttp;
use crate::media;
use crate::transport;
use serde_json::json;
use std::io::Read;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::{io, thread};

static HTTP: OnceLock<Box<dyn MediaStreamHttp>> = OnceLock::new();

pub(crate) fn install(backend: Box<dyn MediaStreamHttp>) {
    let _ = HTTP.set(backend);
}
pub(super) fn available() -> bool {
    HTTP.get().is_some()
}

struct Body {
    backend: &'static dyn MediaStreamHttp,
    handle: u64,
    remaining: u64,
}
impl Drop for Body {
    fn drop(&mut self) {
        if self.handle != 0 {
            self.backend.close(self.handle);
        }
    }
}
impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.handle == 0 {
            return Ok(0);
        }
        let limit = usize::try_from(self.remaining.min(65_536))
            .unwrap_or(65_536)
            .min(buffer.len())
            .max(1);
        let bytes = self
            .backend
            .read(self.handle, u32::try_from(limit).unwrap_or(65_536));
        if bytes.len() > limit || bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("Media range exceeded its bounds."));
        }
        if bytes.is_empty() {
            if self.remaining != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Incomplete media range.",
                ));
            }
            self.backend.close(self.handle);
            self.handle = 0;
            return Ok(0);
        }
        buffer[..bytes.len()].copy_from_slice(&bytes);
        self.remaining -= bytes.len() as u64;
        Ok(bytes.len())
    }
}

pub(super) fn open(source: &Source, start: u64, end: u64) -> io::Result<Box<dyn Read + Send>> {
    if start > end || end >= source.length || end - start >= CHUNK {
        return Err(io::Error::other("Invalid media range."));
    }
    let Some(backend) = HTTP.get() else {
        return Ok(Box::new(io::Cursor::new(transport::range_bytes(
            source, start, end,
        )?)));
    };
    let mut url = source
        .redirected
        .lock()
        .map_err(|_| io::Error::other("Media transport state is unavailable. Restart GoVue."))?
        .clone()
        .unwrap_or_else(|| source.url.clone());
    let mut post = source.post.load(Ordering::Relaxed);
    let mut redirects = 0;
    let mut attempt = 0;
    while attempt < 3 {
        transport::note("upstreamStart", json!(start));
        let reply = backend.open_range(
            url.clone(),
            source.user_agent.clone(),
            source.origin.clone(),
            if post { "POST" } else { "GET" }.into(),
            start,
            end,
        );
        let body = Body {
            backend: &**backend,
            handle: reply.handle,
            remaining: end - start + 1,
        };
        if matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            let _span = crate::trace::Section::begin(c"GoVue transport: redirect");
            redirects += 1;
            let next = (redirects <= 4)
                .then_some(reply.location.as_deref())
                .flatten()
                .filter(|v| {
                    !v.is_empty()
                        && v.len() <= 30_000
                        && v.trim() == *v
                        && !v.chars().any(char::is_control)
                })
                .and_then(|v| url::Url::parse(&url).ok()?.join(v).ok())
                .and_then(|v| media::media_url(v.as_str()))
                .ok_or_else(|| io::Error::other("Invalid media redirect."))?;
            if matches!(reply.status, 301..=303) {
                post = false;
            }
            url = next;
            continue;
        }
        if matches!(reply.status, 200 | 206)
            && reply.handle != 0
            && (reply.length == -1 || u64::try_from(reply.length).ok() == Some(end - start + 1))
        {
            source.post.store(post, Ordering::Relaxed);
            if url != source.url {
                *source.redirected.lock().map_err(|_| {
                    io::Error::other("Media transport state is unavailable. Restart GoVue.")
                })? = Some(url);
            }
            transport::note("lastError", serde_json::Value::Null);
            return Ok(Box::new(body));
        }
        transport::note("lastError", json!(format!("Media HTTP {}.", reply.status)));
        if reply.status == 403 {
            post = true;
        }
        attempt += 1;
        if attempt < 3 {
            thread::sleep(Duration::from_millis(150 * attempt));
        }
    }
    Err(io::Error::other("Media range unavailable."))
}
