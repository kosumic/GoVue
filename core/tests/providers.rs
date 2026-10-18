#![cfg(test)]

//! Boundary regressions, independent of account entitlement and live device checks.
//! The committed RSA fixture signs synthetic test identities only.
use govue_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, install_ai_platform,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    nonce: Mutex<String>,
    mode: Mutex<String>,
    refreshes: AtomicUsize,
}
struct FakePlatform(Arc<Backend>);
impl AiPlatform for FakePlatform {
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
        let mut secrets = self.0.secrets.lock().unwrap();
        if value.is_empty() {
            secrets.remove(&key);
        } else {
            secrets.insert(key, value);
        }
        true
    }
    fn request(&self, request: AiHttpRequest, sink: Arc<AiResponse>) {
        let mode = self.0.mode.lock().unwrap().clone();
        self.0.requests.lock().unwrap().push(json!({"url":request.url,"method":request.method,"body":request.body,"headers":request.headers.iter().map(|h|json!([h.name,h.value])).collect::<Vec<_>>()}));
        sink.start(200);
        let payload = if request.url.ends_with("openid-configuration") {
            json!({"issuer":"https://auth.openai.com","jwks_uri":"https://auth.openai.com/.well-known/jwks.json","revocation_endpoint":"https://auth.openai.com/api/accounts/oauth/revoke"})
        } else if request.url.ends_with("jwks.json") {
            serde_json::from_str(include_str!("fixtures/ai-test-jwks.json")).unwrap()
        } else if request.url.ends_with("oauth/token") {
            let fields: HashMap<_, _> = url::form_urlencoded::parse(request.body.as_bytes())
                .into_owned()
                .collect();
            if fields["grant_type"] == "refresh_token" {
                self.0.refreshes.fetch_add(1, Ordering::SeqCst);
                // Racing reuse is a real rotating-token failure, not another success.
                if fields["refresh_token"] != "test-refresh-1" {
                    sink.start(401);
                    return;
                }
                thread::sleep(Duration::from_millis(30));
                json!({"access_token":"test-access-2","refresh_token":"test-refresh-2","token_type":"Bearer","expires_in":3600})
            } else {
                let nonce = self.0.nonce.lock().unwrap().clone();
                json!({"access_token":"test-access-1","refresh_token":"test-refresh-1","token_type":"Bearer","expires_in":3600,"id_token":identity(&nonce,&mode),"scope":if mode=="no_plan" {"openid email profile"} else {"openid email profile offline_access resource.invoke chatgpt.tokens.use.direct"}})
            }
        } else if request.url.ends_with("oauth/revoke") {
            return;
        } else if request.url.ends_with("/models") {
            json!({"models":[{"slug":"test-text","display_name":"Test text","visibility":"list"},{"slug":"hidden-model","visibility":"hide"}],"data":[{"id":"test-text"}]})
        } else {
            if mode == "http_error" {
                sink.start(401);
                let _ = sink.push(br#"{"error":{"message":"DO_NOT_ECHO_CREDENTIAL"}}"#.to_vec());
                return;
            }
            let stream = if request.url.ends_with("/responses") {
                let mut s =
                    event(json!({"type":"response.output_text.delta","delta":"Hello 世界"}));
                if mode == "quota" {
                    s += &event(
                        json!({"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"DO_NOT_ECHO_CREDENTIAL"}}}),
                    );
                } else if mode != "incomplete" && mode != "paused" {
                    s += &event(json!({"type":"response.completed"}));
                }
                s
            } else if request.url.ends_with("/messages") {
                let mut s = event(
                    json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello 世界"}}),
                );
                if mode != "incomplete" {
                    s += &event(json!({"type":"message_stop"}));
                }
                s
            } else if request.url.contains(":streamGenerateContent?alt=sse") {
                let mut s =
                    event(json!({"candidates":[{"content":{"parts":[{"text":"Hello 世界"}]}}]}));
                if mode != "incomplete" {
                    s += &event(json!({"candidates":[{"finishReason":"STOP"}]}));
                }
                s
            } else if request.url.ends_with("/chat/completions") {
                let mut s = event(
                    json!({"choices":[{"delta":{"content":"Hello 世界"},"finish_reason":null}]}),
                );
                if mode != "incomplete" {
                    s += &event(json!({"choices":[{"delta":{},"finish_reason":"stop"}]}));
                    s += "data: [DONE]\r\n\r\n";
                }
                s
            } else {
                sink.start(404);
                return;
            };
            // Deliberately split UTF-8, CRLF and JSON tokens at arbitrary byte boundaries.
            for chunk in stream.as_bytes().chunks(3) {
                if !sink.push(chunk.to_vec()) {
                    return;
                }
            }
            if mode == "paused" {
                for _ in 0..200 {
                    if sink.is_cancelled() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            return;
        };
        let _ = sink.push(payload.to_string().into_bytes());
    }
}
fn event(value: Value) -> String {
    format!("event: ignored-name\r\ndata: {value}\r\n\r\n")
}
fn identity(nonce: &str, mode: &str) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = json!({"iss":if mode=="wrong_issuer" {"https://attacker.invalid"} else {"https://auth.openai.com"},"aud":if mode=="wrong_audience" {"another-client"} else {"oaiapp_fixture"},"sub":if mode=="wrong_subject" {"another-person"} else {"fixture-person"},"email":"learner@example.test","nonce":if mode=="wrong_nonce" {"another-nonce"} else {nonce},"exp":if mode=="expired" {now-600} else {now+600}});
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("govue-test".into());
    let mut token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_der(include_bytes!("fixtures/ai-test-private.der")),
    )
    .unwrap();
    if mode == "bad_signature" {
        let i = token.rfind('.').unwrap() + 1;
        let replace = if token.as_bytes()[i] == b'A' {
            "B"
        } else {
            "A"
        };
        token.replace_range(i..=i, replace);
    }
    token
}
fn call(root: &Path, command: Value) -> Value {
    serde_json::from_str(&ai_execute(
        command.to_string(),
        root.to_str().unwrap().into(),
    ))
    .unwrap()
}
fn ok(root: &Path, command: Value) -> Value {
    let r = call(root, command);
    assert_eq!(r["ok"], true, "{r}");
    r["value"].clone()
}
fn wait(root: &Path, task: &str) -> Value {
    for _ in 0..1000 {
        let s = ok(root, json!({"op":"ai_poll","taskId":task}));
        if s["status"] != "running" {
            ok(root, json!({"op":"ai_forget","taskId":task}));
            return s;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("Provider task did not terminate")
}
fn start(root: &Path, id: &str, kind: &str, extra: Value) -> String {
    let mut c = json!({"op":"ai_start","id":id,"kind":kind});
    if let Some(fields) = extra.as_object() {
        for (k, v) in fields {
            c[k] = v.clone();
        }
    }
    ok(root, c)["taskId"].as_str().unwrap().into()
}
fn add(root: &Path, kind: &str) -> String {
    let mut c = json!({"op":"ai_save","provider":{"id":"","kind":kind,"label":kind,"baseUrl":if kind=="custom" {"https://example.test/v1"} else {""},"model":"test-text"}});
    if kind != "chatgpt" {
        c["key"] = json!("TEST-API-KEY-DO-NOT-USE");
    }
    ok(root, c)["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn auth_start(root: &Path, id: &str, backend: &Backend) -> (Value, HashMap<String, String>) {
    let started = ok(root, json!({"op":"ai_login","id":id}));
    let u = Url::parse(started["url"].as_str().unwrap()).unwrap();
    assert_eq!(u.origin().ascii_serialization(), "https://auth.openai.com");
    let fields: HashMap<_, _> = u.query_pairs().into_owned().collect();
    backend.nonce.lock().unwrap().clone_from(&fields["nonce"]);
    assert_eq!(fields["code_challenge_method"], "S256");
    assert!(fields["ext_agent_host_id"].starts_with("urn:uuid:"));
    (started, fields)
}
fn send_callback(
    fields: &HashMap<String, String>,
    state: &str,
    client: Option<&str>,
) -> ureq::Response {
    let mut u = Url::parse(&fields["redirect_uri"]).unwrap();
    u.query_pairs_mut()
        .append_pair("state", state)
        .append_pair("code", "fixture-code");
    if let Some(client) = client {
        u.query_pairs_mut().append_pair("client_id", client);
    }
    match ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .get(u.as_str())
        .call()
    {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("Loopback callback failed: {e}"),
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the end-to-end fixture setup, actions, and assertions for this scenario together."
)]
fn providers_preserve_oauth_boundaries_credentials_and_stream_completion() {
    let root = Temp(std::env::temp_dir().join(format!(
            "govue-ai-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(&root.0).unwrap();
    let root = &root.0;
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(FakePlatform(backend.clone())));

    for bad in [
        "http://example.test/v1",
        "https://user:pass@example.test/v1",
        "https://example.test/v1?key=SECRET",
        "https://example.test/v1#SECRET",
    ] {
        assert_eq!(
            call(
                root,
                json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":"bad","baseUrl":bad,"model":"model"},"key":"TEST-API-KEY-DO-NOT-USE"})
            )["ok"],
            false
        );
    }
    let id = add(root, "chatgpt");
    let (first, fields) = auth_start(root, &id, &backend);
    assert_eq!(fields["client_id"], "dynamic_agent_client");
    assert_eq!(fields["agent_name_hint"], "GoVue");
    assert_eq!(
        send_callback(&fields, "forged-state", Some("oaiapp_fixture")).status(),
        400
    );
    assert_eq!(
        ok(root, json!({"op":"ai_poll","taskId":first["taskId"]}))["status"],
        "running"
    );
    let response = send_callback(&fields, &fields["state"], Some("oaiapp_fixture"));
    assert_eq!(response.status(), 302);
    assert_eq!(response.header("Location"), first["returnUrl"].as_str());
    assert!(
        !response
            .header("Location")
            .unwrap()
            .contains("fixture-code")
    );
    assert_eq!(
        wait(root, first["taskId"].as_str().unwrap())["status"],
        "completed"
    );
    let original = backend.secrets.lock().unwrap()[&format!("ai.{id}")].clone();
    let config = ok(root, json!({"op":"ai_config"}));
    assert_eq!(config["providers"][0]["planEnabled"], true);

    for mode in [
        "bad_signature",
        "wrong_issuer",
        "wrong_audience",
        "wrong_nonce",
        "expired",
        "wrong_subject",
    ] {
        *backend.mode.lock().unwrap() = mode.into();
        let (s, f) = auth_start(root, &id, &backend);
        assert_eq!(f["client_id"], "oaiapp_fixture");
        assert!(!f.contains_key("agent_name_hint"));
        assert_eq!(f["ext_agent_host_id"], fields["ext_agent_host_id"]);
        send_callback(&f, &f["state"], None);
        assert_eq!(
            wait(root, s["taskId"].as_str().unwrap())["status"],
            "failed",
            "{mode}"
        );
        assert_eq!(
            backend.secrets.lock().unwrap()[&format!("ai.{id}")],
            original,
            "Invalid identity replaced a verified account"
        );
    }
    *backend.mode.lock().unwrap() = String::new();
    let (s, f) = auth_start(root, &id, &backend);
    send_callback(&f, &f["state"], Some("another-registration"));
    assert_eq!(
        wait(root, s["taskId"].as_str().unwrap())["status"],
        "failed"
    );
    assert_eq!(
        backend.secrets.lock().unwrap()[&format!("ai.{id}")],
        original
    );

    let task = start(root, &id, "models", Value::Null);
    let result = wait(root, &task);
    assert_eq!(result["result"]["models"].as_array().unwrap().len(), 1);
    assert_eq!(result["result"]["models"][0]["id"], "test-text");

    // Two requests near expiry must rotate the refresh token only once.
    let mut stored: Value = serde_json::from_str(&original).unwrap();
    stored["expires_at"] = json!(0);
    backend
        .secrets
        .lock()
        .unwrap()
        .insert(format!("ai.{id}"), stored.to_string());
    let a = start(root, &id, "models", Value::Null);
    let b = start(root, &id, "chat", json!({"prompt":"hello"}));
    assert_eq!(wait(root, &a)["status"], "completed");
    assert_eq!(wait(root, &b)["status"], "completed");
    assert_eq!(backend.refreshes.load(Ordering::SeqCst), 1);

    // Exercise all four wire protocols, with incomplete streams rejected for each.
    for kind in ["openai", "anthropic", "gemini", "openrouter", "custom"] {
        let provider = add(root, kind);
        for (mode, status) in [("", "completed"), ("incomplete", "failed")] {
            *backend.mode.lock().unwrap() = mode.into();
            let task = start(root, &provider, "chat", json!({"prompt":"hello"}));
            let result = wait(root, &task);
            assert_eq!(result["status"], status, "{kind} {mode}");
            assert_eq!(result["text"], "Hello 世界");
        }
    }
    for mode in ["quota", "http_error"] {
        *backend.mode.lock().unwrap() = mode.into();
        let task = start(root, &id, "chat", json!({"prompt":"hello"}));
        let result = wait(root, &task);
        assert_eq!(result["status"], "failed");
        assert!(!result.to_string().contains("DO_NOT_ECHO_CREDENTIAL"));
    }
    *backend.mode.lock().unwrap() = "paused".into();
    let task = start(root, &id, "chat", json!({"prompt":"hello"}));
    for _ in 0..100 {
        if !ok(root, json!({"op":"ai_poll","taskId":task}))["text"]
            .as_str()
            .unwrap()
            .is_empty()
        {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    ok(root, json!({"op":"ai_cancel","taskId":task}));
    assert_eq!(wait(root, &task)["status"], "cancelled");
    *backend.mode.lock().unwrap() = String::new();

    let raw = json!({"type":"transcript","requestId":"fixture","videoId":"abcdefghijk","ok":true,"transcript":{"videoId":"abcdefghijk","title":"Fixture title","language":"en","trackId":"en","tracks":[{"id":"en","label":"English","language":"en","automatic":false}],"segments":[{"start":0,"duration":3,"text":"CAPTION_PRIVATE_MARKER"}]}});
    let accepted: Value=serde_json::from_str(&govue_core::execute(json!({"op":"accept","videoId":"abcdefghijk","requestId":"fixture","origin":"https://www.youtube.com/watch?v=abcdefghijk","raw":raw.to_string()}).to_string(),root.to_str().unwrap().into())).unwrap();
    assert_eq!(accepted["ok"], true, "{accepted}");
    for attach in [false, true] {
        let task = start(
            root,
            &id,
            "chat",
            json!({"prompt":"Summarize","includeTranscript":attach,"videoId":"abcdefghijk","trackId":"en"}),
        );
        assert_eq!(wait(root, &task)["status"], "completed");
        let requests = backend.requests.lock().unwrap();
        let body = requests.last().unwrap()["body"].as_str().unwrap();
        assert_eq!(body.contains("CAPTION_PRIVATE_MARKER"), attach);
        drop(requests);
    }
    let settings = fs::read_to_string(root.join("ai/providers.json")).unwrap();
    for secret in [
        "TEST-API-KEY-DO-NOT-USE",
        "test-access-1",
        "test-access-2",
        "test-refresh",
        "fixture-code",
        "CAPTION_PRIVATE_MARKER",
    ] {
        assert!(!settings.contains(secret));
    }
    let public = ok(root, json!({"op":"ai_config"})).to_string();
    assert!(!public.contains("test-access") && !public.contains("TEST-API-KEY"));

    let task = start(root, &id, "disconnect", Value::Null);
    assert_eq!(wait(root, &task)["status"], "completed");
    let stored: Value =
        serde_json::from_str(&backend.secrets.lock().unwrap()[&format!("ai.{id}")]).unwrap();
    assert_eq!(stored["client_id"], "oaiapp_fixture");
    assert!(
        stored.get("access_token").is_none()
            && stored.get("refresh_token").is_none()
            && stored.get("id_token").is_none()
    );
    assert_eq!(
        ok(root, json!({"op":"ai_config"}))["providers"][0]["connected"],
        false
    );

    // A cancelled login listener must stop promptly and release its loopback port.
    let (s, f) = auth_start(root, &id, &backend);
    ok(root, json!({"op":"ai_cancel","taskId":s["taskId"]}));
    assert_eq!(
        wait(root, s["taskId"].as_str().unwrap())["status"],
        "cancelled"
    );
    thread::sleep(Duration::from_millis(250));
    assert!(
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(100))
            .build()
            .get(&f["redirect_uri"])
            .call()
            .is_err()
    );

    // Finished requests left by a JS reload must not exhaust the active-request budget.
    let mut abandoned = Vec::new();
    for _ in 0..20 {
        let task = start(root, &id, "models", Value::Null);
        for _ in 0..100 {
            if ok(root, json!({"op":"ai_poll","taskId":task}))["status"] != "running" {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        abandoned.push(task);
    }
    for task in abandoned {
        let _ = call(root, json!({"op":"ai_forget","taskId":task}));
    }

    // A damaged credential must not hide all providers or prevent its own removal.
    let broken = add(root, "openai");
    backend
        .secrets
        .lock()
        .unwrap()
        .insert(format!("ai.{broken}"), "unreadable credential".into());
    let config = ok(root, json!({"op":"ai_config"}));
    let row = config["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == broken)
        .unwrap();
    assert_eq!(row["connected"], false);
    assert!(row["credentialError"].is_string());
    let task = start(root, &broken, "remove", Value::Null);
    assert_eq!(wait(root, &task)["status"], "completed");
    assert!(
        !backend
            .secrets
            .lock()
            .unwrap()
            .contains_key(&format!("ai.{broken}"))
    );
}
