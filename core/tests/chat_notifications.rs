#![cfg(test)]

//! Completion alerts follow durable replies and the currently attended chat.
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret, ChatNotificationPlatform};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    gate: Mutex<Option<mpsc::Receiver<bool>>>,
    entered: AtomicUsize,
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
        let gate = self.0.gate.lock().unwrap().take();
        self.0.entered.fetch_add(1, Ordering::SeqCst);
        let success = gate
            .map(|gate| gate.recv_timeout(Duration::from_secs(5)).unwrap())
            .unwrap_or(true);
        if !success {
            response.start(500);
            return;
        }
        response.start(200);
        response.push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Saved reply\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_vec());
    }
}
struct Alerts {
    root: String,
    events: mpsc::Sender<(String, bool, u64)>,
}
impl ChatNotificationPlatform for Alerts {
    fn update(&self, chat: String, show: bool, revision: u64) {
        if show {
            // Reentrant reads prove delivery happens after saving and outside
            // the conversation lock. Presentation never receives reply text.
            let saved = call(&self.root, json!({"op":"ai_chat_get","chatId":chat}));
            assert!(saved["activeTask"].is_null());
            assert_eq!(
                saved["messages"].as_array().unwrap().last().unwrap()["status"],
                "completed"
            );
        }
        self.events.send((chat, show, revision)).unwrap();
    }
}
fn call(root: &str, command: Value) -> Value {
    let result: Value =
        serde_json::from_str(&govue_core::ai_execute(command.to_string(), root.into())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn attention(command: Value) {
    let result: Value =
        serde_json::from_str(&govue_core::state_execute(command.to_string())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
fn wait(root: &str, task: &Value) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while call(root, json!({"op":"ai_poll","taskId":task}))["status"] == "running" {
        assert!(Instant::now() < deadline, "reply did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn gate(backend: &Backend) -> mpsc::Sender<bool> {
    let (send, receive) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(receive);
    send
}
fn entered(backend: &Backend, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while backend.entered.load(Ordering::SeqCst) < count {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn successful_away_replies_notify_once_and_visible_failed_stopped_deleted_replies_stay_quiet() {
    let directory =
        std::env::temp_dir().join(format!("govue-notifications-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let root = directory.to_str().unwrap();
    let backend = Arc::new(Backend::default());
    govue_core::install_ai_platform(Box::new(Platform(backend.clone())));
    let (send, events) = mpsc::channel();
    govue_core::install_chat_notification_platform(Box::new(Alerts {
        root: root.into(),
        events: send,
    }));
    let config = call(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":"Fixture","protocol":"responses","baseUrl":"https://fixture.test/v1","model":"fixture","models":[{"id":"fixture","name":"Fixture"}]},"key":"fixture"}),
    );
    let provider = config["providers"][0]["id"].clone();

    // First replies may complete before React receives the new chat ID.
    attention(json!({"op":"chat_notification_visibility","chatId":null,"sessionId":"view"}));
    let first = call(
        root,
        json!({"op":"ai_chat_session_send","sessionId":"view","version":1,"command":{"id":provider,"prompt":"Visible"}}),
    );
    wait(root, &first["taskId"]);
    assert!(events.try_recv().is_err());
    let chat = first["conversation"]["id"].clone();

    // Closing the sheet while a request runs must allow a durable alert.
    let release = gate(&backend);
    let hidden = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Hidden"}),
    );
    attention(json!({"op":"chat_notification_visibility","chatId":null}));
    release.send(true).unwrap();
    wait(root, &hidden["taskId"]);
    let (target, show, revision) = events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(target, chat.as_str().unwrap());
    assert!(show);
    assert!(events.try_recv().is_err());

    attention(json!({"op":"chat_notification_visibility","chatId":chat,"sessionId":"view"}));
    let (_, show, dismissed) = events.recv().unwrap();
    assert!(!show);
    assert!(
        dismissed > revision,
        "opening the chat supersedes pending presentations"
    );

    attention(json!({"op":"chat_notification_foreground","active":false}));
    let background = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Background"}),
    );
    wait(root, &background["taskId"]);
    assert!(events.recv_timeout(Duration::from_secs(5)).unwrap().1);
    attention(json!({"op":"chat_notification_foreground","active":true}));
    assert!(!events.recv().unwrap().1);

    let visible = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Visible again"}),
    );
    wait(root, &visible["taskId"]);
    assert!(events.try_recv().is_err());
    attention(json!({"op":"chat_notification_visibility","chatId":null}));

    let release = gate(&backend);
    let failed = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Failure"}),
    );
    release.send(false).unwrap();
    wait(root, &failed["taskId"]);
    assert!(events.try_recv().is_err());

    let release = gate(&backend);
    let stopped = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Stop"}),
    );
    entered(&backend, 6);
    call(root, json!({"op":"ai_cancel","taskId":stopped["taskId"]}));
    release.send(true).unwrap();
    wait(root, &stopped["taskId"]);
    assert!(events.try_recv().is_err());

    let release = gate(&backend);
    let deleted = call(
        root,
        json!({"op":"ai_chat_send","id":provider,"chatId":chat,"prompt":"Delete"}),
    );
    entered(&backend, 7);
    call(root, json!({"op":"ai_chat_delete","chatId":chat}));
    release.send(true).unwrap();
    wait(root, &deleted["taskId"]);
    assert!(events.try_recv().is_err());
    let invalid: Value = serde_json::from_str(&govue_core::state_execute(
        json!({"op":"chat_notification_visibility","chatId":"../private"}).to_string(),
    ))
    .unwrap();
    assert_eq!(invalid["ok"], false);
    std::fs::remove_dir_all(directory).unwrap();
}
