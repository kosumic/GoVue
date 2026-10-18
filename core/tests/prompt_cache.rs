//! Multi-turn wire histories and provider-reported cache usage.
use lingoplay_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, ai_execute, execute, install_ai_platform,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<Value>>,
    headers: Mutex<Vec<Value>>,
}
struct Platform(Arc<Backend>);
fn push(response: &AiResponse, value: Value) {
    assert!(response.push(format!("data: {value}\n\n").into_bytes()));
}
fn field(protocol: &str) -> &str {
    match protocol {
        "responses" => "input",
        "gemini" => "contents",
        _ => "messages",
    }
}
fn user_text(row: &Value) -> Option<&str> {
    if row["role"] != "user" {
        return None;
    }
    row["content"]
        .as_str()
        .or_else(|| row["parts"][0]["text"].as_str())
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
        let protocol = if request.url.ends_with("/responses") {
            "responses"
        } else if request.url.ends_with("/messages") {
            "anthropic"
        } else if request.url.contains("streamGenerateContent") {
            "gemini"
        } else {
            "chat"
        };
        let rows = body[field(protocol)].as_array().unwrap();
        let question = rows.iter().rev().find_map(user_text).unwrap();
        let last = rows.last().unwrap();
        let continued = last["type"] == "function_call_output"
            || last["role"] == "tool"
            || last["content"][0]["type"] == "tool_result"
            || last["parts"][0]["functionResponse"].is_object();
        let tool = question.ends_with("exercise") && !continued;
        let missing = question.contains("unknown") && !continued;
        let cached = if question.contains("zero") { 0 } else { 40 };
        self.0.requests.lock().unwrap().push(body.clone());
        self.0.headers.lock().unwrap().push(json!(
            request
                .headers
                .iter()
                .map(|header| (header.name.clone(), json!(header.value)))
                .collect::<serde_json::Map<_, _>>()
        ));
        response.start(200);
        let definition = json!({"title":"Practice","html":"<button>Hi</button>","css":"","script":"",
            "rules":json!({"initialState":{},"actions":[{"name":"choose","updates":[]}]}).to_string(),"cueIds":[]});
        match protocol {
            "responses" => {
                let mut usage = json!({"input_tokens":100,"output_tokens":20});
                if !missing {
                    usage["input_tokens_details"] = json!({"cached_tokens":cached});
                }
                let item = if tool {
                    json!({"id":"fc_fixture","type":"function_call","call_id":"call_fixture","name":"create_interactive_game","arguments":definition.to_string()})
                } else {
                    json!({"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Answer","annotations":[]}]})
                };
                let reasoning = json!({"id":"rs_fixture","type":"reasoning","summary":[],"encrypted_content":"PRIVATE_SIGNATURE"});
                if !tool {
                    push(
                        &response,
                        json!({"type":"response.output_text.delta","delta":"Answer"}),
                    );
                }
                push(
                    &response,
                    json!({"type":"response.completed","response":{"output":[reasoning,item],"usage":usage}}),
                );
            }
            "anthropic" => {
                let mut usage = json!({"input_tokens":60,"cache_creation_input_tokens":0});
                if missing {
                    usage["input_tokens"] = json!(100);
                } else {
                    usage["cache_read_input_tokens"] = json!(cached);
                    usage["input_tokens"] = json!(100 - cached);
                }
                push(
                    &response,
                    json!({"type":"message_start","message":{"usage":usage}}),
                );
                push(
                    &response,
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"Plan","signature":"PRIVATE_SIGNATURE"}}),
                );
                if tool {
                    push(
                        &response,
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_fixture","name":"create_interactive_game","input":definition}}),
                    );
                } else {
                    push(
                        &response,
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
                    );
                    push(
                        &response,
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Answer"}}),
                    );
                }
                push(
                    &response,
                    json!({"type":"message_delta","usage":{"output_tokens":20}}),
                );
                push(&response, json!({"type":"message_stop"}));
            }
            "gemini" => {
                let mut usage = json!({"promptTokenCount":100,"candidatesTokenCount":20});
                if !missing {
                    usage["cachedContentTokenCount"] = json!(cached);
                }
                let part = if tool {
                    json!({"functionCall":{"name":"create_interactive_game","args":definition},"thoughtSignature":"PRIVATE_SIGNATURE"})
                } else {
                    json!({"text":"Answer","thoughtSignature":"PRIVATE_SIGNATURE"})
                };
                push(
                    &response,
                    json!({"candidates":[{"content":{"parts":[part]},"finishReason":"STOP"}],"usageMetadata":usage}),
                );
            }
            _ => {
                let mut usage = json!({"prompt_tokens":100,"completion_tokens":20});
                if !missing {
                    usage["prompt_tokens_details"] = json!({"cached_tokens":cached});
                }
                let delta = if tool {
                    json!({"tool_calls":[{"index":0,"id":"call_fixture","function":{"name":"create_interactive_game","arguments":definition.to_string()}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"PRIVATE_SIGNATURE"}]})
                } else {
                    json!({"content":"Answer"})
                };
                push(
                    &response,
                    json!({"choices":[{"delta":delta,"finish_reason":if tool {"tool_calls"} else {"stop"}}],"usage":usage}),
                );
                assert!(response.push(b"data: [DONE]\n\n".to_vec()));
            }
        }
    }
}
fn ok(root: &Path, command: Value) -> Value {
    let result: Value = serde_json::from_str(&ai_execute(
        command.to_string(),
        root.to_string_lossy().into_owned(),
    ))
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn send(root: &Path, command: Value) -> Value {
    let started = ok(root, command);
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":started["taskId"]}));
        if state["status"] != "running" {
            assert_eq!(state["status"], "completed", "{state}");
            ok(root, json!({"op":"ai_forget","taskId":started["taskId"]}));
            return ok(
                root,
                json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("Prompt cache fixture timed out");
}
fn transcript(root: &Path, text: &str) {
    let raw = json!({"type":"transcript","requestId":"cache:fixture","videoId":"abcdefghijk","ok":true,"transcript":{
        "videoId":"abcdefghijk","title":"Practice reference","trackId":".en","language":"English",
        "tracks":[{"id":".en","label":"English","language":"en","automatic":false}],"segments":[{"start":1,"duration":2,"text":text}]
    }});
    let result: Value = serde_json::from_str(&execute(json!({"op":"accept","videoId":"abcdefghijk","requestId":"cache:fixture","origin":"https://www.youtube.com","raw":raw.to_string()}).to_string(),root.to_string_lossy().into_owned())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
#[test]
fn requests_preserve_prefixes_tools_and_unknown_cache_counts() {
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    let directory = std::env::temp_dir().join(format!(
        "lingoplay-prompt-cache-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    for protocol in ["responses", "chat", "anthropic", "gemini"] {
        let root = directory.join(protocol);
        fs::create_dir_all(&root).unwrap();
        let config = ok(
            &root,
            json!({"op":"ai_save","key":"fixture-key","provider":{"id":"","label":"Fixture","kind":"custom","baseUrl":"https://fixture.example","protocol":protocol,"model":"fixture-model"}}),
        );
        let id = config["providers"][0]["id"].clone();
        transcript(&root, "Original caption context.");
        let first = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"prompt":"first","transcript":{"videoId":"abcdefghijk","trackId":".en"}}),
        );
        let chat_id = first["id"].clone();
        let first_request = backend.requests.lock().unwrap().last().unwrap().clone();
        let initial_rows = first_request[field(protocol)].as_array().unwrap().clone();
        let second = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":"follow-up"}),
        );
        let second_request = backend.requests.lock().unwrap().last().unwrap().clone();
        let second_rows = second_request[field(protocol)].as_array().unwrap();
        assert_eq!(
            &second_rows[..initial_rows.len()],
            initial_rows,
            "{protocol}: preserve the entire initial request"
        );
        assert!(
            !user_text(second_rows.last().unwrap())
                .unwrap()
                .contains("<transcript>"),
            "{protocol}: unchanged captions are not duplicated"
        );
        if protocol == "responses" {
            assert_eq!(first_request["prompt_cache_key"], chat_id);
            assert_eq!(second_request["prompt_cache_key"], chat_id);
        }
        assert!(
            second["messages"][0].get("preparedInput").is_none(),
            "request context stays off the UI bridge"
        );

        let game = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":"exercise"}),
        );
        assert_eq!(game["messages"][5]["games"].as_array().unwrap().len(), 1);
        let tool_request = backend.requests.lock().unwrap().last().unwrap().clone();
        send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":"follow game"}),
        );
        let next_request = backend.requests.lock().unwrap().last().unwrap().clone();
        let tool_rows = tool_request[field(protocol)].as_array().unwrap();
        assert_eq!(
            &next_request[field(protocol)].as_array().unwrap()[..tool_rows.len()],
            tool_rows,
            "{protocol}: tool calls/results are replayed exactly"
        );
        assert!(
            next_request.to_string().contains("PRIVATE_SIGNATURE"),
            "{protocol}: signed continuation survives in memory"
        );
        let path = root.join(format!("ai/chats/{}.json", chat_id.as_str().unwrap()));
        let disk = fs::read_to_string(&path).unwrap();
        assert!(
            !disk.contains("PRIVATE_SIGNATURE"),
            "{protocol}: provider signatures never persist"
        );
        let saved: Value = serde_json::from_str(&disk).unwrap();
        assert!(
            saved["messages"][0]["preparedInput"]["content"]
                .as_str()
                .unwrap()
                .contains("Original caption context.")
        );

        transcript(&root, "Updated caption context.");
        send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":"refresh"}),
        );
        let refreshed = backend.requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(
            &refreshed[field(protocol)].as_array().unwrap()[..initial_rows.len()],
            initial_rows
        );
        assert!(
            user_text(
                refreshed[field(protocol)]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()
            )
            .unwrap()
            .contains("Updated caption context.")
        );

        send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"retry":true,"retryMessageId":first["messages"][1]["id"]}),
        );
        let retried = backend.requests.lock().unwrap().last().unwrap().clone();
        assert!(
            !retried.to_string().contains("follow game"),
            "{protocol}: regeneration cannot replay abandoned future turns"
        );
        assert!(!retried.to_string().contains("PRIVATE_SIGNATURE"));
        let mut provider = config["providers"][0].clone();
        provider.as_object_mut().unwrap().retain(|key, _| {
            [
                "id",
                "label",
                "kind",
                "baseUrl",
                "protocol",
                "model",
                "enabled",
                "auth",
                "azure",
                "appendV1Suffix",
                "customUserAgent",
                "models",
            ]
            .contains(&key.as_str())
        });
        ok(
            &root,
            json!({"op":"ai_save","provider":provider,"key":"replacement-key"}),
        );
        send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":"after reconnect"}),
        );
        assert!(
            !backend
                .requests
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .to_string()
                .contains("PRIVATE_SIGNATURE"),
            "{protocol}: reconnect clears signed replay data"
        );

        let unknown = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"prompt":"unknown cache"}),
        );
        assert!(unknown["messages"][1]["usage"]["cacheReadTokens"].is_null());
        assert_eq!(
            unknown["messages"][1]["usageDetails"]["cacheStatus"],
            "unknown"
        );
        let hit = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"chatId":unknown["id"],"prompt":"hit cache"}),
        );
        assert_eq!(hit["messages"][3]["usageDetails"]["cacheHitRate"], 40.0);
        assert_eq!(
            ok(&root, json!({"op":"ai_usage","chatId":unknown["id"]}))["stats"]["cacheHitRate"],
            40.0,
            "{protocol}: unknown samples cannot dilute the rate"
        );
        let zero = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"prompt":"zero cache"}),
        );
        assert_eq!(zero["messages"][1]["usageDetails"]["cacheStatus"], "miss");
        assert_eq!(
            ok(&root, json!({"op":"ai_usage","chatId":zero["id"]}))["stats"]["cacheHitRate"],
            0.0
        );
        let partial = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"prompt":"unknown exercise"}),
        );
        assert_eq!(partial["messages"][1]["usage"]["inputTokens"], 200);
        assert!(partial["messages"][1]["usage"]["cacheReadTokens"].is_null());
        assert_eq!(
            partial["messages"][1]["usageDetails"]["cacheStatus"], "unknown",
            "{protocol}: every tool round must report its cache count"
        );
    }
    for kind in ["chatgpt", "codex"] {
        let root = directory.join(kind);
        fs::create_dir_all(&root).unwrap();
        let config = ok(
            &root,
            json!({"op":"ai_save","provider":{"id":"","label":"Account fixture","kind":kind,"model":"fixture-model"}}),
        );
        let id = config["providers"][0]["id"].as_str().unwrap();
        backend.secrets.lock().unwrap().insert(
            format!("ai.{id}"),
            json!({"access_token":"fixture-access","expires_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+3600,"scope":"chatgpt.tokens.use.direct"}).to_string(),
        );
        let first = send(&root, json!({"op":"ai_chat_send","id":id,"prompt":"first"}));
        let chat_id = &first["id"];
        for prompt in ["follow-up", "exercise", "after game"] {
            send(
                &root,
                json!({"op":"ai_chat_send","id":id,"chatId":chat_id,"prompt":prompt}),
            );
            let requests = backend.requests.lock().unwrap();
            let headers = backend.headers.lock().unwrap();
            for (body, headers) in requests.iter().zip(headers.iter()) {
                if body["prompt_cache_key"] == *chat_id {
                    assert_eq!(
                        headers["session-id"], *chat_id,
                        "{kind}: stable cache affinity on every turn and tool round"
                    );
                    assert_eq!(headers["thread-id"], *chat_id);
                }
            }
        }
        let other = send(
            &root,
            json!({"op":"ai_chat_send","id":id,"prompt":"new chat"}),
        );
        assert_ne!(other["id"], *chat_id);
        let headers = backend.headers.lock().unwrap().last().unwrap().clone();
        assert_eq!(headers["session-id"], other["id"]);
    }
    fs::remove_dir_all(directory).unwrap();
}
