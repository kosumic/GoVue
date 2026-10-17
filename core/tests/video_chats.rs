//! Regressions after the Pixel video chat opened empty with its transcript attached.
use lingoplay_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, execute, install_ai_platform,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
}
struct Platform(Arc<Backend>);
impl AiPlatform for Platform {
    fn secret_read(&self, key: String) -> AiSecret {
        AiSecret {
            ok: true,
            value: self
                .0
                .secrets
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .unwrap_or_default(),
        }
    }
    fn secret_write(&self, key: String, value: String) -> bool {
        self.0.secrets.lock().unwrap().insert(key, value);
        true
    }
    fn request(&self, request: AiHttpRequest, response: Arc<AiResponse>) {
        self.0
            .requests
            .lock()
            .unwrap()
            .push(serde_json::from_str(&request.body).unwrap());
        response.start(200);
        response.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_vec());
    }
}
struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lingoplay-video-chats-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn call(root: &Path, command: Value) -> Value {
    serde_json::from_str(&ai_execute(
        command.to_string(),
        root.to_string_lossy().into_owned(),
    ))
    .unwrap()
}
fn ok(root: &Path, command: Value) -> Value {
    let result = call(root, command);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn cache(root: &Path, video: &str) {
    cache_track(root, video, ".en");
}
fn cache_track(root: &Path, video: &str, track: &str) {
    let raw = json!({"type":"transcript","requestId":"video-chat:fixture","videoId":video,"ok":true,"transcript":{
        "videoId":video,"title":"Video fixture","trackId":track,"language":if track == ".en" { "English" } else { "Japanese" },
        "tracks":[{"id":".en","label":"English","language":"en","automatic":false},{"id":".ja","label":"Japanese","language":"ja","automatic":false}],
        "segments":[{"start":12.5,"duration":2,"text":"Caption context for the question."}]
    }});
    let result: Value = serde_json::from_str(&execute(json!({"op":"accept","videoId":video,"requestId":"video-chat:fixture","origin":"https://www.youtube.com","raw":raw.to_string()}).to_string(), root.to_string_lossy().into_owned())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
fn wait(root: &Path, started: &Value) {
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":started["taskId"]}));
        if state["status"] != "running" {
            assert_eq!(state["status"], "completed", "{state}");
            ok(root, json!({"op":"ai_forget","taskId":started["taskId"]}));
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("Video chat fixture timed out");
}

#[test]
fn video_chats_are_created_empty_reused_and_keep_context_across_models() {
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    let store = Store::new();
    let root = &store.0;
    let video = "abcdefghijk";
    let other = "0123456789a";

    // Opening requires neither a provider nor a transcript and never starts inference.
    let draft = ok(
        root,
        json!({"op":"ai_chat_video_open","videoId":video,"title":"Video fixture"}),
    );
    assert_eq!(draft["messages"], json!([]));
    assert!(draft["transcript"].is_null());
    assert!(draft["activeTask"].is_null());
    let id = draft["id"].as_str().unwrap().to_owned();
    assert_eq!(
        ok(root, json!({"op":"ai_chat_get","chatId":id}))["videoId"],
        video
    );
    let opens: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            thread::spawn(move || ok(&root, json!({"op":"ai_chat_video_open","videoId":video})))
        })
        .collect();
    for open in opens {
        assert_eq!(open.join().unwrap()["id"], id);
    }
    assert_eq!(
        ok(root, json!({"op":"ai_chats"}))["chats"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let second = ok(root, json!({"op":"ai_chat_video_open","videoId":other}));
    assert_ne!(second["id"], id);
    assert!(backend.requests.lock().unwrap().is_empty());
    assert_eq!(
        call(
            root,
            json!({"op":"ai_chat_video_open","videoId":"../unsafe"})
        )["ok"],
        false
    );

    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":"Fixture provider","baseUrl":"http://127.0.0.1:12345/v1","protocol":"chat","model":"first","models":[{"id":"first"},{"id":"second"}]},"key":"FIXTURE_SECRET"}),
    );
    let provider = cfg["providers"][0]["id"].as_str().unwrap();
    ok(
        root,
        json!({"op":"ai_chat_model","chatId":id,"id":provider}),
    );
    assert_eq!(
        call(
            root,
            json!({"op":"ai_chat_send","chatId":id,"id":provider,"prompt":"A question"})
        )["ok"],
        false
    );
    assert!(backend.requests.lock().unwrap().is_empty());
    cache(root, video);
    cache(root, other);
    let attached = ok(
        root,
        json!({"op":"ai_chat_video_open","videoId":video,"transcript":{"videoId":video,"trackId":".en"}}),
    );
    assert_eq!(attached["id"], id);
    assert_eq!(attached["transcript"]["videoId"], video);
    assert_eq!(attached["messages"], json!([]));
    assert!(backend.requests.lock().unwrap().is_empty());

    // Listening uses the attached caption without starting inference or requiring a game.
    let audio =
        json!({"op":"ai_chat_audio","chatId":id,"videoId":video,"trackId":".en","cueId":"cue-0"});
    assert_eq!(ok(root, audio)["needsMedia"], true);
    assert!(backend.requests.lock().unwrap().is_empty());
    cache_track(root, video, ".ja");
    let comparison_audio =
        json!({"op":"ai_chat_audio","chatId":id,"videoId":video,"trackId":".ja","cueId":"cue-0"});
    assert_eq!(ok(root, comparison_audio.clone())["needsMedia"], true);
    let mut invalid = comparison_audio.clone();
    invalid["videoId"] = json!(other);
    assert_eq!(call(root, invalid)["ok"], false);
    let mut invalid = comparison_audio;
    invalid["trackId"] = json!(".fr");
    assert_eq!(call(root, invalid)["ok"], false);

    // Explicitly sending null cannot remove a video chat's required context.
    let sent = ok(
        root,
        json!({"op":"ai_chat_send","chatId":id,"id":provider,"prompt":"What does the speaker say?","transcript":null}),
    );
    wait(root, &sent);
    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let wire = requests[0].to_string();
    assert!(wire.contains("Caption context for the question."));
    assert!(wire.contains("0:12"));
    assert!(wire.contains("What does the speaker say?"));
    assert!(!wire.contains("Summarize this video"));
    drop(requests);
    let reopened = ok(root, json!({"op":"ai_chat_video_open","videoId":video}));
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["messages"].as_array().unwrap().len(), 2);
    ok(
        root,
        json!({"op":"ai_select","id":provider,"model":"second"}),
    );
    let changed = ok(
        root,
        json!({"op":"ai_chat_model","chatId":id,"id":provider}),
    );
    assert_eq!(changed["id"], id);
    assert_eq!(changed["model"], "second");
    assert_eq!(changed["messages"], reopened["messages"]);
    assert_eq!(changed["transcript"], reopened["transcript"]);

    // A second entry point that supplies only the video context reuses the same chat.
    let again = ok(
        root,
        json!({"op":"ai_chat_send","id":provider,"prompt":"Follow up","transcript":{"videoId":video,"trackId":".en"}}),
    );
    assert_eq!(again["conversation"]["id"], id);
    wait(root, &again);
    assert_eq!(
        call(
            root,
            json!({"op":"ai_chat_video_open","videoId":video,"transcript":{"videoId":other,"trackId":".en"}})
        )["ok"],
        false
    );
    assert_eq!(
        call(
            root,
            json!({"op":"ai_chat_send","chatId":id,"id":provider,"prompt":"Wrong video","transcript":{"videoId":other,"trackId":".en"}})
        )["ok"],
        false
    );
    assert_eq!(backend.requests.lock().unwrap().len(), 2);
    let saved = ok(root, json!({"op":"ai_chat_get","chatId":id}));
    assert_eq!(saved["messages"].as_array().unwrap().len(), 4);

    // Legacy duplicates remain on disk, while navigation exposes one canonical video chat.
    let mut legacy = saved.clone();
    legacy.as_object_mut().unwrap().remove("videoId");
    legacy["id"] = json!("legacy_duplicate");
    legacy["updatedAt"] = json!(u64::MAX);
    let legacy_path = root.join("ai/chats/legacy_duplicate.json");
    fs::write(&legacy_path, legacy.to_string()).unwrap();
    let visible = ok(root, json!({"op":"ai_chats"}));
    let rows = visible["chats"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|row| row["videoId"] == video).count(), 1);
    assert_eq!(
        rows.iter().find(|row| row["videoId"] == video).unwrap()["id"],
        id
    );
    assert!(legacy_path.exists());
    assert_eq!(
        ok(root, json!({"op":"ai_chat_video_open","videoId":video}))["id"],
        id
    );
    let path = root.join(format!("ai/chats/{id}.json"));
    let mut old = saved;
    old.as_object_mut().unwrap().remove("videoId");
    fs::write(&path, old.to_string()).unwrap();
    fs::remove_file(&legacy_path).unwrap();
    assert_eq!(
        ok(root, json!({"op":"ai_chat_video_open","videoId":video}))["id"],
        id
    );
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("FIXTURE_SECRET")
    );

    // Editing validates before persisting, then removes the abandoned branch from inference.
    let before = ok(root, json!({"op":"ai_chat_get","chatId":id}));
    let first_user = before["messages"][0]["id"].clone();
    let second_user = before["messages"][2]["id"].clone();
    let bytes = fs::read(&path).unwrap();
    for command in [
        json!({"editMessageId":before["messages"][1]["id"],"prompt":"Changed"}),
        json!({"editMessageId":"missing","prompt":"Changed"}),
        json!({"editMessageId":second_user,"prompt":"  "}),
        json!({"editMessageId":second_user,"prompt":"Changed","retry":true}),
    ] {
        let mut request = json!({"op":"ai_chat_send","chatId":id,"id":provider});
        request
            .as_object_mut()
            .unwrap()
            .extend(command.as_object().unwrap().clone());
        assert_eq!(call(root, request)["ok"], false);
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "invalid edits preserve history"
        );
    }
    let edited = ok(
        root,
        json!({"op":"ai_chat_send","chatId":id,"id":provider,"editMessageId":second_user,"prompt":"Edited follow up"}),
    );
    assert_eq!(edited["conversation"]["messages"][0], before["messages"][0]);
    assert_eq!(edited["conversation"]["messages"][1], before["messages"][1]);
    assert_eq!(edited["conversation"]["messages"][2]["id"], second_user);
    assert_ne!(
        edited["conversation"]["messages"][3]["id"],
        before["messages"][3]["id"]
    );
    wait(root, &edited);
    let reset = ok(
        root,
        json!({"op":"ai_chat_send","chatId":id,"id":provider,"editMessageId":first_user,"prompt":"Restart with a new question"}),
    );
    assert_eq!(
        reset["conversation"]["messages"].as_array().unwrap().len(),
        2
    );
    assert_eq!(reset["conversation"]["messages"][0]["id"], first_user);
    assert_eq!(reset["conversation"]["transcript"], before["transcript"]);
    assert_eq!(reset["conversation"]["model"], before["model"]);
    wait(root, &reset);
    let request = backend.requests.lock().unwrap().last().unwrap().clone();
    let wire = request.to_string();
    assert!(wire.contains("Restart with a new question"));
    assert!(!wire.contains("Edited follow up"));
    assert!(!wire.contains("Follow up"));
    assert!(wire.contains("Caption context for the question."));
    assert_eq!(request["model"], "second");
    assert_eq!(
        ok(root, json!({"op":"ai_chat_get","chatId":id}))["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
