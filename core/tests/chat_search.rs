#![cfg(test)]

use govue_core::{ai_execute, execute};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "govue-chat-search-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("ai/chats")).unwrap();
        Self(path)
    }
    fn write(&self, id: &str, title: &str, updated: u64, texts: &[&str]) {
        let messages: Vec<_> = texts.iter().enumerate().map(|(index, text)| json!({
            "id":format!("{id}_{index}"),"role":if index % 2 == 0 { "user" } else { "assistant" },
            "content":text,"status":"completed",
            "preparedInput":if index % 2 == 0 { json!({"content":"hidden provider context"}) } else { Value::Null }
        })).collect();
        fs::write(self.0.join(format!("ai/chats/{id}.json")), json!({
            "id":id,"title":title,"providerId":"fixture","providerLabel":"Local","model":"fixture",
            "createdAt":1,"updatedAt":updated,"transcript":null,"messages":messages,"activeTask":null
        }).to_string()).unwrap();
    }
    fn call(&self, command: Value) -> Value {
        serde_json::from_str(&ai_execute(
            command.to_string(),
            self.0.to_string_lossy().into_owned(),
        ))
        .unwrap()
    }
    fn ok(&self, command: Value) -> Value {
        let result = self.call(command);
        assert_eq!(result["ok"], true, "{result}");
        result["value"].clone()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn history_searches_titles_and_message_text_without_exposing_prepared_context() {
    let store = Store::new();
    store.write(
        "older",
        "French lesson",
        1,
        &["What is café?", "CAFÉ means coffee."],
    );
    store.write("newer", "Café vocabulary", 2, &["A question", "A reply"]);
    let result = store.ok(json!({"op":"ai_chats","query":"  CAFÉ  "}));
    assert_eq!(result["query"], "CAFÉ");
    assert_eq!(result["chats"].as_array().unwrap().len(), 2);
    assert_eq!(result["chats"][0]["id"], "newer");
    assert!(result["chats"][0]["searchMatch"]["messageId"].is_null());
    assert_eq!(result["chats"][1]["searchMatch"]["messageId"], "older_0");
    assert_eq!(result["chats"][1]["searchMatch"]["matched"], "café");
    assert_eq!(
        store.ok(json!({"op":"ai_chats","query":"hidden provider context"}))["chats"],
        json!([])
    );
    assert_eq!(
        store.ok(json!({"op":"ai_chats","query":"   "}))["chats"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(store.call(json!({"op":"ai_chats","query":42}))["ok"], false);
}

#[test]
fn history_index_lists_and_searches_metadata_without_opening_message_records() {
    let store = Store::new();
    store.write(
        "older",
        "French lesson",
        1,
        &["Private prompt", "Private reply"],
    );
    store.write(
        "newer",
        "Vocabulary",
        2,
        &["A question", "Searchable answer"],
    );
    let expected = store.ok(json!({"op":"ai_chats"}));
    let index = fs::read_to_string(store.0.join("ai/chat-index.json")).unwrap();
    assert!(!index.contains("messages"));
    assert!(!index.contains("Private prompt"));
    assert!(!index.contains("hidden provider context"));
    // Cached listing must still work when a body cannot be decoded. Opening
    // that chat continues to validate the full saved record independently.
    fs::write(store.0.join("ai/chats/older.json"), "unreadable body").unwrap();
    assert_eq!(store.ok(json!({"op":"ai_chats"})), expected);
    assert_eq!(
        store.call(json!({"op":"ai_chat_get","chatId":"older"}))["ok"],
        false
    );
    let titles = store.ok(json!({"op":"ai_chats","query":"French"}));
    assert_eq!(titles["chats"][0]["id"], "older");
    assert_eq!(titles["chats"][0]["searchMatch"]["matched"], "French");
    let content = store.ok(json!({"op":"ai_chats","query":"Searchable"}));
    assert_eq!(content["chats"].as_array().unwrap().len(), 1);
    assert_eq!(content["chats"][0]["searchMatch"]["messageId"], "newer_1");
}

#[test]
fn history_index_recovers_save_and_delete_when_index_commit_fails() {
    let store = Store::new();
    store.write("chat", "Original", 1, &["A question", "A reply"]);
    store.ok(json!({"op":"ai_chats"}));
    let blocked = store.0.join("ai/chat-index.json.tmp");
    let dirty = store.0.join("ai/chat-index.dirty");
    fs::create_dir(&blocked).unwrap();
    // The body commits but replacing the index fails. A later reader must
    // rebuild from the committed body rather than return the stale index.
    assert_eq!(
        store.call(
            json!({"op":"ai_chat_update","chatId":"chat","title":"Committed title","pinned":true})
        )["ok"],
        false
    );
    assert!(dirty.exists());
    assert_eq!(
        store.ok(json!({"op":"ai_chat_get","chatId":"chat"}))["title"],
        "Committed title"
    );
    fs::remove_dir(&blocked).unwrap();
    let listed = store.ok(json!({"op":"ai_chats"}));
    assert_eq!(listed["chats"][0]["title"], "Committed title");
    assert_eq!(listed["chats"][0]["pinned"], true);
    assert!(!dirty.exists());

    fs::create_dir(&blocked).unwrap();
    assert_eq!(
        store.call(json!({"op":"ai_chat_delete","chatId":"chat"}))["ok"],
        false
    );
    assert!(dirty.exists());
    assert!(!store.0.join("ai/chats/chat.json").exists());
    fs::remove_dir(&blocked).unwrap();
    assert_eq!(store.ok(json!({"op":"ai_chats"}))["chats"], json!([]));
    assert!(!dirty.exists());
    store.ok(json!({"op":"ai_chat_delete","chatId":"chat"}));
}

#[test]
fn failed_body_write_does_not_publish_uncommitted_metadata() {
    let store = Store::new();
    store.write("chat", "Original", 1, &["A question", "A reply"]);
    store.ok(json!({"op":"ai_chats"}));
    fs::create_dir(store.0.join("ai/chats/chat.json.tmp")).unwrap();
    assert_eq!(
        store.call(json!({"op":"ai_chat_update","chatId":"chat","title":"Uncommitted title"}))["ok"],
        false
    );
    assert_eq!(
        store.ok(json!({"op":"ai_chats"}))["chats"][0]["title"],
        "Original"
    );
    assert_eq!(
        store.ok(json!({"op":"ai_chat_get","chatId":"chat"}))["title"],
        "Original"
    );
    assert!(!store.0.join("ai/chat-index.dirty").exists());
}

#[test]
fn history_index_rebuilds_missing_invalid_oversized_and_interrupted_indexes() {
    let store = Store::new();
    store.write("chat", "Original", 1, &["A question", "A reply"]);
    store.ok(json!({"op":"ai_chats"}));
    let path = store.0.join("ai/chat-index.json");
    let stale = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(
        store.ok(json!({"op":"ai_chats"}))["chats"][0]["title"],
        "Original"
    );
    for invalid in [
        "unreadable index".to_owned(),
        json!({"version":0,"chats":[]}).to_string(),
        " ".repeat(1_000_001),
    ] {
        fs::write(&path, invalid).unwrap();
        assert_eq!(
            store.ok(json!({"op":"ai_chats"}))["chats"][0]["title"],
            "Original"
        );
    }
    // Simulate a restart after the record committed and before index commit.
    store.write("chat", "After interruption", 2, &["A question", "A reply"]);
    fs::write(&path, stale).unwrap();
    fs::write(store.0.join("ai/chat-index.dirty"), "1").unwrap();
    let listed = store.ok(json!({"op":"ai_chats"}));
    assert_eq!(listed["chats"][0]["title"], "After interruption");
    assert_eq!(listed["chats"][0]["updatedAt"], 2);
    assert!(!store.0.join("ai/chat-index.dirty").exists());
}

#[test]
fn find_is_literal_unicode_safe_and_retains_selection_across_new_messages() {
    let store = Store::new();
    store.write("chat", "Find", 1, &["😀 İSTANBUL a.b", "A.B and aXb"]);
    let result = store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"i"}));
    assert_eq!(result["matches"][0]["matched"], "İ");
    assert_eq!(result["matches"][0]["before"], "😀 ");
    let result = store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"a.b"}));
    assert_eq!(result["matches"].as_array().unwrap().len(), 2);
    assert_eq!(result["matches"][1]["matched"], "A.B");
    let previous =
        json!({"query":"a.b","messageId":"chat_0","offset":result["matches"][0]["offset"]});
    let result = store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"a.b","previous":previous,"direction":"previous"}));
    assert_eq!(result["selected"], 1);
    let previous =
        json!({"query":"a.b","messageId":"chat_1","offset":result["matches"][1]["offset"]});
    store.write(
        "chat",
        "Find",
        2,
        &["😀 İSTANBUL a.b", "A.B and aXb", "a.b again", "An answer"],
    );
    let result =
        store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"a.b","previous":previous}));
    assert_eq!(result["selected"], 1);
    let result = store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"a.b","previous":previous,"direction":"next"}));
    assert_eq!(result["selected"], 2);
    assert_eq!(
        store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"missing"}))["selected"],
        Value::Null
    );
}

#[test]
fn find_bounds_results_and_excerpts_and_rejects_oversized_queries() {
    let store = Store::new();
    let text = "😀 café ".repeat(600);
    store.write("chat", "Bounds", 1, &[&text, "A reply"]);
    let result = store.ok(json!({"op":"ai_chat_find","chatId":"chat","query":"café"}));
    assert_eq!(result["matches"].as_array().unwrap().len(), 500);
    assert_eq!(result["truncated"], true);
    for hit in result["matches"].as_array().unwrap() {
        assert!(hit["before"].as_str().unwrap().chars().count() <= 48);
        assert!(hit["after"].as_str().unwrap().chars().count() <= 48);
    }
    assert_eq!(
        store.call(json!({"op":"ai_chat_find","chatId":"chat","query":"a".repeat(201)}))["ok"],
        false
    );
    assert_eq!(
        store.call(json!({"op":"ai_chat_find","chatId":"../chat","query":"café"}))["ok"],
        false
    );
}

#[test]
fn history_actions_preserve_old_chats_order_pins_and_keep_archives_searchable() {
    let store = Store::new();
    store.write("older", "Older lesson", 1, &["A phrase", "A reply"]);
    store.write(
        "newer",
        "Newer lesson",
        2,
        &["Another phrase", "Another reply"],
    );
    let changed = store.ok(json!({"op":"ai_chat_update","chatId":"older","pinned":true,"title":"  My French lesson  "}));
    assert_eq!(changed["title"], "My French lesson");
    assert_eq!(changed["messages"].as_array().unwrap().len(), 2);
    assert_eq!(
        store.ok(json!({"op":"ai_chats"}))["chats"][0]["id"],
        "older"
    );
    store.ok(json!({"op":"ai_chat_update","chatId":"older","archived":true}));
    let visible = store.ok(json!({"op":"ai_chats"}));
    assert_eq!(visible["chats"].as_array().unwrap().len(), 1);
    assert_eq!(visible["chats"][0]["id"], "newer");
    let archived = store.ok(json!({"op":"ai_chats","archived":true,"query":"PHRASE"}));
    assert_eq!(archived["chats"][0]["id"], "older");
    assert_eq!(archived["chats"][0]["pinned"], true);
    assert_eq!(
        store.ok(json!({"op":"ai_chat_get","chatId":"older"}))["archived"],
        true
    );
    for values in [
        json!({"title":"  "}),
        json!({"title":"a\nb"}),
        json!({"title":"a".repeat(201)}),
        json!({"pinned":"true"}),
        json!({"archived":1}),
    ] {
        let mut command = values;
        command["op"] = json!("ai_chat_update");
        command["chatId"] = json!("older");
        assert_eq!(store.call(command)["ok"], false);
    }
    assert_eq!(
        store.ok(json!({"op":"ai_chat_get","chatId":"older"}))["title"],
        "My French lesson"
    );
    store.ok(json!({"op":"ai_chat_update","chatId":"older","archived":false,"pinned":false}));
    assert_eq!(
        store.ok(json!({"op":"ai_chats"}))["chats"][0]["id"],
        "newer"
    );
    store.ok(json!({"op":"ai_chat_delete","chatId":"older"}));
    assert_eq!(
        store.ok(json!({"op":"ai_chats","query":"French"}))["chats"],
        json!([])
    );
}

#[test]
fn loading_a_videos_transcript_does_not_overwrite_a_custom_name() {
    let store = Store::new();
    let video = "abcdefghijk";
    let chat =
        store.ok(json!({"op":"ai_chat_video_open","videoId":video,"title":"Original video title"}));
    let id = &chat["id"];
    store.ok(json!({"op":"ai_chat_update","chatId":id,"title":"My pronunciation practice"}));
    let transcript = json!({"type":"transcript","requestId":"chat-search:fixture","videoId":video,"ok":true,"transcript":{
        "videoId":video,"title":"Changed video title","trackId":".en","language":"English",
        "tracks":[{"id":".en","label":"English","language":"en","automatic":false}],
        "segments":[{"start":0,"duration":1,"text":"Hello"}]
    }});
    let result: Value = serde_json::from_str(&execute(json!({"op":"accept","videoId":video,"requestId":"chat-search:fixture","origin":"https://www.youtube.com","raw":transcript.to_string()}).to_string(), store.0.to_string_lossy().into_owned())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let reopened = store.ok(json!({"op":"ai_chat_video_open","videoId":video,"transcript":{"videoId":video,"trackId":".en"}}));
    assert_eq!(reopened["title"], "My pronunciation practice");
    assert_eq!(reopened["transcript"]["title"], "Changed video title");
}
