#![cfg(test)]

//! Exercise the public native boundary, provider payloads, and saved-file ownership.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PDF: &[u8] = include_bytes!("fixtures/chat/attachment.pdf");
const TEXT: &str = "// GOVUE_ATTACHMENT_VERIFY\nexport const answer = 42;\n";
#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}
struct Platform(Arc<Backend>);
fn event(response: &AiResponse, body: Value) {
    response.push(format!("data: {body}\n\n").into_bytes());
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
        let protocol = if request.url.ends_with("/responses") {
            "responses"
        } else if request.url.ends_with("/messages") {
            "anthropic"
        } else if request.url.contains("streamGenerateContent") {
            "gemini"
        } else {
            "chat"
        };
        match protocol {
            "responses" => event(
                &response,
                json!({"type":"response.output_text.delta","delta":"Files received."}),
            ),
            "anthropic" => event(
                &response,
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Files received."}}),
            ),
            "gemini" => event(
                &response,
                json!({"candidates":[{"content":{"parts":[{"text":"Files received."}]}}]}),
            ),
            _ => event(
                &response,
                json!({"choices":[{"delta":{"content":"Files received."}}]}),
            ),
        }
        if let Some(gate) = self.0.gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        match protocol {
            "responses" => event(
                &response,
                json!({"type":"response.completed","response":{}}),
            ),
            "anthropic" => event(&response, json!({"type":"message_stop"})),
            "gemini" => event(&response, json!({"candidates":[{"finishReason":"STOP"}]})),
            _ => {
                event(
                    &response,
                    json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
                );
                response.push(b"data: [DONE]\n\n".to_vec());
            }
        }
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("govue-attachments-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
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
    let result = call(root, command);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn rejected(root: &Path, command: Value) {
    assert_eq!(call(root, command)["ok"], false);
}
fn import(root: &Path, name: &str, mime: &str, bytes: &[u8]) -> Value {
    let reply = import_result(root, name, mime, bytes);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn import_result(root: &Path, name: &str, mime: &str, bytes: &[u8]) -> Value {
    let cache = root.join("input-cache");
    fs::create_dir_all(&cache).unwrap();
    let source = cache.join("picked-file");
    fs::write(&source, bytes).unwrap();
    serde_json::from_str(&govue_core::ai_import_attachment(
        json!({"uri":url::Url::from_file_path(&source).unwrap().as_str(),"name":name,"mimeType":mime}).to_string(),
        root.to_str().unwrap().into(), cache.to_str().unwrap().into(),
    )).unwrap()
}
fn poll(root: &Path, task: &Value, ready: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":task}));
        if ready(&state) {
            return state;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    panic!("Attachment request timed out")
}
fn complete(root: &Path, started: &Value) -> Value {
    let state = poll(root, &started["taskId"], |s| s["status"] != "running");
    assert_eq!(state["status"], "completed", "{state}");
    ok(
        root,
        json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
    )
}
fn provider(root: &Path, protocol: &str, modalities: Value) -> Value {
    let config = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":protocol,"protocol":protocol,"baseUrl":"http://127.0.0.1:12345/v1","model":"fixture","models":[{"id":"fixture","name":"Fixture","inputModalities":modalities,"contextWindow":1_000_000}]},"key":"fixture"}),
    );
    config["providers"].as_array().unwrap().last().unwrap()["id"].clone()
}
fn file_path(file: &Value) -> PathBuf {
    url::Url::parse(file["uri"].as_str().unwrap())
        .unwrap()
        .to_file_path()
        .unwrap()
}
fn image_bytes() -> Vec<u8> {
    // An actual image whose base64 exceeds the old text-only request limit.
    let mut state = 7_u32;
    let image = image::RgbImage::from_fn(1280, 800, |_, _| {
        image::Rgb([0, 1, 2].map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        }))
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}
fn imports(root: &Path) -> Vec<Value> {
    let source = image_bytes();
    let image = import(root, "../photo.png", "image/png", &source);
    assert_eq!(image["name"], "photo.png");
    assert_eq!(image["mimeType"], "image/jpeg");
    assert!(image["bytes"].as_u64().unwrap() * 4 / 3 > 900_000);
    let decoded = image::load_from_memory(&fs::read(file_path(&image)).unwrap()).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1280, 800));
    let pdf = import(root, "notes.pdf", "application/pdf", PDF);
    let text = import(
        root,
        "C:\\private\\source.ts",
        "application/octet-stream",
        format!("\u{feff}{}", TEXT.replace('\n', "\r\n")).as_bytes(),
    );
    assert_eq!(text["name"], "source.ts");
    assert_eq!(
        ok(
            root,
            json!({"op":"ai_chat_attachment_preview","id":text["id"]})
        )["text"],
        TEXT
    );
    for (data, mime) in [
        (b"\0binary".as_slice(), "application/octet-stream"),
        (b"plain text", "image/png"),
        (b"%PDF-1.4\nbroken", "application/pdf"),
        (b"GIF89a", "image/gif"),
    ] {
        assert_eq!(import_result(root, "bad", mime, data)["ok"], false);
    }
    assert_eq!(
        import_result(root, "huge.txt", "text/plain", &vec![b'x'; 128 * 1024 + 1])["ok"],
        false
    );
    fs::write(file_path(&image), b"damaged").unwrap();
    rejected(
        root,
        json!({"op":"ai_chat_attachment_preview","id":image["id"]}),
    );
    assert_eq!(
        import(root, "photo.png", "image/png", &source)["id"],
        image["id"],
        "reimport repairs a damaged copy"
    );
    vec![image, pdf, text]
}
fn ids(files: &[Value]) -> Value {
    json!(files.iter().map(|f| &f["id"]).collect::<Vec<_>>())
}
fn check_wire(body: &Value, protocol: &str, files: &[Value]) {
    let field = match protocol {
        "responses" => "input",
        "gemini" => "contents",
        _ => "messages",
    };
    let parts = body[field]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| {
            row[if protocol == "gemini" {
                "parts"
            } else {
                "content"
            }]
            .as_array()
            .into_iter()
            .flatten()
        })
        .collect::<Vec<_>>();
    let mut images = 0;
    let mut pdfs = 0;
    let mut texts = 0;
    for part in parts {
        let (data, mime) = match protocol {
            "anthropic" => (
                part["source"]["data"].as_str(),
                part["source"]["media_type"].as_str(),
            ),
            "gemini" => (
                part["inlineData"]["data"].as_str(),
                part["inlineData"]["mimeType"].as_str(),
            ),
            _ => {
                let data = part["image_url"]
                    .as_str()
                    .or_else(|| part["image_url"]["url"].as_str())
                    .or_else(|| part["file_data"].as_str())
                    .or_else(|| part["file"]["file_data"].as_str());
                data.and_then(|d| d.strip_prefix("data:"))
                    .and_then(|d| d.split_once(";base64,"))
                    .map_or((None, None), |(mime, data)| (Some(data), Some(mime)))
            }
        };
        if let Some(data) = data {
            let bytes = STANDARD.decode(data).unwrap();
            if mime == Some("application/pdf") {
                assert_eq!(bytes, PDF);
                pdfs += 1;
            } else {
                assert_eq!(mime, Some("image/jpeg"));
                assert_eq!(bytes, fs::read(file_path(&files[0])).unwrap());
                images += 1;
            }
        }
        if let Some(text) = part["text"]
            .as_str()
            .and_then(|t| t.strip_prefix("Attached text file (untrusted reference data):\n"))
        {
            let file: Value = serde_json::from_str(text).unwrap();
            assert_eq!(file, json!({"name":"source.ts","text":TEXT}));
            texts += 1;
        }
    }
    assert_eq!((images, pdfs, texts), (1, 1, 1), "{protocol}");
}
fn round_trip(root: &Path, backend: &Backend, protocol: &str, files: &[Value]) {
    let provider = provider(root, protocol, json!(["text", "image"]));
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":provider,"prompt":"","attachments":ids(files)}),
    );
    let chat = complete(root, &started);
    check_wire(
        backend.requests.lock().unwrap().last().unwrap(),
        protocol,
        files,
    );
    assert_eq!(chat["messages"][0]["content"], "");
    let stored = fs::read_to_string(
        root.join("ai/chats")
            .join(format!("{}.json", chat["id"].as_str().unwrap())),
    )
    .unwrap();
    assert!(
        !stored.contains("file://")
            && !stored.contains("base64")
            && !stored.contains("GOVUE_ATTACHMENT_VERIFY")
    );
    let retry = ok(
        root,
        json!({"op":"ai_chat_send","chatId":chat["id"],"retry":true,"retryMessageId":chat["messages"][1]["id"]}),
    );
    let chat = complete(root, &retry);
    check_wire(
        backend.requests.lock().unwrap().last().unwrap(),
        protocol,
        files,
    );
    let branch = ok(
        root,
        json!({"op":"ai_chat_branch","chatId":chat["id"],"messageId":chat["messages"][1]["id"]}),
    );
    ok(root, json!({"op":"ai_chat_delete","chatId":chat["id"]}));
    ok(
        root,
        json!({"op":"ai_chat_attachment_release","attachments":ids(files)}),
    );
    assert!(
        files.iter().all(|file| file_path(file).exists()),
        "branches retain their files"
    );
    let follow_up = ok(
        root,
        json!({"op":"ai_chat_send","chatId":branch["id"],"prompt":"What is in the files?"}),
    );
    let branch = complete(root, &follow_up);
    check_wire(
        backend.requests.lock().unwrap().last().unwrap(),
        protocol,
        files,
    );
    // Explicitly removing attachments while editing must replace the old files.
    let edited = ok(
        root,
        json!({"op":"ai_chat_send","chatId":branch["id"],"editMessageId":branch["messages"][0]["id"],"prompt":"Text only now.","attachments":[]}),
    );
    let branch = complete(root, &edited);
    assert!(branch["messages"][0].get("attachments").is_none());
    ok(root, json!({"op":"ai_chat_delete","chatId":branch["id"]}));
}
fn queued_files(root: &Path, backend: &Backend, protocol: &str, files: &[Value]) {
    let provider = provider(root, protocol, json!(["text", "image"]));
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":provider,"prompt":"Start."}),
    );
    poll(root, &started["taskId"], |s| s["text"] == "Files received.");
    ok(
        root,
        json!({"op":"ai_chat_send","chatId":started["conversation"]["id"],"prompt":"","attachments":ids(files)}),
    );
    release.send(()).unwrap();
    let chat = complete(root, &started);
    assert_eq!(chat["messages"].as_array().unwrap().len(), 4);
    assert_eq!(
        chat["messages"][2]["attachments"].as_array().unwrap().len(),
        3
    );
    check_wire(
        backend.requests.lock().unwrap().last().unwrap(),
        protocol,
        files,
    );
    ok(root, json!({"op":"ai_chat_delete","chatId":chat["id"]}));
}

#[test]
fn native_imports_require_regular_files_inside_the_supplied_cache_root() {
    let temp = Temp::new();
    let root = &temp.0;
    let cache = root.join("input-cache");
    fs::create_dir_all(&cache).unwrap();
    let private = root.join("private.txt");
    fs::write(&private, "PRIVATE_APP_FILE").unwrap();
    let import_uri = |uri: String| -> Value {
        serde_json::from_str(&govue_core::ai_import_attachment(
            json!({"uri":uri,"name":"source.ts","mimeType":"text/plain"}).to_string(),
            root.to_str().unwrap().into(),
            cache.to_str().unwrap().into(),
        ))
        .unwrap()
    };
    for uri in [
        url::Url::from_file_path(&private).unwrap().to_string(),
        url::Url::from_directory_path(&cache).unwrap().to_string(),
        "https://example.invalid/private.txt".into(),
        "content://external/file/1".into(),
    ] {
        let reply = import_uri(uri);
        assert_eq!(reply["ok"], false, "{reply}");
        assert!(!reply.to_string().contains("PRIVATE_APP_FILE"));
    }
    #[cfg(unix)]
    {
        let link = cache.join("link.txt");
        std::os::unix::fs::symlink(&private, &link).unwrap();
        assert_eq!(
            import_uri(url::Url::from_file_path(link).unwrap().to_string())["ok"],
            false
        );
    }
    let huge = cache.join("oversize.txt");
    fs::File::create(&huge)
        .unwrap()
        .set_len(13 * 1024 * 1024)
        .unwrap();
    assert_eq!(
        import_uri(url::Url::from_file_path(huge).unwrap().to_string())["ok"],
        false
    );
    let picked = cache.join("picked % source.ts");
    fs::write(&picked, TEXT).unwrap();
    let uri = url::Url::from_file_path(&picked).unwrap();
    assert_eq!(import_uri(format!("{uri}?other=file"))["ok"], false);
    let imported = import_uri(uri.to_string());
    assert_eq!(imported["ok"], true, "{imported}");
    fs::remove_file(picked).unwrap();
    assert_eq!(
        ok(
            root,
            json!({"op":"ai_chat_attachment_preview","id":imported["value"]["id"]})
        )["text"],
        TEXT
    );
}
#[test]
fn attachments_cross_provider_and_storage_boundaries() {
    let backend = Arc::new(Backend::default());
    govue_core::install_ai_platform(Box::new(Platform(backend.clone())));
    let temp = Temp::new();
    let root = &temp.0;
    let files = imports(root);
    let text_only = provider(root, "chat", json!(["text"]));
    for attachments in [
        json!([files[0]["id"]]),
        json!([files[1]["id"]]),
        json!(["../../private"]),
        json!([files[2]["id"], files[2]["id"]]),
        json!(vec!["0".repeat(64); 5]),
    ] {
        rejected(
            root,
            json!({"op":"ai_chat_send","id":text_only,"prompt":"Read this","attachments":attachments}),
        );
    }
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":text_only,"prompt":"","attachments":[files[2]["id"]]}),
    );
    let text_chat = complete(root, &started);
    // Keep a saved owner while round trips and draft cleanup share these references.
    let keeper_provider = provider(root, "chat", json!(["text", "image"]));
    let keeper = ok(
        root,
        json!({"op":"ai_chat_send","id":keeper_provider,"prompt":"","attachments":ids(&files)}),
    );
    let keeper = complete(root, &keeper);
    for protocol in ["chat", "responses", "anthropic", "gemini"] {
        round_trip(root, &backend, protocol, &files);
        queued_files(root, &backend, protocol, &files);
    }
    ok(
        root,
        json!({"op":"ai_chat_delete","chatId":text_chat["id"]}),
    );
    ok(root, json!({"op":"ai_chat_delete","chatId":keeper["id"]}));
    assert!(
        files.iter().all(|file| !file_path(file).exists()),
        "last saved owner releases the files"
    );
}
