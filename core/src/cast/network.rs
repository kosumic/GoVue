//! Separate, revocable LAN listener. Never changes the loopback player proxy.
use super::manifest::Bundle;
use crate::{Result, transport};
use std::{
    io::{Read, Write},
    net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub struct Transport {
    pub base: String,
    pub media: String,
    pub mime: &'static str,
    live: AtomicBool,
    peers: Mutex<Vec<(usize, TcpStream)>>,
    serial: AtomicUsize,
    requests: AtomicUsize,
    last_media: Mutex<Option<Instant>>,
}
impl Transport {
    pub fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
        for (_, socket) in self
            .peers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
        {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
    pub fn active(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    pub fn connected(&self) -> bool {
        self.active()
            && self
                .last_media
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(30))
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn private(address: &str) -> Option<IpAddr> {
    let ip: IpAddr = address.parse().ok()?;
    match ip {
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => Some(IpAddr::V4(ip)),
        IpAddr::V6(ip) if (ip.segments()[0] & 0xfe00) == 0xfc00 => Some(IpAddr::V6(ip)),
        _ => None,
    }
}
pub fn start(
    address: IpAddr,
    receiver: Option<IpAddr>,
    bundle: Bundle,
    title: &str,
    position: f64,
) -> Result<Arc<Transport>> {
    let listener = TcpListener::bind(SocketAddr::new(address, 0))
        .map_err(|_| "Could not open the Cast link on Wi-Fi.")?;
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        // A VPN can forbid explicit network selection (EPERM). The listener
        // remains bound to this exact private Wi-Fi address; in that case let
        // Android's ordinary routing policy apply rather than bypassing it.
        let _ = super::platform()?.bind_wifi_socket(
            u32::try_from(listener.as_raw_fd()).map_err(|_| "Invalid Cast listener socket.")?,
            address.to_string(),
        );
    }
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not start the Cast listener.")?;
    let endpoint = listener.local_addr().map_err(|_| "Missing Cast address.")?;
    let capability = super::random()?;
    let root = format!("/{capability}/");
    let base = format!("http://{endpoint}{root}");
    let progressive = bundle.manifest.is_empty();
    let file = if progressive {
        "media.mp4"
    } else {
        "manifest.mpd"
    };
    let mime = if progressive {
        "video/mp4"
    } else {
        "application/dash+xml"
    };
    let title = html_escape::encode_text(title);
    let page = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>*{{box-sizing:border-box}}body{{margin:0;background:#000;color:#fff;font:15px system-ui;height:100dvh;display:flex;flex-direction:column}}header{{padding:12px 16px;background:#111;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}}video{{flex:1;min-height:0;width:100%;object-fit:contain}}p{{padding:16px}}</style><header>{title}</header><video controls autoplay playsinline data-file=\"{file}\" data-start=\"{position:.3}\"></video><p hidden>Could not play this video. Keep GoVue open on the same Wi-Fi network.</p><script src=\"dash.all.min.js\"></script><script src=\"player.js\"></script></html>"
    );
    let state = Arc::new(Transport {
        base: base.clone(),
        media: format!("{base}{file}"),
        mime,
        live: AtomicBool::new(true),
        peers: Mutex::new(Vec::new()),
        serial: AtomicUsize::new(0),
        requests: AtomicUsize::new(0),
        last_media: Mutex::new(None),
    });
    let weak = Arc::downgrade(&state);
    let bundle = Arc::new(bundle);
    thread::spawn(move || {
        let end = Instant::now() + Duration::from_hours(4);
        loop {
            let Some(state) = weak
                .upgrade()
                .filter(|s| s.active() && Instant::now() < end)
            else {
                break;
            };
            match listener.accept() {
                Ok((socket, peer)) => {
                    if receiver.is_some_and(|ip| ip != peer.ip())
                        || private(&peer.ip().to_string()).is_none()
                        || state.requests.fetch_add(1, Ordering::SeqCst) >= 8
                    {
                        // Decrement only if the slot was actually acquired.
                        if receiver.is_none_or(|ip| ip == peer.ip())
                            && private(&peer.ip().to_string()).is_some()
                        {
                            state.requests.fetch_sub(1, Ordering::SeqCst);
                        }
                        let _ = socket.shutdown(Shutdown::Both);
                        continue;
                    }
                    let id = state.serial.fetch_add(1, Ordering::SeqCst);
                    if let Ok(copy) = socket.try_clone() {
                        state
                            .peers
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((id, copy));
                    }
                    let root = root.clone();
                    let bundle = bundle.clone();
                    let page = page.clone();
                    thread::spawn(move || {
                        let _ = serve(
                            socket,
                            &state,
                            &bundle,
                            &root,
                            &endpoint.to_string(),
                            receiver.is_some(),
                            &page,
                        );
                        state
                            .peers
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .retain(|(n, _)| *n != id);
                        state.requests.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
        if let Some(state) = weak.upgrade() {
            state.close();
        }
    });
    Ok(state)
}

fn reply(
    socket: &mut TcpStream,
    status: &str,
    mime: &str,
    bytes: &[u8],
    head: bool,
    cors: bool,
) -> std::io::Result<()> {
    headers(socket, status, mime, bytes.len() as u64, None, cors)?;
    if !head {
        socket.write_all(bytes)?;
    }
    Ok(())
}
fn headers(
    socket: &mut TcpStream,
    status: &str,
    mime: &str,
    length: u64,
    range: Option<String>,
    cors: bool,
) -> std::io::Result<()> {
    write!(
        socket,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {length}\r\nConnection: close\r\nCache-Control: no-store\r\nAccept-Ranges: bytes\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'\r\n"
    )?;
    if let Some(range) = range {
        write!(socket, "Content-Range: {range}\r\n")?;
    }
    if cors {
        write!(
            socket,
            "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, HEAD, OPTIONS\r\nAccess-Control-Allow-Headers: Range, Content-Type\r\nAccess-Control-Expose-Headers: Content-Range, Accept-Ranges, Content-Length\r\n"
        )?;
    }
    write!(socket, "\r\n")
}
#[allow(
    clippy::too_many_lines,
    reason = "All receiver routes share the same authenticated connection and response lifecycle."
)]
fn serve(
    mut socket: TcpStream,
    state: &Transport,
    bundle: &Bundle,
    root: &str,
    host: &str,
    receiver: bool,
    page: &str,
) -> std::io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(10)))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut data = Vec::new();
    while !data.ends_with(b"\r\n\r\n") {
        if !state.active() || data.len() >= 8192 || Instant::now() > deadline {
            return Ok(());
        }
        let mut byte = [0];
        if socket.read(&mut byte)? == 0 {
            return Ok(());
        }
        data.push(byte[0]);
    }
    let Ok(text) = std::str::from_utf8(&data) else {
        return Ok(());
    };
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap_or("").split(' ').collect::<Vec<_>>();
    if line.len() != 3 || !matches!(line[2], "HTTP/1.1" | "HTTP/1.0") {
        return Ok(());
    }
    let mut fields = std::collections::HashMap::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let Some((key, value)) = line.split_once(':') else {
            return Ok(());
        };
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.chars().any(|c| c.is_control() && c != '\t')
            || fields
                .insert(key.to_ascii_lowercase(), value.trim())
                .is_some()
        {
            return Ok(());
        }
    }
    let head = line[0] == "HEAD";
    if fields.get("host") != Some(&host)
        || fields.contains_key("transfer-encoding")
        || fields.get("content-length").is_some_and(|n| *n != "0")
        || fields.contains_key("expect")
    {
        return reply(
            &mut socket,
            "400 Bad Request",
            "text/plain",
            b"Invalid request.",
            head,
            false,
        );
    }
    let Some(path) = line[1].strip_prefix(root).filter(|s| {
        !s.is_empty()
            && s.len() < 100
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    }) else {
        return reply(
            &mut socket,
            "404 Not Found",
            "text/plain",
            b"Link unavailable.",
            head,
            false,
        );
    };
    if !receiver
        && fields
            .get("origin")
            .is_some_and(|origin| *origin != format!("http://{host}"))
    {
        return reply(
            &mut socket,
            "403 Forbidden",
            "text/plain",
            b"Origin unavailable.",
            head,
            false,
        );
    }
    let known = bundle.parts.contains_key(path)
        || matches!(
            path,
            "manifest.mpd"
                | "player"
                | "player.js"
                | "dash.all.min.js"
                | "dash.all.min.js.LICENSE.txt"
        );
    if !known {
        return reply(
            &mut socket,
            "404 Not Found",
            "text/plain",
            b"Link unavailable.",
            head,
            receiver,
        );
    }
    if line[0] == "OPTIONS" {
        return reply(
            &mut socket,
            "204 No Content",
            "text/plain",
            b"",
            true,
            receiver,
        );
    }
    if !matches!(line[0], "GET" | "HEAD") {
        return reply(
            &mut socket,
            "405 Method Not Allowed",
            "text/plain",
            b"Method unavailable.",
            false,
            receiver,
        );
    }
    if !head && (path == "manifest.mpd" || bundle.parts.contains_key(path)) {
        *state
            .last_media
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    }
    match path {
        "player" if !receiver => {
            return reply(
                &mut socket,
                "200 OK",
                "text/html; charset=utf-8",
                page.as_bytes(),
                head,
                false,
            );
        }
        "player.js" if !receiver => {
            return reply(
                &mut socket,
                "200 OK",
                "application/javascript",
                include_bytes!("assets/player.js"),
                head,
                false,
            );
        }
        "dash.all.min.js" if !receiver => {
            return reply(
                &mut socket,
                "200 OK",
                "application/javascript",
                include_bytes!("assets/dash.all.min.js"),
                head,
                false,
            );
        }
        "dash.all.min.js.LICENSE.txt" if !receiver => {
            return reply(
                &mut socket,
                "200 OK",
                "text/plain",
                include_bytes!("assets/dash.all.min.js.LICENSE.txt"),
                head,
                false,
            );
        }
        "manifest.mpd" if !bundle.manifest.is_empty() => {
            return reply(
                &mut socket,
                "200 OK",
                "application/dash+xml",
                bundle.manifest.as_bytes(),
                head,
                receiver,
            );
        }
        _ => {}
    }
    let Some(part) = bundle.parts.get(path) else {
        return reply(
            &mut socket,
            "404 Not Found",
            "text/plain",
            b"Link unavailable.",
            head,
            receiver,
        );
    };
    let length = part.end - part.start + 1;
    let bounds = fields
        .get("range")
        .map(|s| range(s, length))
        .unwrap_or(Some((0, length - 1)));
    let Some((start, end)) = bounds else {
        headers(
            &mut socket,
            "416 Range Not Satisfiable",
            "text/plain",
            0,
            Some(format!("bytes */{length}")),
            receiver,
        )?;
        return Ok(());
    };
    headers(
        &mut socket,
        if fields.contains_key("range") {
            "206 Partial Content"
        } else {
            "200 OK"
        },
        part.source.mime(),
        end - start + 1,
        fields
            .contains_key("range")
            .then(|| format!("bytes {start}-{end}/{length}")),
        receiver,
    )?;
    if head {
        return Ok(());
    }
    let mut next = part.start + start;
    while next <= part.start + end && state.active() {
        let last = (next + 1_048_575).min(part.start + end);
        let bytes = transport::range_bytes(&part.source, next, last)?;
        if !state.active() {
            break;
        }
        socket.write_all(&bytes)?;
        next = last + 1;
    }
    Ok(())
}
pub fn range(raw: &str, length: u64) -> Option<(u64, u64)> {
    if length == 0 {
        return None;
    }
    let (a, b) = raw.strip_prefix("bytes=")?.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (start, end) = if a.is_empty() {
        if !digits(b) {
            return None;
        }
        let tail = b.parse::<u64>().ok()?;
        if tail == 0 {
            return None;
        }
        (length.saturating_sub(tail), length - 1)
    } else {
        if !digits(a) || (!b.is_empty() && !digits(b)) {
            return None;
        }
        (
            a.parse::<u64>().ok()?,
            if b.is_empty() {
                length - 1
            } else {
                b.parse::<u64>().ok()?.min(length - 1)
            },
        )
    };
    (start <= end && start < length).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_private_numeric_lan_addresses_are_accepted() {
        for ip in [
            "192.168.1.2",
            "10.1.2.3",
            "172.16.0.2",
            "169.254.1.2",
            "fd12::3",
        ] {
            assert!(private(ip).is_some(), "{ip}");
        }
        for ip in [
            "localhost",
            "cast.local",
            "127.0.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "192.168.1.2:8009",
            "192.168.1.2.evil.example",
            "100.64.0.1",
            "::1",
            "2001:db8::1",
        ] {
            assert!(private(ip).is_none(), "{ip}");
        }
    }
    #[test]
    fn ranges_are_single_bounded_and_relative_to_the_registered_part() {
        assert_eq!(range("bytes=0-", 0), None);
        assert_eq!(range("bytes=0-63", 100), Some((0, 63)));
        assert_eq!(range("bytes=50-", 100), Some((50, 99)));
        assert_eq!(range("bytes=-20", 100), Some((80, 99)));
        assert_eq!(range("bytes=-200", 100), Some((0, 99)));
        assert_eq!(range("bytes=50-200", 100), Some((50, 99)));
        for raw in [
            "bytes=0-5,10-15",
            "bytes=100-",
            "bytes=90-80",
            "bytes=-0",
            "bytes=+0-1",
            "bytes=0-18446744073709551616",
            "bytes= 0-1",
            "items=0-1",
            "bytes=--1",
        ] {
            assert_eq!(range(raw, 100), None, "{raw}");
        }
    }
}
