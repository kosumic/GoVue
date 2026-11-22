use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Notify, broadcast};

use crate::Core;

const SCRIPT: &str = include_str!("../../../src/youtube/generated/nativeShellTokenRuntime.js");
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REPLY: usize = 2_000_000;

#[derive(Deserialize)]
struct Snapshot {
    requests: Vec<String>,
    tasks: Vec<Delivery>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Delivery {
    request_id: String,
    task: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    request_id: String,
    task_id: u64,
    value: Option<String>,
}

struct Pending {
    id: u64,
    waiting: bool,
}

/// Services the Rust extractor with one isolated interpreter per request.
/// Each interpreter retains the snapshot state needed by the mint task.
#[derive(Clone)]
pub struct TokenRuntime {
    core: Core,
    element: ElementRef,
    ready: Rc<Cell<bool>>,
    work: Rc<RefCell<HashMap<String, Pending>>>,
    wake: Rc<Notify>,
}

impl TokenRuntime {
    pub fn new(core: Core) -> Self {
        let updates = core.subscribe();
        let runtime = Self {
            core,
            element: element_ref(),
            ready: Rc::new(Cell::new(false)),
            work: Rc::new(RefCell::new(HashMap::new())),
            wake: Rc::new(Notify::new()),
        };
        spawn(runtime.clone().run(updates));
        runtime
    }

    pub fn view(&self) -> NativeView {
        let runtime = self.clone();
        nsh_core::element::native("govue/token-runtime")
            .element_ref(&self.element)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .prop("script", json!(SCRIPT))
            .on_native_event(move |event: &NativeEvent| match event.name.as_ref() {
                "ready" => {
                    runtime.ready.set(true);
                    runtime.wake.notify_one();
                }
                "token" => {
                    if let Ok(reply) = Reply::deserialize(&event.data) {
                        runtime.reply(reply);
                    }
                }
                _ => {}
            })
    }

    async fn run(self, mut updates: broadcast::Receiver<String>) {
        loop {
            tokio::select! {
                () = self.wake.notified() => {},
                update = updates.recv() => match update {
                    Ok(channel) if channel == "extraction-token" => {},
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            if self.ready.get()
                && let Ok(snapshot) = self
                    .core
                    .execute::<Snapshot>(json!({"op":"extract_runtime_tasks"}))
                    .await
            {
                self.sync(snapshot);
            }
        }
    }

    fn sync(&self, snapshot: Snapshot) {
        self.work.borrow_mut().retain(|request, _| {
            let active = snapshot.requests.contains(request);
            if !active {
                self.element.command("release", json!(request));
            }
            active
        });
        for Delivery { request_id, task } in snapshot.tasks {
            let Some(id) = task["taskId"].as_u64().filter(|id| *id > 0) else {
                continue;
            };
            {
                let mut work = self.work.borrow_mut();
                if work
                    .get(&request_id)
                    .is_some_and(|previous| previous.id >= id)
                {
                    continue;
                }
                work.insert(request_id.clone(), Pending { id, waiting: true });
            }
            self.element
                .command("evaluate", json!({"requestId":request_id,"task":task}));
            let runtime = self.clone();
            spawn(async move {
                timer(TIMEOUT).await;
                runtime.reply(Reply {
                    request_id,
                    task_id: id,
                    value: None,
                });
            });
        }
    }

    fn reply(&self, reply: Reply) {
        {
            let mut work = self.work.borrow_mut();
            let Some(pending) = work.get_mut(&reply.request_id) else {
                return;
            };
            if pending.id != reply.task_id || !pending.waiting {
                return;
            }
            pending.waiting = false;
        }
        let runtime = self.clone();
        spawn(async move {
            if let Some(value) = reply
                .value
                .filter(|token| !token.is_empty() && token.len() <= MAX_REPLY)
                && runtime
                    .core
                    .execute::<Value>(json!({"op":"extract_token","requestId":reply.request_id,
                    "taskId":reply.task_id,"value":value}))
                    .await
                    .is_ok()
            {
                return;
            }
            runtime.element.command("release", json!(reply.request_id));
            let _ = runtime
                .core
                .execute::<Value>(json!({"op":"extract_release","requestId":reply.request_id}))
                .await;
        });
    }
}
