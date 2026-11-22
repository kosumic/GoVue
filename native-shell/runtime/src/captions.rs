use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use nsh_core::prelude::{cleanup, spawn};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, broadcast};

use crate::{BrowserDocument, BrowserEvent, BrowserTab, Core};

const SCRIPT: &str = include_str!("../../../src/youtube/captionBridge.js");
const MAX_REPLY_BYTES: usize = 8_000_000;
static NEXT_ADAPTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct Registration {
    id: String,
    video: String,
    tab: BrowserTab,
    document: BrowserDocument,
    ready: bool,
}

impl Registration {
    fn matches(&self, tab: &BrowserTab, document: &BrowserDocument) -> bool {
        self.tab.is_same(tab) && self.document.same_document(document)
    }

    fn cancel<'a>(&self, requests: impl Iterator<Item = &'a String>) {
        let requests = requests.collect::<Vec<_>>();
        if !requests.is_empty() {
            self.tab.inject(
                &self.document,
                &format!(
                    "for (const id of {}) window.__govueCaptions?.cancel(id);",
                    json!(requests)
                ),
            );
        }
    }
}

#[derive(Deserialize)]
struct Snapshot {
    tasks: Vec<Delivery>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Delivery {
    request_id: String,
    task: Task,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    adapter_id: String,
    task_id: u64,
    video_id: String,
    track_id: String,
    #[serde(flatten)]
    options: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageReply {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    task_id: u64,
    #[serde(default)]
    video_id: String,
    #[serde(default)]
    track_id: String,
}

struct Pending {
    task: u64,
    track: String,
    answered: bool,
}

#[derive(Default)]
struct State {
    selected: Option<BrowserTab>,
    registration: Option<Registration>,
    pending: HashMap<String, Pending>,
}

/// One live caption source per GoVue app. The active tab serves caption pages
/// for the Rust reader and player through the same extraction jobs as GoVue.
#[derive(Clone)]
pub struct CaptionPage {
    core: Core,
    state: Rc<RefCell<State>>,
    wake: Rc<Notify>,
}

impl CaptionPage {
    pub fn new(core: Core) -> Self {
        let updates = core.subscribe();
        let page = Self {
            core,
            state: Rc::new(RefCell::new(State::default())),
            wake: Rc::new(Notify::new()),
        };
        spawn(page.clone().run(updates));
        cleanup({
            let page = page.clone();
            move || page.select(None)
        });
        page
    }

    /// Select the visible tab. A parked tab remains mounted and can be
    /// selected again when its document is brought forward.
    pub fn select(&self, tab: Option<&BrowserTab>) {
        self.state.borrow_mut().selected = tab.cloned();
        self.update_registration();
    }

    fn update_registration(&self) {
        let mut state = self.state.borrow_mut();
        let target = state.selected.as_ref().and_then(|tab| {
            let document = tab.document.get_untracked()?;
            Some((tab.clone(), document))
        });
        if let Some((tab, target)) = &target
            && let Some(registration) = &mut state.registration
            && registration.matches(tab, &target.document)
        {
            if target.ready && !registration.ready {
                registration.ready = true;
                self.wake.notify_one();
            }
            return;
        }
        if let Some(previous) = state.registration.take() {
            let _ = self.core.register_caption_page(&previous.id, None);
            previous.cancel(state.pending.keys());
            state.pending.clear();
        }
        if let Some((tab, target)) = target
            && let Ok(identity) = self
                .core
                .state::<Value>(json!({"op":"browser_identity", "url": target.document.url}))
            && let Some(video) = identity["videoId"].as_str()
        {
            let id = format!(
                "caption-page:{}",
                NEXT_ADAPTER.fetch_add(1, Ordering::Relaxed)
            );
            if self.core.register_caption_page(&id, Some(video)).is_ok() {
                state.registration = Some(Registration {
                    id,
                    video: video.to_owned(),
                    tab,
                    document: target.document,
                    ready: target.ready,
                });
                if target.ready {
                    self.wake.notify_one();
                }
            }
        }
    }

    pub(crate) fn receive(&self, tab: &BrowserTab, event: &BrowserEvent) -> bool {
        match event {
            BrowserEvent::Ready { .. }
            | BrowserEvent::Navigation { .. }
            | BrowserEvent::Error { .. } => {}
            BrowserEvent::PageMessage { document, raw, .. } => {
                return self.reply(tab, document, raw);
            }
            _ => return false,
        }
        if self
            .state
            .borrow()
            .selected
            .as_ref()
            .is_some_and(|selected| selected.is_same(tab))
        {
            self.update_registration();
        }
        false
    }

    fn reply(&self, tab: &BrowserTab, document: &BrowserDocument, raw: &str) -> bool {
        if raw.len() > MAX_REPLY_BYTES {
            return true;
        }
        let Ok(reply) = serde_json::from_str::<PageReply>(raw) else {
            return false;
        };
        if reply.kind != "captionPage" {
            return false;
        }
        let mut state = self.state.borrow_mut();
        if state.registration.as_ref().is_some_and(|registration| {
            registration.matches(tab, document) && registration.video == reply.video_id
        }) && let Some(pending) = state.pending.get_mut(&reply.request_id)
            && (pending.task, &pending.track) == (reply.task_id, &reply.track_id)
            && !pending.answered
        {
            pending.answered = true;
            let request = json!({"op":"extract_page", "requestId":reply.request_id,
                "origin":document.url, "raw":raw});
            let core = self.core.clone();
            spawn(async move {
                let _ = core.execute::<Value>(request).await;
            });
        }
        true
    }

    async fn run(self, mut updates: broadcast::Receiver<String>) {
        loop {
            tokio::select! {
                () = self.wake.notified() => {},
                update = updates.recv() => match update {
                    Ok(channel) if channel == "extraction-page" => {},
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            let Some(registration) = self
                .state
                .borrow()
                .registration
                .as_ref()
                .filter(|registration| registration.ready)
                .cloned()
            else {
                continue;
            };
            if let Ok(snapshot) = self
                .core
                .execute::<Snapshot>(json!({"op":"extract_page_tasks"}))
                .await
            {
                self.deliver(&registration, snapshot);
            }
        }
    }

    fn deliver(&self, registration: &Registration, snapshot: Snapshot) {
        let mut state = self.state.borrow_mut();
        if state.registration.as_ref().map(|current| &current.id) != Some(&registration.id) {
            return;
        }
        let tasks = snapshot
            .tasks
            .into_iter()
            .filter(|delivery| {
                delivery.task.adapter_id == registration.id
                    && delivery.task.video_id == registration.video
            })
            .map(|delivery| (delivery.request_id, delivery.task))
            .collect::<HashMap<_, _>>();
        let mut cancelled = Vec::new();
        state.pending.retain(|request, pending| {
            let current = tasks
                .get(request)
                .is_some_and(|task| task.task_id == pending.task);
            if !current {
                cancelled.push(request.clone());
            }
            current
        });
        registration.cancel(cancelled.iter());
        for (request, task) in tasks {
            if let std::collections::hash_map::Entry::Vacant(slot) = state.pending.entry(request) {
                let script = format!(
                    "{SCRIPT}\nwindow.__govueCaptions?.request({},{});",
                    json!(slot.key()),
                    json!(task)
                );
                slot.insert(Pending {
                    task: task.task_id,
                    track: task.track_id,
                    answered: false,
                });
                registration.tab.inject(&registration.document, &script);
            }
        }
    }
}
