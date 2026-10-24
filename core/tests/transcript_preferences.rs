#![cfg(test)]

use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const FIRST: &str = "abcdefghijk";
const SECOND: &str = "0123456789a";
const THIRD: &str = "1234567890b";

struct Store {
    path: PathBuf,
    version: u64,
}

impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "govue-transcript-preferences-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path, version: 0 }
    }

    fn call(&self, command: Value) -> Value {
        let reply: Value = serde_json::from_str(&govue_core::execute(
            command.to_string(),
            self.path.to_str().unwrap().into(),
        ))
        .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }

    fn seed(&self, video: &str, tracks: Value) {
        for track in tracks.as_array().unwrap() {
            let data = json!({"videoId":video,"title":"Preferences fixture","trackId":track["id"],
                "language":track["language"],"tracks":tracks,"segments":[{"start":0,"duration":1,"text":"Caption line"}]});
            let raw = json!({"type":"transcript","requestId":"seed","videoId":video,"ok":true,"transcript":data});
            let result = self.call(json!({"op":"accept","videoId":video,"requestId":"seed",
                "origin":"https://www.youtube.com","raw":raw.to_string()}));
            assert_eq!(result["cacheSaved"], true);
        }
    }

    fn select(&self, video: &str, tracks: Value, toggle: &str) -> Value {
        self.call(json!({"op":"transcript_select","videoId":video,
            "trackId":tracks[0],"trackIds":tracks,"toggleId":toggle}))
    }

    fn read(&mut self, video: &str) -> Value {
        self.read_with_initial(video, Value::Null)
    }

    fn read_with_initial(&mut self, video: &str, initial: Value) -> Value {
        self.version += 1;
        self.call(
            json!({"op":"transcript_session_load","sessionId":"preferences","version":self.version,
            "videoId":video,"trackIds":["default"],"initial":initial}),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state =
                self.call(json!({"op":"transcript_session_snapshot","sessionId":"preferences"}));
            if state["loading"] == false {
                assert!(state["error"].is_null(), "{state}");
                let ids: Vec<_> = state["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|document| document["data"]["trackId"].clone())
                    .collect();
                assert_eq!(json!(ids), state["trackIds"]);
                self.call(json!({"op":"transcript_session_dispose","sessionId":"preferences","version":self.version}));
                return state["trackIds"].clone();
            }
            assert!(Instant::now() < deadline, "Reader stalled: {state}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn preferences(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.path.join("media/transcript-preferences.json")).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn tracks(source: &str, language: &str) -> Value {
    json!([
        {"id":source,"language":language,"label":language,"automatic":false,"translatable":true},
        {"id":format!("translate:ja:{source}"),"language":"ja","label":"Japanese","automatic":false,"translatedFrom":source}
    ])
}

#[test]
fn seeded_readers_restore_preferences_and_reuse_their_initial_document() {
    let mut store = Store::new();
    store.seed(FIRST, tracks(".en", "en"));
    store.select(FIRST, json!([".en"]), "translate:ja:.en");
    let initial = json!({"videoId":FIRST,"title":"Seeded reader","trackId":".en","language":"English",
        "tracks":tracks(".en","en"),"segments":[{"start":0,"duration":1,"text":"Initial caption"}]});
    assert_eq!(
        store.read_with_initial(FIRST, initial),
        json!([".en", "translate:ja:.en"])
    );
    // A source document supplied by chat need not have been saved to disk.
    let initial = json!({"videoId":SECOND,"title":"Uncached seed","trackId":".es","language":"Spanish",
        "tracks":[{"id":".es","language":"es","label":"Spanish","automatic":false}],
        "segments":[{"start":0,"duration":1,"text":"Initial caption"}]});
    assert_eq!(store.read_with_initial(SECOND, initial), json!([".es"]));
}

#[test]
fn translations_and_off_state_survive_new_readers_and_videos_without_losing_unavailable_languages()
{
    let mut store = Store::new();
    store.seed(FIRST, tracks(".en", "en"));
    store.seed(SECOND, tracks("a.fr:new-video", "fr"));
    store.seed(
        THIRD,
        json!([{"id":".es","language":"es","label":"Spanish","automatic":false}]),
    );
    assert_eq!(
        store.select(FIRST, json!([".en"]), "translate:ja:.en"),
        json!([".en", "translate:ja:.en"])
    );
    let enabled = store.preferences();
    assert_eq!(
        enabled,
        json!([{"type":"source"},{"type":"language","language":"ja","kind":"translated"}])
    );
    assert_eq!(store.read(FIRST), json!([".en", "translate:ja:.en"]));
    assert_eq!(
        store.read(SECOND),
        json!(["a.fr:new-video", "translate:ja:a.fr:new-video"])
    );
    assert_eq!(store.read(THIRD), json!([".es"]));
    assert_eq!(store.preferences(), enabled);
    assert_eq!(
        store.read(SECOND),
        json!(["a.fr:new-video", "translate:ja:a.fr:new-video"])
    );

    store.select(
        FIRST,
        json!([".en", "translate:ja:.en"]),
        "translate:ja:.en",
    );
    assert_eq!(store.preferences(), json!([{"type":"source"}]));
    assert_eq!(store.read(SECOND), json!(["a.fr:new-video"]));
    store.select(
        SECOND,
        json!(["a.fr:new-video"]),
        "translate:ja:a.fr:new-video",
    );
    assert_eq!(store.read(FIRST), json!([".en", "translate:ja:.en"]));
}

#[test]
fn restores_translation_kind_source_visibility_and_selection_order() {
    let mut store = Store::new();
    let mut available = tracks(".en", "en");
    available.as_array_mut().unwrap().extend([
        json!({"id":".ja","language":"ja","label":"Japanese","automatic":false}),
        json!({"id":"a.ja","language":"ja","label":"Japanese auto","automatic":true}),
    ]);
    store.seed(FIRST, available);
    store.select(FIRST, json!([".en"]), "translate:ja:.en");
    store.select(FIRST, json!([".en", "translate:ja:.en"]), ".en");
    assert_eq!(store.read(FIRST), json!(["translate:ja:.en"]));

    // A machine translation of ASR is still a translation, even though the
    // track also carries the source's automatic flag.
    let mut second = tracks("a.en", "en");
    second[0]["automatic"] = json!(true);
    second[1]["automatic"] = json!(true);
    second.as_array_mut().unwrap().insert(
        1,
        json!({"id":".ja:uploaded","language":"ja","label":"Japanese","automatic":false}),
    );
    store.seed(SECOND, second);
    assert_eq!(store.read(SECOND), json!(["translate:ja:a.en"]));
    store.select(SECOND, json!(["translate:ja:a.en"]), "a.en");
    assert_eq!(store.read(FIRST), json!(["translate:ja:.en", ".en"]));

    store.select(
        FIRST,
        json!(["translate:ja:.en", ".en"]),
        "translate:ja:.en",
    );
    store.select(FIRST, json!([".en"]), "a.ja");
    assert_eq!(store.read(FIRST), json!([".en", "a.ja"]));
    store.seed(
        THIRD,
        json!([
            {"id":".fr","language":"fr","label":"French","automatic":false},
            {"id":".ja:fallback","language":"ja","label":"Japanese","automatic":false}
        ]),
    );
    assert_eq!(store.read(THIRD), json!([".fr", ".ja:fallback"]));
}

#[test]
fn off_and_invalid_preferences_choose_source_even_when_latest_cached_document_is_translated() {
    let mut store = Store::new();
    store.seed(FIRST, tracks(".en", "en"));
    // Make the restore candidate deterministic instead of relying on filesystem
    // timestamp resolution between two quick cache writes.
    for entry in fs::read_dir(&store.path).unwrap().flatten() {
        let data: Value = serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
        if data["trackId"] == "translate:ja:.en" {
            fs::File::open(entry.path())
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(10)),
                )
                .unwrap();
        }
    }
    assert_eq!(
        store.call(json!({"op":"restore","videoId":FIRST}))["trackId"],
        "translate:ja:.en"
    );
    assert_eq!(store.read(FIRST), json!([".en"]));
    store.select(FIRST, json!([".en"]), "translate:ja:.en");
    store.select(
        FIRST,
        json!([".en", "translate:ja:.en"]),
        "translate:ja:.en",
    );
    assert_eq!(store.read(FIRST), json!([".en"]));
    let file = store.path.join("media/transcript-preferences.json");
    for invalid in [
        "not json",
        "[]",
        r#"[{"type":"language","language":"https://invalid","kind":"translated"}]"#,
    ] {
        fs::write(&file, invalid).unwrap();
        assert_eq!(store.read(FIRST), json!([".en"]));
    }
}
