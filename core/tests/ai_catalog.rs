//! Native-independent boundary/routing regressions, added after Pixel streaming checks.
use lingoplay_core::{
    ai_execute, install_ai_platform, AiHttpRequest, AiPlatform, AiResponse, AiSecret,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    paused: AtomicBool,
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
        let mut s = self.0.secrets.lock().unwrap();
        if value.is_empty() {
            s.remove(&key);
        } else {
            s.insert(key, value);
        }
        true
    }
    fn request(&self, r: AiHttpRequest, sink: Arc<AiResponse>) {
        let body: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
        self.0.requests.lock().unwrap().push(json!({"url":r.url,"headers":r.headers.iter().map(|h|(&h.name,&h.value)).collect::<HashMap<_,_>>(),"body":body}));
        sink.start(200);
        if r.url.ends_with("/models") {
            sink.push(json!({"data":[{"id":"one","name":"One","context_length":32768,"supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"future-invalid-tier"}]},{"id":"two","name":"Two"},{"id":"bad\nmodel"},{"id":"unknown-limits","name":"模".repeat(200),"context_length":0,"max_output_tokens":u64::MAX}]}).to_string().into_bytes());
            return;
        }
        let model = body["model"].as_str().unwrap_or("");
        if model == "paused" {
            sink.push(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Partial answer\"}}]}\n\n".to_vec(),
            );
            self.0.paused.store(true, Ordering::SeqCst);
            for _ in 0..1000 {
                if sink.is_cancelled() {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            return;
        }
        if matches!(model, "limited" | "denied" | "server-error") {
            sink.start(match model {
                "limited" => 429,
                "denied" => 401,
                _ => 503,
            });
            sink.push(br#"{"error":{"message":"SECRET_UPSTREAM_BODY"}}"#.to_vec());
            return;
        }
        let mut events = if r.url.ends_with("/responses") {
            vec![
                json!({"type":"response.reasoning_summary_text.delta","delta":"Reasoning summary"}),
                json!({"type":"response.output_text.delta","delta":"Hello 世界"}),
                json!({"type":"response.completed","response":{"usage":{"input_tokens":17,"output_tokens":5,"output_tokens_details":{"reasoning_tokens":3}}}}),
            ]
        } else if r.url.ends_with("/messages") {
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":17}}}),
                json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Reasoning summary"}}),
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello 世界"}}),
                json!({"type":"message_delta","usage":{"output_tokens":5}}),
                json!({"type":"message_stop"}),
            ]
        } else if r.url.contains(":streamGenerateContent") {
            vec![
                json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"Reasoning summary"},{"text":"Hello 世界"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":17,"candidatesTokenCount":5}}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"reasoning_content":"Reasoning summary"}}]}),
                json!({"choices":[{"delta":{"content":"Hello 世界"},"finish_reason":"stop"}],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
            ]
        };
        if model == "partial" {
            events = vec![
                json!({"choices":[{"delta":{"content":"Partial answer"}}]}),
                json!({"error":{"message":"SECRET_UPSTREAM_BODY"}}),
            ];
        }
        let mut wire = events
            .iter()
            .map(|v| format!("data: {v}\r\n\r\n"))
            .collect::<String>();
        if r.url.ends_with("/chat/completions") {
            wire.push_str("data: [DONE]\r\n\r\n");
        }
        for bytes in wire.as_bytes().chunks(3) {
            if !sink.push(bytes.to_vec()) {
                break;
            }
        }
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn call(root: &Path, c: Value) -> Value {
    serde_json::from_str(&ai_execute(c.to_string(), root.to_str().unwrap().into())).unwrap()
}
fn ok(root: &Path, c: Value) -> Value {
    let r = call(root, c);
    assert_eq!(r["ok"], true, "{r}");
    r["value"].clone()
}
fn wait(root: &Path, id: &str) -> Value {
    for _ in 0..1000 {
        let s = ok(root, json!({"op":"ai_poll","taskId":id}));
        if s["status"] != "running" {
            ok(root, json!({"op":"ai_forget","taskId":id}));
            return s;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("AI job stuck");
}
fn run(root: &Path, id: &str, kind: &str) -> Value {
    let s = ok(
        root,
        json!({"op":"ai_start","id":id,"kind":kind,"prompt":"hello"}),
    );
    wait(root, s["taskId"].as_str().unwrap())
}
fn add(root: &Path, kind: &str, protocol: &str, model: &str) -> String {
    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":kind,"label":kind,"baseUrl":if kind=="custom"{"http://127.0.0.1:12345/v1"}else{""},"protocol":protocol,"model":model},"key":"SYNTHETIC_API_KEY"}),
    );
    cfg["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn model(root: &Path, id: &str, name: &str) -> Value {
    ok(
        root,
        json!({"op":"ai_save_model","id":id,"model":{"id":name,"name":name}}),
    )
}
fn select(root: &Path, id: &str, name: &str) {
    ok(root, json!({"op":"ai_select","id":id,"model":name}));
}
fn group(root: &Path, id: &str, members: &[&str], strategy: &str, any: bool) -> String {
    let cfg = ok(
        root,
        json!({"op":"ai_save_group","group":{"id":"","name":"Test group","members":members.iter().map(|m|json!({"providerId":id,"model":m})).collect::<Vec<_>>(),"strategy":strategy,"fallbackOnAnyError":any,"reasoning":"","contextLimit":null}}),
    );
    cfg["groups"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn saved_models_groups_configuration_and_wire_controls_work_together() {
    let temp = Temp(std::env::temp_dir().join(format!(
            "lingoplay-catalog-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let root = &temp.0;
    fs::create_dir_all(root.join("ai")).unwrap();
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    fs::write(root.join("ai/providers.json"),json!({"providers":[{"id":"legacy","label":"Old account","kind":"custom","baseUrl":"https://old.example.test/v1","model":"old-model"}],"activeId":"legacy"}).to_string()).unwrap();
    let cfg = ok(root, json!({"op":"ai_config"}));
    assert_eq!(cfg["providers"][0]["enabled"], true);
    assert_eq!(cfg["providers"][0]["models"][0]["id"], "old-model");
    assert_eq!(cfg["choices"].as_array().unwrap().len(), 10);
    let id = add(root, "custom", "chat", "one");
    let discovered = run(root, &id, "models");
    assert_eq!(discovered["status"], "completed");
    assert_eq!(discovered["result"]["models"].as_array().unwrap().len(), 3);
    assert_eq!(
        discovered["result"]["models"][2]["name"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        200
    );
    let cfg = ok(root, json!({"op":"ai_config"}));
    let p = cfg["providers"].as_array().unwrap().last().unwrap();
    assert_eq!(p["models"][0]["contextWindow"], 32768);
    assert_eq!(p["models"][0]["reasoningEfforts"], json!(["low", "medium"]));
    ok(
        root,
        json!({"op":"ai_save_model","id":id,"model":{"id":"one","name":"Custom name","contextWindow":32768,"maxOutputTokens":2048,"reasoning":"high","reasoningEfforts":["low","medium"],"temperature":0.25,"topP":0.9},"options":{"headers":{"X-Test-Key":"HEADER_SECRET"},"body":{"seed":123,"private_extra":"BODY_SECRET"}}}),
    );
    let done = run(root, &id, "chat");
    assert_eq!(done["status"], "completed");
    assert_eq!(done["text"], "Hello 世界");
    assert_eq!(done["reasoning"], "Reasoning summary");
    assert_eq!(done["usage"]["inputTokens"], 17);
    let req = backend.requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(req["body"]["reasoning_effort"], "medium");
    assert_eq!(req["body"]["max_completion_tokens"], 2048);
    assert_eq!(req["headers"]["X-Test-Key"], "HEADER_SECRET");
    assert_eq!(req["body"]["seed"], 123);
    let exported = ok(root, json!({"op":"ai_export"}));
    let serialized = exported.to_string();
    for marker in ["SYNTHETIC_API_KEY", "HEADER_SECRET", "BODY_SECRET"] {
        assert!(!serialized.contains(marker));
        assert!(!fs::read_to_string(root.join("ai/providers.json"))
            .unwrap()
            .contains(marker));
    }
    for options in [
        json!({"headers":{"X-Test":"bad\r\nInjected: yes"}}),
        json!({"headers":{"Host":"evil.test"}}),
        json!({"body":{"messages":[]}}),
        json!({"body":{"store":true}}),
    ] {
        assert_eq!(
            call(
                root,
                json!({"op":"ai_save_model","id":id,"model":{"id":"one","name":"bad"},"options":options})
            )["ok"],
            false
        );
    }
    assert_eq!(run(root, &id, "models")["status"], "completed");
    let cfg = ok(root, json!({"op":"ai_config"}));
    let p = cfg["providers"].as_array().unwrap().last().unwrap();
    assert_eq!(p["models"][0]["name"], "Custom name");
    assert_eq!(p["models"][0]["maxOutputTokens"], 2048);
    for name in ["limited", "denied", "server-error", "partial"] {
        model(root, &id, name);
    }
    let fallback = group(root, &id, &["limited", "two"], "fallback", false);
    let result = run(root, &fallback, "chat");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["model"], "two");
    let server = group(root, &id, &["server-error", "two"], "fallback", false);
    assert_eq!(run(root, &server, "chat")["model"], "two");
    let denied = group(root, &id, &["denied", "two"], "fallback", false);
    let before = backend.requests.lock().unwrap().len();
    let result = run(root, &denied, "chat");
    assert_eq!(result["status"], "failed");
    assert!(!result.to_string().contains("SECRET_UPSTREAM_BODY"));
    assert_eq!(backend.requests.lock().unwrap().len(), before + 1);
    let any = group(root, &id, &["denied", "two"], "fallback", true);
    assert_eq!(run(root, &any, "chat")["model"], "two");
    let partial = group(root, &id, &["partial", "two"], "fallback", true);
    let before = backend.requests.lock().unwrap().len();
    let result = run(root, &partial, "chat");
    assert_eq!(result["status"], "failed");
    assert_eq!(result["text"], "Partial answer");
    assert_eq!(backend.requests.lock().unwrap().len(), before + 1);
    model(root, &id, "paused");
    let cancellable = group(root, &id, &["paused", "two"], "fallback", true);
    let task = ok(
        root,
        json!({"op":"ai_start","kind":"chat","id":cancellable,"prompt":"hello"}),
    );
    for _ in 0..500 {
        if backend.paused.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(backend.paused.load(Ordering::SeqCst));
    let mut paused_provider = ok(root, json!({"op":"ai_export"}))["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
        .clone();
    paused_provider["enabled"] = json!(false);
    ok(root, json!({"op":"ai_save","provider":paused_provider}));
    assert_eq!(
        wait(root, task["taskId"].as_str().unwrap())["status"],
        "cancelled"
    );
    paused_provider["enabled"] = json!(true);
    ok(root, json!({"op":"ai_save","provider":paused_provider}));
    let balance = group(root, &id, &["one", "two"], "loadBalance", false);
    assert_eq!(run(root, &balance, "chat")["model"], "one");
    assert_eq!(run(root, &balance, "chat")["model"], "two");
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":fallback,"prompt":"hello","transcript":null}),
    );
    let chat_id = started["conversation"]["id"].as_str().unwrap();
    let result = wait(root, started["taskId"].as_str().unwrap());
    assert_eq!(result["status"], "completed");
    let chat = ok(root, json!({"op":"ai_chat_get","chatId":chat_id}));
    assert_eq!(chat["messages"][1]["model"], "two");
    assert_eq!(chat["messages"][1]["usage"]["outputTokens"], 5);
    assert_eq!(chat["messages"][1]["reasoning"], "Reasoning summary");
    // Chat effort overrides group/model defaults and is stored independently.
    let options = ok(
        root,
        json!({"op":"ai_reasoning_options","id":id,"model":"one"}),
    );
    assert_eq!(options["levels"], json!(["low", "medium"]));
    assert_eq!(options["defaultEffort"], "high");
    let reasoned = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"low effort","reasoningEffort":"low"}),
    );
    let reasoned_id = reasoned["conversation"]["id"].as_str().unwrap();
    assert_eq!(
        wait(root, reasoned["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap()["body"]["reasoning_effort"],
        "low"
    );
    let saved = ok(root, json!({"op":"ai_chat_get","chatId":reasoned_id}));
    assert_eq!(saved["reasoningEffort"], "low");
    assert_eq!(saved["messages"][1]["reasoningEffort"], "low");
    let changed = ok(
        root,
        json!({"op":"ai_chat_reasoning","chatId":reasoned_id,"reasoningEffort":"medium"}),
    );
    assert_eq!(changed["reasoningEffort"], "medium");
    assert_eq!(changed["messages"][1]["reasoningEffort"], "low");
    let continued = ok(
        root,
        json!({"op":"ai_chat_send","chatId":reasoned_id,"prompt":"remember effort"}),
    );
    assert_eq!(
        wait(root, continued["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap()["body"]["reasoning_effort"],
        "medium"
    );
    assert_eq!(
        ok(
            root,
            json!({"op":"ai_reasoning_options","id":id,"model":"one"})
        )["defaultEffort"],
        "high"
    );
    assert_eq!(
        call(
            root,
            json!({"op":"ai_chat_reasoning","chatId":reasoned_id,"reasoningEffort":"invalid"})
        )["ok"],
        false
    );
    assert_eq!(
        ok(root, json!({"op":"ai_chat_get","chatId":reasoned_id}))["reasoningEffort"],
        "medium"
    );
    ok(
        root,
        json!({"op":"ai_chat_reasoning","chatId":reasoned_id,"reasoningEffort":null}),
    );
    let restored = ok(
        root,
        json!({"op":"ai_chat_send","chatId":reasoned_id,"prompt":"model default"}),
    );
    assert_eq!(
        wait(root, restored["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap()["body"]["reasoning_effort"],
        "medium"
    );
    assert!(
        ok(root, json!({"op":"ai_chat_get","chatId":reasoned_id}))["reasoningEffort"].is_null()
    );
    let grouped = ok(
        root,
        json!({"op":"ai_chat_send","id":fallback,"prompt":"group effort","reasoningEffort":"low"}),
    );
    assert_eq!(
        wait(root, grouped["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap()["body"]["reasoning_effort"],
        "low"
    );
    let grouped_chat = ok(
        root,
        json!({"op":"ai_chat_get","chatId":grouped["conversation"]["id"]}),
    );
    assert_eq!(grouped_chat["messages"][1]["reasoningEffort"], "low");
    assert_eq!(grouped_chat["messages"][1]["model"], "two");
    // Old chat files still load without either newly persisted effort field.
    let saved_path = root.join("ai/chats").join(format!("{reasoned_id}.json"));
    let mut legacy: Value = serde_json::from_slice(&fs::read(&saved_path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("reasoningEffort");
    for message in legacy["messages"].as_array_mut().unwrap() {
        message.as_object_mut().unwrap().remove("reasoningEffort");
    }
    fs::write(&saved_path, legacy.to_string()).unwrap();
    let migrated = ok(root, json!({"op":"ai_chat_get","chatId":reasoned_id}));
    assert!(migrated["reasoningEffort"].is_null());
    assert_eq!(migrated["messages"][1]["reasoningEffort"], "");
    let direct = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"hello","transcript":null}),
    );
    let direct_chat = direct["conversation"]["id"].as_str().unwrap();
    assert_eq!(
        wait(root, direct["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    // Hiding affects new selections while existing chats and explicit groups remain usable.
    ok(
        root,
        json!({"op":"ai_save_model","id":id,"model":{"id":"one","name":"Hidden model","hidden":true}}),
    );
    assert_eq!(
        call(root, json!({"op":"ai_select","id":id,"model":"one"}))["ok"],
        false
    );
    let continued = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"chatId":direct_chat,"prompt":"follow up","transcript":null}),
    );
    assert_eq!(
        wait(root, continued["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    let data = ok(root, json!({"op":"ai_export"}));
    let original_groups = data["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].clone())
        .collect::<Vec<_>>();
    let original_ids = data["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect::<Vec<_>>();
    let imported = ok(root, json!({"op":"ai_import","data":data}));
    let fresh = imported["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| !original_ids.contains(&p["id"]))
        .collect::<Vec<_>>();
    assert_eq!(fresh.len(), 2);
    for p in &fresh {
        assert_ne!(p["id"], id);
        assert!(!p.to_string().contains("HEADER_SECRET"));
    }
    for g in imported["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| !original_groups.contains(&g["id"]))
    {
        for m in g["members"].as_array().unwrap() {
            assert!(fresh.iter().any(|p| p["id"] == m["providerId"]));
        }
    }
    // Disabling and ordering are persisted independently of account credentials.
    let mut p = ok(root, json!({"op":"ai_export"}))["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
        .clone();
    p["enabled"] = json!(false);
    ok(root, json!({"op":"ai_save","provider":p}));
    assert_eq!(run(root, &id, "chat")["status"], "failed");
    let cfg = ok(root, json!({"op":"ai_config"}));
    let ids = cfg["providers"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|p| p["id"].clone())
        .collect::<Vec<_>>();
    let reordered = ok(root, json!({"op":"ai_reorder","ids":ids}));
    assert_eq!(reordered["providers"][0]["id"], ids[0]);
    assert_eq!(
        call(root, json!({"op":"ai_reorder","ids":["missing"]}))["ok"],
        false
    );
    // Public model lists may omit efforts; the documented Sol fallback excludes
    // unsupported Off/Minimal and clamps old higher choices on the wire.
    let known = add(root, "custom", "responses", "gpt-6.1-sol");
    let options = ok(root, json!({"op":"ai_reasoning_options","id":known}));
    assert_eq!(
        options["levels"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    let capped = ok(
        root,
        json!({"op":"ai_chat_send","id":known,"prompt":"capped effort","reasoningEffort":"ultra"}),
    );
    assert_eq!(
        wait(root, capped["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap()["body"]["reasoning"]["effort"],
        "max"
    );
    assert_eq!(
        ok(
            root,
            json!({"op":"ai_chat_get","chatId":capped["conversation"]["id"]})
        )["messages"][1]["reasoningEffort"],
        "max"
    );
    for protocol in ["responses", "anthropic", "gemini"] {
        let p = add(root, "custom", protocol, "one");
        ok(
            root,
            json!({"op":"ai_save_model","id":p,"model":{"id":"one","name":"One","maxOutputTokens":2048,"reasoning":"low"}}),
        );
        let result = run(root, &p, "chat");
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(result["text"], "Hello 世界");
        assert_eq!(result["usage"]["outputTokens"], 5);
        assert!(!result["reasoning"].as_str().unwrap().is_empty());
        let wire = backend.requests.lock().unwrap().last().unwrap().clone();
        match protocol {
            "responses" => {
                assert_eq!(wire["body"]["reasoning"]["effort"], "low");
                assert_eq!(wire["body"]["max_output_tokens"], 2048);
            }
            "anthropic" => {
                assert_eq!(wire["body"]["thinking"]["type"], "enabled");
                assert_eq!(wire["body"]["max_tokens"], 2048);
            }
            _ => {
                assert_eq!(
                    wire["body"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
                    1024
                );
            }
        }
        let chat = ok(
            root,
            json!({"op":"ai_chat_send","id":p,"prompt":"chat-specific effort","reasoningEffort":"medium"}),
        );
        assert_eq!(
            wait(root, chat["taskId"].as_str().unwrap())["status"],
            "completed"
        );
        let wire = backend.requests.lock().unwrap().last().unwrap().clone();
        match protocol {
            "responses" => assert_eq!(wire["body"]["reasoning"]["effort"], "medium"),
            "anthropic" => assert_eq!(wire["body"]["thinking"]["budget_tokens"], 2047),
            _ => assert_eq!(
                wire["body"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
                8192
            ),
        }
        let saved = ok(
            root,
            json!({"op":"ai_chat_get","chatId":chat["conversation"]["id"]}),
        );
        assert_eq!(saved["messages"][1]["reasoningEffort"], "medium");
    }
    // Removing a connection removes empty groups and never resurrects its credentials.
    ok(root, json!({"op":"ai_select","id":fallback}));
    assert_eq!(run(root, &id, "remove")["status"], "completed");
    let cfg = ok(root, json!({"op":"ai_config"}));
    assert!(!cfg["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["id"] == fallback));
    assert!(!backend
        .secrets
        .lock()
        .unwrap()
        .contains_key(&format!("ai.{id}")));
    select(root, "legacy", "old-model");
}
