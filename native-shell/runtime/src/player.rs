use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nsh_core::{element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::oneshot;

const COMPONENT: &str = "govue/player-runtime";
const MAX_PENDING: usize = 4;
const MAX_SOURCE: usize = 2_000_000;
const MAX_SIGNATURE: usize = 10_000;
const TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_TASK: AtomicU64 = AtomicU64::new(1);

/// The only fields a player evaluation can send to its caller.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct PlayerSignatures {
    pub n: Option<String>,
    pub sig: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerRuntimeError {
    InvalidRequest,
    Busy,
    Cancelled,
    TimedOut,
    Failed,
}

impl fmt::Display for PlayerRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest | Self::Busy => "Player script is too large or busy.",
            Self::Cancelled => "Extraction cancelled.",
            Self::TimedOut => "Player script timed out.",
            Self::Failed => "Could not solve the YouTube player signature.",
        })
    }
}

impl std::error::Error for PlayerRuntimeError {}

type Outcome = Result<PlayerSignatures, PlayerRuntimeError>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    request_id: String,
    task_id: String,
    signatures: Option<PlayerSignatures>,
}

struct Job {
    request_id: String,
    task_id: String,
    source: Option<String>,
    reply: oneshot::Sender<Outcome>,
}

/// Bind one instance to the owner of the current playback surface. The native
/// view is an isolated renderer and occupies no space in the player layout.
#[derive(Clone)]
pub struct PlayerRuntime {
    element: ElementRef,
    jobs: Rc<RefCell<VecDeque<Job>>>,
}

impl PlayerRuntime {
    /// Creates the runtime and its owner-scoped cancellation handler.
    pub fn new() -> Self {
        let runtime = Self {
            element: element_ref(),
            jobs: Rc::new(RefCell::new(VecDeque::new())),
        };
        cleanup({
            let runtime = runtime.clone();
            move || runtime.remove(|_| true, PlayerRuntimeError::Cancelled)
        });
        runtime
    }

    /// Mount once next to the GoVue player. Recreating the element cancels its
    /// native work, including any script that was running when it was removed.
    pub fn view(&self) -> NativeView {
        let runtime = self.clone();
        nsh_core::element::native(COMPONENT)
            .element_ref(&self.element)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .on_native_event(move |event: &NativeEvent| runtime.receive(event))
    }

    /// Evaluates the source extracted for a pending media request. The caller
    /// cancels that request when its video or document changes.
    pub async fn evaluate(&self, request_id: &str, source: String) -> Outcome {
        if request_id.is_empty()
            || request_id.len() > 100
            || request_id.chars().any(char::is_control)
            || source.is_empty()
            || source.encode_utf16().count() > MAX_SOURCE
        {
            return Err(PlayerRuntimeError::InvalidRequest);
        }
        let task_id = NEXT_TASK.fetch_add(1, Ordering::Relaxed).to_string();
        let (send, reply) = oneshot::channel();
        {
            let mut jobs = self.jobs.borrow_mut();
            if jobs.len() >= MAX_PENDING {
                return Err(PlayerRuntimeError::Busy);
            }
            jobs.push_back(Job {
                request_id: request_id.to_owned(),
                task_id: task_id.clone(),
                source: Some(source),
                reply: send,
            });
        }
        let _pending = PendingEvaluation(self, &task_id);
        self.dispatch();
        tokio::select! {
            result = reply => result.unwrap_or(Err(PlayerRuntimeError::Cancelled)),
            () = timer(TIMEOUT) => {
                self.remove(|job| job.task_id == task_id, PlayerRuntimeError::TimedOut);
                Err(PlayerRuntimeError::TimedOut)
            }
        }
    }

    pub fn cancel(&self, request_id: &str) {
        self.remove(
            |job| job.request_id == request_id,
            PlayerRuntimeError::Cancelled,
        );
    }

    fn dispatch(&self) {
        if let Some(job) = self.jobs.borrow_mut().front_mut()
            && let Some(source) = job.source.take()
        {
            self.element.command(
                "evaluate",
                json!({"requestId": job.request_id, "taskId": job.task_id, "source": source}),
            );
        }
    }

    fn receive(&self, event: &NativeEvent) {
        if event.name.as_ref() != "evaluated" {
            return;
        }
        let Ok(reply) = serde_json::from_value::<Reply>(event.data.clone()) else {
            return;
        };
        let job = {
            let mut jobs = self.jobs.borrow_mut();
            if !jobs.front().is_some_and(|job| {
                job.source.is_none()
                    && job.request_id == reply.request_id
                    && job.task_id == reply.task_id
            }) {
                return;
            }
            jobs.pop_front()
        };
        if let Some(job) = job {
            let result = reply
                .signatures
                .ok_or(PlayerRuntimeError::Failed)
                .map(|mut values| {
                    values.n = bounded(values.n);
                    values.sig = bounded(values.sig);
                    values
                });
            let _ = job.reply.send(result);
        }
        self.dispatch();
    }

    fn remove(&self, matches: impl Fn(&Job) -> bool, error: PlayerRuntimeError) {
        let mut jobs = self.jobs.borrow_mut();
        let active = jobs
            .front()
            .filter(|job| job.source.is_none() && matches(job))
            .map(|job| job.task_id.clone());
        for job in std::mem::take(&mut *jobs) {
            if matches(&job) {
                let _ = job.reply.send(Err(error));
            } else {
                jobs.push_back(job);
            }
        }
        drop(jobs);
        if let Some(task_id) = active {
            self.element.command("cancel", json!(task_id));
            self.dispatch();
        }
    }
}

impl Default for PlayerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

struct PendingEvaluation<'a>(&'a PlayerRuntime, &'a str);

impl Drop for PendingEvaluation<'_> {
    fn drop(&mut self) {
        self.0
            .remove(|job| job.task_id == self.1, PlayerRuntimeError::Cancelled);
    }
}

fn bounded(value: Option<String>) -> Option<String> {
    value.filter(|value| value.encode_utf16().count() <= MAX_SIGNATURE)
}
