#![cfg(test)]

use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "govue-download-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn call(&self, command: Value) -> Value {
        serde_json::from_str(&govue_core::execute(
            command.to_string(),
            self.0.to_string_lossy().into_owned(),
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

fn prepared_download(store: &Store) -> (Value, Vec<Value>) {
    let queued = store.value(json!({"op":"downloads_enqueue","items":[{"videoId":"abcdefghijk","title":"Public video","author":"Author"}],"config":{"includeCover":true,"fileName":"Port test"}}));
    let id = queued["entries"][0]["id"].clone();
    store.value(json!({"op":"downloads_claim","wifi":true}));
    let prepared = store.value(json!({"op":"downloads_prepared","id":id,"catalog":{"videoId":"abcdefghijk","title":"Public video","author":"Author","streams":[{"key":"18:","itag":18,"kind":"combined","height":360,"fps":30,"bytes":1000,"mimeType":"video/mp4","url":"https://r1.googlevideo.com/videoplayback?sig=secret"}]}}));
    (
        id,
        prepared["entries"][0]["artifacts"]
            .as_array()
            .unwrap()
            .clone(),
    )
}

#[test]
fn publication_requires_verified_artifacts_and_recovers_after_restart() {
    let store = Store::new();
    let (id, artifacts) = prepared_download(&store);
    store.value(json!({"op":"downloads_event","id":id,"event":"combining"}));
    let planned = store.value(json!({"op":"downloads_publication_plan","id":id}));
    let generation = &planned["publication"]["generation"];
    assert_eq!(
        planned["publication"]["artifacts"][0]["name"],
        "Port test.mp4"
    );
    let hash = "ab".repeat(32);
    for (index, artifact) in artifacts.iter().enumerate() {
        assert_eq!(store.call(json!({"op":"downloads_event","id":id,"event":"complete","publicationRequired":true}))["ok"], false);
        let uri = format!("content://media/external_primary/downloads/{}", index + 1);
        let file = &artifact["file"];
        store.value(json!({"op":"downloads_publication_target","id":id,"generation":generation,"file":file,"uri":uri}));
        for (bytes, digest) in [(999, hash.clone()), (1000, "cd".repeat(32))] {
            assert_eq!(store.call(json!({"op":"downloads_publication_verified","id":id,"generation":generation,"file":file,"uri":uri,"sourceBytes":1000,"copiedBytes":bytes,"sourceSha256":hash,"copiedSha256":digest}))["ok"], false);
        }
        store.value(json!({"op":"downloads_publication_verified","id":id,"generation":generation,"file":file,"uri":uri,"sourceBytes":1000,"copiedBytes":1000,"sourceSha256":hash.to_uppercase(),"copiedSha256":hash}));
    }
    // Simulate a process dying after the final copy, before the JS completion event.
    let restored = store.value(json!({"op":"downloads_restore"}));
    assert_eq!(restored["entries"][0]["state"], "complete");
    assert_eq!(
        restored["entries"][0]["artifacts"][0]["publishedBytes"],
        1000
    );
    assert_eq!(
        restored["entries"][0]["artifacts"][0]["publishedSha256"],
        hash
    );
    assert_eq!(
        store.call(json!({"op":"downloads_publication_check","id":id,"generation":generation}))["ok"],
        false
    );
    let replanned = store.value(json!({"op":"downloads_publication_plan","id":id}));
    assert!(
        replanned["publication"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["published"] == true)
    );
    let stored = fs::read_to_string(store.0.join("media/downloads.json")).unwrap();
    for secret in ["googlevideo", "secret", "\"url\""] {
        assert!(!stored.contains(secret));
    }
    store.value(json!({"op":"downloads_event","id":id,"event":"retry"}));
    let (retry_id, retried) = prepared_download(&store);
    assert_eq!(retry_id, id);
    for artifact in retried {
        assert!(artifact["publishedUri"].is_string());
        assert_eq!(artifact["published"], false);
        assert!(artifact["publishedSha256"].is_null());
    }
    assert_eq!(
        store.call(
            json!({"op":"downloads_event","id":id,"event":"complete","publicationRequired":true})
        )["ok"],
        false
    );
}

#[test]
fn publication_rejects_foreign_targets_and_superseded_or_cancelled_work() {
    let store = Store::new();
    let (id, artifacts) = prepared_download(&store);
    let file = &artifacts[0]["file"];
    let first = store.value(json!({"op":"downloads_publication_plan","id":id}))["publication"]["generation"].clone();
    let generation = store.value(json!({"op":"downloads_publication_plan","id":id}))["publication"]
        ["generation"]
        .clone();
    assert_eq!(
        store.call(json!({"op":"downloads_publication_check","id":id,"generation":first}))["ok"],
        false
    );
    for uri in [
        "https://r1.googlevideo.com/private?sig=secret",
        "content://other/external/downloads/1",
        "content://media/external/images/1",
        "content://media/external//downloads/1",
        "content://media/external/downloads/1?token=secret",
        "content://media/external/downloads/1#secret",
        "content://media/external/downloads/%31",
    ] {
        assert_eq!(store.call(json!({"op":"downloads_publication_target","id":id,"generation":generation,"file":file,"uri":uri}))["ok"], false, "{uri}");
    }
    let uri = "content://media/external_primary/downloads/1";
    assert_eq!(store.call(json!({"op":"downloads_publication_target","id":id,"generation":generation,"file":"foreign.mp4","uri":uri}))["ok"], false);
    store.value(json!({"op":"downloads_publication_target","id":id,"generation":generation,"file":file,"uri":uri}));
    assert_eq!(store.call(json!({"op":"downloads_publication_target","id":id,"generation":generation,"file":file,"uri":"content://media/external_primary/downloads/2"}))["ok"], false);
    store.value(json!({"op":"downloads_event","id":id,"event":"pause"}));
    store.value(json!({"op":"downloads_event","id":id,"event":"resume","retained":true}));
    assert_eq!(
        store.call(json!({"op":"downloads_publication_check","id":id,"generation":generation}))["ok"],
        false
    );
    let next = store.value(json!({"op":"downloads_publication_plan","id":id}))["publication"]["generation"].clone();
    let pending = store.value(json!({"op":"downloads_restore"}));
    assert_eq!(pending["entries"][0]["state"], "paused");
    assert_eq!(
        store.call(json!({"op":"downloads_publication_check","id":id,"generation":next}))["ok"],
        false
    );
    store.value(json!({"op":"downloads_event","id":id,"event":"cancel"}));
    assert_eq!(
        store.call(json!({"op":"downloads_publication_plan","id":id}))["ok"],
        false
    );
    let mut forged = pending["entries"].clone();
    forged[0]["artifacts"][0]["published"] = json!(true);
    assert_eq!(
        store.call(json!({"op":"downloads_history","entries":forged}))["ok"],
        false
    );
    let stored = fs::read_to_string(store.0.join("media/downloads.json")).unwrap();
    assert!(!stored.contains("secret"));
}

#[test]
fn completed_downloads_keep_safe_history_and_export_owned_artifacts() {
    let store = Store::new();
    let prefs = store.value(json!({"op":"downloads_config"}));
    let mut config = prefs["lastConfig"].clone();
    config["includeCover"] = json!(true);
    config["fileName"] = json!("Port test");
    let item = json!({"videoId":"abcdefghijk","title":"Public video","author":"Author","url":"https://r1.googlevideo.com/private?sig=secret"});
    let queued = store.value(json!({"op":"downloads_enqueue","items":[item],"config":config}));
    let id = queued["entries"][0]["id"].clone();
    let duplicate = store.value(json!({"op":"downloads_enqueue","items":[item],"config":config}));
    assert_eq!(duplicate["entries"].as_array().unwrap().len(), 1);
    let claimed = store.value(json!({"op":"downloads_claim","wifi":true}));
    assert_eq!(claimed["claimed"], json!([id]));
    let catalog = json!({"videoId":"abcdefghijk","title":"Public video","author":"Author","streams":[{"key":"18:","itag":18,"kind":"combined","height":360,"fps":30,"bytes":1000,"mimeType":"video/mp4","url":"https://r1.googlevideo.com/videoplayback?sig=secret"}]});
    let prepared = store.value(json!({"op":"downloads_prepared","id":id,"catalog":catalog}));
    assert_eq!(
        prepared["plan"]["parts"][0]["url"],
        catalog["streams"][0]["url"]
    );
    let artifacts = prepared["entries"][0]["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    store.value(json!({"op":"downloads_event","id":id,"event":"progress","received":400}));
    store.value(json!({"op":"downloads_event","id":id,"event":"pause"}));
    let restored = store.value(json!({"op":"downloads_restore"}));
    assert_eq!(restored["entries"][0]["state"], "paused");
    assert_eq!(restored["entries"][0]["received"], 400);
    store.value(json!({"op":"downloads_event","id":id,"event":"resume","retained":true}));
    let completed = store.value(json!({"op":"downloads_event","id":id,"event":"complete"}));
    assert_eq!(completed["entries"][0]["received"], 1000);
    for (artifact, expected) in artifacts.iter().zip(["Port test.mp4", "Port test.jpg"]) {
        assert_eq!(
            store.value(json!({"op":"downloads_export_name","id":id,"file":artifact["file"]})),
            expected
        );
    }
    assert_eq!(
        store.call(json!({"op":"downloads_export_name","id":id,"file":"another.mp4"}))["ok"],
        false
    );
    assert_eq!(
        store.call(json!({"op":"downloads_event","id":id,"event":"progress","received":900}))["ok"],
        false
    );
    let stored = fs::read_to_string(store.0.join("media/downloads.json")).unwrap();
    assert!(!stored.contains("googlevideo"));
    assert!(!stored.contains("secret"));
    assert!(!stored.contains("\"url\""));
    let next = json!({"videoId":"0123456789a","title":"Next video","author":"Author"});
    store.value(json!({"op":"downloads_enqueue","items":[next],"config":config}));
    store.value(json!({"op":"downloads_claim","wifi":true}));
    let terminal = store
        .value(json!({"op":"downloads_view","entries":completed["entries"],"filter":"terminal"}));
    assert_eq!(terminal["items"].as_array().unwrap().len(), 1);
    let cleared = store.value(json!({"op":"downloads_clear_history"}));
    assert_eq!(cleared["removed"][0]["id"], id);
    assert_eq!(cleared["entries"].as_array().unwrap().len(), 1);
    assert_eq!(cleared["entries"][0]["state"], "resolving");
    let recovered = store.value(json!({"op":"downloads_restore"}));
    assert_eq!(recovered["entries"][0]["state"], "paused");
}
