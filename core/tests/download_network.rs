#![cfg(test)]

use govue_core::{DownloadHttp, DownloadRequest, MediaRangeReply, execute, install_download_http};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

const CHUNK: u64 = 1_048_576;
struct Pending {
    request: DownloadRequest,
    reply: Sender<MediaRangeReply>,
}
impl Pending {
    fn respond(self, status: i32) {
        let body = if status == 206 {
            let range = self.request.range.as_ref().unwrap();
            vec![
                u8::try_from(range.start / CHUNK).unwrap();
                usize::try_from(range.end - range.start + 1).unwrap()
            ]
        } else if status == 200 {
            vec![0xff, 0xd8, 0xff]
        } else {
            vec![]
        };
        self.reply
            .send(MediaRangeReply {
                status,
                body,
                location: (status == 302)
                    .then(|| "https://r2.googlevideo.com/videoplayback".into()),
            })
            .unwrap();
    }
}
struct Backend {
    wifi: Arc<AtomicBool>,
    requests: Mutex<Sender<Pending>>,
}
impl DownloadHttp for Backend {
    fn wifi_connected(&self) -> bool {
        self.wifi.load(Ordering::SeqCst)
    }
    fn fetch(&self, request: DownloadRequest) -> MediaRangeReply {
        let (reply, response) = mpsc::channel();
        self.requests
            .lock()
            .unwrap()
            .send(Pending { request, reply })
            .unwrap();
        response.recv_timeout(Duration::from_secs(5)).unwrap()
    }
}
struct Download {
    root: PathBuf,
    id: Value,
}
impl Download {
    fn start(name: &str, connections: u64, chunks: u64, cover: bool, wifi_only: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("govue-network-{}-{name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut download = Self {
            root,
            id: Value::Null,
        };
        download.call(json!({"op":"downloads_config","settings":{"wifiOnly":wifi_only,"maxConnections":connections}}));
        download.id = download.call(json!({"op":"downloads_enqueue","items":[{"videoId":"abcdefghijk","title":"Fixture"}],"config":{"includeCover":cover}}))["entries"][0]["id"].clone();
        download.call(json!({"op":"downloads_claim","wifi":true}));
        let catalog = json!({"videoId":"abcdefghijk","origin":"https://m.youtube.com","streams":[{"kind":"combined","key":"18:","itag":18,"height":360,"bytes":CHUNK*chunks,"mimeType":"video/mp4","url":"https://r1.googlevideo.com/videoplayback"}]});
        download.call(json!({"op":"downloads_prepared","catalog":catalog}));
        download.call(json!({"op":"downloads_transfer_start","catalog":catalog}));
        download
    }
    fn call(&self, mut command: Value) -> Value {
        command["id"] = self.id.clone();
        let reply: Value = serde_json::from_str(&execute(
            command.to_string(),
            self.root.to_string_lossy().into_owned(),
        ))
        .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
    fn poll(&self) -> Value {
        self.call(json!({"op":"downloads_transfer_poll"}))
    }
    fn done(&self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = self.poll();
            if status["done"] == true {
                return status;
            }
            assert!(Instant::now() < deadline, "Transfer did not stop: {status}");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        self.call(json!({"op":"downloads_transfer_release"}));
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn next(requests: &Receiver<Pending>) -> Pending {
    requests.recv_timeout(Duration::from_secs(2)).unwrap()
}
fn blocked(requests: &Receiver<Pending>) {
    assert!(matches!(
        requests.recv_timeout(Duration::from_millis(650)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
}

#[test]
fn native_policy_gates_chunks_redirects_retries_parallel_workers_and_covers_without_js() {
    let wifi = Arc::new(AtomicBool::new(false));
    let (send, requests) = mpsc::channel();
    install_download_http(Box::new(Backend {
        wifi: wifi.clone(),
        requests: Mutex::new(send),
    }));
    let download = Download::start("lifecycle", 1, 4, false, true);
    blocked(&requests);
    assert_eq!(download.poll()["networkWaiting"], true);
    wifi.store(true, Ordering::SeqCst);
    next(&requests).respond(206);
    let pending = next(&requests);
    assert_eq!(pending.request.range.as_ref().unwrap().start, CHUNK);
    wifi.store(false, Ordering::SeqCst);
    pending.respond(-1); // Wi-Fi vanished between Rust's gate and native I/O.
    blocked(&requests);
    assert_eq!(download.poll()["received"], CHUNK);
    download.call(json!({"op":"downloads_transfer_pause","paused":true}));
    wifi.store(true, Ordering::SeqCst);
    blocked(&requests); // Wi-Fi returning must preserve a manual pause.
    download.call(json!({"op":"downloads_transfer_pause","paused":false}));
    let pending = next(&requests);
    wifi.store(false, Ordering::SeqCst);
    pending.respond(302);
    blocked(&requests); // Redirects must also wait for Wi-Fi.
    wifi.store(true, Ordering::SeqCst);
    let pending = next(&requests);
    assert!(pending.request.url.contains("r2.googlevideo.com"));
    wifi.store(false, Ordering::SeqCst);
    pending.respond(403);
    blocked(&requests); // Neither retry nor POST fallback may bypass policy.
    download.call(json!({"op":"downloads_config","settings":{"wifiOnly":false}}));
    let pending = next(&requests);
    assert!(!pending.request.wifi_only);
    assert_eq!(pending.request.method, "POST");
    pending.respond(206);
    let pending = next(&requests);
    download.call(json!({"op":"downloads_config","settings":{"wifiOnly":true}}));
    pending.respond(206);
    blocked(&requests); // Enabling the preference takes effect mid-transfer.
    download.call(json!({"op":"downloads_transfer_cancel"}));
    assert!(download.done()["error"].is_string()); // Cancel wakes network waits.
    drop(download);

    wifi.store(true, Ordering::SeqCst);
    let download = Download::start("parallel", 4, 6, true, true);
    let pending = (0..4).map(|_| next(&requests)).collect::<Vec<_>>();
    wifi.store(false, Ordering::SeqCst);
    for request in pending {
        request.respond(-1);
    }
    blocked(&requests);
    wifi.store(true, Ordering::SeqCst);
    // Native policy interruptions do not consume the HTTP retry budget.
    for _ in 0..4 {
        next(&requests).respond(-1);
    }
    for _ in 0..6 {
        let pending = next(&requests);
        assert!(pending.request.wifi_only);
        pending.respond(206);
    }
    let cover = next(&requests);
    assert!(cover.request.range.is_none());
    assert!(cover.request.wifi_only);
    wifi.store(false, Ordering::SeqCst);
    cover.respond(-1);
    blocked(&requests);
    wifi.store(true, Ordering::SeqCst);
    next(&requests).respond(200);
    let status = download.done();
    assert!(status["error"].is_null(), "{status}");
    assert_eq!(status["received"], 6 * CHUNK);
    let path = status["files"][0]
        .as_str()
        .unwrap()
        .strip_prefix("file://")
        .unwrap();
    let bytes = fs::read(path).unwrap();
    for (index, chunk) in bytes.chunks(usize::try_from(CHUNK).unwrap()).enumerate() {
        assert!(chunk.iter().all(|byte| usize::from(*byte) == index));
    }
    let cover = status["coverFile"]
        .as_str()
        .unwrap()
        .strip_prefix("file://")
        .unwrap();
    assert_eq!(fs::read(cover).unwrap(), [0xff, 0xd8, 0xff]);
}
