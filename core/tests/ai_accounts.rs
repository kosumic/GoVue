#![cfg(test)]

//! Account protocol tests use synthetic keys/tokens; they do not establish plan access.
use govue_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, install_ai_platform,
};
use serde_json::{Value, json};
use std::fmt::Write as _;
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
struct Platform(Arc<Backend>);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn token(ec: bool, nonce: &str) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    let mut header = Header::new(if ec {
        Algorithm::ES256
    } else {
        Algorithm::RS256
    });
    header.kid = Some(if ec { "govue-ec-test" } else { "govue-test" }.into());
    let claims = json!({"iss":if ec{"https://auth.x.ai"}else{"https://auth.openai.com"},"aud":if ec{"b1a00492-073a-47ea-816f-4c329264a828"}else{"app_EMoamEEZ73f0CkXaXp7hrann"},"sub":"fixture-person","email":"fixture@example.test","nonce":nonce,"exp":now()+3600,"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}});
    let key = if ec {
        EncodingKey::from_ec_der(include_bytes!("fixtures/ai-test-ec-private.der"))
    } else {
        EncodingKey::from_rsa_der(include_bytes!("fixtures/ai-test-private.der"))
    };
    encode(&header, &claims, &key).unwrap()
}
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
        let mut all = self.0.secrets.lock().unwrap();
        if value.is_empty() {
            all.remove(&key);
        } else {
            all.insert(key, value);
        }
        true
    }
    fn request(&self, r: AiHttpRequest, sink: Arc<AiResponse>) {
        let body = serde_json::from_str::<Value>(&r.body).unwrap_or_else(|_| {
            Value::Object(
                url::form_urlencoded::parse(r.body.as_bytes())
                    .map(|(k, v)| (k.into_owned(), json!(v)))
                    .collect(),
            )
        });
        let hs = r
            .headers
            .iter()
            .map(|h| (h.name.clone(), json!(h.value)))
            .collect::<serde_json::Map<_, _>>();
        self.0
            .requests
            .lock()
            .unwrap()
            .push(json!({"url":r.url,"body":body,"headers":hs}));
        sink.start(200);
        let xai = r.url.contains("auth.x.ai");
        let mode = self.0.mode.lock().unwrap().clone();
        let data = if r.url.ends_with("openid-configuration") {
            if xai {
                json!({"issuer":"https://auth.x.ai","authorization_endpoint":"https://auth.x.ai/oauth2/authorize","token_endpoint":"https://auth.x.ai/oauth2/token","jwks_uri":"https://auth.x.ai/.well-known/jwks.json"})
            } else {
                json!({"issuer":"https://auth.openai.com","jwks_uri":"https://auth.openai.com/.well-known/jwks.json"})
            }
        } else if r.url.ends_with("jwks.json") {
            serde_json::from_str(if xai {
                include_str!("fixtures/ai-test-ec-jwks.json")
            } else {
                include_str!("fixtures/ai-test-jwks.json")
            })
            .unwrap()
        } else if r.url.ends_with("device_authorization") || r.url.ends_with("device/code") {
            json!({"device_code":"fixture-device","user_code":"ABCD-EFGH","verification_uri":if r.url.contains("github.com"){"https://github.com/login/device"}else{"https://www.kimi.com/code/login"},"expires_in":600,"interval":1})
        } else if r.url.ends_with("copilot_internal/v2/token") {
            assert_eq!(hs["Authorization"], "token fixture-github-token");
            json!({"token":"fixture-copilot-session","expires_at":now()+1200})
        } else if r.url.ends_with("auth/keys") {
            assert_eq!(body["code_challenge_method"], "S256");
            assert!(body["code_verifier"].as_str().unwrap().len() >= 43);
            json!({"key":"fixture-openrouter-key"})
        } else if r.url.ends_with("/token") || r.url.ends_with("/access_token") {
            if body["grant_type"] == "refresh_token" {
                self.0.refreshes.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(30));
                if mode == "refresh-network" {
                    sink.failed();
                    return;
                }
                if mode == "refresh-server" {
                    sink.start(503);
                    sink.push(br#"{"error":"server_error"}"#.to_vec());
                    return;
                }
                if mode == "refresh-rejected" {
                    sink.start(401);
                    sink.push(
                        br#"{"error":"invalid_grant","error_description":"SECRET_UPSTREAM_BODY"}"#
                            .to_vec(),
                    );
                    return;
                }
                json!({"access_token":"fixture-refreshed","expires_in":3600})
            } else if body["grant_type"] == "urn:ietf:params:oauth:grant-type:device_code" {
                if mode == "pending" {
                    sink.start(400);
                    json!({"error":"authorization_pending"})
                } else {
                    json!({"access_token":if r.url.contains("github.com"){"fixture-github-token"}else{"fixture-kimi-token"},"refresh_token":"fixture-refresh","expires_in":3600})
                }
            } else {
                assert_eq!(body["grant_type"], "authorization_code");
                assert!(body["code_verifier"].as_str().unwrap().len() >= 43);
                if r.url.contains("claude.ai") {
                    assert!(body["state"].is_string());
                    assert_eq!(hs["X-App"], "cli");
                }
                let mut data = json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","expires_in":3600});
                if xai || r.url.contains("auth.openai.com") {
                    let nonce = if mode == "wrong-nonce" {
                        "forged".to_string()
                    } else {
                        self.0.nonce.lock().unwrap().clone()
                    };
                    data["id_token"] = json!(token(xai, &nonce));
                }
                data
            }
        } else if r.url.contains("/models") {
            json!({"models":[{"slug":"one","visibility":"list"}],"data":[{"id":"one","model_picker_enabled":true,"capabilities":{"type":"chat"}}]})
        } else {
            let events = if r.url.ends_with("/responses") {
                vec![
                    json!({"type":"response.output_text.delta","delta":"Account protocol passed"}),
                    json!({"type":"response.completed"}),
                ]
            } else if r.url.ends_with("/messages") {
                vec![
                    json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Account protocol passed"}}),
                    json!({"type":"message_stop"}),
                ]
            } else {
                vec![
                    json!({"choices":[{"delta":{"content":"Account protocol passed"},"finish_reason":"stop"}]}),
                ]
            };
            let mut wire = events.iter().fold(String::new(), |mut wire, event| {
                let _ = write!(wire, "data: {event}\n\n");
                wire
            });
            if r.url.ends_with("/chat/completions") {
                wire.push_str("data: [DONE]\n\n");
            }
            sink.push(wire.into_bytes());
            return;
        };
        sink.push(data.to_string().into_bytes());
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
fn wait(root: &Path, task: &str) -> Value {
    for _ in 0..2000 {
        let v = ok(root, json!({"op":"ai_poll","taskId":task}));
        if v["status"] != "running" {
            ok(root, json!({"op":"ai_forget","taskId":task}));
            return v;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("Account job stuck");
}
fn login_url(root: &Path, task: &str) -> Value {
    for _ in 0..300 {
        let v = ok(root, json!({"op":"ai_poll","taskId":task}));
        if v["login"].is_object() {
            return v["login"].clone();
        }
        assert_eq!(v["status"], "running", "{v}");
        thread::sleep(Duration::from_millis(5));
    }
    panic!("No login URL");
}
fn add(root: &Path, kind: &str) -> String {
    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":kind,"label":kind,"auth":"oauth","baseUrl":"","model":"one"}}),
    );
    cfg["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn start(root: &Path, id: &str, kind: &str) -> String {
    ok(
        root,
        json!({"op":"ai_start","id":id,"kind":kind,"prompt":"hello"}),
    )["taskId"]
        .as_str()
        .unwrap()
        .into()
}
fn request_callback(url: &Url) -> u16 {
    match ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .get(url.as_str())
        .call()
    {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r.status(),
        Err(e) => panic!("Callback failed: {e}"),
    }
}
fn browser_login(root: &Path, id: &str, backend: &Backend) -> Value {
    let started = ok(root, json!({"op":"ai_login","id":id}));
    let task = started["taskId"].as_str().unwrap();
    let login = login_url(root, task);
    let u = Url::parse(login["url"].as_str().unwrap()).unwrap();
    let fields = u.query_pairs().into_owned().collect::<HashMap<_, _>>();
    *backend.nonce.lock().unwrap() = fields.get("nonce").cloned().unwrap_or_default();
    assert_eq!(fields["code_challenge_method"], "S256");
    let is_router = u.host_str() == Some("openrouter.ai");
    let mut callback = Url::parse(
        &fields[if is_router {
            "callback_url"
        } else {
            "redirect_uri"
        }],
    )
    .unwrap();
    let mut forged = callback.clone();
    if is_router {
        forged.set_path("/callback/forged");
    } else {
        forged.query_pairs_mut().append_pair("state", "forged");
    }
    forged.query_pairs_mut().append_pair("code", "fixture-code");
    assert_eq!(request_callback(&forged), 400);
    callback
        .query_pairs_mut()
        .append_pair("code", "fixture-code");
    if !is_router {
        callback
            .query_pairs_mut()
            .append_pair("state", &fields["state"]);
    }
    assert_eq!(request_callback(&callback), 302);
    wait(root, task)
}
#[test]
fn account_flows_verify_callbacks_refresh_and_keep_credential_boundaries() {
    let temp =
        Temp(std::env::temp_dir().join(format!("govue-accounts-{}-{}", std::process::id(), now())));
    let root = &temp.0;
    fs::create_dir_all(root).unwrap();
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    ok(root, json!({"op":"ai_foreground","active":true}));
    for kind in ["codex", "anthropic", "openrouter", "xai"] {
        let id = add(root, kind);
        ok(
            root,
            json!({"op":"ai_save_model","id":id,"model":{"id":"one","name":"One"},"options":{"headers":{"X-Preserved":"MODEL_SECRET"},"body":{}}}),
        );
        let result = browser_login(root, &id, &backend);
        assert_eq!(result["status"], "completed", "{kind}: {result}");
        assert_eq!(
            wait(root, &start(root, &id, "models"))["status"],
            "completed"
        );
        assert_eq!(wait(root, &start(root, &id, "chat"))["status"], "completed");
        let req = backend.requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(req["headers"]["X-Preserved"], "MODEL_SECRET");
        if kind == "anthropic" {
            assert!(req["headers"].get("x-api-key").is_none());
            assert!(
                req["headers"]["anthropic-beta"]
                    .as_str()
                    .unwrap()
                    .contains("oauth-2025-04-20")
            );
            assert!(
                req["body"]["system"]
                    .as_str()
                    .unwrap()
                    .starts_with("You are Claude Code")
            );
        }
        if kind == "codex" {
            assert_eq!(req["headers"]["ChatGPT-Account-Id"], "fixture-account");
            assert_eq!(req["body"]["store"], false);
        }
        if kind != "openrouter" {
            let key = format!("ai.{id}");
            {
                let mut secrets = backend.secrets.lock().unwrap();
                let mut v: Value = serde_json::from_str(&secrets[&key]).unwrap();
                v["expires_at"] = json!(0);
                secrets.insert(key.clone(), v.to_string());
            }
            let before = backend.refreshes.load(Ordering::SeqCst);
            let a = start(root, &id, "models");
            let b = start(root, &id, "models");
            assert_eq!(wait(root, &a)["status"], "completed");
            assert_eq!(wait(root, &b)["status"], "completed");
            assert_eq!(backend.refreshes.load(Ordering::SeqCst), before + 1);
            let stored: Value =
                serde_json::from_str(&backend.secrets.lock().unwrap()[&key]).unwrap();
            assert_eq!(stored["refresh_token"], "fixture-refresh");
            assert_eq!(
                stored["modelOptions"]["one"]["headers"]["X-Preserved"],
                "MODEL_SECRET"
            );
            if kind == "xai" {
                assert_eq!(stored["token_endpoint"], "https://auth.x.ai/oauth2/token");
            }
        }
    }
    let bad = add(root, "xai");
    *backend.mode.lock().unwrap() = "wrong-nonce".into();
    let result = browser_login(root, &bad, &backend);
    assert_eq!(result["status"], "failed");
    assert!(
        !backend
            .secrets
            .lock()
            .unwrap()
            .get(&format!("ai.{bad}"))
            .is_some_and(|s| s.contains("fixture-access"))
    );
    *backend.mode.lock().unwrap() = String::new();
    for kind in ["kimi", "copilot"] {
        let id = add(root, kind);
        if kind == "copilot" {
            assert_eq!(call(root, json!({"op":"ai_login","id":id}))["ok"], false);
        }
        let started = ok(root, json!({"op":"ai_login","id":id,"consent":true}));
        let task = started["taskId"].as_str().unwrap();
        let login = login_url(root, task);
        assert_eq!(login["mode"], "device");
        assert_eq!(login["userCode"], "ABCD-EFGH");
        assert_eq!(wait(root, task)["status"], "completed");
        assert_eq!(
            wait(root, &start(root, &id, "models"))["status"],
            "completed"
        );
        assert_eq!(wait(root, &start(root, &id, "chat"))["status"], "completed");
        let requests = backend.requests.lock().unwrap();
        if kind == "copilot" {
            for req in requests.iter().filter(|r| {
                r["url"]
                    .as_str()
                    .unwrap()
                    .starts_with("https://api.githubcopilot.com")
            }) {
                assert_eq!(
                    req["headers"]["Authorization"],
                    "Bearer fixture-copilot-session"
                );
                assert!(!req.to_string().contains("fixture-github-token"));
            }
        }
    }
    let id = add(root, "anthropic");
    assert_eq!(browser_login(root, &id, &backend)["status"], "completed");
    let key = format!("ai.{id}");
    {
        let mut all = backend.secrets.lock().unwrap();
        let mut v: Value = serde_json::from_str(&all[&key]).unwrap();
        v["expires_at"] = json!(0);
        all.insert(key.clone(), v.to_string());
    }
    for mode in ["refresh-network", "refresh-server"] {
        {
            let mut all = backend.secrets.lock().unwrap();
            let mut v: Value = serde_json::from_str(&all[&key]).unwrap();
            v["expires_at"] = json!(now() + 30);
            all.insert(key.clone(), v.to_string());
        }
        *backend.mode.lock().unwrap() = mode.into();
        assert_eq!(
            wait(root, &start(root, &id, "models"))["status"],
            "completed"
        );
    }
    {
        let mut all = backend.secrets.lock().unwrap();
        let mut v: Value = serde_json::from_str(&all[&key]).unwrap();
        v["expires_at"] = json!(0);
        all.insert(key.clone(), v.to_string());
    }
    *backend.mode.lock().unwrap() = "refresh-rejected".into();
    let result = wait(root, &start(root, &id, "models"));
    assert_eq!(result["status"], "failed");
    assert!(!result.to_string().contains("SECRET_UPSTREAM_BODY"));
    assert!(backend.secrets.lock().unwrap()[&key].contains("fixture-refresh"));
    let cfg = ok(root, json!({"op":"ai_config"}));
    assert_eq!(
        cfg["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()["needsReauth"],
        true
    );
    *backend.mode.lock().unwrap() = "pending".into();
    let pending = add(root, "kimi");
    let started = ok(root, json!({"op":"ai_login","id":pending}));
    let task = started["taskId"].as_str().unwrap();
    login_url(root, task);
    ok(root, json!({"op":"ai_cancel","taskId":task}));
    assert_eq!(wait(root, task)["status"], "cancelled");
    thread::sleep(Duration::from_millis(150));
    assert!(
        !backend
            .secrets
            .lock()
            .unwrap()
            .get(&format!("ai.{pending}"))
            .is_some_and(|v| v.contains("access_token"))
    );
    let export = ok(root, json!({"op":"ai_export"})).to_string();
    for secret in [
        "fixture-access",
        "fixture-refresh",
        "fixture-github-token",
        "fixture-copilot-session",
        "MODEL_SECRET",
    ] {
        assert!(!export.contains(secret));
        assert!(
            !fs::read_to_string(root.join("ai/providers.json"))
                .unwrap()
                .contains(secret)
        );
    }
}
