//! Provider-specific tool framing stays out of conversations and game logic.
use super::*;
use std::collections::BTreeMap;

pub(super) fn merge_usage(prefix: &Value, next: &Value) -> Value {
    if prefix.is_null() {
        return next.clone();
    }
    if next.is_null() {
        return json!({});
    }
    let mut result = json!({});
    if let Some(fields) = next.as_object() {
        for (key, value) in fields {
            if key == "latestContextTokens" {
                result[key] = value.clone();
            } else if let Some(n) = value
                .as_u64()
                .and_then(|n| prefix[key].as_u64().and_then(|old| n.checked_add(old)))
                .filter(|n| *n <= usage::MAX_COUNT)
            {
                result[key] = json!(n);
            }
        }
    }
    result
}

#[derive(Default)]
pub(super) struct Stream {
    responses: Vec<Value>,
    response_indices: BTreeMap<u64, usize>,
    items: BTreeMap<u64, Value>,
    chat: BTreeMap<u64, Value>,
    chat_details: Vec<Value>,
    gemini: Vec<Value>,
}
impl Stream {
    pub fn gemini_len(&self) -> usize {
        self.gemini.len()
    }
    // Identity and name only: arguments and signed reasoning never enter UI activity.
    pub fn call_headers(&self, protocol: &str) -> Vec<(String, String)> {
        match protocol {
            "responses" => self
                .responses
                .iter()
                .filter(|v| v["type"] == "function_call")
                .map(|v| {
                    (
                        v["call_id"].as_str().unwrap_or_default().into(),
                        v["name"].as_str().unwrap_or_default().into(),
                    )
                })
                .collect(),
            "anthropic" => self
                .items
                .values()
                .filter(|v| v["type"] == "tool_use")
                .map(|v| {
                    (
                        v["id"].as_str().unwrap_or_default().into(),
                        v["name"].as_str().unwrap_or_default().into(),
                    )
                })
                .collect(),
            "gemini" => Vec::new(),
            _ => self
                .chat
                .values()
                .map(|v| {
                    (
                        v["id"].as_str().unwrap_or_default().into(),
                        v["function"]["name"].as_str().unwrap_or_default().into(),
                    )
                })
                .collect(),
        }
    }
    pub fn event(&mut self, protocol: &str, v: &Value) -> Result<()> {
        match protocol {
            "responses" => {
                match v["type"].as_str().unwrap_or_default() {
                    "response.output_item.added" | "response.output_item.done" => {
                        self.response_item(&v["item"], v["output_index"].as_u64())?;
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some(index) = self
                            .response_position(v["item_id"].as_str(), None)
                            .or_else(|| {
                                v["output_index"]
                                    .as_u64()
                                    .and_then(|i| self.response_indices.get(&i).copied())
                            })
                            && let Some(item) = self.responses.get_mut(index)
                        {
                            append(item, "arguments", &v["delta"])?;
                        } else {
                            return Err("Tool arguments arrived without a call.".into());
                        }
                    }
                    "response.completed" => {
                        if let Some(items) = v["response"]["output"].as_array() {
                            // Some streaming endpoints send an empty or partial final
                            // snapshot. Completed item events must survive it.
                            for item in items {
                                self.response_item(item, None)?;
                            }
                        }
                    }
                    _ => {}
                }
            }
            "anthropic" => {
                let index = v["index"].as_u64().unwrap_or(0);
                if v["type"] == "content_block_start" {
                    let mut block = v["content_block"].clone();
                    if block["type"] == "tool_use" {
                        block["arguments"] = json!("");
                    }
                    self.items.insert(index, block);
                }
                if v["type"] == "content_block_delta"
                    && let Some(block) = self.items.get_mut(&index)
                {
                    match v["delta"]["type"].as_str().unwrap_or_default() {
                        "input_json_delta" => {
                            append(block, "arguments", &v["delta"]["partial_json"])?;
                        }
                        "text_delta" => append(block, "text", &v["delta"]["text"])?,
                        "thinking_delta" => append(block, "thinking", &v["delta"]["thinking"])?,
                        "signature_delta" => append(block, "signature", &v["delta"]["signature"])?,
                        _ => {}
                    }
                }
            }
            "gemini" => {
                if let Some(parts) = v["candidates"][0]["content"]["parts"].as_array() {
                    self.gemini.extend(parts.iter().cloned());
                }
            }
            _ => {
                if let Some(details) = v["choices"][0]["delta"]["reasoning_details"].as_array() {
                    // Preserve signed/encrypted chunks verbatim and in stream order.
                    self.chat_details.extend(details.iter().cloned());
                }
                if let Some(calls) = v["choices"][0]["delta"]["tool_calls"].as_array() {
                    for call in calls {
                        let index = call["index"].as_u64().ok_or("Missing tool call index.")?;
                        let item = self.chat.entry(index).or_insert_with(|| json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                        if let Some(id) = call["id"].as_str() {
                            item["id"] = json!(id);
                        }
                        append(&mut item["function"], "name", &call["function"]["name"])?;
                        append(
                            &mut item["function"],
                            "arguments",
                            &call["function"]["arguments"],
                        )?;
                    }
                }
            }
        }
        if self.responses.len() + self.items.len() + self.chat.len() > 256
            || self.response_indices.len() > 256
            || self.gemini.len() + self.chat_details.len() > 4096
            || self.value(protocol, "", "").to_string().len() > 500_000
        {
            return Err("Provider tool output is too large.".into());
        }
        Ok(())
    }
    fn response_position(&self, id: Option<&str>, call_id: Option<&str>) -> Option<usize> {
        self.responses.iter().position(|item| {
            id.filter(|id| !id.is_empty())
                .is_some_and(|id| item["id"] == id)
                || call_id
                    .filter(|id| !id.is_empty())
                    .is_some_and(|id| item["call_id"] == id)
        })
    }
    fn response_item(&mut self, item: &Value, output_index: Option<u64>) -> Result<()> {
        let fields = item.as_object().ok_or("Invalid response output item.")?;
        let position = self
            .response_position(item["id"].as_str(), item["call_id"].as_str())
            .or_else(|| output_index.and_then(|i| self.response_indices.get(&i).copied()));
        let position = if let Some(position) = position {
            // Final item fields replace streamed values, rather than appending
            // arguments or encrypted reasoning a second time.
            self.responses[position]
                .as_object_mut()
                .ok_or("Invalid response output item.")?
                .extend(fields.clone());
            position
        } else {
            self.responses.push(item.clone());
            self.responses.len() - 1
        };
        if let Some(index) = output_index {
            self.response_indices.insert(index, position);
        }
        Ok(())
    }
    pub fn value(&self, protocol: &str, text: &str, reasoning: &str) -> Value {
        let mut calls = Vec::new();
        let continuation = match protocol {
            "responses" => {
                for item in self
                    .responses
                    .iter()
                    .filter(|v| v["type"] == "function_call")
                {
                    calls.push(json!({"id":item["call_id"],"name":item["name"],"arguments":item["arguments"]}));
                }
                json!(self.responses)
            }
            "anthropic" => {
                let mut blocks = Vec::new();
                for item in self.items.values() {
                    let mut item = item.clone();
                    if item["type"] == "tool_use" {
                        let arguments = item
                            .as_object_mut()
                            .and_then(|fields| fields.remove("arguments"))
                            .unwrap_or(json!(""));
                        if let Some(raw) = arguments.as_str().filter(|s| !s.is_empty()) {
                            item["input"] = serde_json::from_str(raw).unwrap_or(Value::Null);
                        }
                        calls.push(json!({"id":item["id"],"name":item["name"],"arguments":item["input"].to_string()}));
                    }
                    blocks.push(item);
                }
                json!({"role":"assistant","content":blocks})
            }
            "gemini" => {
                for (i, part) in self
                    .gemini
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v["functionCall"].is_object())
                {
                    let call = &part["functionCall"];
                    calls.push(json!({"id":call["id"].as_str().map(str::to_owned).unwrap_or_else(|| format!("call-{i}")),"providerId":call["id"],"name":call["name"],"arguments":call["args"].to_string()}));
                }
                json!({"role":"model","parts":self.gemini})
            }
            _ => {
                for item in self.chat.values() {
                    calls.push(json!({"id":item["id"],"name":item["function"]["name"],"arguments":item["function"]["arguments"]}));
                }
                let mut row = json!({"role":"assistant","content":if text.is_empty(){Value::Null}else{json!(text)},"tool_calls":self.chat.values().collect::<Vec<_>>()});
                if !self.chat_details.is_empty() {
                    row["reasoning_details"] = json!(self.chat_details);
                } else if !reasoning.is_empty() {
                    row["reasoning_content"] = json!(reasoning);
                }
                row
            }
        };
        json!({"calls":calls,"continuation":continuation})
    }
}
fn append(target: &mut Value, name: &str, delta: &Value) -> Result<()> {
    if let Some(delta) = delta.as_str() {
        let mut current = target[name].as_str().unwrap_or_default().to_owned();
        if current.len() + delta.len() > 200_000 {
            return Err("Tool arguments are too large.".into());
        }
        current.push_str(delta);
        target[name] = json!(current);
    }
    Ok(())
}

pub(super) fn install(body: &mut Value, protocol: &str) {
    let definitions = std::iter::once(games::tool()).chain(practice::tools());
    let declarations = definitions.map(|tool| match protocol {
        "responses" => json!({"type":"function","name":tool["name"],"description":tool["description"],"parameters":tool["parameters"],"strict":false}),
        "anthropic" => json!({"name":tool["name"],"description":tool["description"],"input_schema":tool["parameters"]}),
        "gemini" => {
            let mut parameters = tool["parameters"].clone();
            if let Some(fields) = parameters.as_object_mut() { fields.remove("additionalProperties"); }
            json!({"name":tool["name"],"description":tool["description"],"parameters":parameters})
        }
        _ => json!({"type":"function","function":tool}),
    }).collect::<Vec<_>>();
    body["tools"] = match protocol {
        "gemini" => json!([{"functionDeclarations":declarations}]),
        _ => json!(declarations),
    };
    if protocol != "gemini" && protocol != "anthropic" {
        body["parallel_tool_calls"] = json!(false);
    }
    if protocol == "responses" {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
}

pub(super) fn append_user(body: &mut Value, protocol: &str, content: &str) -> Result<()> {
    let rows = body[replay::field(protocol)]
        .as_array_mut()
        .ok_or("Invalid conversation continuation.")?;
    if matches!(protocol, "anthropic" | "gemini")
        && let Some(last) = rows.last_mut().filter(|last| last["role"] == "user")
    {
        let (field, part) = if protocol == "gemini" {
            ("parts", json!({"text":content}))
        } else {
            ("content", json!({"type":"text","text":content}))
        };
        last[field]
            .as_array_mut()
            .ok_or("Invalid tool result content.")?
            .push(part);
    } else {
        rows.push(if protocol == "gemini" {
            json!({"role":"user","parts":[{"text":content}]})
        } else {
            json!({"role":"user","content":content})
        });
    }
    Ok(())
}

pub(super) fn continue_with(
    body: &mut Value,
    protocol: &str,
    response: &Value,
    results: Vec<Value>,
) -> Result<()> {
    let field = match protocol {
        "responses" => "input",
        "gemini" => "contents",
        _ => "messages",
    };
    let rows = body[field]
        .as_array_mut()
        .ok_or("Invalid tool continuation.")?;
    match protocol {
        "responses" => {
            let items = response["tools"]["continuation"]
                .as_array()
                .ok_or("Missing response items.")?;
            rows.extend(items.clone());
            if !response["text"].as_str().unwrap_or_default().is_empty()
                && !items
                    .iter()
                    .any(|item| item["type"] == "message" || item["role"] == "assistant")
            {
                rows.push(json!({"role":"assistant","content":response["text"]}));
            }
            rows.extend(results.iter().map(|r| json!({"type":"function_call_output","call_id":r["id"],"output":r["output"].to_string()})));
        }
        "anthropic" => {
            let mut assistant = response["tools"]["continuation"].clone();
            if assistant["content"].as_array().is_some_and(Vec::is_empty) {
                assistant["content"] = json!([{"type":"text","text":response["text"]}]);
            }
            rows.push(assistant);
            if !results.is_empty() {
                rows.push(json!({"role":"user","content":results.iter().map(|r|json!({"type":"tool_result","tool_use_id":r["id"],"content":r["output"].to_string(),"is_error":r["output"]["error"].is_string()})).collect::<Vec<_>>()}));
            }
        }
        "gemini" => {
            rows.push(response["tools"]["continuation"].clone());
            let parts = results
                .iter()
                .map(|result| {
                    let mut response = json!({"name":result["name"],"response":result["output"]});
                    if let Some(id) = result["providerId"].as_str() {
                        response["id"] = json!(id);
                    }
                    json!({"functionResponse":response})
                })
                .collect::<Vec<_>>();
            if !parts.is_empty() {
                rows.push(json!({"role":"user","parts":parts}));
            }
        }
        _ => {
            rows.push(response["tools"]["continuation"].clone());
            rows.extend(results.iter().map(
                |r| json!({"role":"tool","tool_call_id":r["id"],"content":r["output"].to_string()}),
            ));
        }
    }
    Ok(())
}
