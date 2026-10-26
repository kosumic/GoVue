#![cfg(test)]

//! QR boundary regressions after the native generation / styled-image round trip.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: AtomicUsize,
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
    fn request(&self, _: AiHttpRequest, response: Arc<AiResponse>) {
        self.0.requests.fetch_add(1, Ordering::SeqCst);
        response.failed();
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn call(root: &Path, command: Value) -> Value {
    serde_json::from_str(&govue_core::ai_execute(
        command.to_string(),
        root.to_str().unwrap().into(),
    ))
    .unwrap()
}
fn ok(root: &Path, command: Value) -> Value {
    let response = call(root, command);
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}
fn pack(bytes: &[u8]) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).unwrap();
    format!(
        "govue:provider:1:{}",
        URL_SAFE_NO_PAD.encode(encoder.finish().unwrap())
    )
}
fn unpack(payload: &str) -> Value {
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.strip_prefix("govue:provider:1:").unwrap())
        .unwrap();
    let mut json = String::new();
    ZlibDecoder::new(bytes.as_slice())
        .read_to_string(&mut json)
        .unwrap();
    serde_json::from_str(&json).unwrap()
}
fn settings(root: &Path) -> Vec<u8> {
    fs::read(root.join("ai/providers.json")).unwrap()
}

fn roundtrip(sender: &Path, receiver: &Path, backend: &Backend) -> (String, Value) {
    let model = json!({"id":"first","name":"First model","contextWindow":8192,"temperature":0.3,"reasoning":"medium","inputModalities":["text","image"]});
    let config = ok(
        sender,
        json!({"op":"ai_save","provider":{"id":"","kind":"openai","label":"Shared 日本語","model":"first","protocol":"chat","models":[model,{"id":"second","name":"Second model"}]},"key":"NEVER_EXPORT_API_KEY"}),
    );
    let id = config["providers"][0]["id"].clone();
    ok(
        sender,
        json!({"op":"ai_save_model","id":id,"model":model,"options":{"headers":{"X-Private":"NEVER_EXPORT_HEADER"},"body":{"seed":7}}}),
    );
    ok(
        sender,
        json!({"op":"ai_save","provider":{"id":"","kind":"gemini","label":"Unrelated provider"}}),
    );
    let original = ok(sender, json!({"op":"ai_export"}))["providers"][0].clone();
    let before = settings(sender);
    let qr = ok(sender, json!({"op":"ai_provider_qr","id":id}));
    assert_eq!(before, settings(sender), "sharing must not change settings");
    let payload = qr["payload"].as_str().unwrap().to_owned();
    assert!(payload.len() <= 1000);
    let raw = unpack(&payload);
    let mut expected = original.clone();
    expected["id"] = json!("shared");
    assert_eq!(raw, expected, "only the selected connection is shared");
    assert!(!raw.to_string().contains("NEVER_EXPORT"));
    let modules = qr["modules"].as_array().unwrap();
    assert!((21..=177).contains(&modules.len()));
    assert!(modules.iter().all(|row| {
        row.as_str().is_some_and(|s| {
            s.len() == modules.len() && s.bytes().all(|c| matches!(c, b'0' | b'1'))
        })
    }));

    let existing = ok(
        receiver,
        json!({"op":"ai_save","provider":{"id":"","kind":"gemini","label":"Keep me"},"key":"LOCAL_ONLY_KEY"}),
    );
    let before = settings(receiver);
    let secrets = backend.secrets.lock().unwrap().clone();
    let preview = ok(
        receiver,
        json!({"op":"ai_preview_provider_qr","payload":payload}),
    );
    assert_eq!(preview["modelCount"], 2);
    assert_eq!(preview["label"], original["label"]);
    assert_eq!(preview["protocol"], "chat");
    assert_eq!(before, settings(receiver), "preview cannot import");
    let imported = ok(
        receiver,
        json!({"op":"ai_import_provider_qr","payload":payload}),
    );
    assert_eq!(imported["providers"].as_array().unwrap().len(), 2);
    assert_eq!(imported["activeId"], existing["activeId"]);
    assert_ne!(imported["providers"][1]["id"], id);
    assert_ne!(
        imported["providers"][1]["id"],
        existing["providers"][0]["id"]
    );
    assert_eq!(imported["providers"][1]["connected"], false);
    assert_eq!(
        *backend.secrets.lock().unwrap(),
        secrets,
        "import cannot transfer or overwrite secrets"
    );
    let mut imported_settings = ok(receiver, json!({"op":"ai_export"}))["providers"][1].clone();
    imported_settings["id"] = original["id"].clone();
    assert_eq!(imported_settings, original);
    (payload, raw)
}

fn reject_untrusted(receiver: &Path, payload: &str, raw: &Value) {
    let mut with_key = raw.clone();
    with_key["key"] = json!("UNTRUSTED_SECRET");
    let mut with_token = raw.clone();
    with_token["access_token"] = json!("UNTRUSTED_SECRET");
    let mut with_headers = raw.clone();
    with_headers["models"][0]["headers"] = json!({"Authorization":"UNTRUSTED_SECRET"});
    let mut insecure = raw.clone();
    insecure["baseUrl"] = json!("http://example.com/v1");
    let mut oauth = raw.clone();
    oauth["kind"] = json!("codex");
    oauth["auth"] = json!("oauth");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.strip_prefix("govue:provider:1:").unwrap())
        .unwrap();
    let mut trailing = bytes.clone();
    trailing.push(0);
    let truncated = &bytes[..bytes.len() - 1];
    let mut invalid = vec![
        "https://example.com".into(),
        payload.replacen(":1:", ":2:", 1),
        "x".repeat(1001),
        "govue:provider:1:???".into(),
        format!("govue:provider:1:{}", URL_SAFE_NO_PAD.encode(truncated)),
        format!("govue:provider:1:{}", URL_SAFE_NO_PAD.encode(trailing)),
        pack(&vec![b' '; 256_001]),
        pack(b"not json"),
    ];
    invalid.extend(
        [with_key, with_token, with_headers, insecure, oauth]
            .iter()
            .map(|v| pack(v.to_string().as_bytes())),
    );
    let before = settings(receiver);
    for payload in invalid {
        for op in ["ai_preview_provider_qr", "ai_import_provider_qr"] {
            let result = call(receiver, json!({"op":op,"payload":payload}));
            assert_eq!(result["ok"], false, "{op} accepted invalid input");
            assert!(!result.to_string().contains("UNTRUSTED_SECRET"));
            assert_eq!(settings(receiver), before, "invalid input changed settings");
        }
    }
}

fn large_libraries(sender: &Path, receiver: &Path, raw: &Value) {
    let models: Vec<_> = (0_u32..512)
        .map(|i| {
            let mut id = String::with_capacity(64);
            for byte in Sha256::digest(i.to_le_bytes()) {
                write!(id, "{byte:02x}").unwrap();
            }
            json!({"id":id,"name":format!("Model {i}")})
        })
        .collect();
    let config = ok(
        sender,
        json!({"op":"ai_save","provider":{"id":"","kind":"openai","label":"Large library","model":models[0]["id"],"models":models}}),
    );
    let id = config["providers"].as_array().unwrap().last().unwrap()["id"].clone();
    let before = settings(sender);
    assert_eq!(
        call(sender, json!({"op":"ai_provider_qr","id":id}))["ok"],
        false
    );
    let qr = ok(
        sender,
        json!({"op":"ai_provider_qr","id":id,"selectedModelOnly":true}),
    );
    let decoded = unpack(qr["payload"].as_str().unwrap());
    assert_eq!(decoded["models"].as_array().unwrap().len(), 1);
    assert_eq!(decoded["models"][0]["id"], decoded["model"]);
    assert_eq!(
        settings(sender),
        before,
        "limiting the share must not prune the library"
    );

    let providers: Vec<_> = (0..32)
        .map(|i| {
            let mut p = raw.clone();
            p["id"] = json!(format!("limit_{i}"));
            p
        })
        .collect();
    fs::write(
        receiver.join("ai/providers.json"),
        json!({"providers":providers,"groups":[],"activeId":"limit_0"}).to_string(),
    )
    .unwrap();
    let before = settings(receiver);
    assert_eq!(
        call(
            receiver,
            json!({"op":"ai_import_provider_qr","payload":qr["payload"]})
        )["ok"],
        false
    );
    assert_eq!(settings(receiver), before);
}

#[test]
fn provider_qr_roundtrip_and_untrusted_import_boundaries() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = format!("govue-qr-{}-{stamp}", std::process::id());
    let temp = Temp(std::env::temp_dir().join(directory));
    let sender = temp.0.join("sender");
    let receiver = temp.0.join("receiver");
    let backend = Arc::new(Backend::default());
    govue_core::install_ai_platform(Box::new(Platform(backend.clone())));
    let (payload, raw) = roundtrip(&sender, &receiver, &backend);
    reject_untrusted(&receiver, &payload, &raw);
    large_libraries(&sender, &receiver, &raw);
    assert_eq!(
        backend.requests.load(Ordering::SeqCst),
        0,
        "QR sharing must be offline"
    );
}
