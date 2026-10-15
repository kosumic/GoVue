use lingoplay_core::execute;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const VIDEO: &str = "abcdefghijk";
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lingoplay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn call(&self, command: Value) -> Value {
        serde_json::from_str(&execute(
            command.to_string(),
            self.0.to_str().unwrap().into(),
        ))
        .unwrap()
    }
    fn value(&self, command: Value) -> Value {
        let reply = self.call(command);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reply() -> Value {
    json!({"type":"transcript","requestId":"request:1","videoId":VIDEO,"ok":true,"transcript":{
        "videoId":VIDEO,"title":"Example","language":"English","trackId":".en",
        "tracks":[{"id":".en","label":"English","language":"en","automatic":false}],
        "segments":[{"start":12.5,"duration":2,"text":"Hello, world."}]
    }})
}
fn accept(reply: Value) -> Value {
    json!({"op":"accept","videoId":VIDEO,"requestId":"request:1","origin":"https://www.youtube.com","raw":reply.to_string()})
}
fn operation(op: &str) -> Value {
    json!({"op":op,"videoId":VIDEO,"trackId":".en"})
}

#[test]
fn validates_origins_and_correlates_requests_and_video_identity() {
    let store = Store::new();
    for origin in [
        "https://www.youtube.com",
        "https://m.youtube.com/",
        "https://music.youtube.com/",
        "https://www.youtube.com/watch?v=abcdefghijk",
        "https://m.youtube.com/shorts/abcdefghijk",
    ] {
        let mut command = accept(reply());
        command["origin"] = json!(origin);
        assert_eq!(store.value(command)["ok"], true);
    }
    for origin in [
        "https://www.youtube.com.evil.test",
        "http://www.youtube.com",
        "https://www.youtube.com:444",
        "https://user:pass@www.youtube.com",
        "https://www.youtube.com/watch?v=0123456789a",
        "https://www.youtube.com/channel/example",
    ] {
        let mut command = accept(reply());
        command["origin"] = json!(origin);
        assert_eq!(store.call(command)["ok"], false, "{origin}");
    }
    for field in ["requestId", "videoId"] {
        let mut response = reply();
        response[field] = json!("0123456789a");
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    let mut response = reply();
    response["transcript"]["videoId"] = json!("0123456789a");
    assert_eq!(store.call(accept(response))["ok"], false);
}

#[test]
fn rejects_malformed_or_unbounded_data_and_preserves_actionable_errors() {
    let store = Store::new();
    for segments in [
        json!([]),
        json!([{"start":-1,"duration":2,"text":"bad"}]),
        json!([{"start":0,"duration":-1,"text":"bad"}]),
        json!([{"start":0,"duration":1,"text":"x".repeat(50_001)}]),
    ] {
        let mut response = reply();
        response["transcript"]["segments"] = segments;
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    for tracks in [
        json!([null]),
        json!([]),
        json!([{"id":"other","label":"Other","language":"en","automatic":false}]),
    ] {
        let mut response = reply();
        response["transcript"]["tracks"] = tracks;
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    for raw in ["{bad".into(), "x".repeat(8_000_001)] {
        let mut command = accept(reply());
        command["raw"] = json!(raw);
        assert_eq!(store.call(command)["ok"], false);
    }
    let mut response = reply();
    response["ok"] = json!(false);
    response["error"] = json!("No captions");
    let error = store.value(accept(response));
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"], "No captions");
}

#[test]
fn parses_raw_json3_and_keeps_lines_across_the_whole_video() {
    let store = Store::new();
    let mut response = reply();
    response["transcript"]["captionSource"] = json!(json!({"events":[
        {"tStartMs":490000,"dDurationMs":3000,"segs":[{"utf8":"Later line"}]},
        {"tStartMs":7000,"dDurationMs":2000,"segs":[{"utf8":"a new ＨｏｍｅＰｏｄ "},{"utf8":" mini 2,"}]},
        {"tStartMs":45000,"dDurationMs":2000,"segs":[{"utf8":" Inside\n the gap "}]},
        {"tStartMs":-1,"segs":[{"utf8":"invalid timing"}]},
        {"tStartMs":10,"segs":[{"utf8":"\n"}]}
    ]}).to_string());
    let data = store.value(accept(response));
    assert_eq!(
        data["transcript"]["segments"],
        json!([
            {"start":7.0,"duration":2.0,"text":"a new HomePod mini 2,"},
            {"start":45.0,"duration":2.0,"text":"Inside the gap"},
            {"start":490.0,"duration":3.0,"text":"Later line"}
        ])
    );
    let mut search = operation("search");
    search["query"] = json!("ＨＯＭＥＰＯＤ");
    assert_eq!(store.value(search.clone()), json!([0]));
    search["query"] = json!("");
    assert_eq!(store.value(search), json!([0, 1, 2]));
    assert_eq!(
        store.value(operation("load"))["segments"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn decodes_legacy_and_srv3_xml_with_entity_and_word_boundaries() {
    let store = Store::new();
    for (source, expected) in [
        ("<transcript><text start=\"1.5\" dur=\"2\">Tom &amp;amp; Jerry</text></transcript>", json!({"start":1.5,"duration":2.0,"text":"Tom & Jerry"})),
        ("<timedtext><body><p t=\"3601000\" d=\"2500\"><s>Hello </s><s>world</s></p></body></timedtext>", json!({"start":3601.0,"duration":2.5,"text":"Hello world"}))
    ] {
        let mut response = reply();
        response["transcript"]["captionSource"] = json!(source);
        assert_eq!(store.value(accept(response))["transcript"]["segments"], json!([expected]));
    }
    assert!(store
        .value(operation("export"))
        .as_str()
        .unwrap()
        .contains("1:00:01  Hello world"));
}

#[test]
fn restores_normalized_disk_cache_without_raw_sources_or_session_fields() {
    let store = Store::new();
    let mut response = reply();
    response["transcript"]["captionSource"] =
        json!("<transcript><text start=\"12.5\" dur=\"2\">Hello, world.</text></transcript>");
    response["transcript"]["signedUrl"] = json!("private-session-token");
    assert_eq!(store.value(accept(response))["cacheSaved"], true);
    let files: Vec<_> = fs::read_dir(&store.0)
        .unwrap()
        .map(|f| f.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let saved = fs::read_to_string(&files[0]).unwrap();
    assert!(!saved.contains("captionSource"));
    assert!(!saved.contains("private-session-token"));
    let restored = store.value(operation("restore"));
    assert_eq!(restored["segments"][0]["text"], "Hello, world.");
    assert_eq!(
        store.value(operation("export")),
        json!("Example\nhttps://www.youtube.com/watch?v=abcdefghijk\n\n0:12  Hello, world.")
    );
    fs::write(&files[0], "corrupt cache").unwrap();
    assert_eq!(store.value(operation("restore")), Value::Null);
}

#[test]
fn full_storage_does_not_prevent_search_or_export_of_current_transcript() {
    let store = Store::new();
    let blocked = store.0.join("file-not-directory");
    fs::write(&blocked, "occupied").unwrap();
    let call = |command: Value| -> Value {
        serde_json::from_str(&execute(
            command.to_string(),
            blocked.to_str().unwrap().into(),
        ))
        .unwrap()
    };
    let accepted = call(accept(reply()));
    assert_eq!(accepted["value"]["ok"], true);
    assert_eq!(accepted["value"]["cacheSaved"], false);
    assert_eq!(call(operation("export"))["ok"], true);
}
