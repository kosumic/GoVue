#![cfg(test)]

//! Regressions for generated cards, streamed tools, durable actions and lazy clips.
use govue_core::{
    AiHttpRequest, AiPlatform, AiResponse, AiSecret, MediaHttp, MediaRangeReply, ai_execute,
    execute, install_ai_platform, install_media_http,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Backend {
    secrets: Mutex<HashMap<String, String>>,
    definition: Mutex<Value>,
    tool_name: Mutex<Option<String>>,
    requests: Mutex<Vec<Value>>,
    answer_only: AtomicBool,
    fail_final: AtomicBool,
    repeat: AtomicBool,
    legacy_gemini: AtomicBool,
    response_mode: AtomicUsize,
}
struct Platform(Arc<Backend>);
fn event(value: Value) -> String {
    format!("data: {value}\n\n")
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
        response.start(200);
        let final_reply = self.0.answer_only.load(Ordering::SeqCst)
            || continued && !self.0.repeat.swap(false, Ordering::SeqCst);
        let mut stream = String::new();
        if final_reply {
            match protocol {
                "responses" => {
                    stream += &event(
                        json!({"type":"response.output_text.delta","delta":"Try the game. 你好"}),
                    );
                    if !self.0.fail_final.load(Ordering::SeqCst) {
                        stream += &event(json!({"type":"response.completed","response":{}}));
                    }
                }
                "anthropic" => {
                    stream += &event(
                        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Try the game. 你好"}}),
                    );
                    if !self.0.fail_final.load(Ordering::SeqCst) {
                        stream += &event(json!({"type":"message_stop"}));
                    }
                }
                "gemini" => {
                    stream += &event(
                        json!({"candidates":[{"content":{"parts":[{"text":"Try the game. 你好"}]}}]}),
                    );
                    if !self.0.fail_final.load(Ordering::SeqCst) {
                        stream += &event(json!({"candidates":[{"finishReason":"STOP"}]}));
                    }
                }
                _ => {
                    stream +=
                        &event(json!({"choices":[{"delta":{"content":"Try the game. 你好"}}]}));
                    if !self.0.fail_final.load(Ordering::SeqCst) {
                        stream += &event(json!({"choices":[{"delta":{},"finish_reason":"stop"}]}));
                        stream += "data: [DONE]\n\n";
                    }
                }
            }
        } else {
            let definition = self.0.definition.lock().unwrap().clone();
            let name = self
                .0
                .tool_name
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "create_interactive_game".into());
            let raw = definition.to_string();
            let middle = raw.char_indices().nth(raw.chars().count() / 2).unwrap().0;
            match protocol {
                "responses" => {
                    let mode = self.0.response_mode.load(Ordering::SeqCst);
                    let reasoning = json!({"id":"reasoning-1","type":"reasoning","summary":[],"encrypted_content":"OPAQUE_CONTINUATION"});
                    let mut item = json!({"type":"function_call","id":"fc-1","call_id":"call-1","name":name,"arguments":""});
                    let frame = |mut v: Value| {
                        if mode == 3 {
                            v.as_object_mut().unwrap().remove("output_index");
                        }
                        event(v)
                    };
                    if mode != 4 {
                        stream += &frame(
                            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"reasoning-1","type":"reasoning","summary":[]}}),
                        );
                        stream += &frame(
                            json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
                        );
                        stream += &frame(
                            json!({"type":"response.output_item.added","output_index":1,"item":item}),
                        );
                        for part in [&raw[..middle], &raw[middle..]] {
                            stream += &frame(
                                json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc-1","delta":part}),
                            );
                        }
                    }
                    item["arguments"] = json!(raw);
                    if mode != 4 {
                        stream += &frame(
                            json!({"type":"response.output_item.done","output_index":1,"item":item}),
                        );
                    }
                    let output = match mode {
                        1 | 3 => json!([]),
                        2 => json!([reasoning]),
                        5 => json!([item]),
                        _ => json!([reasoning, item]),
                    };
                    stream +=
                        &event(json!({"type":"response.completed","response":{"output":output}}));
                }
                "anthropic" => {
                    stream += &event(
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                    );
                    stream += &event(
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Make a game."}}),
                    );
                    stream += &event(
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"OPAQUE_CONTINUATION"}}),
                    );
                    stream += &event(
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call-1","name":name,"input":{}}}),
                    );
                    for part in [&raw[..middle], &raw[middle..]] {
                        stream += &event(
                            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":part}}),
                        );
                    }
                    stream +=
                        &event(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}));
                    stream += &event(json!({"type":"message_stop"}));
                }
                "gemini" => {
                    let mut call = json!({"id":"call-1","name":name,"args":definition});
                    if self.0.legacy_gemini.load(Ordering::SeqCst) {
                        call.as_object_mut().unwrap().remove("id");
                    }
                    stream += &event(
                        json!({"candidates":[{"content":{"parts":[{"functionCall":call,"thoughtSignature":"OPAQUE_CONTINUATION"}]},"finishReason":"STOP"}]}),
                    );
                }
                _ => {
                    stream += &event(
                        json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.text","text":"Make a game.","signature":"signed-chunk","id":"reasoning-1","format":"anthropic-claude-v1","index":0}]}}]}),
                    );
                    stream += &event(
                        json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"OPAQUE_CONTINUATION","id":"encrypted-1","format":"anthropic-claude-v1","index":1}]}}]}),
                    );
                    stream += &event(
                        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":&name[..name.len()/2],"arguments":&raw[..middle]}}]}}]}),
                    );
                    stream += &event(
                        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":&name[name.len()/2..],"arguments":&raw[middle..]}}]},"finish_reason":"tool_calls"}]}),
                    );
                    stream += "data: [DONE]\n\n";
                }
            }
        }
        // Includes splits inside multibyte text and escaped tool argument strings.
        for chunk in stream.as_bytes().chunks(7) {
            if !response.push(chunk.to_vec()) {
                break;
            }
        }
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "govue-games-{}-{}",
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
fn call(root: &Path, value: Value) -> Value {
    serde_json::from_str(&ai_execute(
        value.to_string(),
        root.to_str().unwrap().into(),
    ))
    .unwrap()
}
fn ok(root: &Path, value: Value) -> Value {
    let reply = call(root, value);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn wait(root: &Path, started: &Value) -> Value {
    for _ in 0..1000 {
        let state = ok(root, json!({"op":"ai_poll","taskId":started["taskId"]}));
        if state["status"] != "running" {
            return state;
        }
        thread::sleep(Duration::from_millis(3));
    }
    panic!("game request timed out")
}
fn definition() -> Value {
    json!({"title":"Arrange 你好","html":"<button id=choice>你好</button>","css":"button{min-height:44px}","script":"game.onState(state => {});","cueIds":[],"rules":json!({"initialState":{"selected":[],"correct":false,"round":0},"actions":[
        {"name":"choose","when":{"op":"and","args":[{"op":"contains","args":[{"literal":["a","b"]},{"input":"id"}]},{"op":"not","args":[{"op":"contains","args":[{"state":"selected"},{"input":"id"}]}]}]},"updates":[{"key":"selected","value":{"op":"append","args":[{"state":"selected"},{"input":"id"}]}}]},
        {"name":"check","updates":[{"key":"correct","value":{"op":"eq","args":[{"state":"selected"},{"literal":["b","a"]}]}},{"key":"round","value":{"op":"add","args":[{"state":"round"},{"literal":1}]}}],"notify":true}
    ]}).to_string()})
}
fn provider(root: &Path, protocol: &str) -> String {
    let config = ok(
        root,
        json!({"op":"ai_save","provider":{"id":"","kind":"custom","label":protocol,"baseUrl":"http://127.0.0.1:12345/v1","protocol":protocol,"model":"fixture","models":[{"id":"fixture"}]}}),
    );
    config["providers"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

#[derive(Default)]
struct Ranges(Mutex<Vec<(u64, u64)>>);
struct Media(Arc<Ranges>);
impl MediaHttp for Media {
    fn fetch_range(
        &self,
        _: String,
        _: String,
        _: String,
        _: String,
        start: u64,
        end: u64,
    ) -> MediaRangeReply {
        self.0.0.lock().unwrap().push((start, end));
        let body = if start == 100 && end == 155 {
            let mut data = vec![0u8; 56];
            data[0..4].copy_from_slice(&56u32.to_be_bytes());
            data[4..8].copy_from_slice(b"sidx");
            data[16..20].copy_from_slice(&1000u32.to_be_bytes());
            data[30..32].copy_from_slice(&2u16.to_be_bytes());
            for at in [32, 44] {
                data[at..at + 4].copy_from_slice(&400u32.to_be_bytes());
                data[at + 4..at + 8].copy_from_slice(&10000u32.to_be_bytes());
            }
            data
        } else {
            (start..=end).map(|n| n.to_le_bytes()[0]).collect()
        };
        MediaRangeReply { status: 206, body }
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the end-to-end fixture setup, actions, and assertions for this scenario together."
)]
fn generated_games_roundtrip_all_protocols_and_keep_actions_and_audio_scoped() {
    let backend = Arc::new(Backend::default());
    install_ai_platform(Box::new(Platform(backend.clone())));
    let ranges = Arc::new(Ranges::default());
    install_media_http(Box::new(Media(ranges.clone())));
    for (protocol, legacy, response_mode) in [
        ("chat", false, 0),
        ("responses", false, 0), // Full terminal snapshot, alongside item events.
        ("responses", false, 1), // Empty terminal snapshot must retain the call.
        ("responses", false, 2), // Only reasoning in the terminal snapshot.
        ("responses", false, 3), // Item IDs correlate events without output indices.
        ("responses", false, 4), // Terminal snapshot alone still supplies the call.
        ("responses", false, 5), // Only the call: retain signed reasoning too.
        ("anthropic", false, 0),
        ("gemini", false, 0),
        ("gemini", true, 0),
    ] {
        backend.response_mode.store(response_mode, Ordering::SeqCst);
        backend.legacy_gemini.store(legacy, Ordering::SeqCst);
        let store = Temp::new();
        let root = &store.0;
        let id = provider(root, protocol);
        *backend.definition.lock().unwrap() = definition();
        backend.requests.lock().unwrap().clear();
        backend.answer_only.store(false, Ordering::SeqCst);
        backend.repeat.store(true, Ordering::SeqCst);
        let started = ok(
            root,
            json!({"op":"ai_chat_send","id":id,"prompt":"Make a game"}),
        );
        assert_eq!(
            wait(root, &started)["status"],
            "completed",
            "{protocol}, response mode {response_mode}"
        );
        let chat = ok(
            root,
            json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
        );
        let reply = chat["messages"].as_array().unwrap().last().unwrap();
        assert_eq!(
            reply["games"].as_array().unwrap().len(),
            1,
            "repeated calls must not duplicate cards"
        );
        assert!(reply["content"].as_str().unwrap().contains("你好"));
        let game = &reply["games"][0];
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[1].to_string().contains("OPAQUE_CONTINUATION"),
            "preserve signed/opaque protocol items"
        );
        if protocol == "chat" {
            let details = &requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["tool_calls"].is_array())
                .unwrap()["reasoning_details"];
            assert_eq!(details.as_array().unwrap().len(), 2);
            assert_eq!(details[0]["signature"], "signed-chunk");
            assert_eq!(details[1]["data"], "OPAQUE_CONTINUATION");
        }
        if protocol == "responses" {
            let input = requests[1]["input"].as_array().unwrap();
            let items = input
                .iter()
                .filter(|item| item["type"] == "reasoning" || item["type"] == "function_call")
                .collect::<Vec<_>>();
            assert_eq!(
                items.len(),
                2,
                "snapshot reconciliation must not duplicate items"
            );
            assert_eq!(items[0]["encrypted_content"], "OPAQUE_CONTINUATION");
            assert_eq!(
                items[1]["arguments"],
                definition().to_string(),
                "final arguments replace deltas instead of appending"
            );
            assert_eq!(
                input
                    .iter()
                    .filter(|item| item["type"] == "function_call_output")
                    .count(),
                1
            );
        }
        assert!(
            !fs::read_to_string(
                root.join("ai/chats")
                    .join(format!("{}.json", chat["id"].as_str().unwrap()))
            )
            .unwrap()
            .contains("OPAQUE_CONTINUATION")
        );
        if legacy {
            assert!(requests[1]["contents"].as_array().unwrap().last().unwrap()["parts"][0]["functionResponse"].get("id").is_none());
        }
        drop(requests);
        let base = json!({"op":"ai_game_action","chatId":chat["id"],"messageId":reply["id"],"gameId":game["id"],"action":"choose","input":{"id":"b"},"eventId":"tap-1","revision":0});
        let mut bad = base.clone();
        bad["input"] = json!({"id":"invented"});
        assert_eq!(call(root, bad)["ok"], false);
        let first = ok(root, base.clone());
        assert_eq!(first["game"]["state"]["selected"], json!(["b"]));
        assert_eq!(
            ok(root, base.clone())["game"]["revision"],
            1,
            "duplicates are idempotent even with their old revision"
        );
        bad = base.clone();
        bad["eventId"] = json!("stale");
        assert_eq!(call(root, bad)["ok"], false);
        let mut second = base.clone();
        second["eventId"] = json!("tap-2");
        second["revision"] = json!(1);
        second["input"] = json!({"id":"a"});
        ok(root, second);
        let mut check = base.clone();
        check["action"] = json!("check");
        check["eventId"] = json!("submit");
        check["revision"] = json!(2);
        check["input"] = json!({});
        let checked = ok(root, check.clone());
        assert_eq!(checked["notify"], true);
        assert_eq!(checked["game"]["state"]["correct"], true);
        assert_eq!(checked["game"]["state"]["round"], 1);
        assert_eq!(
            ok(root, check)["notify"],
            false,
            "a retried submit must not trigger another model turn"
        );
        let export = json!({"op":"ai_game_export","chatId":chat["id"],"messageId":reply["id"],"gameId":game["id"]});
        let copied = ok(root, export.clone());
        let copied: Value = serde_json::from_str(copied["text"].as_str().unwrap()).unwrap();
        assert_eq!(copied["format"], "lingoplay-game");
        assert_eq!(copied["version"], 1);
        assert_eq!(copied["definition"], definition());
        assert_eq!(copied["cues"], json!([]));
        assert!(copied["source"].is_null());
        assert_eq!(copied.as_object().unwrap().len(), 5);
        let fresh_rules: Value =
            serde_json::from_str(copied["definition"]["rules"].as_str().unwrap()).unwrap();
        assert_eq!(fresh_rules["initialState"]["round"], 0);
        assert_eq!(checked["game"]["state"]["round"], 1);
        for private in [&chat["id"], &game["id"], &json!(id)] {
            assert!(!copied.to_string().contains(private.as_str().unwrap()));
        }
        for field in ["messageId", "gameId"] {
            let mut missing = export.clone();
            missing[field] = json!("missing");
            assert_eq!(call(root, missing)["ok"], false);
        }
        backend.answer_only.store(true, Ordering::SeqCst);
        let next = ok(
            root,
            json!({"op":"ai_chat_send","id":id,"chatId":chat["id"],"prompt":"How did I do?","focusGameId":game["id"]}),
        );
        assert_eq!(wait(root, &next)["status"], "completed");
        let request = backend.requests.lock().unwrap().last().unwrap().clone();
        let input = request.to_string();
        assert!(input.contains("game_activity"));
        assert!(input.contains("submit"));
        let field = match protocol {
            "responses" => "input",
            "gemini" => "contents",
            _ => "messages",
        };
        let current = request[field]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .to_string();
        assert!(
            input.contains("<button id=choice>"),
            "retain the original tool call in memory for an unchanged prompt prefix"
        );
        assert!(
            current.contains("game_activity") && !current.contains("<button id=choice>"),
            "new game activity contains progress without duplicating game source"
        );
        let restored = ok(root, json!({"op":"ai_chat_get","chatId":chat["id"]}));
        assert_eq!(
            restored["messages"][1]["games"][0]["state"]["correct"],
            true
        );
        assert_eq!(
            restored["messages"][1]["games"][0], checked["game"],
            "copying must not change progress or activity"
        );
        assert!(
            ranges.0.lock().unwrap().is_empty(),
            "creating and playing text games must not fetch audio"
        );
        library_agent_roundtrip(root, &backend, protocol, &id, &game["id"]);
        let renamed_chat = ok(root, json!({"op":"ai_chat_get","chatId":chat["id"]}));
        assert_eq!(
            renamed_chat["messages"][1]["games"][0]["definition"]["title"],
            "Saved grammar · 过去"
        );
    }
    // A final stream failure must preserve a successfully committed card.
    let store = Temp::new();
    let root = &store.0;
    let id = provider(root, "responses");
    backend.answer_only.store(false, Ordering::SeqCst);
    backend.fail_final.store(true, Ordering::SeqCst);
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"Make another game"}),
    );
    assert_eq!(wait(root, &started)["status"], "failed");
    assert_eq!(
        ok(
            root,
            json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]})
        )["messages"][1]["games"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    backend.fail_final.store(false, Ordering::SeqCst);
    // Unknown operations are returned as tool errors and never install a card.
    let mut invalid = definition();
    invalid["rules"]=json!(json!({"initialState":{},"actions":[{"name":"escape","updates":[{"key":"x","value":{"op":"eval","args":[]}}]}]}).to_string());
    *backend.definition.lock().unwrap() = invalid;
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"Invalid game"}),
    );
    assert_eq!(wait(root, &started)["status"], "completed");
    assert!(
        ok(
            root,
            json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]})
        )["messages"][1]["games"]
            .is_null()
    );
    // Caption references are frozen; lazily fetched audio survives transcript eviction.
    let video = "abcdefghijk";
    let raw = json!({"type":"transcript","requestId":"game-audio","videoId":video,"ok":true,"transcript":{"videoId":video,"title":"Clip fixture","trackId":".en","language":"English","tracks":[{"id":".en","label":"English","language":"en","automatic":false}],"segments":[{"start":12.5,"duration":2,"text":"Spoken words from the video"}]}});
    let accepted:Value=serde_json::from_str(&execute(json!({"op":"accept","videoId":video,"requestId":"game-audio","origin":"https://www.youtube.com","raw":raw.to_string()}).to_string(),root.to_str().unwrap().into())).unwrap();
    assert_eq!(accepted["ok"], true);
    let mut def = definition();
    def["cueIds"] = json!(["cue-0"]);
    *backend.definition.lock().unwrap() = def;
    let started = ok(
        root,
        json!({"op":"ai_chat_send","id":id,"prompt":"Audio game","transcript":{"videoId":video,"trackId":".en"}}),
    );
    assert_eq!(wait(root, &started)["status"], "completed");
    let chat = ok(
        root,
        json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
    );
    let reply = &chat["messages"][1];
    let mut command = json!({"op":"ai_game_audio","chatId":chat["id"],"messageId":reply["id"],"gameId":reply["games"][0]["id"],"cueId":"cue-0"});
    let transcript_audio = json!({"op":"ai_chat_audio","chatId":chat["id"],"videoId":video,"trackId":".en","cueId":"cue-0"});
    assert_eq!(ok(root, transcript_audio.clone())["needsMedia"], true);
    assert_eq!(ok(root, command.clone())["needsMedia"], true);
    assert!(ranges.0.lock().unwrap().is_empty());
    command["catalog"] = json!({"videoId":video,"origin":"https://m.youtube.com","live":false,"streams":[{"itag":140,"kind":"audio","mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","audioOriginal":true,"bitrate":128_000,"bytes":1000,"url":"https://r1.googlevideo.com/audio?sig=PRIVATE_SIGNED_AUDIO","initRange":{"start":0,"end":99},"indexRange":{"start":100,"end":155}}]});
    let clip = ok(root, command.clone());
    assert_eq!(clip["start"], 2.5);
    assert_eq!(clip["end"], 4.5);
    assert_eq!(
        *ranges.0.lock().unwrap(),
        vec![(100, 155), (0, 99), (556, 955)],
        "only initialization, index and overlapping audio are fetched"
    );
    let uri = clip["uri"].as_str().unwrap();
    let manifest = ureq::get(uri).call().unwrap().into_string().unwrap();
    assert!(manifest.contains("segment-0.m4s"));
    assert!(!manifest.contains("PRIVATE_SIGNED_AUDIO"));
    let segment = url::Url::parse(uri).unwrap().join("segment-0.m4s").unwrap();
    let mut data = Vec::new();
    ureq::get(segment.as_str())
        .call()
        .unwrap()
        .into_reader()
        .read_to_end(&mut data)
        .unwrap();
    assert_eq!(data.len(), 400);
    let shared = ok(root, transcript_audio.clone());
    assert_eq!(shared["needsMedia"], false);
    assert_eq!(shared["start"], clip["start"]);
    assert_eq!(shared["end"], clip["end"]);
    assert_eq!(
        ranges.0.lock().unwrap().len(),
        3,
        "the transcript reuses the game's cached bytes without a media catalog"
    );
    ok(
        root,
        json!({"op":"ai_game_audio_release","token":shared["token"]}),
    );
    for (field, value) in [
        ("videoId", json!("0123456789a")),
        ("trackId", json!(".fr")),
        ("cueId", json!("cue-999")),
        ("cueId", json!("../unsafe")),
    ] {
        let mut invalid = transcript_audio.clone();
        invalid[field] = value;
        assert_eq!(call(root, invalid)["ok"], false);
    }
    assert_eq!(ranges.0.lock().unwrap().len(), 3);
    ok(
        root,
        json!({"op":"ai_game_audio_release","token":clip["token"]}),
    );
    assert!(ureq::get(uri).call().is_err());
    command.as_object_mut().unwrap().remove("catalog");
    let cached = ok(root, command.clone());
    assert_eq!(cached["needsMedia"], false);
    assert_eq!(
        ranges.0.lock().unwrap().len(),
        3,
        "replay must use cached bytes"
    );
    command["cueId"] = json!("cue-999");
    assert_eq!(call(root, command)["ok"], false);
    assert_eq!(ranges.0.lock().unwrap().len(), 3);
    ok(
        root,
        json!({"op":"ai_game_audio_release","token":cached["token"]}),
    );
    for entry in fs::read_dir(root.join("ai/game-audio")).unwrap() {
        let path = entry.unwrap().path().join("clip.json");
        assert!(
            !fs::read_to_string(path)
                .unwrap()
                .contains("PRIVATE_SIGNED_AUDIO")
        );
    }
    // Reuse needs the original captions and video, not an expiring media URL.
    let exported = ok(
        root,
        json!({"op":"ai_game_export","chatId":chat["id"],"messageId":reply["id"],"gameId":reply["games"][0]["id"]}),
    );
    let text = exported["text"].as_str().unwrap();
    let copied: Value = serde_json::from_str(text).unwrap();
    assert_eq!(copied["source"]["videoId"], video);
    assert_eq!(copied["source"]["trackId"], ".en");
    assert_eq!(copied["cues"], reply["games"][0]["cues"]);
    assert_eq!(copied["cues"][0]["text"], "Spoken words from the video");
    assert_eq!(copied["cues"][0]["start"], 12.5);
    assert!(!text.contains("PRIVATE_SIGNED_AUDIO"));
    practice_roundtrip(root, &chat, &reply["games"][0], &ranges);
}

fn rename_saved_game(root: &Path, id: &Value) -> Value {
    let before = ok(root, json!({"op":"ai_practice_get","gameId":id}));
    let title = "Saved grammar · 过去";
    let renamed = ok(
        root,
        json!({"op":"ai_practice_rename","gameId":id,"title":format!("  {title}  ")}),
    );
    assert_eq!(renamed["game"]["definition"]["title"], title);
    assert_eq!(renamed["game"]["state"], before["state"]);
    assert_eq!(renamed["game"]["revision"], before["revision"]);
    assert_eq!(renamed["game"]["source"], before["source"]);
    assert_eq!(
        renamed["game"]["definition"]["rules"],
        before["definition"]["rules"]
    );
    for invalid in [
        json!("  "),
        json!("x".repeat(121)),
        json!("line\nbreak"),
        Value::Null,
        json!(7),
    ] {
        assert_eq!(
            call(
                root,
                json!({"op":"ai_practice_rename","gameId":id,"title":invalid})
            )["ok"],
            false
        );
        assert_eq!(
            ok(root, json!({"op":"ai_practice_get","gameId":id}))["title"],
            title
        );
    }
    let action = ok(
        root,
        json!({"op":"ai_practice_action","gameId":id,"action":"check","input":{},"revision":before["revision"],"eventId":"action-after-rename"}),
    );
    assert_eq!(
        action["game"]["definition"]["title"], title,
        "an action using the revision from before rename must preserve the new name"
    );
    ok(root, json!({"op":"ai_practice_get","gameId":id}))
}

fn library_agent_roundtrip(
    root: &Path,
    backend: &Backend,
    protocol: &str,
    provider: &str,
    id: &Value,
) {
    let saved = rename_saved_game(root, id);
    let session = ok(root, json!({"op":"ai_practice_session"}));
    let count = ok(root, json!({"op":"ai_practice_catalog"}))["total"].clone();
    for (name, arguments) in [
        (
            "list_interactive_games",
            json!({"query":"SAVED GRAMMAR","limit":1}),
        ),
        (
            "read_interactive_game",
            json!({"gameId":id,"op":"ai_practice_rename","title":"Cannot mutate through a read tool"}),
        ),
    ] {
        *backend.tool_name.lock().unwrap() = Some(name.into());
        *backend.definition.lock().unwrap() = arguments;
        backend.requests.lock().unwrap().clear();
        backend.answer_only.store(false, Ordering::SeqCst);
        backend.repeat.store(false, Ordering::SeqCst);
        // A fresh chat must find games created elsewhere, without creating another card.
        let started = ok(
            root,
            json!({"op":"ai_chat_send","id":provider,"prompt":"Find my saved grammar game"}),
        );
        assert_eq!(wait(root, &started)["status"], "completed");
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let declarations = if protocol == "gemini" {
            &requests[0]["tools"][0]["functionDeclarations"]
        } else {
            &requests[0]["tools"]
        };
        let names = declarations
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| {
                if protocol == "chat" {
                    tool["function"]["name"].clone()
                } else {
                    tool["name"].clone()
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                json!("create_interactive_game"),
                json!("list_interactive_games"),
                json!("read_interactive_game")
            ]
        );
        let output = library_tool_output(requests.last().unwrap(), protocol);
        drop(requests);
        if name == "list_interactive_games" {
            assert_eq!(output["games"][0]["gameId"], *id);
            assert_eq!(output["games"][0]["title"], saved["title"]);
        } else {
            assert_eq!(
                output, saved,
                "read returns the real definition and progress without mutating it"
            );
        }
        let chat = ok(
            root,
            json!({"op":"ai_chat_get","chatId":started["conversation"]["id"]}),
        );
        assert!(chat["messages"][1]["games"].is_null());
        for block in chat["messages"][1]["blocks"].as_array().unwrap() {
            if block["kind"] == "tool" {
                assert_eq!(block["name"], name);
                assert_eq!(block["status"], "completed");
                assert!(
                    block["gameId"].is_null(),
                    "read tools must not claim to create a playable card in this chat"
                );
            }
        }
        assert_eq!(ok(root, json!({"op":"ai_practice_session"})), session);
        assert_eq!(
            ok(root, json!({"op":"ai_practice_catalog"}))["total"],
            count
        );
    }
    backend.tool_name.lock().unwrap().take();
    *backend.definition.lock().unwrap() = definition();
}

fn library_tool_output(request: &Value, protocol: &str) -> Value {
    if protocol == "gemini" {
        return request["contents"].as_array().unwrap().last().unwrap()["parts"][0]["functionResponse"]["response"].clone();
    }
    let raw = match protocol {
        "responses" => &request["input"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|row| row["type"] == "function_call_output")
            .unwrap()["output"],
        "anthropic" => {
            &request["messages"].as_array().unwrap().last().unwrap()["content"][0]["content"]
        }
        _ => &request["messages"].as_array().unwrap().last().unwrap()["content"],
    };
    serde_json::from_str(raw.as_str().unwrap()).unwrap()
}

fn practice_roundtrip(root: &Path, chat: &Value, game: &Value, ranges: &Ranges) {
    let id = &game["id"];
    let library = ok(root, json!({"op":"ai_practice_list"}));
    let saved = library["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == *id)
        .unwrap();
    assert_eq!(saved["source"]["start"], 12.5);
    let session = ok(root, json!({"op":"ai_practice_start","gameId":id}));
    assert_eq!(session["game"]["id"], *id);
    let review = json!({"op":"ai_practice_review","gameId":id,"revision":session["revision"],"eventId":"swipe-left","verdict":"forgot"});
    let reviewed = ok(root, review.clone());
    assert_eq!(reviewed["forgot"], 1);
    assert_eq!(
        reviewed["remaining"].as_u64().unwrap() + 1,
        session["remaining"]
    );
    assert_eq!(
        ok(root, review.clone()),
        reviewed,
        "a retried swipe cannot advance twice"
    );
    let mut stale = review;
    stale["eventId"] = json!("stale-swipe");
    assert_eq!(call(root, stale)["ok"], false);
    assert_eq!(
        ok(root, json!({"op":"ai_practice_session"})),
        reviewed,
        "opening again restores the same queue"
    );
    let undone = ok(
        root,
        json!({"op":"ai_practice_undo","revision":reviewed["revision"],"eventId":"undo-left"}),
    );
    assert_eq!(undone["game"]["id"], *id);
    assert_eq!(undone["forgot"], 0);
    let library = ok(root, json!({"op":"ai_practice_list"}));
    assert!(
        library["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == *id)
            .unwrap()["verdict"]
            .is_null()
    );
    let action = json!({"op":"ai_practice_action","gameId":id,"action":"choose","input":{"id":"b"},"eventId":"practice-choice","revision":game["revision"]});
    let chosen = ok(root, action.clone());
    assert_eq!(chosen["game"]["state"]["selected"], json!(["b"]));
    assert_eq!(ok(root, action)["game"], chosen["game"]);
    let refreshed = ok(root, json!({"op":"ai_chat_get","chatId":chat["id"]}));
    assert_eq!(
        refreshed["messages"][1]["games"][0], chosen["game"],
        "chat and practice share authoritative progress"
    );
    let reset = ok(
        root,
        json!({"op":"ai_practice_reset","gameId":id,"revision":chosen["game"]["revision"]}),
    );
    assert_eq!(reset["game"]["state"]["selected"], json!([]));
    let reviewed = ok(
        root,
        json!({"op":"ai_practice_review","gameId":id,"revision":undone["revision"],"eventId":"swipe-forgot-again","verdict":"forgot"}),
    );
    let forgotten = ok(root, json!({"op":"ai_practice_start","filter":"forgot"}));
    assert_eq!(forgotten["total"], 1);
    assert_eq!(forgotten["game"]["id"], *id);
    let known = ok(
        root,
        json!({"op":"ai_practice_review","gameId":id,"revision":forgotten["revision"],"eventId":"swipe-right","verdict":"know"}),
    );
    assert!(known["game"].is_null());
    assert_eq!(known["known"], 1);
    assert_eq!(known["remaining"], 0);
    ok(
        root,
        json!({"op":"ai_practice_undo","revision":known["revision"],"eventId":"undo-right"}),
    );
    assert_eq!(
        ok(root, json!({"op":"ai_practice_start","filter":"forgot"}))["total"],
        1,
        "undo restores the previous forgotten rating"
    );
    assert_eq!(reviewed["forgot"], 1);
    practice_survives_chat(root, chat, game, ranges);
}

fn practice_survives_chat(root: &Path, chat: &Value, game: &Value, ranges: &Ranges) {
    let id = &game["id"];
    ok(root, json!({"op":"ai_chat_delete","chatId":chat["id"]}));
    assert_eq!(
        call(root, json!({"op":"ai_chat_get","chatId":chat["id"]}))["ok"],
        false
    );
    assert_eq!(
        ok(root, json!({"op":"ai_practice_session"}))["game"]["source"],
        game["source"]
    );
    let command = json!({"op":"ai_practice_audio","gameId":id,"cueId":"cue-0","prefetch":true});
    for _ in 0..24 {
        assert_eq!(
            ok(root, command.clone()),
            json!({"needsMedia":false}),
            "cache-only preparation cannot allocate a playback route"
        );
    }
    let mut command = command;
    command["prefetch"] = json!(false);
    let clip = ok(root, command);
    assert!(ureq::get(clip["uri"].as_str().unwrap()).call().is_ok());
    ok(
        root,
        json!({"op":"ai_game_audio_release","token":clip["token"]}),
    );
    assert_eq!(
        ranges.0.lock().unwrap().len(),
        3,
        "saved games replay cached bytes after chat deletion"
    );
    let export = ok(root, json!({"op":"ai_practice_export","gameId":id}));
    let copied: Value = serde_json::from_str(export["text"].as_str().unwrap()).unwrap();
    assert_eq!(copied["source"], game["source"]);
    assert_eq!(copied["cues"], game["cues"]);
    let mut revision = ok(root, json!({"op":"ai_practice_session"}))["game"]["revision"]
        .as_u64()
        .unwrap();
    for index in 0..250 {
        let value = ok(
            root,
            json!({"op":"ai_practice_action","gameId":id,"action":"check","input":{},"eventId":format!("repeat-{index}"),"revision":revision}),
        );
        revision = value["game"]["revision"].as_u64().unwrap();
        assert!(value["game"]["events"].as_array().unwrap().len() <= 200);
    }
    let current = ok(root, json!({"op":"ai_practice_session"}));
    assert_eq!(current["game"]["revision"], revision);
    assert_eq!(
        current["game"]["state"]["round"], 250,
        "repeated practice compacts history without exhausting a saved game"
    );
    library_pages(root, id);
}

fn library_pages(root: &Path, id: &Value) {
    let mut offset = json!(0);
    let mut ids = std::collections::HashSet::new();
    loop {
        let page = ok(
            root,
            json!({"op":"ai_practice_catalog","limit":1,"offset":offset}),
        );
        for game in page["games"].as_array().unwrap() {
            assert!(ids.insert(game["gameId"].clone().to_string()));
        }
        if page["nextOffset"].is_null() {
            assert_eq!(ids.len() as u64, page["total"].as_u64().unwrap());
            break;
        }
        offset = page["nextOffset"].clone();
    }
    assert!(
        ids.contains(&id.to_string()),
        "deleted chats' games remain readable in the catalog"
    );
    assert_eq!(
        ok(root, json!({"op":"ai_practice_get","gameId":id}))["gameId"],
        *id
    );
    for command in [
        json!({"op":"ai_practice_catalog","limit":0}),
        json!({"op":"ai_practice_catalog","limit":51}),
        json!({"op":"ai_practice_catalog","offset":-1}),
        json!({"op":"ai_practice_catalog","query":7}),
        json!({"op":"ai_practice_get","gameId":"../unsafe"}),
        json!({"op":"ai_practice_rename","gameId":"../unsafe","title":"Unsafe"}),
    ] {
        assert_eq!(call(root, command)["ok"], false);
    }
}
