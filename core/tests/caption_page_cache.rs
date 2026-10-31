#![cfg(test)]

use govue_core::{ExtractionHttp, ExtractionReply, ExtractionRequest};
use serde_json::{Value, json};
use std::fs;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

const VIDEO: &str = "abcdefghijk";
const TARGET: &str = "translate:zh-Hans:a.en";

struct Denied(Arc<AtomicUsize>);
impl ExtractionHttp for Denied {
    fn request(&self, _: ExtractionRequest) -> ExtractionReply {
        self.0.fetch_add(1, Ordering::Relaxed);
        ExtractionReply {
            status: 403,
            body: vec![],
            headers: vec![],
        }
    }
}

fn call(root: &str, command: Value) -> Value {
    let reply: Value =
        serde_json::from_str(&govue_core::execute(command.to_string(), root.into())).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

#[test]
fn cached_translation_uses_its_source_catalog_and_the_page_before_denied_guest_requests() {
    let directory =
        std::env::temp_dir().join(format!("govue-caption-page-cache-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let root = directory.to_str().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    govue_core::install_extraction_http(Box::new(Denied(calls.clone())));
    for (source, target, automatic) in [
        ("a.en", TARGET, true),
        (".en", "translate:zh-Hans:.en", false),
    ] {
        let tracks = json!([
            {"id":".en","language":"en","label":"English","automatic":false,"translatable":true},
            {"id":"a.en","language":"en","label":"English auto","automatic":true,"translatable":true},
            {"id":target,"language":"zh-Hans","label":"Chinese","automatic":automatic,"translatedFrom":source}
        ]);
        let data = json!({"videoId":VIDEO,"title":"Cached source","trackId":source,"language":"English",
            "tracks":tracks,"segments":[{"start":0,"duration":1,"text":"Source caption"}]});
        let raw = json!({"type":"transcript","requestId":"seed","videoId":VIDEO,"ok":true,"transcript":data});
        call(
            root,
            json!({"op":"accept","videoId":VIDEO,"requestId":"seed",
            "origin":"https://www.youtube.com","raw":raw.to_string()}),
        );
    }
    // The most recent default document has a different translation source.
    for entry in fs::read_dir(root).unwrap().flatten() {
        let data: Value = serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
        if data["trackId"] == ".en" {
            fs::File::open(entry.path())
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(10)),
                )
                .unwrap();
        }
    }
    call(
        root,
        json!({"op":"extract_page_register","adapterId":"fixture-page","videoId":VIDEO}),
    );
    call(
        root,
        json!({"op":"transcript_session_load","sessionId":"fixture","version":1,
        "videoId":VIDEO,"trackIds":[".en",TARGET]}),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let task = loop {
        let state = call(root, json!({"op":"extract_page_tasks"}));
        if let Some(task) = state["tasks"].as_array().unwrap().first() {
            break task.clone();
        }
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "Cached captions must try the registered page first"
        );
        assert!(
            Instant::now() < deadline,
            "Caption task never reached the page"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(task["task"]["trackId"], TARGET);
    assert_eq!(task["task"]["sourceTrackId"], "a.en");
    let source =
        json!({"events":[{"tStartMs":0,"dDurationMs":1000,"segs":[{"utf8":"你好"}]}]}).to_string();
    let raw = json!({"type":"captionPage","requestId":task["requestId"],"taskId":task["task"]["taskId"],
        "videoId":VIDEO,"trackId":TARGET,"ok":true,"format":"timedtext","source":source});
    call(
        root,
        json!({"op":"extract_page","requestId":task["requestId"],
        "origin":"https://m.youtube.com","raw":raw.to_string()}),
    );
    loop {
        let state = call(
            root,
            json!({"op":"transcript_session_snapshot","sessionId":"fixture"}),
        );
        if state["loading"] == false {
            assert!(state["error"].is_null(), "{state}");
            assert_eq!(state["documents"][1]["data"]["segments"][0]["text"], "你好");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Translated captions never reached the reader"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    call(
        root,
        json!({"op":"transcript_session_dispose","sessionId":"fixture","version":1}),
    );
    call(
        root,
        json!({"op":"extract_page_register","adapterId":"fixture-page","videoId":null}),
    );
    fs::remove_dir_all(directory).unwrap();
}
