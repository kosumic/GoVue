#![cfg(test)]

use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static SESSIONS: Mutex<()> = Mutex::new(());

fn call(command: Value) -> Value {
    serde_json::from_str(&govue_core::dictation_execute(command.to_string())).unwrap()
}
fn ok(command: Value) -> Value {
    let reply = call(command);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
struct Session(&'static str);
impl Session {
    fn new(id: &'static str) -> Self {
        assert_eq!(
            ok(json!({"op":"begin", "sessionId":id, "draft":""}))["phase"],
            "preparing"
        );
        Self(id)
    }
    fn step(&self, op: &str) -> Value {
        ok(json!({"op":op, "sessionId":self.0}))
    }
    fn result(&self, text: &str, final_result: bool, draft: &str) -> Value {
        ok(
            json!({"op":"result", "sessionId":self.0, "text":text, "isFinal":final_result, "draft":draft}),
        )
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        call(json!({"op":"end", "sessionId":self.0}));
    }
}

#[test]
fn preview_never_changes_the_draft_and_cancellation_rejects_late_results() {
    let _lock = SESSIONS.lock().unwrap();
    let session = Session::new("preview");
    assert!(session.result("too early", true, "Keep this")["draft"].is_null());
    session.step("listening");
    assert_eq!(
        ok(json!({"op":"result", "sessionId":"preview", "text":"partial", "isFinal":false}))["interimText"],
        "partial"
    );
    assert_eq!(
        call(json!({"op":"result", "sessionId":"preview", "text":"final", "isFinal":true}))["ok"],
        false
    );
    assert_eq!(
        session.result("hel", false, "Keep this")["interimText"],
        "hel"
    );
    let preview = session.result("hello", false, "Keep this");
    assert_eq!(preview["interimText"], "hello");
    assert!(preview["draft"].is_null());
    assert_eq!(session.step("cancel")["interimText"], "");
    assert!(session.result("late", true, "Keep this")["draft"].is_null());
    assert_eq!(session.step("end")["noSpeech"], false);

    let next = Session::new("next");
    assert_eq!(
        call(json!({"op":"end", "sessionId":"preview"}))["ok"],
        false
    );
    assert_eq!(
        call(json!({"op":"begin", "sessionId":"other", "draft":""}))["ok"],
        false
    );
    assert_eq!(next.step("listening")["phase"], "listening");
}

#[test]
fn finals_join_the_latest_draft_with_script_and_punctuation_aware_spacing() {
    let _lock = SESSIONS.lock().unwrap();
    let session = Session::new("joining");
    session.step("listening");
    for (draft, text, expected) in [
        ("Existing draft", " new words ", "Existing draft new words"),
        ("Edited\n", "new words", "Edited\nnew words"),
        ("你好", "世界", "你好世界"),
        ("これは", "テストです", "これはテストです"),
        ("Hello", ", world", "Hello, world"),
        ("word", "。", "word。"),
        ("again", "again", "again again"),
    ] {
        let reply = session.result(text, true, draft);
        assert_eq!(reply["draft"], expected);
        assert_eq!(reply["interimText"], "");
    }
    assert_eq!(session.step("finish")["phase"], "finishing");
    assert_eq!(
        session.result("last words", true, "Latest edit")["draft"],
        "Latest edit last words"
    );
    assert_eq!(session.step("end")["noSpeech"], false);
}

#[test]
fn empty_sessions_report_no_speech_and_utf16_limits_preserve_whole_characters() {
    let _lock = SESSIONS.lock().unwrap();
    let session = Session::new("limits");
    session.step("listening");
    assert!(session.result("  ", true, "Unchanged")["draft"].is_null());
    assert_eq!(session.step("end")["noSpeech"], true);

    assert_eq!(
        call(json!({"op":"begin", "sessionId":"full", "draft":"a".repeat(8000)}))["ok"],
        false
    );
    let session = Session::new("unicode");
    session.step("listening");
    let draft = "你".repeat(7998);
    let reply = session.result("😀x", true, &draft);
    let text = reply["draft"].as_str().unwrap();
    assert_eq!(text.encode_utf16().count(), 8000);
    assert!(text.ends_with('😀'));
    let draft = "你".repeat(7999);
    assert_eq!(session.result("😀", true, &draft)["draft"], draft);
    assert_eq!(
        call(
            json!({"op":"result", "sessionId":"unicode", "text":"x".repeat(16001), "isFinal":true, "draft":"Keep"})
        )["ok"],
        false
    );
    assert_eq!(session.result("valid", true, "Keep")["draft"], "Keep valid");
}

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "govue-dictation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("ai/chats")).unwrap();
        Self(root)
    }
    fn cache(&self, text: String) {
        let transcript = json!({"videoId":"abcdefghijk", "title":"Caption title", "language":"English", "trackId":".en",
            "tracks":[{"id":".en", "label":"English", "language":"en", "automatic":false}],
            "segments":[{"start":0,"duration":1,"text":text}]});
        let raw = json!({"type":"transcript", "requestId":"voice", "videoId":"abcdefghijk", "ok":true, "transcript":transcript});
        let result: Value = serde_json::from_str(&govue_core::execute(json!({"op":"accept", "videoId":"abcdefghijk", "origin":"https://www.youtube.com", "requestId":"voice", "raw":raw.to_string()}).to_string(), self.0.to_string_lossy().into_owned())).unwrap();
        assert_eq!(result["ok"], true, "{result}");
    }
    fn hints(&self, fields: Value) -> Value {
        let mut command = fields;
        command["op"] = json!("ai_chat_speech_hints");
        let reply: Value = serde_json::from_str(&govue_core::ai_execute(
            command.to_string(),
            self.0.to_string_lossy().into_owned(),
        ))
        .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn hints_include_chat_and_captions_without_hidden_reasoning_or_storage_changes() {
    let store = Store::new();
    store.cache("Elephant Café Cafe\u{301} Ｑｕｏｋｋａ".into());
    let chat = json!({"id":"voicechat", "videoId":"abcdefghijk", "title":"Conversation", "providerId":"fixture", "providerLabel":"Fixture", "model":"fixture", "createdAt":1, "updatedAt":1, "activeTask":null,
        "transcript":{"videoId":"abcdefghijk", "trackId":".en", "title":"Caption title", "language":"English"},
        "messages":[{"id":"question", "role":"user", "content":"Quokka", "status":"completed"}, {"id":"answer", "role":"assistant", "content":"Wombat", "reasoning":"Hiddenreasoning", "status":"completed"}]});
    let path = store.0.join("ai/chats/voicechat.json");
    fs::write(&path, chat.to_string()).unwrap();
    let before = fs::read(&path).unwrap();
    let hints = store.hints(json!({"chatId":"voicechat", "draft":"Platypus"}));
    let words: Vec<_> = hints
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_lowercase())
        .collect();
    for expected in ["quokka", "wombat", "platypus", "elephant", "café"] {
        assert!(words.iter().any(|word| word == expected), "{words:?}");
    }
    assert!(!words.iter().any(|word| word == "hiddenreasoning"));
    assert_eq!(
        words.iter().filter(|word| word.as_str() == "café").count(),
        1
    );
    assert_eq!(fs::read(path).unwrap(), before);
    let fallback = store.hints(json!({"chatId":"voicechat", "draft":"Platypus", "transcript":{"videoId":"abcdefghijk", "trackId":"missing"}}));
    assert!(
        fallback
            .as_array()
            .unwrap()
            .iter()
            .any(|word| word == "Wombat")
    );
}

#[test]
fn large_hint_sources_are_balanced_bounded_unique_and_draft_fallback_works() {
    let store = Store::new();
    store.cache(
        (0..200)
            .map(|i| format!("captionword{i}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let draft = (0..200)
        .map(|i| format!("chatword{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let hints = store
        .hints(json!({"draft":draft, "transcript":{"videoId":"abcdefghijk", "trackId":".en"}}));
    let words: Vec<_> = hints
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(words.len() <= 100);
    assert!(
        words
            .iter()
            .filter(|word| word.starts_with("captionword"))
            .count()
            >= 40
    );
    assert!(
        words
            .iter()
            .filter(|word| word.starts_with("chatword"))
            .count()
            >= 40
    );
    assert_eq!(
        words
            .iter()
            .map(|word| word.to_lowercase())
            .collect::<HashSet<_>>()
            .len(),
        words.len()
    );
    let fallback = ok(json!({"op":"hints", "draft":"Café Cafe\u{301} Ｑｕｏｋｋａ"}));
    assert_eq!(fallback.as_array().unwrap().len(), 2);
}
