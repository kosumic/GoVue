//! Native credential and HTTPS callbacks, bounded provider responses and cancellation.
use crate::Result;
use crate::ai::jobs::Job;
use crate::ai::sse::Sse;
use crate::ai::tools;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

pub(super) const MAX_OUTPUT: usize = 2_000_000;
static PLATFORM: OnceLock<Box<dyn AiPlatform>> = OnceLock::new();

/// Result of reading one credential from the platform's protected storage.
#[derive(uniffi::Record)]
pub struct AiSecret {
    /// False when storage could not be unlocked or read.
    pub ok: bool,
    /// Stored credential text; empty with `ok = true` when the key is absent.
    pub value: String,
}
/// A provider request header. Values may contain credentials and must not be logged.
#[derive(uniffi::Record)]
pub struct AiHeader {
    pub name: String,
    pub value: String,
}
/// One HTTPS provider request, used only in memory by the native adapter.
#[derive(uniffi::Record)]
pub struct AiHttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<AiHeader>,
    pub body: String,
}
/// Protected storage and blocking HTTPS effects shared by all provider protocols.
///
/// Rust calls these methods on workers. The adapter must support concurrent
/// requests and keep credentials, signed URLs and raw responses out of logs and
/// persistent HTTP caches. Protocol validation and parsing remain in Rust.
#[uniffi::export(callback_interface)]
pub trait AiPlatform: Send + Sync {
    /// Reads a protected key, distinguishing an absent value from a storage error.
    fn secret_read(&self, key: String) -> AiSecret;
    /// Writes a protected key, or deletes it for an empty value; false means failure.
    fn secret_write(&self, key: String, value: String) -> bool;
    /// Completes one request synchronously, with automatic redirects disabled.
    ///
    /// Call `start` when headers arrive, then `push` for each body chunk, and
    /// return only when the response finishes or stops. Check `is_cancelled`
    /// during I/O and stop when `push` returns false. Report transport failure
    /// with `failed`; Rust handles HTTP errors from the status passed to `start`.
    fn request(&self, request: AiHttpRequest, response: Arc<AiResponse>);
}
/// Installs the provider adapter. The first adapter remains installed for this process.
#[uniffi::export]
pub fn install_ai_platform(platform: Box<dyn AiPlatform>) {
    let _ = PLATFORM.set(platform);
}

pub(super) fn platform() -> Result<&'static dyn AiPlatform> {
    PLATFORM
        .get()
        .map(std::convert::AsRef::as_ref)
        .ok_or("AI platform is not available in this build.".into())
}

pub(super) struct ResponseState {
    pub(super) status: i32,
    pub(super) bytes: Vec<u8>,
    pub(super) stream: Option<Sse>,
    pub(super) error: Option<crate::Error>,
    pub(super) total: usize,
}
/// Correlated response sink retained only for its pending provider request.
#[derive(uniffi::Object)]
pub struct AiResponse {
    pub(super) state: Mutex<ResponseState>,
    pub(super) job: Option<Arc<Job>>,
}
#[uniffi::export]
impl AiResponse {
    /// Whether cancellation, expiry or unavailable state requires the adapter to stop.
    pub fn is_cancelled(&self) -> bool {
        self.state.is_poisoned() || self.job.as_ref().is_some_and(|j| j.pending().is_err())
    }
    /// Records the HTTP status before any response body is pushed.
    pub fn start(&self, status: i32) {
        if let Ok(mut state) = self.state.lock() {
            state.status = status;
        }
    }
    /// Consumes the next body chunk in order, preserving partial UTF-8 sequences.
    ///
    /// False means the request was cancelled, exceeded its limits, failed parsing
    /// or encountered unavailable state. Release network resources and stop pushing.
    pub fn push(&self, bytes: Vec<u8>) -> bool {
        if self.job.as_ref().is_some_and(|j| j.pending().is_err()) {
            return false;
        }
        let Ok(mut s) = self.state.lock() else {
            return false;
        };
        s.total += bytes.len();
        let result = if s.total > 20_000_000 {
            Err("Provider response is too large.".into())
        } else if (200..300).contains(&s.status)
            && let Some(parser) = s.stream.as_mut()
        {
            parser.push(&bytes).and_then(|()| {
                if let Some(job) = &self.job {
                    let mut state = job
                        .state
                        .lock()
                        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                    state["text"] = json!(format!(
                        "{}{}",
                        state["textPrefix"].as_str().unwrap_or_default(),
                        parser.text
                    ));
                    state["reasoning"] = json!(format!(
                        "{}{}",
                        state["reasoningPrefix"].as_str().unwrap_or_default(),
                        parser.reasoning
                    ));
                    state["usage"] = tools::merge_usage(&state["usagePrefix"], &parser.usage);
                    state["blocks"] = json!(
                        job.activity
                            .lock()
                            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                            .blocks
                    );
                    drop(state);
                    job.notifications.changed();
                }
                Ok(())
            })
        } else if s.bytes.len() + bytes.len() <= MAX_OUTPUT {
            s.bytes.extend(bytes);
            Ok(())
        } else {
            Err("Provider response is too large.".into())
        };
        if let Err(e) = result {
            s.error = Some(e);
            drop(s);
            return false;
        }
        true
    }
    /// Records a transport failure without exposing the upstream body or credentials.
    pub fn failed(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.error =
                Some("Could not reach the provider. Check your connection and try again.".into());
        }
    }
}
pub(super) fn http(
    url: String,
    method: &str,
    headers: Vec<(&str, String)>,
    body: String,
    stream: Option<&str>,
    job: Option<&Arc<Job>>,
) -> Result<Value> {
    http_impl(url, method, headers, body, stream, job, false)
}
pub(super) fn http_impl(
    url: String,
    method: &str,
    headers: Vec<(&str, String)>,
    body: String,
    stream: Option<&str>,
    job: Option<&Arc<Job>>,
    raw: bool,
) -> Result<Value> {
    if let Some(j) = job {
        j.pending()?;
    }
    let response = Arc::new(AiResponse {
        state: Mutex::new(ResponseState {
            status: 0,
            bytes: Vec::new(),
            stream: stream.map(|kind| Sse::new(kind, job.cloned())),
            error: None,
            total: 0,
        }),
        job: job.cloned(),
    });
    platform()?.request(
        AiHttpRequest {
            url,
            method: method.into(),
            headers: headers
                .into_iter()
                .map(|(n, v)| AiHeader {
                    name: n.into(),
                    value: v,
                })
                .collect(),
            body,
        },
        response.clone(),
    );
    if let Some(j) = job {
        j.pending()?;
    }
    let s = response
        .state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    if let Some(j) = job {
        j.last_status.store(s.status, Ordering::Relaxed);
    }
    if let Some(e) = &s.error {
        return Err(e.clone());
    }
    if raw {
        return Ok(
            json!({"status":s.status,"data":serde_json::from_slice::<Value>(&s.bytes).unwrap_or(Value::Null)}),
        );
    }
    if !(200..300).contains(&s.status) {
        // Never return arbitrary upstream bodies: they may echo credentials or caption text.
        return Err(match s.status {
            401 | 403 => {
                "Provider denied access. Check your key, account, model and plan permissions."
                    .into()
            }
            429 => "Provider usage limit reached. Check your plan or try again later.".into(),
            n => {
                format!("Provider request failed (HTTP {n}). Check the endpoint and model.").into()
            }
        });
    }
    if let Some(parser) = &s.stream {
        if !parser.done {
            return Err("The provider stream ended before completion. Please retry.".into());
        }
        let tools = parser
            .tools
            .value(stream.unwrap_or_default(), &parser.text, &parser.reasoning);
        if parser.text.trim().is_empty() && tools["calls"].as_array().is_none_or(Vec::is_empty) {
            return Err("The provider completed without a text response.".into());
        }
        return Ok(
            json!({"text":parser.text,"reasoning":parser.reasoning,"usage":parser.usage,"tools":tools}),
        );
    }
    if s.bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&s.bytes).map_err(|_| "Provider returned an unreadable response.".into())
}
pub(super) fn json_http(
    url: String,
    headers: Vec<(&str, String)>,
    body: Option<Value>,
    job: &Arc<Job>,
) -> Result<Value> {
    let mut headers = headers;
    headers.push(("Content-Type", "application/json".into()));
    http(
        url,
        if body.is_some() { "POST" } else { "GET" },
        headers,
        body.map(|v| v.to_string()).unwrap_or_default(),
        None,
        Some(job),
    )
}
