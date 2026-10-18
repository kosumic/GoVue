#![cfg(test)]

//! Usage regressions added after the Pixel settings/report navigation check.
use govue_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, install_ai_platform,
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
struct Platform(Mutex<HashMap<String, String>>);
impl AiPlatform for Platform {
    fn secret_read(&self, key: String) -> AiSecret {
        AiSecret {
            ok: true,
            value: self
                .0
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .unwrap_or_default(),
        }
    }
    fn secret_write(&self, key: String, value: String) -> bool {
        self.0.lock().unwrap().insert(key, value);
        true
    }
    fn request(&self, request: AiHttpRequest, response: Arc<AiResponse>) {
        response.start(200);
        let body: Value = serde_json::from_str(&request.body).unwrap();
        let invalid = body["model"] == "invalid-cache";
        let mut events = if request.url.ends_with("/responses") {
            vec![
                json!({"type":"response.output_text.delta","delta":"PRIVATE_"}),
                json!({"type":"response.output_text.delta","delta":"REPLY_BODY"}),
                json!({"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":40},"output_tokens_details":{"reasoning_tokens":5}}}}),
            ]
        } else if request.url.ends_with("/messages") {
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":30,"cache_read_input_tokens":40,"cache_creation_input_tokens":30}}}),
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"PRIVATE_"}}),
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"REPLY_BODY"}}),
                json!({"type":"message_delta","usage":{"output_tokens":20}}),
                json!({"type":"message_stop"}),
            ]
        } else if request.url.contains(":streamGenerateContent") {
            vec![
                json!({"candidates":[{"content":{"parts":[{"text":"PRIVATE_"}]}}]}),
                json!({"candidates":[{"content":{"parts":[{"text":"REPLY_BODY"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":100,"cachedContentTokenCount":40,"candidatesTokenCount":15,"thoughtsTokenCount":5}}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"content":"PRIVATE_"}}]}),
                json!({"choices":[{"delta":{"content":"REPLY_BODY"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":if invalid {200}else{40},"completion_tokens_details":{"reasoning_tokens":5}}}),
            ]
        };
        if body["model"] == "canonical-cache" {
            events.last_mut().unwrap()["usage"]["prompt_tokens_details"] =
                json!({"cached_tokens":60});
        }
        if body["model"] == "zero-native-cache" {
            events.last_mut().unwrap()["usage"]["prompt_tokens_details"] =
                json!({"cached_tokens":0});
        }
        if body["model"] == "no-usage" {
            events.last_mut().unwrap()["usage"] = Value::Null;
        }
        for event in events {
            assert!(response.push(format!("data: {event}\n\n").into_bytes()));
            thread::sleep(Duration::from_millis(8));
        }
        if request.url.ends_with("/chat/completions") {
            response.push(b"data: [DONE]\n\n".to_vec());
        }
    }
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "govue-usage-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request(root: &Path, command: Value) -> Value {
    serde_json::from_str(&ai_execute(
        command.to_string(),
        root.to_string_lossy().into_owned(),
    ))
    .unwrap()
}
fn ok(root: &Path, command: Value) -> Value {
    let result = request(root, command);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn wait(root: &Path, task: &Value) -> Value {
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":task}));
        if state["status"] != "running" {
            assert_eq!(state["status"], "completed", "{state}");
            ok(root, json!({"op":"ai_forget","taskId":task}));
            return state;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("Usage fixture timed out");
}
fn provider(root: &Path, protocol: &str) -> String {
    let config = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":format!("Provider {protocol}"),"baseUrl":"http://127.0.0.1:12345/v1","protocol":protocol,"model":"fixture","models":[{"id":"fixture","name":"字".repeat(200),"contextWindow":32768,"maxOutputTokens":2048,"reasoning":"high"}]},"key":"PRIVATE_API_KEY"}),
    );
    config["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn send(root: &Path, id: &str, chat: Option<&str>) -> Value {
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"chatId":chat,"prompt":"Usage fixture"}),
    );
    wait(root, &started["taskId"]);
    ok(
        root,
        json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
    )
}

#[test]
fn usage_survives_provider_changes_normalizes_protocols_and_preserves_legacy_history() {
    install_ai_platform(Box::<Platform>::default());
    let temp = Temp::new();
    let root = &temp.0;
    let mut saved = Vec::new();
    let mut ids = Vec::new();
    for protocol in ["chat", "responses", "anthropic", "gemini"] {
        let id = provider(root, protocol);
        let chat = send(root, &id, None);
        let reply = &chat["messages"][1];
        assert_eq!(reply["usage"]["inputTokens"], 100, "{protocol}");
        assert_eq!(reply["usage"]["outputTokens"], 20, "{protocol}");
        assert_eq!(reply["usage"]["cacheReadTokens"], 40);
        assert_eq!(
            reply["usage"]["cacheCreationTokens"],
            json!(if protocol == "anthropic" {
                Some(30)
            } else {
                None
            })
        );
        assert_eq!(
            reply["usage"]["uncachedInputTokens"],
            if protocol == "anthropic" { 30 } else { 60 }
        );
        assert_eq!(reply["usage"]["latestContextTokens"], 100);
        assert!(reply["usage"]["streamMs"].as_u64().unwrap() > 0);
        assert_eq!(reply["providerId"], id);
        assert_eq!(reply["modelName"], "字".repeat(200));
        assert!(reply["completedAt"].as_u64().unwrap() > 0);
        let report = ok(root, json!({"op":"ai_usage","chatId":chat["id"]}));
        assert_eq!(report["context"]["used"], 100);
        assert_eq!(report["context"]["window"], 32768);
        assert_eq!(report["context"]["maxOutputTokens"], 2048);
        assert_eq!(
            report["context"]["reasoningEffort"],
            reply["reasoningEffort"]
        );
        saved.push(chat);
        ids.push(id);
    }
    send(root, &ids[0], saved[0]["id"].as_str());
    let report = ok(root, json!({"op":"ai_usage"}));
    assert_eq!(report["stats"]["inputTokens"], 500);
    assert_eq!(report["stats"]["outputTokens"], 100);
    assert_eq!(report["stats"]["cacheReadTokens"], 200);
    assert_eq!(report["stats"]["cacheCreationTokens"], 30);
    assert_eq!(report["stats"]["uncachedInputTokens"], 270);
    assert_eq!(report["stats"]["cacheHitRate"], 40.0);
    assert_eq!(report["stats"]["chatCount"], 4);
    assert_eq!(report["stats"]["replyCount"], 5);
    assert_eq!(report["stats"]["averagePerChat"], 150.0);
    let measured_ms = report["stats"]["streamMs"].as_u64().unwrap();
    assert_eq!(
        report["stats"]["outputTokensPerSecond"],
        100_000.0 / f64::from(u32::try_from(measured_ms).unwrap())
    );

    // Renaming or removing a configured model must not reattribute past usage.
    let mut changed = ok(root, json!({"op":"ai_config"}))["providers"][0].clone();
    for field in [
        "connected",
        "account",
        "planEnabled",
        "needsReauth",
        "credentialError",
        "connection",
    ] {
        changed.as_object_mut().unwrap().remove(field);
    }
    changed["label"] = json!("Renamed connection");
    changed["models"][0]["name"] = json!("Renamed model");
    ok(root, json!({"op":"ai_save","provider":changed}));
    let task = ok(root, json!({"op":"ai_start","id":ids[0],"kind":"remove"}));
    wait(root, &task["taskId"]);
    let report = ok(root, json!({"op":"ai_usage","chatId":saved[0]["id"]}));
    assert_eq!(report["providers"][0]["label"], "Provider chat");
    assert_eq!(
        report["providers"][0]["models"][0]["modelName"],
        "字".repeat(200)
    );
    assert_eq!(report["providers"][0]["models"][0]["removed"], true);
    assert_eq!(report["stats"]["inputTokens"], 200);
    assert_eq!(report["context"]["window"], 32768);

    // Old rows remain readable and are excluded from speed's paired samples.
    let mut legacy = saved[0].clone();
    legacy["id"] = json!("legacy");
    legacy["activeTask"] = Value::Null;
    for message in legacy["messages"].as_array_mut().unwrap() {
        for field in [
            "providerId",
            "modelName",
            "completedAt",
            "contextWindow",
            "maxOutputTokens",
            "model",
            "providerLabel",
        ] {
            message.as_object_mut().unwrap().remove(field);
        }
    }
    legacy["messages"][1]["usage"] = json!({"inputTokens":10,"outputTokens":2});
    let legacy_path = root.join("ai/chats/legacy.json");
    let legacy_bytes = legacy.to_string();
    fs::write(&legacy_path, &legacy_bytes).unwrap();
    let report = ok(root, json!({"op":"ai_usage"}));
    assert_eq!(report["stats"]["inputTokens"], 510);
    assert_eq!(report["stats"]["outputTokens"], 102);
    assert_eq!(
        report["stats"]["outputTokensPerSecond"],
        100_000.0 / f64::from(u32::try_from(measured_ms).unwrap())
    );
    assert_eq!(report["stats"]["activeDays"], Value::Null);
    assert_eq!(report["stats"]["legacyUsage"], true);
    assert_eq!(fs::read_to_string(&legacy_path).unwrap(), legacy_bytes);
    assert!(!report.to_string().contains("PRIVATE_REPLY_BODY"));
    assert!(!report.to_string().contains("PRIVATE_API_KEY"));
    let single = ok(root, json!({"op":"ai_usage","chatId":"legacy"}));
    assert_eq!(single["providers"][0]["models"][0]["estimated"], true);
    assert_eq!(single["stats"]["outputTokensPerSecond"], Value::Null);
    ok(root, json!({"op":"ai_chat_delete","chatId":"legacy"}));
    assert_eq!(
        ok(root, json!({"op":"ai_usage"}))["stats"]["inputTokens"],
        500
    );
    assert_eq!(
        request(root, json!({"op":"ai_usage","chatId":"../providers"}))["ok"],
        false
    );
    assert_eq!(
        request(root, json!({"op":"ai_usage","chatId":42}))["ok"],
        false
    );

    // Malformed provider cache subsets cannot inflate the input or hit rate.
    let id = provider(root, "chat");
    ok(
        root,
        json!({"op":"ai_save_model","id":id,"model":{"id":"invalid-cache","name":"Invalid cache"}}),
    );
    ok(
        root,
        json!({"op":"ai_select","id":id,"model":"invalid-cache"}),
    );
    let invalid = send(root, &id, None);
    assert_eq!(invalid["messages"][1]["usage"]["inputTokens"], 100);
    assert_eq!(
        invalid["messages"][1]["usage"]["cacheReadTokens"],
        Value::Null
    );
    assert_eq!(
        ok(root, json!({"op":"ai_usage","chatId":invalid["id"]}))["stats"]["cacheHitRate"],
        Value::Null
    );
    for (model, expected) in [
        ("canonical-cache", Some(60)),
        ("zero-native-cache", Some(40)),
        ("no-usage", None),
    ] {
        ok(
            root,
            json!({"op":"ai_save_model","id":id,"model":{"id":model,"name":model}}),
        );
        ok(root, json!({"op":"ai_select","id":id,"model":model}));
        let chat = send(root, &id, None);
        let report = ok(root, json!({"op":"ai_usage","chatId":chat["id"]}));
        assert_eq!(report["stats"]["cacheReadTokens"], json!(expected));
        if expected.is_none() {
            assert_eq!(chat["messages"][1]["usage"], Value::Null);
            assert_eq!(report["stats"]["replyCount"], 0);
            assert_eq!(report["stats"]["unreportedReplies"], 1);
            assert_eq!(report["stats"]["inputTokens"], Value::Null);
            assert_eq!(report["stats"]["outputTokensPerSecond"], Value::Null);
        }
    }

    // Bounded arithmetic rejects unsafe totals rather than wrapping or rounding.
    let overflow = Temp::new();
    fs::create_dir_all(overflow.0.join("ai/chats")).unwrap();
    for id in ["overflow-a", "overflow-b"] {
        let mut chat = saved[0].clone();
        chat["id"] = json!(id);
        chat["messages"][1]["usage"] = json!({"inputTokens":9_007_199_254_740_991u64});
        fs::write(
            overflow.0.join(format!("ai/chats/{id}.json")),
            chat.to_string(),
        )
        .unwrap();
    }
    assert_eq!(request(&overflow.0, json!({"op":"ai_usage"}))["ok"], false);
}
