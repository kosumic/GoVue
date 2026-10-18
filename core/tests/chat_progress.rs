#![cfg(test)]

//! Live plans, provider phases, and recovery use the same persisted activity tape.
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
        atomic::{AtomicU8, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    round: AtomicUsize,
    mode: AtomicU8,
    bad: Mutex<Value>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}
struct Platform(Arc<Backend>);
fn push(response: &AiResponse, value: Value) {
    for bytes in format!("data: {value}\n\n").as_bytes().chunks(7) {
        if !response.push(bytes.to_vec()) {
            break;
        }
    }
}
fn text(response: &AiResponse, protocol: &str, round: usize, content: &str, phase: &str) {
    match protocol {
        "responses" => {
            let item = json!({"type":"message","role":"assistant","id":format!("message-{round}"),"phase":phase,"content":[{"type":"output_text","text":content}]});
            push(
                response,
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","role":"assistant","id":item["id"],"phase":phase,"content":[]}}),
            );
            push(
                response,
                json!({"type":"response.output_text.delta","output_index":0,"item_id":item["id"],"delta":content}),
            );
            push(
                response,
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
            );
        }
        "anthropic" => {
            push(
                response,
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            );
            push(
                response,
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":content}}),
            );
        }
        "gemini" => push(
            response,
            json!({"candidates":[{"content":{"parts":[{"text":content}]}}]}),
        ),
        _ => push(response, json!({"choices":[{"delta":{"content":content}}]})),
    }
}
fn tool(response: &AiResponse, protocol: &str, round: usize, name: &str, args: Value) {
    let id = format!("call-{round}");
    let raw = args.to_string();
    match protocol {
        "responses" => {
            let item = json!({"type":"function_call","id":format!("function-{round}"),"call_id":id,"name":name,"arguments":raw});
            push(
                response,
                json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":item["id"],"call_id":id,"name":name,"arguments":""}}),
            );
            push(
                response,
                json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":raw}),
            );
            push(
                response,
                json!({"type":"response.output_item.done","output_index":1,"item":item}),
            );
        }
        "anthropic" => {
            push(
                response,
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}}),
            );
            push(
                response,
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":raw}}),
            );
        }
        "gemini" => push(
            response,
            json!({"candidates":[{"content":{"parts":[{"functionCall":{"id":id,"name":name,"args":args}}]}}]}),
        ),
        _ => push(
            response,
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":raw}}]}}]}),
        ),
    }
}
fn finish(response: &AiResponse, protocol: &str) {
    match protocol {
        "responses" => push(response, json!({"type":"response.completed","response":{}})),
        "anthropic" => push(response, json!({"type":"message_stop"})),
        "gemini" => push(response, json!({"candidates":[{"finishReason":"STOP"}]})),
        _ => {
            push(
                response,
                json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
            );
            response.push(b"data: [DONE]\n\n".to_vec());
        }
    }
}
fn plan(done: bool) -> Value {
    json!({"explanation":"A focused greeting exercise.","plan":[
        {"step":"Choose an exercise","status":"completed"},
        {"step":"Build the game","status":if done {"completed"} else {"in_progress"}}
    ]})
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
        let protocol = if request.url.ends_with("/responses") {
            "responses"
        } else if request.url.ends_with("/messages") {
            "anthropic"
        } else if request.url.contains("streamGenerateContent") {
            "gemini"
        } else {
            "chat"
        };
        let round = self.0.round.fetch_add(1, Ordering::SeqCst);
        let mode = self.0.mode.load(Ordering::SeqCst);
        response.start(200);
        if mode == 4 || (protocol == "responses" && round < 2) {
            text(
                &response,
                protocol,
                round,
                "I’ll prepare a greeting game. 你好!",
                "commentary",
            );
            finish(&response, protocol);
            return;
        }
        let mut stage = round - if protocol == "responses" { 2 } else { 0 };
        if mode == 1 && stage == 0 {
            tool(
                &response,
                protocol,
                round,
                "update_plan",
                self.0.bad.lock().unwrap().clone(),
            );
            finish(&response, protocol);
            return;
        }
        if mode == 1 {
            stage -= 1;
        }
        if stage == 1 {
            let gate = self.0.gate.lock().unwrap().take();
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            if response.is_cancelled() {
                return;
            }
            if mode == 3 {
                response.start(503);
                return;
            }
        }
        match stage {
            0 => {
                text(
                    &response,
                    protocol,
                    round,
                    "I’ve chosen a greeting exercise.",
                    "commentary",
                );
                tool(&response, protocol, round, "update_plan", plan(false));
            }
            1 => tool(
                &response,
                protocol,
                round,
                "create_interactive_game",
                json!({"title":"Greetings","html":"<button>Hello</button>","css":"","script":"game.onState(()=>{});","rules":json!({"initialState":{},"actions":[{"name":"choose","updates":[]}]}).to_string(),"cueIds":[]}),
            ),
            2 if mode != 2 => tool(&response, protocol, round, "update_plan", plan(true)),
            _ => text(
                &response,
                protocol,
                round,
                "Your game is ready.",
                "final_answer",
            ),
        }
        finish(&response, protocol);
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "govue-progress-{}-{}",
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
fn ok(root: &Path, command: Value) -> Value {
    let reply: Value = serde_json::from_str(&ai_execute(
        command.to_string(),
        root.to_str().unwrap().into(),
    ))
    .unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn poll(root: &Path, task: &Value, ready: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..1500 {
        let state = ok(root, json!({"op":"ai_poll","taskId":task}));
        if ready(&state) {
            return state;
        }
        thread::sleep(Duration::from_millis(3));
    }
    let state = ok(root, json!({"op":"ai_poll","taskId":task}));
    panic!(
        "Progress request did not finish: status={}, blocks={}",
        state["status"], state["blocks"]
    )
}
fn start(root: &Path, backend: &Backend, provider: &Value, mode: u8) -> Value {
    backend.round.store(0, Ordering::SeqCst);
    backend.mode.store(mode, Ordering::SeqCst);
    backend.requests.lock().unwrap().clear();
    ok(
        root,
        json!({"op":"ai_chat_send","id":provider,"prompt":"Create a greeting game"}),
    )
}
fn saved(root: &Path, started: &Value) -> Value {
    ok(
        root,
        json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
    )["messages"][1]
        .clone()
}
fn checklist(reply: &Value) -> &Value {
    let plans: Vec<_> = reply["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["kind"] == "plan")
        .collect();
    assert_eq!(plans.len(), 1, "one plan updates in place");
    plans[0]
}

#[test]
fn plans_stream_persist_recover_and_remain_bounded_across_providers() {
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    let temp = Temp::new();
    let root = &temp.0;
    let mut responses = Value::Null;
    for protocol in ["responses", "chat", "anthropic", "gemini"] {
        let config = ok(
            root,
            json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":protocol,"protocol":protocol,"baseUrl":"http://127.0.0.1:12345/v1","model":"fixture","models":[{"id":"fixture","name":"Fixture"}]},"key":"fixture-only"}),
        );
        let provider = config["providers"].as_array().unwrap().last().unwrap()["id"].clone();
        if protocol == "responses" {
            responses = provider.clone();
        }
        let (release, gate) = mpsc::channel();
        *backend.gate.lock().unwrap() = Some(gate);
        let started = start(root, &backend, &provider, 0);
        let running = poll(root, &started["taskId"], |state| {
            state["blocks"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|b| b["kind"] == "plan"))
        });
        let current = checklist(&running);
        assert_eq!(current["status"], "running");
        assert_eq!(current["steps"][1]["status"], "in_progress");
        let plan_id = current["id"].clone();
        release.send(()).unwrap();
        let done = poll(root, &started["taskId"], |state| {
            state["status"] != "running"
        });
        assert_eq!(done["status"], "completed", "{protocol}: {done}");
        let reply = saved(root, &started);
        assert_eq!(reply["games"].as_array().unwrap().len(), 1);
        assert_eq!(checklist(&reply)["id"], plan_id);
        assert_eq!(checklist(&reply)["status"], "completed");
        assert_eq!(checklist(&reply)["steps"][1]["status"], "completed");
        assert_eq!(done["blocks"], reply["blocks"]);
        let requests = backend.requests.lock().unwrap().clone();
        for body in &requests {
            assert!(body.to_string().contains("Keep the user informed"));
            let tools = if protocol == "gemini" {
                &body["tools"][0]["functionDeclarations"]
            } else {
                &body["tools"]
            };
            let plan_tool = tools
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| {
                    tool["name"] == "update_plan" || tool["function"]["name"] == "update_plan"
                })
                .unwrap();
            if protocol == "gemini" {
                assert!(!plan_tool.to_string().contains("additionalProperties"));
            }
        }
        if protocol == "responses" {
            assert_eq!(
                requests.len(),
                6,
                "preambles and plans leave room for real work"
            );
            assert_eq!(
                requests[1]["input"].as_array().unwrap().last().unwrap()["phase"],
                "commentary"
            );
            assert_eq!(
                reply["blocks"].as_array().unwrap().last().unwrap()["phase"],
                "final_answer"
            );
        }
    }
    for bad in [
        json!({"plan":[{"step":"Invented status","status":"finished"}]}),
        json!({"plan":[{"step":"A","status":"in_progress"},{"step":"B","status":"in_progress"}]}),
        json!({"plan":vec![json!({"step":"Too many","status":"pending"});9]}),
        json!({"plan":[{"step":"Large","status":"pending"}],"explanation":"x".repeat(7000)}),
    ] {
        *backend.bad.lock().unwrap() = bad;
        let started = start(root, &backend, &responses, 1);
        assert_eq!(
            poll(root, &started["taskId"], |s| s["status"] != "running")["status"],
            "completed"
        );
        let reply = saved(root, &started);
        assert_eq!(checklist(&reply)["status"], "completed");
        assert!(
            reply["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["kind"] == "tool"
                    && b["name"] == "update_plan"
                    && b["status"] == "failed"
                    && b["error"].is_string())
        );
    }
    for (mode, status, plan_status) in [(2, "completed", "stopped"), (3, "failed", "failed")] {
        let started = start(root, &backend, &responses, mode);
        assert_eq!(
            poll(root, &started["taskId"], |s| s["status"] != "running")["status"],
            status
        );
        let reply = saved(root, &started);
        assert_eq!(checklist(&reply)["status"], plan_status);
        assert_eq!(checklist(&reply)["steps"][0]["status"], "completed");
        assert_eq!(checklist(&reply)["steps"][1]["status"], "in_progress");
    }
    let (release, gate) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some(gate);
    let started = start(root, &backend, &responses, 0);
    poll(root, &started["taskId"], |s| {
        backend.gate.lock().unwrap().is_none()
            && s["blocks"]
                .as_array()
                .is_some_and(|b| b.iter().any(|b| b["kind"] == "plan"))
    });
    ok(root, json!({"op":"ai_cancel","taskId":started["taskId"]}));
    assert_eq!(checklist(&saved(root, &started))["status"], "stopped");
    release.send(()).unwrap();
    let started = start(root, &backend, &responses, 4);
    assert_eq!(
        poll(root, &started["taskId"], |s| s["status"] != "running")["status"],
        "failed"
    );
    assert_eq!(
        backend.round.load(Ordering::SeqCst),
        12,
        "commentary-only loops are bounded"
    );
}
