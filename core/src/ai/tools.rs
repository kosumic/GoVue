//! Provider-specific tool framing stays out of conversations and game logic.
use super::*;
use std::collections::BTreeMap;

pub(super) fn merge_usage(prefix: &Value, next: &Value) -> Value {
    if prefix.is_null() {
        return next.clone();
    }
    if next.is_null() {
        return prefix.clone();
    }
    let mut result = prefix.clone();
    if let Some(fields) = next.as_object() {
        for (key, value) in fields {
            if key == "latestContextTokens" {
                result[key] = value.clone();
            } else if let Some(n) = value
                .as_u64()
                .and_then(|n| n.checked_add(result[key].as_u64().unwrap_or(0)))
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
    items: BTreeMap<u64, Value>,
    chat: BTreeMap<u64, Value>,
    chat_details: Vec<Value>,
    gemini: Vec<Value>,
}
impl Stream {
    pub fn event(&mut self, protocol: &str, v: &Value) -> Result<()> {
        match protocol {
            "responses" => {
                let index = v["output_index"].as_u64().unwrap_or(0);
                match v["type"].as_str().unwrap_or_default() {
                    "response.output_item.added" | "response.output_item.done" => {
                        self.items.insert(index, v["item"].clone());
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some(item) = self.items.get_mut(&index) {
                            append(item, "arguments", &v["delta"])?;
                        } else {
                            return Err("Tool arguments arrived without a call.".into());
                        }
                    }
                    "response.completed" => {
                        if let Some(items) = v["response"]["output"].as_array() {
                            self.items = items
                                .iter()
                                .enumerate()
                                .map(|(i, v)| (i as u64, v.clone()))
                                .collect();
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
                            append(block, "arguments", &v["delta"]["partial_json"])?
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
        if self.items.len() + self.chat.len() > 256
            || self.gemini.len() + self.chat_details.len() > 4096
            || self.value(protocol, "", "").to_string().len() > 500_000
        {
            return Err("Provider tool output is too large.".into());
        }
        Ok(())
    }
    pub fn value(&self, protocol: &str, text: &str, reasoning: &str) -> Value {
        let mut calls = Vec::new();
        let continuation = match protocol {
            "responses" => {
                for item in self.items.values().filter(|v| v["type"] == "function_call") {
                    calls.push(json!({"id":item["call_id"],"name":item["name"],"arguments":item["arguments"]}));
                }
                json!(self.items.values().collect::<Vec<_>>())
            }
            "anthropic" => {
                let mut blocks = Vec::new();
                for item in self.items.values() {
                    let mut item = item.clone();
                    if item["type"] == "tool_use" {
                        let arguments = item
                            .as_object_mut()
                            .unwrap()
                            .remove("arguments")
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
    let tool = games::tool();
    body["tools"] = match protocol {
        "responses" => {
            json!([{"type":"function","name":tool["name"],"description":tool["description"],"parameters":tool["parameters"],"strict":false}])
        }
        "anthropic" => {
            json!([{"name":tool["name"],"description":tool["description"],"input_schema":tool["parameters"]}])
        }
        "gemini" => {
            let mut parameters = tool["parameters"].clone();
            parameters
                .as_object_mut()
                .unwrap()
                .remove("additionalProperties");
            json!([{"functionDeclarations":[{"name":tool["name"],"description":tool["description"],"parameters":parameters}]}])
        }
        _ => json!([{"type":"function","function":tool}]),
    };
    if protocol != "gemini" && protocol != "anthropic" {
        body["parallel_tool_calls"] = json!(false);
    }
    if protocol == "responses" {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
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
            rows.extend(
                response["tools"]["continuation"]
                    .as_array()
                    .ok_or("Missing response items.")?
                    .clone(),
            );
            rows.extend(results.iter().map(|r| json!({"type":"function_call_output","call_id":r["id"],"output":r["output"].to_string()})));
        }
        "anthropic" => {
            rows.push(response["tools"]["continuation"].clone());
            rows.push(json!({"role":"user","content":results.iter().map(|r|json!({"type":"tool_result","tool_use_id":r["id"],"content":r["output"].to_string(),"is_error":r["output"]["error"].is_string()})).collect::<Vec<_>>()}));
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
            rows.push(json!({"role":"user","parts":parts}));
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
