#![cfg(test)]

//! The UI receives an ordered, durable activity tape, never provider continuation data.
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
    mode: AtomicU8,
    requests: Mutex<Vec<Value>>,
}
struct Platform(Arc<Backend>);
fn push(response: &AiResponse, value: Value) {
    let bytes = format!("data: {value}\n\n");
    // Both SSE framing and activity must survive split multibyte characters.
    for chunk in bytes.as_bytes().chunks(3) {
        if !response.push(chunk.to_vec()) {
            break;
        }
    }
}
fn definition() -> Value {
    json!({"title":"你好 practice","html":"<button>Hello</button>","css":"","script":"// PRIVATE_GAME_SOURCE",
        "rules":json!({"initialState":{},"actions":[{"name":"choose","updates":[]}]}).to_string(),"cueIds":[]})
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
        let body: Value = serde_json::from_str(&request.body).unwrap();
        self.0.requests.lock().unwrap().push(body.clone());
        let protocol = if request.url.ends_with("/responses") {
            "responses"
        } else if request.url.ends_with("/messages") {
            "anthropic"
        } else if request.url.contains("streamGenerateContent") {
            "gemini"
        } else {
            "chat"
        };
        let continued = body["input"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["type"] == "function_call_output"))
            || body["messages"].as_array().is_some_and(|rows| {
                rows.iter().any(|r| {
                    r["role"] == "tool"
                        || r["content"]
                            .as_array()
                            .is_some_and(|parts| parts.iter().any(|p| p["type"] == "tool_result"))
                })
            })
            || body["contents"].as_array().is_some_and(|rows| {
                rows.iter().any(|r| {
                    r["parts"].as_array().is_some_and(|parts| {
                        parts.iter().any(|p| p["functionResponse"].is_object())
                    })
                })
            });
        let mode = self.0.mode.load(Ordering::SeqCst);
        if continued && mode == 2 {
            response.start(500);
            return;
        }
        response.start(200);
        if mode == 4 {
            push(
                &response,
                json!({"choices":[{"delta":{"content":"Text-only reply."},"finish_reason":"stop"}]}),
            );
            if let Some(gate) = self.0.gate.lock().unwrap().take() {
                let _ = gate.recv();
            }
            response.push(b"data: [DONE]\n\n".to_vec());
            return;
        }
        if continued {
            match protocol {
                "responses" => {
                    push(
                        &response,
                        json!({"type":"response.reasoning_summary_text.delta","delta":"Tool returned."}),
                    );
                    push(
                        &response,
                        json!({"type":"response.output_text.delta","delta":"Play now."}),
                    );
                    push(
                        &response,
                        json!({"type":"response.completed","response":{}}),
                    );
                }
                "anthropic" => {
                    push(
                        &response,
                        json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Tool returned."}}),
                    );
                    push(
                        &response,
                        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Play now."}}),
                    );
                    push(&response, json!({"type":"message_stop"}));
                }
                "gemini" => push(
                    &response,
                    json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"Tool returned."},{"text":"Play now."}]},"finishReason":"STOP"}]}),
                ),
                _ => {
                    push(
                        &response,
                        json!({"choices":[{"delta":{"reasoning_content":"Tool returned."}}]}),
                    );
                    push(
                        &response,
                        json!({"choices":[{"delta":{"content":"Play now."},"finish_reason":"stop"}]}),
                    );
                    response.push(b"data: [DONE]\n\n".to_vec());
                }
            }
            return;
        }
        let mut args = definition();
        if mode == 1 {
            args["html"] = json!("");
        }
        let raw = args.to_string();
        let before = if mode == 3 { "" } else { "Before tool." };
        match protocol {
            "responses" => {
                push(
                    &response,
                    json!({"type":"response.reasoning_summary_text.delta","delta":"Plan 你好."}),
                );
                push(
                    &response,
                    json!({"type":"response.output_text.delta","delta":before}),
                );
                push(
                    &response,
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","encrypted_content":"OPAQUE_SIGNATURE"}}),
                );
                push(
                    &response,
                    json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"create_interactive_game","arguments":""}}),
                );
                push(
                    &response,
                    json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":raw}),
                );
            }
            "anthropic" => {
                push(
                    &response,
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Plan 你好."}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"OPAQUE_SIGNATURE"}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":before}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call-1","name":"create_interactive_game","input":{}}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":raw}}),
                );
            }
            "gemini" => {
                push(
                    &response,
                    json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"Plan 你好."},{"text":before},{"functionCall":{"name":"create_interactive_game","args":args},"thoughtSignature":"OPAQUE_SIGNATURE"}]}}]}),
                );
            }
            _ => {
                push(
                    &response,
                    json!({"choices":[{"delta":{"reasoning_content":"Plan 你好.","reasoning_details":[{"type":"reasoning.encrypted","data":"OPAQUE_SIGNATURE"}]}}]}),
                );
                push(&response, json!({"choices":[{"delta":{"content":before}}]}));
                push(
                    &response,
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"create_interactive_game","arguments":raw}}]}}]}),
                );
            }
        }
        if let Some(gate) = self.0.gate.lock().unwrap().take() {
            let _ = gate.recv();
        }
        if response.is_cancelled() {
            return;
        }
        match protocol {
            "responses" => push(
                &response,
                json!({"type":"response.completed","response":{}}),
            ),
            "anthropic" => push(&response, json!({"type":"message_stop"})),
            "gemini" => push(&response, json!({"candidates":[{"finishReason":"STOP"}]})),
            _ => {
                push(
                    &response,
                    json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                );
                response.push(b"data: [DONE]\n\n".to_vec());
            }
        }
    }
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "govue-activity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
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
    let reply = call(root, command);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn poll(root: &Path, task: &Value, ready: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":task}));
        if ready(&state) {
            return state;
        }
        thread::sleep(Duration::from_millis(3));
    }
    panic!("Activity request timed out")
}
fn provider(root: &Path, protocol: &str) -> Value {
    let cfg = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":protocol,"protocol":protocol,"baseUrl":"http://127.0.0.1:12345/v1","model":"fixture","models":[{"id":"fixture","name":"Fixture"}]},"key":"PRIVATE_KEY"}),
    );
    cfg["providers"].as_array().unwrap().last().unwrap()["id"].clone()
}
fn saved(root: &Path, started: &Value) -> Value {
    ok(
        root,
        json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
    )
}

fn text_only_steering(root: &Path, backend: &Backend) {
    backend.mode.store(4, Ordering::SeqCst);
    let id = provider(root, "chat");
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"A question"}),
    );
    poll(root, &started["taskId"], |s| {
        s["text"] == "Text-only reply."
    });
    ok(
        root,
        json!({"op":"ai_chat_send","chatId":started["conversation"]["id"],"prompt":"A follow-up"}),
    );
    release.send(()).unwrap();
    assert_eq!(
        poll(root, &started["taskId"], |s| s["status"] != "running")["status"],
        "completed"
    );
    let chat = saved(root, &started);
    assert_eq!(chat["messages"].as_array().unwrap().len(), 4);
    assert_eq!(chat["messages"][2]["content"], "A follow-up");
    assert_eq!(chat["messages"][3]["content"], "Text-only reply.");
    assert!(chat.get("pendingMessages").is_none());
    backend.mode.store(0, Ordering::SeqCst);
}

fn queued_steering(root: &Path, backend: &Backend) {
    let prompt = "Use Spanish.\n\nKeep it short.";
    // A tool-only segment must also survive normalized history and provider replay.
    backend.mode.store(3, Ordering::SeqCst);
    for protocol in ["chat", "responses", "anthropic", "gemini"] {
        backend.requests.lock().unwrap().clear();
        let id = provider(root, protocol);
        let (release, gate) = mpsc::channel();
        *backend.gate.lock().unwrap() = Some(gate);
        let started = ok(
            root,
            json!({"op":"ai_chat_send","id":id,"prompt":"Create an exercise"}),
        );
        poll(root, &started["taskId"], |s| {
            s["blocks"].as_array().is_some_and(|b| {
                b.iter()
                    .any(|b| b["kind"] == "tool" && b["status"] == "preparing")
            })
        });
        for content in ["Use Spanish.", "Keep it short."] {
            let queued = ok(
                root,
                json!({"op":"ai_chat_send","chatId":started["conversation"]["id"],"prompt":content}),
            );
            assert_eq!(queued["taskId"], started["taskId"]);
            assert_eq!(
                queued["conversation"]["messages"].as_array().unwrap().len(),
                2
            );
        }
        let pending = saved(root, &started);
        assert_eq!(pending["pendingMessages"].as_array().unwrap().len(), 2);
        assert_eq!(
            backend.requests.lock().unwrap().len(),
            1,
            "queueing cannot start another model request"
        );
        let projected = ok(
            root,
            json!({"op":"ai_chat_poll","taskId":started["taskId"]}),
        );
        assert_eq!(
            projected["state"]["pendingMessages"],
            pending["pendingMessages"]
        );
        release.send(()).unwrap();
        let done = poll(root, &started["taskId"], |s| s["status"] != "running");
        assert_eq!(done["status"], "completed", "{protocol}: {done}");
        let chat = saved(root, &started);
        assert_eq!(chat["messages"].as_array().unwrap().len(), 4);
        assert_eq!(chat["messages"][1]["content"], "");
        assert_eq!(chat["messages"][1]["status"], "completed");
        assert_eq!(chat["messages"][1]["games"].as_array().unwrap().len(), 1);
        assert_eq!(chat["messages"][2]["content"], prompt);
        assert_eq!(chat["messages"][3]["content"], "Play now.");
        assert_eq!(done["replyId"], chat["messages"][3]["id"]);
        assert_eq!(done["pendingMessages"], json!([]));
        assert!(chat["messages"][3].get("games").is_none());
        assert!(!chat.to_string().contains("OPAQUE_SIGNATURE"));
        {
            let requests = backend.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            let field = match protocol {
                "responses" => "input",
                "gemini" => "contents",
                _ => "messages",
            };
            let last = requests[1][field]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .clone();
            drop(requests);
            assert_eq!(last["role"], "user");
            match protocol {
                "anthropic" => {
                    assert_eq!(last["content"][0]["type"], "tool_result");
                    assert_eq!(
                        last["content"].as_array().unwrap().last().unwrap()["text"],
                        prompt
                    );
                }
                "gemini" => {
                    assert!(last["parts"][0]["functionResponse"].is_object());
                    assert_eq!(
                        last["parts"].as_array().unwrap().last().unwrap()["text"],
                        prompt
                    );
                }
                _ => assert_eq!(last["content"], prompt),
            }
        }
        let next = ok(
            root,
            json!({"op":"ai_chat_send","chatId":chat["id"],"prompt":"Another question"}),
        );
        assert_eq!(
            poll(root, &next["taskId"], |s| s["status"] != "running")["status"],
            "completed"
        );
    }
    backend.mode.store(0, Ordering::SeqCst);
}

#[test]
fn ordered_activity_streams_and_persists_across_protocols_failures_and_cancellation() {
    let backend = Arc::new(Backend::default());
    govue_core::install_ai_platform(Box::new(Platform(backend.clone())));
    let temp = Temp::new();
    let root = &temp.0;
    let mut last = Value::Null;
    for protocol in ["chat", "responses", "anthropic", "gemini"] {
        let id = provider(root, protocol);
        let (release, gate) = mpsc::channel();
        *backend.gate.lock().unwrap() = Some(gate);
        let started = ok(
            root,
            json!({"op":"ai_chat_send","id":id,"prompt":"Create an exercise"}),
        );
        let state = poll(root, &started["taskId"], |s| {
            s["blocks"].as_array().is_some_and(|b| b.len() == 3)
        });
        assert_eq!(state["blocks"][0]["content"], "Plan 你好.");
        assert_eq!(state["blocks"][0]["status"], "completed");
        assert_eq!(state["blocks"][1]["kind"], "text");
        assert_eq!(state["blocks"][2]["status"], "preparing");
        assert!(!state["blocks"].to_string().contains("PRIVATE_GAME_SOURCE"));
        assert!(!state["blocks"].to_string().contains("OPAQUE_SIGNATURE"));
        let ids = state["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].clone())
            .collect::<Vec<_>>();
        release.send(()).unwrap();
        let done = poll(root, &started["taskId"], |s| s["status"] != "running");
        assert_eq!(done["status"], "completed", "{protocol}: {done}");
        let chat = saved(root, &started);
        let reply = &chat["messages"][1];
        let blocks = reply["blocks"].as_array().unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|b| b["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["reasoning", "text", "tool", "reasoning", "text"],
            "{protocol}"
        );
        assert_eq!(
            blocks[..3]
                .iter()
                .map(|b| b["id"].clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(blocks.iter().all(|b| b["status"] == "completed"));
        assert_eq!(blocks[2]["gameId"], reply["games"][0]["id"]);
        assert_eq!(blocks[2]["title"], "你好 practice");
        assert_eq!(reply["content"], "Before tool.\n\nPlay now.");
        assert_eq!(done["blocks"], reply["blocks"]);
        assert!(!chat.to_string().contains("OPAQUE_SIGNATURE"));
        last = started;
    }
    queued_steering(root, &backend);
    text_only_steering(root, &backend);
    let id = provider(root, "chat");
    for mode in [1, 2] {
        backend.mode.store(mode, Ordering::SeqCst);
        let started = ok(
            root,
            json!({"op":"ai_chat_send","id":id,"prompt":"Exercise"}),
        );
        poll(root, &started["taskId"], |s| s["status"] != "running");
        let chat = saved(root, &started);
        let reply = &chat["messages"][1];
        assert_eq!(
            reply["blocks"][2]["status"],
            if mode == 1 { "failed" } else { "completed" }
        );
        assert_eq!(
            reply["status"],
            if mode == 1 { "completed" } else { "failed" }
        );
        if mode == 1 {
            assert!(reply["blocks"][2]["error"].is_string());
            assert!(reply.get("games").is_none());
        } else {
            assert_eq!(reply["blocks"][2]["gameId"], reply["games"][0]["id"]);
        }
    }
    backend.mode.store(0, Ordering::SeqCst);
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"Stop exercise"}),
    );
    poll(root, &started["taskId"], |s| {
        s["blocks"][2]["status"] == "preparing"
    });
    ok(
        root,
        json!({"op":"ai_chat_send","chatId":started["conversation"]["id"],"prompt":"Keep this instruction"}),
    );
    let cancelled = ok(root, json!({"op":"ai_cancel","taskId":started["taskId"]}));
    assert_eq!(
        cancelled["conversation"]["pendingMessages"][0]["content"],
        "Keep this instruction"
    );
    let stopped = saved(root, &started);
    assert_eq!(stopped["messages"][1]["status"], "stopped");
    assert_eq!(stopped["messages"][1]["blocks"][2]["status"], "stopped");
    assert_eq!(
        ok(root, json!({"op":"ai_poll","taskId":started["taskId"]}))["blocks"][2]["status"],
        "stopped"
    );
    release.send(()).unwrap();
    let queued = &stopped["pendingMessages"][0];
    let resent = ok(
        root,
        json!({"op":"ai_chat_send","chatId":stopped["id"],"prompt":queued["content"],"queuedMessageId":queued["id"]}),
    );
    assert_eq!(
        poll(root, &resent["taskId"], |s| s["status"] != "running")["status"],
        "completed"
    );
    assert!(saved(root, &resent).get("pendingMessages").is_none());

    // Chats saved before activity existed still load; orphaned pending tools stop on recovery.
    let mut chat = saved(root, &last);
    let path = root
        .join("ai/chats")
        .join(format!("{}.json", chat["id"].as_str().unwrap()));
    chat["activeTask"] = json!("missing-task");
    chat["messages"][1]["status"] = json!("streaming");
    chat["messages"][1]["blocks"][2]["status"] = json!("running");
    fs::write(&path, chat.to_string()).unwrap();
    assert_eq!(
        saved(root, &last)["messages"][1]["blocks"][2]["status"],
        "stopped"
    );
    chat["activeTask"] = Value::Null;
    chat["messages"][1]["status"] = json!("completed");
    chat["messages"][1]
        .as_object_mut()
        .unwrap()
        .remove("blocks");
    fs::write(&path, chat.to_string()).unwrap();
    assert!(saved(root, &last)["messages"][1].get("blocks").is_none());
    chat["messages"][1]["blocks"] = json!([{"id":"invalid","kind":"tool","callId":"call","name":"tool","status":"invented","startedAt":0,"durationMs":0}]);
    fs::write(&path, chat.to_string()).unwrap();
    assert_eq!(
        call(root, json!({"op":"ai_chat_get","chatId":chat["id"]}))["ok"],
        false
    );
}
