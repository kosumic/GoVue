#![cfg(test)]

use govue_core::{
    DownloadHttp, DownloadPlatform, DownloadRequest, ExtractionHttp, ExtractionReply,
    ExtractionRequest, MediaRangeReply,
};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use url::Url;
#[derive(Default)]
struct Network {
    wifi: AtomicBool,
    blocked: Mutex<bool>,
    wake: Condvar,
    requests: AtomicUsize,
    muxes: AtomicUsize,
}
struct Http(Arc<Network>);
impl DownloadHttp for Http {
    fn wifi_connected(&self) -> bool {
        self.0.wifi.load(Ordering::SeqCst)
    }
    fn fetch(&self, request: DownloadRequest) -> MediaRangeReply {
        self.0.requests.fetch_add(1, Ordering::SeqCst);
        let mut blocked = self.0.blocked.lock().unwrap();
        while *blocked {
            let next = self
                .0
                .wake
                .wait_timeout(blocked, Duration::from_secs(5))
                .unwrap();
            assert!(!next.1.timed_out());
            blocked = next.0;
        }
        drop(blocked);
        let range = request.range.unwrap();
        MediaRangeReply {
            status: 206,
            body: vec![42; usize::try_from(range.end - range.start + 1).unwrap()],
            location: None,
        }
    }
}
struct Extraction;
impl ExtractionHttp for Extraction {
    fn request(&self, request: ExtractionRequest) -> ExtractionReply {
        let url = Url::parse(&request.url).unwrap();
        let body = if url.path() == "/watch" {
            format!(
                "ytcfg.set({});",
                json!({"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"fixture"}}})
            )
        } else {
            let id = serde_json::from_str::<Value>(&request.body).unwrap()["videoId"].clone();
            json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":id,"title":"Fixture","author":"Fixture","lengthSeconds":"10"},"streamingData":{"adaptiveFormats":[
                {"itag":160,"mimeType":"video/mp4; codecs=\"avc1.4d400c\"","width":256,"height":144,"fps":30,"contentLength":"500","url":"https://r1.googlevideo.com/videoplayback?sig=memory-only-video"},
                {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":64000,"contentLength":"500","url":"https://r1.googlevideo.com/videoplayback?sig=memory-only-audio"}
            ]}}).to_string()
        };
        ExtractionReply {
            status: 200,
            body: body.into_bytes(),
            headers: vec![],
        }
    }
}
struct Platform {
    root: PathBuf,
    network: Arc<Network>,
}
impl DownloadPlatform for Platform {
    fn directory(&self) -> String {
        self.root.join("downloads").to_string_lossy().into_owned()
    }
    fn export_directory(&self) -> String {
        self.root.join("exports").to_string_lossy().into_owned()
    }
    fn supported_formats(&self, raw: String) -> String {
        serde_json::from_str::<Vec<Value>>(&raw)
            .unwrap()
            .iter()
            .map(|v| v["key"].clone())
            .collect::<Value>()
            .to_string()
    }
    fn mux(&self, video: String, audio: String, output: String) -> bool {
        let path = |uri: &str| Url::parse(uri).unwrap().to_file_path().unwrap();
        assert_eq!(path(&video).extension().unwrap(), "mp4");
        assert_eq!(path(&audio).extension().unwrap(), "m4a");
        let mut bytes = fs::read(path(&video)).unwrap();
        bytes.extend(fs::read(path(&audio)).unwrap());
        fs::write(path(&output), bytes).unwrap();
        self.network.muxes.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn publish(&self, _id: String) -> i32 {
        0
    }
    fn delete_published(&self, _uri: String) -> bool {
        true
    }
    fn activity(&self, _count: u32, _title: String, _received: u64, _total: u64) {}
}
fn call(root: &str, command: Value) -> Value {
    let response: Value =
        serde_json::from_str(&govue_core::execute(command.to_string(), root.into())).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}
fn wait(root: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = call(root, json!({"op":"downloads_runner_snapshot"}));
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "Runner stalled: {snapshot}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn native_runner_advances_downloads_without_javascript_and_preserves_requested_files() {
    let root = std::env::temp_dir().join(format!("govue-runner-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let network = Arc::new(Network::default());
    *network.blocked.lock().unwrap() = true;
    govue_core::install_download_platform(Box::new(Platform {
        root: root.clone(),
        network: network.clone(),
    }));
    govue_core::install_download_http(Box::new(Http(network.clone())));
    govue_core::install_extraction_http(Box::new(Extraction));
    let root = root.to_str().unwrap();
    call(root, json!({"op":"downloads_runner_start"}));
    wait(root, |s| s["ready"] == true);
    call(
        root,
        json!({"op":"downloads_config","settings":{"wifiOnly":true}}),
    );
    let queued = call(
        root,
        json!({"op":"downloads_runner_enqueue","items":[{"videoId":"abcdefghijk","title":"First"},{"videoId":"0123456789a","title":"Second"},{"videoId":"ABCDEFGHIJK","title":"Third"}],"config":{"includeCover":false,"qualityHeight":144}}),
    );
    assert_eq!(queued["entries"].as_array().unwrap().len(), 3);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(network.requests.load(Ordering::SeqCst), 0);
    network.wifi.store(true, Ordering::SeqCst);
    let active = wait(root, |s| {
        s["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["state"] == "downloading")
            .count()
            == 2
    });
    assert_eq!(
        active["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["state"] == "queued")
            .count(),
        1
    );
    let entries = active["entries"].as_array().unwrap();
    let first = entries
        .iter()
        .find(|e| e["videoId"] == "abcdefghijk")
        .unwrap()
        .clone();
    let second = entries
        .iter()
        .find(|e| e["videoId"] == "0123456789a")
        .unwrap()
        .clone();
    call(
        root,
        json!({"op":"downloads_runner_pause","id":first["id"],"paused":true}),
    );
    call(
        root,
        json!({"op":"downloads_runner_cancel","id":second["id"]}),
    );
    *network.blocked.lock().unwrap() = false;
    network.wake.notify_all();
    let state = wait(root, |s| {
        s["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["videoId"] == "ABCDEFGHIJK" && e["state"] == "complete")
    });
    assert_eq!(
        state["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == first["id"])
            .unwrap()["state"],
        "paused"
    );
    call(
        root,
        json!({"op":"downloads_runner_pause","id":first["id"],"paused":false}),
    );
    let done = wait(root, |s| {
        s["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["state"] == "complete")
            .count()
            == 2
    });
    assert_eq!(network.muxes.load(Ordering::SeqCst), 2);
    let first = done["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == first["id"])
        .unwrap();
    let local = call(root, json!({"op":"downloads_runner_uri","id":first["id"]}));
    let local = Url::parse(local.as_str().unwrap())
        .unwrap()
        .to_file_path()
        .unwrap();
    assert_eq!(fs::metadata(&local).unwrap().len(), 1000);
    let export = call(
        root,
        json!({"op":"downloads_runner_export","id":first["id"]}),
    );
    assert!(
        Url::parse(export["uri"].as_str().unwrap())
            .unwrap()
            .to_file_path()
            .unwrap()
            .is_file()
    );
    call(
        root,
        json!({"op":"downloads_runner_remove","ids":[first["id"]],"deleteMedia":false}),
    );
    wait(root, |s| {
        !s["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == first["id"])
    });
    assert!(
        local.is_file(),
        "Removing history must preserve media when requested"
    );
    let third = done["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["videoId"] == "ABCDEFGHIJK")
        .unwrap();
    let third_file = PathBuf::from(root)
        .join("downloads")
        .join(third["file"].as_str().unwrap());
    call(
        root,
        json!({"op":"downloads_runner_remove","ids":[third["id"],second["id"]],"deleteMedia":true}),
    );
    wait(root, |s| s["entries"].as_array().unwrap().is_empty());
    assert!(!third_file.exists());
    let saved = fs::read_to_string(PathBuf::from(root).join("media/downloads.json")).unwrap();
    assert!(!saved.contains("memory-only"));
    fs::remove_dir_all(root).unwrap();
}
