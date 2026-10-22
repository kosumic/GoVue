#![cfg(test)]

//! Connection regressions after Android discovery/inference checks on a local endpoint.
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

#[derive(Default)]
struct Backend {
    root: PathBuf,
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    pause: AtomicBool,
    entered: AtomicBool,
    fail_config_on_secret: AtomicBool,
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
        if self.0.fail_config_on_secret.swap(false, Ordering::SeqCst) {
            fs::rename(
                self.0.root.join("ai/providers.json"),
                self.0.root.join("ai/providers.saved"),
            )
            .unwrap();
            fs::create_dir(self.0.root.join("ai/providers.json")).unwrap();
        }
        true
    }
    fn request(&self, req: AiHttpRequest, sink: Arc<AiResponse>) {
        let headers: Vec<_> = req
            .headers
            .iter()
            .map(|h| json!([h.name, h.value]))
            .collect();
        self.0.requests.lock().unwrap().push(json!({"url":req.url,"headers":headers,"body":serde_json::from_str::<Value>(&req.body).unwrap_or(Value::Null)}));
        sink.start(200);
        let url = url::Url::parse(&req.url).unwrap();
        if url.path().ends_with("/models") {
            let second = url.query_pairs().any(|(k, _)| k == "after_id");
            sink.push(json!({"data":[{"id":if second {"two"} else {"one"}}],"has_more":!second,"last_id":"one"}).to_string().into_bytes());
            return;
        }
        if self.0.pause.load(Ordering::SeqCst) {
            self.0.entered.store(true, Ordering::SeqCst);
            sink.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Partial\"}}]}\n\n".to_vec());
            for _ in 0..2000 {
                if sink.is_cancelled() {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            return;
        }
        let events = if url.path().ends_with("/responses") {
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Works\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n"
        } else if url.path().ends_with("/messages") {
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Works\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n"
        } else if url.path().contains(":streamGenerateContent") {
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Works\"}]},\"finishReason\":\"STOP\"}]}\n\n"
        } else {
            "data: {\"choices\":[{\"delta\":{\"content\":\"Works\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        };
        sink.push(events.as_bytes().to_vec());
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn call(root: &Path, c: Value) -> Value {
    serde_json::from_str(&govue_core::ai_execute(
        c.to_string(),
        root.to_str().unwrap().into(),
    ))
    .unwrap()
}
fn ok(root: &Path, c: Value) -> Value {
    let r = call(root, c);
    assert_eq!(r["ok"], true, "{r}");
    r["value"].clone()
}
fn exported(root: &Path, id: &str) -> Value {
    ok(root, json!({"op":"ai_export"}))["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
        .clone()
}
fn secret(b: &Backend, id: &str) -> Value {
    serde_json::from_str(b.secrets.lock().unwrap().get(&format!("ai.{id}")).unwrap()).unwrap()
}
fn header(req: &Value, name: &str) -> Vec<String> {
    req["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h[0].as_str().unwrap().eq_ignore_ascii_case(name))
        .map(|h| h[1].as_str().unwrap().into())
        .collect()
}
fn wait(root: &Path, task: &Value) -> Value {
    for _ in 0..1500 {
        let s = ok(root, json!({"op":"ai_poll","taskId":task["taskId"]}));
        if s["status"] != "running" {
            ok(root, json!({"op":"ai_forget","taskId":task["taskId"]}));
            return s;
        }
        thread::sleep(Duration::from_millis(3));
    }
    panic!("request did not finish")
}
fn run(root: &Path, id: &str, kind: &str) -> Value {
    let task = ok(
        root,
        json!({"op":"ai_start","id":id,"kind":kind,"prompt":"hello","includeTranscript":false}),
    );
    let s = wait(root, &task);
    assert_eq!(s["status"], "completed", "{s}");
    s
}
fn last(b: &Backend) -> Value {
    b.requests.lock().unwrap().last().unwrap().clone()
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the end-to-end fixture setup, actions, and assertions for this scenario together."
)]
fn connection_edits_preserve_identity_and_apply_to_all_request_paths() {
    let temp = Temp(std::env::temp_dir().join(format!(
            "govue-connections-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let root = &temp.0;
    fs::create_dir_all(root.join("ai")).unwrap();
    let b = Arc::new(Backend {
        root: root.clone(),
        ..Backend::default()
    });
    govue_core::install_ai_platform(Box::new(Platform(b.clone())));
    // Missing fields preserve an old URL exactly rather than appending /v1.
    fs::write(root.join("ai/providers.json"),json!({"providers":[{"id":"legacy","label":"Legacy","kind":"custom","baseUrl":"https://legacy.example.test/gateway","model":"one"}],"activeId":"legacy"}).to_string()).unwrap();
    let cfg = ok(root, json!({"op":"ai_config"}));
    assert_eq!(cfg["providers"][0]["appendV1Suffix"], false);
    assert_eq!(cfg["providers"][0]["customUserAgent"], "");
    run(root, "legacy", "chat");
    assert_eq!(
        last(&b)["url"],
        "https://legacy.example.test/gateway/chat/completions"
    );

    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","label":"Relay","kind":"custom","baseUrl":"https://relay.example.test/alpha/","appendV1Suffix":true,"customUserAgent":" Provider-QA/1.0 ","model":"one"},"key":"SYNTHETIC_CONNECTION_KEY"}),
    );
    let id = cfg["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut p = exported(root, &id);
    assert_eq!(p["baseUrl"], "https://relay.example.test/alpha");
    assert_eq!(p["customUserAgent"], "Provider-QA/1.0");
    let preview = ok(root, json!({"op":"ai_connection_preview","provider":p}));
    assert_eq!(
        preview["effectiveBaseUrl"],
        "https://relay.example.test/alpha/v1"
    );
    let before = b.requests.lock().unwrap().len();
    run(root, &id, "models");
    let calls = b.requests.lock().unwrap()[before..].to_vec();
    assert_eq!(calls.len(), 2);
    for r in &calls {
        assert!(
            r["url"]
                .as_str()
                .unwrap()
                .starts_with("https://relay.example.test/alpha/v1/models")
        );
        assert_eq!(header(r, "user-agent"), vec!["Provider-QA/1.0"]);
    }
    run(root, &id, "chat");
    assert_eq!(
        last(&b)["url"],
        "https://relay.example.test/alpha/v1/chat/completions"
    );
    assert_eq!(
        header(&last(&b), "authorization"),
        vec!["Bearer SYNTHETIC_CONNECTION_KEY"]
    );
    let m = exported(root, &id)["models"][0].clone();
    ok(
        root,
        json!({"op":"ai_save_model","id":id,"model":m,"options":{"headers":{"user-agent":"Model-QA/2.0"}}}),
    );
    run(root, &id, "chat");
    assert_eq!(header(&last(&b), "User-Agent"), vec!["Model-QA/2.0"]);
    run(root, &id, "models");
    assert_eq!(header(&last(&b), "User-Agent"), vec!["Provider-QA/1.0"]);
    let task = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"saved chat"}),
    );
    assert_eq!(wait(root, &task)["status"], "completed");
    let chat_id = task["conversation"]["id"].clone();
    let cfg = ok(
        root,
        json!({"op":"ai_save_group","group":{"id":"","name":"Relay group","strategy":"fallback","members":[{"providerId":id,"model":"one"}]}}),
    );
    let group = cfg["groups"][0]["id"].as_str().unwrap().to_owned();
    p = exported(root, &id);
    p["baseUrl"] = json!("https://relay.example.test/beta/v1/");
    ok(root, json!({"op":"ai_save","provider":p}));
    assert_eq!(secret(&b, &id)["key"], "SYNTHETIC_CONNECTION_KEY");
    assert_eq!(
        secret(&b, &id)["modelOptions"]["one"]["headers"]["user-agent"],
        "Model-QA/2.0"
    );
    assert_eq!(exported(root, &id)["models"].as_array().unwrap().len(), 2);
    assert_eq!(
        ok(root, json!({"op":"ai_export"}))["groups"][0]["members"][0]["providerId"],
        id
    );
    let continued = ok(
        root,
        json!({"op":"ai_chat_send","chatId":chat_id,"prompt":"after editing"}),
    );
    assert_eq!(wait(root, &continued)["status"], "completed");
    assert_eq!(
        last(&b)["url"],
        "https://relay.example.test/beta/v1/chat/completions"
    );
    assert_eq!(
        ok(root, json!({"op":"ai_chat_get","chatId":chat_id}))["providerId"],
        id
    );
    p = exported(root, &id);
    p["baseUrl"] = json!("https://relay.example.test/beta");
    p["appendV1Suffix"] = json!(false);
    ok(root, json!({"op":"ai_save","provider":p}));
    run(root, &group, "chat");
    assert_eq!(
        last(&b)["url"],
        "https://relay.example.test/beta/chat/completions"
    );

    // Editing routing cancels a request that is using a group member.
    b.pause.store(true, Ordering::SeqCst);
    let paused = ok(
        root,
        json!({"op":"ai_start","kind":"chat","id":group,"prompt":"hello"}),
    );
    for _ in 0..1000 {
        if b.entered.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(b.entered.load(Ordering::SeqCst));
    p = exported(root, &id);
    p["baseUrl"] = json!("https://relay.example.test/next");
    ok(root, json!({"op":"ai_save","provider":p}));
    assert_eq!(wait(root, &paused)["status"], "cancelled");
    b.pause.store(false, Ordering::SeqCst);

    // Both URL modes reach all inference adapters; Azure retains its query.
    for (protocol, suffix) in [
        ("responses", "/v1/responses"),
        ("anthropic", "/v1/messages"),
        ("gemini", "/models/one:streamGenerateContent?alt=sse"),
    ] {
        p = exported(root, &id);
        p["protocol"] = json!(protocol);
        p["appendV1Suffix"] = json!(protocol != "gemini");
        ok(root, json!({"op":"ai_save","provider":p}));
        run(root, &id, "chat");
        assert_eq!(
            last(&b)["url"],
            format!("https://relay.example.test/next{suffix}")
        );
        assert_eq!(header(&last(&b), "user-agent"), vec!["Model-QA/2.0"]);
    }
    p = exported(root, &id);
    p["protocol"] = json!("responses");
    p["baseUrl"] = json!("https://azure.example.test/?api-version=2025-04-01-preview");
    p["azure"] = json!(true);
    ok(root, json!({"op":"ai_save","provider":p}));
    run(root, &id, "chat");
    assert_eq!(
        last(&b)["url"],
        "https://azure.example.test/openai/deployments/one/responses?api-version=2025-04-01-preview"
    );
    assert_eq!(
        header(&last(&b), "api-key"),
        vec!["SYNTHETIC_CONNECTION_KEY"]
    );

    // Invalid settings do not replace the stored configuration or credentials.
    let original = exported(root, &id);
    for (field, value) in [
        ("customUserAgent", json!("bad\r\nInjected: yes")),
        ("customUserAgent", json!("x".repeat(513))),
        ("appendV1Suffix", json!(true)),
        ("auth", json!("oauth")),
        ("baseUrl", json!("http://remote.example.test")),
    ] {
        let mut bad = original.clone();
        bad[field] = value;
        assert_eq!(
            call(root, json!({"op":"ai_save","provider":bad}))["ok"],
            false
        );
        assert_eq!(exported(root, &id), original);
        assert_eq!(secret(&b, &id)["key"], "SYNTHETIC_CONNECTION_KEY");
    }
    let portable = ok(root, json!({"op":"ai_export"}));
    assert!(!portable.to_string().contains("SYNTHETIC_CONNECTION_KEY"));
    let imported = ok(root, json!({"op":"ai_import","data":portable}));
    let copy = imported["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["label"] == "Relay" && p["id"] != id)
        .unwrap();
    assert_eq!(copy["customUserAgent"], "Provider-QA/1.0");
    assert_eq!(copy["appendV1Suffix"], false);

    // Switching authentication keeps identity/model options but removes old credentials.
    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"anthropic","label":"Auth QA","baseUrl":"https://relay.example.test","model":"one"},"key":"OLD_SYNTHETIC_KEY"}),
    );
    let account = cfg["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        root,
        json!({"op":"ai_save_model","id":account,"model":{"id":"one"},"options":{"body":{"test_option":true}}}),
    );
    let before_secret = secret(&b, &account);
    let mut account_p = exported(root, &account);
    account_p["auth"] = json!("oauth");
    account_p["baseUrl"] = json!("");
    // Simulate config persistence failing after the credential store has changed.
    b.fail_config_on_secret.store(true, Ordering::SeqCst);
    assert_eq!(
        call(root, json!({"op":"ai_save","provider":account_p}))["ok"],
        false
    );
    assert_eq!(secret(&b, &account), before_secret);
    fs::remove_dir(root.join("ai/providers.json")).unwrap();
    fs::rename(
        root.join("ai/providers.saved"),
        root.join("ai/providers.json"),
    )
    .unwrap();
    let cfg = ok(root, json!({"op":"ai_save","provider":account_p}));
    let saved = cfg["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == account)
        .unwrap();
    assert_eq!(saved["connected"], false);
    assert!(secret(&b, &account)["key"].is_null());
    assert_eq!(
        secret(&b, &account)["modelOptions"]["one"]["body"]["test_option"],
        true
    );
    let mut bad = exported(root, &account);
    bad["baseUrl"] = json!("https://unrelated.example.test");
    assert_eq!(
        call(root, json!({"op":"ai_save","provider":bad}))["ok"],
        false
    );
    account_p = exported(root, &account);
    account_p["auth"] = json!("key");
    account_p["baseUrl"] = json!("https://relay.example.test/new");
    ok(
        root,
        json!({"op":"ai_save","provider":account_p,"key":"NEW_SYNTHETIC_KEY"}),
    );
    assert_eq!(secret(&b, &account)["key"], "NEW_SYNTHETIC_KEY");
    assert!(secret(&b, &account)["access_token"].is_null());
    assert_eq!(exported(root, &account)["models"][0]["id"], "one");
}
