#![cfg(test)]

use govue_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, install_ai_platform,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
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
    fn request(&self, _request: AiHttpRequest, response: Arc<AiResponse>) {
        response.start(200);
        let gate = self.0.gate.lock().unwrap().take();
        self.0.entered.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = gate {
            let _ = gate.recv_timeout(Duration::from_secs(5));
        }
        if response.is_cancelled() {
            return;
        }
        response.push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Fixture answer\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_vec());
    }
}
fn reply(root: &str, command: Value) -> Value {
    serde_json::from_str(&ai_execute(command.to_string(), root.into())).unwrap()
}
fn call(root: &str, command: Value) -> Value {
    let reply = reply(root, command);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn entered(backend: &Backend, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while backend.entered.load(Ordering::SeqCst) < count {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn sessions_serialize_queueing_and_cancel_only_the_owned_generation() {
    let root = std::env::temp_dir().join(format!("govue-chat-session-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let root = root.to_str().unwrap();
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    let config = call(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":"Fixture","protocol":"responses","baseUrl":"https://fixture.test/v1","model":"fixture","models":[{"id":"fixture","name":"Fixture"}]},"key":"fixture-only"}),
    );
    let provider = config["providers"][0]["id"].clone();
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let first = call(
        root,
        json!({"op":"ai_chat_session_send","sessionId":"main","version":1,"command":{"id":provider,"prompt":"First message"}}),
    );
    entered(&backend, 1);
    let chat = first["conversation"]["id"].clone();
    let queued = call(
        root,
        json!({"op":"ai_chat_session_send","sessionId":"main","version":1,"queue":true,"command":{"chatId":chat,"prompt":"Queued message"}}),
    );
    assert_eq!(queued["taskId"], first["taskId"]);
    assert_eq!(
        queued["conversation"]["pendingMessages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        call(
            root,
            json!({"op":"ai_chat_session_cancel","sessionId":"main","version":1,"chatId":chat})
        )
        .is_null()
    );
    assert_eq!(
        call(root, json!({"op":"ai_chat_get","chatId":chat}))["activeTask"],
        first["taskId"]
    );
    let stopped = call(
        root,
        json!({"op":"ai_chat_session_cancel","sessionId":"main","version":2,"chatId":chat}),
    );
    assert!(stopped["conversation"]["activeTask"].is_null());
    assert_eq!(stopped["conversation"]["messages"][1]["status"], "stopped");
    assert_eq!(
        reply(
            root,
            json!({"op":"ai_chat_session_send","sessionId":"main","version":1,"command":{"id":provider,"chatId":chat,"prompt":"Stale"}})
        )["ok"],
        false
    );
    release.send(()).unwrap();
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let second = call(
        root,
        json!({"op":"ai_chat_session_send","sessionId":"main","version":2,"command":{"id":provider,"chatId":chat,"prompt":"New generation"}}),
    );
    entered(&backend, 2);
    call(
        root,
        json!({"op":"ai_chat_session_cancel","sessionId":"main","version":2,"chatId":chat}),
    );
    let _ = reply(
        root,
        json!({"op":"ai_chat_session_forget","sessionId":"main","version":1,"taskId":first["taskId"]}),
    );
    assert_eq!(
        call(root, json!({"op":"ai_chat_get","chatId":chat}))["activeTask"],
        second["taskId"],
        "Late cancellation and cleanup cannot stop a new send"
    );
    let reopened = call(
        root,
        json!({"op":"ai_chat_session_cancel","sessionId":"reopened","version":1,"chatId":chat}),
    );
    assert!(
        reopened["conversation"]["activeTask"].is_null(),
        "Reopened views must adopt a saved active task before stopping it"
    );
    release.send(()).unwrap();
    fs::remove_dir_all(root).unwrap();
}
