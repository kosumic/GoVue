#![cfg(test)]

use govue_core::{
    ExtractionHttp, ExtractionReply, ExtractionRequest, execute, install_extraction_http,
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Default)]
struct Gate {
    blocked: Mutex<bool>,
    entered: Mutex<bool>,
    wake: Condvar,
}
struct Http(Arc<Gate>);
impl ExtractionHttp for Http {
    fn request(&self, request: ExtractionRequest) -> ExtractionReply {
        let url = Url::parse(&request.url).unwrap();
        let tracks = json!({"playerCaptionsTracklistRenderer":{"captionTracks":[
            {"vssId":".en","languageCode":"en","name":{"simpleText":"English"},"baseUrl":"https://www.youtube.com/api/timedtext?v=abcdefghijk&lang=en"},
            {"vssId":".ja","languageCode":"ja","name":{"simpleText":"Japanese"},"baseUrl":"https://www.youtube.com/api/timedtext?v=abcdefghijk&lang=ja"}
        ]}});
        let player = json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":"abcdefghijk","title":"Session fixture","lengthSeconds":"10"},"captions":tracks});
        let body = match url.path() {
            "/watch" => format!(
                "ytcfg.set({});var ytInitialPlayerResponse = {player};",
                json!({"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"fixture"}}})
            ),
            "/youtubei/v1/player" => player.to_string(),
            "/api/timedtext" => {
                *self.0.entered.lock().unwrap() = true;
                self.0.wake.notify_all();
                let mut blocked = self.0.blocked.lock().unwrap();
                while *blocked {
                    let result = self
                        .0
                        .wake
                        .wait_timeout(blocked, Duration::from_secs(5))
                        .unwrap();
                    assert!(!result.1.timed_out(), "Caption fixture stayed blocked");
                    blocked = result.0;
                }
                drop(blocked);
                json!({"events":[{"tStartMs":0,"dDurationMs":1000,"segs":[{"utf8":"Loaded comparison"}]}]}).to_string()
            }
            path => panic!("Unexpected fixture path {path}"),
        };
        ExtractionReply {
            status: 200,
            body: body.into_bytes(),
            headers: vec![],
        }
    }
}
fn reply(root: &str, command: Value) -> Value {
    serde_json::from_str(&execute(command.to_string(), root.into())).unwrap()
}
fn call(root: &str, command: Value) -> Value {
    let result = reply(root, command);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn wait(root: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = call(
            root,
            json!({"op":"transcript_session_snapshot","sessionId":"fixture"}),
        );
        if predicate(&state) {
            return state;
        }
        assert!(Instant::now() < deadline, "Reader stalled: {state}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn readers_retain_primary_cancel_stale_work_and_restore_normalized_cache() {
    let root = std::env::temp_dir().join(format!("govue-reader-session-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let root = root.to_str().unwrap();
    let gate = Arc::new(Gate::default());
    install_extraction_http(Box::new(Http(gate.clone())));
    let primary = json!({"videoId":"abcdefghijk","title":"Seed","language":"English","trackId":".en",
        "tracks":[{"id":".en","language":"en","label":"English","automatic":false},{"id":".ja","language":"ja","label":"Japanese","automatic":false}],
        "segments":[{"start":1,"duration":2,"text":"Primary line"}]});
    let load = |version, tracks: Value, seed: Value, fresh| {
        call(
            root,
            json!({"op":"transcript_session_load","sessionId":"fixture","version":version,"videoId":"abcdefghijk","trackIds":tracks,"initial":seed,"fresh":fresh}),
        )
    };
    load(1, json!([".en"]), primary, false);
    let primary = wait(root, |s| s["loading"] == false)["documents"][0]["data"].clone();
    assert_eq!(primary["segments"][0]["text"], "Primary line");
    *gate.blocked.lock().unwrap() = true;
    load(2, json!([".en", ".ja"]), Value::Null, false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !*gate.entered.lock().unwrap() {
        assert!(Instant::now() < deadline, "Comparison never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    let pending = call(
        root,
        json!({"op":"transcript_session_snapshot","sessionId":"fixture"}),
    );
    assert_eq!(pending["documents"][0]["data"], primary);
    assert_eq!(pending["loading"], true);
    call(
        root,
        json!({"op":"transcript_session_release","sessionId":"fixture","version":2}),
    );
    let retained = load(3, json!([".en"]), Value::Null, false);
    assert_eq!(
        retained["documents"][0]["data"], primary,
        "Release must retain the usable primary"
    );
    *gate.blocked.lock().unwrap() = false;
    gate.wake.notify_all();
    let state = wait(root, |s| s["version"] == 3 && s["loading"] == false);
    assert_eq!(state["documents"].as_array().unwrap().len(), 1);
    load(4, json!([".en", ".missing"]), Value::Null, false);
    let failed = wait(root, |s| s["loading"] == false);
    assert_eq!(failed["documents"][0]["data"], primary);
    assert!(
        failed["error"].is_string(),
        "A failed comparison must keep primary usable"
    );
    load(5, json!([".en", ".ja"]), Value::Null, false);
    let complete = wait(root, |s| s["loading"] == false);
    assert!(complete["error"].is_null(), "{complete}");
    assert_eq!(complete["documents"][1]["data"]["trackId"], ".ja");
    call(
        root,
        json!({"op":"transcript_session_dispose","sessionId":"fixture","version":5}),
    );
    assert!(
        load(4, json!([".en"]), Value::Null, false).is_null(),
        "Disposed generations cannot resurrect the reader"
    );
    assert_eq!(
        reply(
            root,
            json!({"op":"transcript_session_snapshot","sessionId":"fixture"})
        )["ok"],
        false
    );
    load(6, json!([".ja"]), Value::Null, false);
    let cached = wait(root, |s| s["loading"] == false);
    assert_eq!(cached["documents"][0]["cached"], true);
    load(7, json!([".ja"]), Value::Null, true);
    let fresh = wait(root, |s| s["loading"] == false);
    assert_eq!(fresh["documents"][0]["cached"], false);
    call(
        root,
        json!({"op":"transcript_session_dispose","sessionId":"fixture","version":7}),
    );
    call(
        root,
        json!({"op":"transcript_session_release","sessionId":"fixture","version":8}),
    );
    assert!(
        load(8, json!([".en"]), Value::Null, false).is_null(),
        "A release delivered before its load must still cancel that generation"
    );
    load(9, json!([".ja"]), Value::Null, false);
    wait(root, |s| s["loading"] == false);
    call(
        root,
        json!({"op":"transcript_session_release","sessionId":"fixture","version":9}),
    );
    call(
        root,
        json!({"op":"transcript_session_dispose","sessionId":"fixture","version":9}),
    );
    assert_eq!(
        reply(
            root,
            json!({"op":"transcript_session_snapshot","sessionId":"fixture"})
        )["ok"],
        false,
        "Disposal must release documents even after cancellation of the same generation"
    );
    for entry in fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
    {
        let saved = fs::read_to_string(entry.path()).unwrap();
        assert!(!saved.contains("timedtext") && !saved.contains("baseUrl"));
    }
    fs::remove_dir_all(root).unwrap();
}
