//! Incremental SSE framing and provider event normalization.
use crate::Result;
use crate::ai::http::MAX_OUTPUT;
use crate::ai::jobs::Job;
use crate::ai::{activity, tools, usage};
use serde_json::{Value, json};
use std::mem;
use std::sync::Arc;
use std::time::Instant;

pub(super) struct Sse {
    pub(super) tools: tools::Stream,
    job: Option<Arc<Job>>,
    kind: String,
    pending: Vec<u8>,
    data: String,
    pub(super) text: String,
    pub(super) reasoning: String,
    phase: Option<activity::Phase>,
    pub(super) usage: Value,
    counter: usage::Counter,
    first_output: Option<Instant>,
    last_output: Option<Instant>,
    pub(super) done: bool,
    stopped: bool,
}
impl Sse {
    pub(super) fn new(kind: &str, job: Option<Arc<Job>>) -> Self {
        Self {
            tools: tools::Stream::default(),
            job,
            kind: kind.into(),
            pending: Vec::new(),
            data: String::new(),
            text: String::new(),
            reasoning: String::new(),
            phase: None,
            usage: Value::Null,
            counter: usage::Counter::default(),
            first_output: None,
            last_output: None,
            done: false,
            stopped: false,
        }
    }
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        while let Some(i) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=i).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "Provider stream contains invalid text.")?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = mem::take(&mut self.data);
                    self.event(data.trim_end())?;
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                self.data.push('\n');
            }
            if self.data.len() > MAX_OUTPUT {
                return Err("Provider event is too large.".into());
            }
        }
        if self.pending.len() > MAX_OUTPUT
            || self.text.len() > MAX_OUTPUT
            || self.reasoning.len() > MAX_OUTPUT
        {
            return Err("Provider response is too large.".into());
        }
        Ok(())
    }
    pub(super) fn event(&mut self, data: &str) -> Result<()> {
        if data == "[DONE]" {
            if self.kind == "chat" && self.stopped {
                self.done = true;
            }
            return Ok(());
        }
        let v: Value =
            serde_json::from_str(data).map_err(|_| "Provider sent an unreadable stream event.")?;
        let gemini_offset = self.tools.gemini_len();
        self.tools.event(&self.kind, &v)?;
        if self.kind == "responses" && v["item"]["type"] == "message" {
            let phase = serde_json::from_value(v["item"]["phase"].clone()).ok();
            if v["type"] == "response.output_item.added" {
                self.phase = phase;
                if !self.text.is_empty() {
                    self.text.push_str("\n\n");
                }
            }
            if let Some(job) = &self.job {
                let mut tape = job
                    .activity
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                if v["type"] == "response.output_item.done" {
                    tape.phase(phase);
                }
                tape.close_output();
            }
        }
        let previous_output = self.text.len() + self.reasoning.len();
        let usage = if self.kind == "gemini" {
            &v["usageMetadata"]
        } else if self.kind == "responses" {
            &v["response"]["usage"]
        } else if v["type"] == "message_start" {
            &v["message"]["usage"]
        } else {
            &v["usage"]
        };
        if usage.is_object() {
            self.counter.update(&self.kind, usage);
        }
        if matches!(
            v["type"].as_str(),
            Some("response.reasoning_summary_text.delta" | "response.reasoning_text.delta")
        ) {
            self.output(true, v["delta"].as_str().unwrap_or_default())?;
        }
        if v["delta"]["type"] == "thinking_delta" {
            self.output(true, v["delta"]["thinking"].as_str().unwrap_or_default())?;
        }
        if self.kind == "chat" {
            self.output(
                true,
                v["choices"][0]["delta"]["reasoning_content"]
                    .as_str()
                    .or_else(|| v["choices"][0]["delta"]["reasoning"].as_str())
                    .unwrap_or_default(),
            )?;
            if let Some(details) = v["choices"][0]["delta"]["reasoning_details"].as_array() {
                for detail in details {
                    self.output(
                        true,
                        detail["text"]
                            .as_str()
                            .or_else(|| detail["summary"].as_str())
                            .unwrap_or_default(),
                    )?;
                }
            }
        }
        if v.get("error").is_some()
            || matches!(
                v["type"].as_str(),
                Some("error" | "response.failed" | "response.incomplete")
            )
        {
            return Err("Provider could not complete the request. Check model access and usage limits, then retry.".into());
        }
        let delta = match self.kind.as_str() {
            "responses" => {
                if v["type"] == "response.completed" {
                    self.done = true;
                }
                if v["type"] == "response.output_text.delta" {
                    v["delta"].as_str().unwrap_or("")
                } else {
                    ""
                }
            }
            "anthropic" => {
                if v["type"] == "message_stop" {
                    self.done = true;
                }
                if matches!(
                    v["delta"]["stop_reason"].as_str(),
                    Some("max_tokens" | "refusal")
                ) {
                    return Err("The provider stopped before completing its answer.".into());
                }
                if v["type"] == "content_block_delta" && v["delta"]["type"] == "text_delta" {
                    v["delta"]["text"].as_str().unwrap_or("")
                } else {
                    ""
                }
            }
            "gemini" => {
                if v["promptFeedback"]["blockReason"].is_string() {
                    return Err("The provider declined this request.".into());
                }
                if let Some(reason) = v["candidates"][0]["finishReason"].as_str() {
                    if reason != "STOP" {
                        return Err("The provider stopped before completing its answer.".into());
                    }
                    self.done = true;
                }
                if let Some(parts) = v["candidates"][0]["content"]["parts"].as_array() {
                    for (index, part) in parts.iter().enumerate() {
                        self.output(part["thought"] == true, part["text"].as_str().unwrap_or(""))?;
                        if let Some(call) = part.get("functionCall")
                            && let Some(job) = &self.job
                        {
                            let id = call["id"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| format!("call-{}", gemini_offset + index));
                            job.activity
                                .lock()
                                .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                                .prepare(&id, call["name"].as_str().unwrap_or_default())?;
                        }
                    }
                }
                ""
            }
            _ => {
                if let Some(reason) = v["choices"][0]["finish_reason"].as_str() {
                    if !matches!(reason, "stop" | "tool_calls") {
                        return Err("The provider stopped before completing its answer.".into());
                    }
                    self.stopped = true;
                }
                v["choices"][0]["delta"]["content"].as_str().unwrap_or("")
            }
        };
        self.output(false, delta)?;
        if let Some(job) = &self.job {
            let mut tape = job
                .activity
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            for (id, name) in self.tools.call_headers(&self.kind) {
                tape.prepare(&id, &name)?;
            }
            if self.done || self.stopped {
                tape.close_output();
            }
        }
        if self.text.len() + self.reasoning.len() > previous_output {
            let tick = Instant::now();
            self.first_output.get_or_insert(tick);
            self.last_output = Some(tick);
        }
        self.usage = self.counter.value(&self.kind);
        if let (Some(first), Some(last)) = (self.first_output, self.last_output) {
            let ms = u64::try_from(last.saturating_duration_since(first).as_millis())
                .unwrap_or(u64::MAX);
            if ms > 0 && self.usage["outputTokens"].as_u64().is_some_and(|n| n > 0) {
                self.usage["streamMs"] = json!(ms.min(usage::MAX_COUNT));
            }
        }
        Ok(())
    }
    fn output(&mut self, reasoning: bool, delta: &str) -> Result<()> {
        if let Some(job) = &self.job {
            job.activity
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                .output(reasoning, delta, self.phase)?;
        }
        if reasoning {
            self.reasoning.push_str(delta);
        } else {
            self.text.push_str(delta);
        }
        Ok(())
    }
}
