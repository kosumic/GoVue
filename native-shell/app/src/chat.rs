use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use govue_shell_runtime::{Core, CoreError};
use nsh_core::{
    element::{Owner, Spawned},
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

const SESSION: &str = "native:chat";
static REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub video_id: Option<String>,
    pub transcript: Option<ChatTranscript>,
    pub provider_id: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub active_task: Option<String>,
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub pending_messages: Vec<QueuedMessage>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct QueuedMessage {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatTranscript {
    pub video_id: String,
    pub track_id: String,
    pub title: String,
    pub language: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub status: String,
    pub error: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub blocks: Vec<ChatBlock>,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub provider_label: String,
    #[serde(default)]
    pub usage: Value,
    #[serde(default)]
    pub usage_details: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatBlock {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub content: Option<String>,
    pub explanation: Option<String>,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub uri: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedChat {
    pub id: String,
    pub title: String,
    pub video_id: Option<String>,
    pub model: String,
    pub pinned: bool,
    pub archived: bool,
    pub search_match: Option<HistoryMatch>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMatch {
    pub message_id: Option<String>,
    pub before: String,
    pub matched: String,
    pub after: String,
    pub leading: bool,
    pub trailing: bool,
}

#[derive(Deserialize)]
struct History {
    chats: Vec<SavedChat>,
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct Matches {
    pub query: String,
    pub matches: Vec<Hit>,
    pub selected: Option<usize>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub message_id: String,
    pub offset: usize,
}

impl Matches {
    pub fn current(&self) -> Option<&Hit> {
        self.matches.get(self.selected?)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    task_id: String,
    conversation: Conversation,
}

#[derive(Deserialize)]
struct Poll {
    state: Progress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    text: String,
    reply_id: Option<String>,
    status: String,
    reasoning: Option<String>,
    blocks: Option<Vec<ChatBlock>>,
    model: Option<String>,
    provider_label: Option<String>,
    usage: Value,
    usage_details: Value,
    pending_messages: Option<Vec<QueuedMessage>>,
}

/// Keeps the displayed conversation and its streaming task in sync with GoVue's
/// Rust chat store. Its owner is the app root, so closing the sheet keeps replies.
#[derive(Clone)]
pub struct Chat {
    core: Core,
    owner: Owner,
    epoch: Rc<Cell<u64>>,
    load: Rc<RefCell<Option<Spawned<()>>>>,
    task: Signal<Option<String>>,
    current: Signal<Option<Rc<Conversation>>>,
    config: Signal<Value>,
    history: Signal<Vec<SavedChat>>,
    history_epoch: Rc<Cell<u64>>,
    history_filter: Signal<(String, bool)>,
    pub history_pending: Signal<bool>,
    pub history_error: Signal<Option<String>>,
    matches: Signal<Option<Rc<Matches>>>,
    find_epoch: Rc<Cell<u64>>,
    visible: Signal<bool>,
    loading: Signal<bool>,
    sending: Signal<bool>,
    error: Signal<Option<String>>,
    draft_revision: Signal<u64>,
    pub sent_revision: Signal<u64>,
    draft_reasoning: Signal<Option<String>>,
    reasoning_revision: Rc<Cell<u64>>,
    settings_busy: Signal<bool>,
}

impl Chat {
    pub fn new(core: Core) -> Self {
        Self {
            core,
            owner: Owner::current(),
            epoch: Rc::new(Cell::new(REVISION.fetch_add(1, Ordering::Relaxed))),
            load: Rc::new(RefCell::new(None)),
            task: signal(None),
            current: signal(None),
            config: signal(Value::Null),
            history: signal(Vec::new()),
            history_epoch: Rc::new(Cell::new(0)),
            history_filter: signal((String::new(), false)),
            history_pending: signal(false),
            history_error: signal(None),
            matches: signal(None),
            find_epoch: Rc::new(Cell::new(0)),
            visible: signal(false),
            loading: signal(false),
            sending: signal(false),
            error: signal(None),
            draft_revision: signal(0),
            sent_revision: signal(0),
            draft_reasoning: signal(None),
            reasoning_revision: Rc::new(Cell::new(0)),
            settings_busy: signal(false),
        }
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }
    pub fn loading(&self) -> bool {
        self.loading.get()
    }
    pub fn busy(&self) -> bool {
        self.task.with(Option::is_some)
    }
    pub fn sending(&self) -> bool {
        self.sending.get()
    }
    pub fn error(&self) -> Option<String> {
        self.error.get()
    }
    pub fn current(&self) -> Option<Rc<Conversation>> {
        self.current.get()
    }
    pub fn history(&self) -> Vec<SavedChat> {
        self.history.get()
    }
    pub fn config(&self) -> Value {
        self.config.get()
    }

    pub fn draft_revision(&self) -> u64 {
        self.draft_revision.get()
    }

    pub fn reasoning(&self) -> Option<String> {
        self.current
            .with(|current| current.as_ref().map(|chat| chat.reasoning_effort.clone()))
            .unwrap_or_else(|| self.draft_reasoning.get())
    }

    pub fn settings_busy(&self) -> bool {
        self.settings_busy.get()
    }

    pub fn set_reasoning(&self, value: Option<String>, saved: impl FnOnce() + 'static) {
        if self.settings_busy() || self.sending() || self.loading() {
            return;
        }
        let Some(current) = self.current.get_untracked() else {
            self.draft_reasoning.set(value);
            saved();
            return;
        };
        let epoch = self.epoch.get();
        self.settings_busy.set(true);
        self.error.set(None);
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result: Result<Conversation, _> = chat.core.ai(json!({"op":"ai_chat_reasoning","chatId":current.id,"reasoningEffort":value})).await;
                chat.settings_busy.set(false);
                if !chat.is_current(epoch) { return; }
                match result {
                    Ok(updated) => {
                        chat.reasoning_revision.set(chat.reasoning_revision.get() + 1);
                        chat.current.update(|thread| {
                            if let Some(thread) = thread {
                                Rc::make_mut(thread).reasoning_effort = updated.reasoning_effort;
                            }
                        });
                        saved();
                    }
                    Err(error) => chat.error.set(Some(error.to_string())),
                }
            });
        });
    }

    pub fn core(&self) -> Core {
        self.core.clone()
    }

    pub async fn update_config(&self, request: Value) -> Result<Value, CoreError> {
        let config: Value = self.core.ai(request).await?;
        self.config.set(config.clone());
        Ok(config)
    }

    pub fn matches(&self) -> Option<Rc<Matches>> {
        self.matches.get()
    }

    pub fn find_messages(&self, query: String, direction: Option<&'static str>) {
        let find = self.find_epoch.get() + 1;
        self.find_epoch.set(find);
        let Some(thread) = self
            .current
            .get_untracked()
            .filter(|_| !query.trim().is_empty())
        else {
            self.matches.set(None);
            return;
        };
        let previous = self.matches.get_untracked().filter(|result| result.query == query.trim())
            .and_then(|result| result.current().map(|hit|json!({"query":result.query,"messageId":hit.message_id,"offset":hit.offset})));
        let version = self.epoch.get();
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                if direction.is_none() {
                    timer(Duration::from_millis(100)).await;
                }
                if chat.find_epoch.get() != find || !chat.is_current(version) {
                    return;
                }
                let result = chat
                    .core
                    .ai::<Matches>(json!({"op":"ai_chat_find","chatId":thread.id,
                "query":query,"previous":previous,"direction":direction}))
                    .await;
                if chat.find_epoch.get() == find && chat.is_current(version) {
                    match result {
                        Ok(matches) => chat.matches.set(Some(Rc::new(matches))),
                        Err(error) => chat.error.set(Some(error.to_string())),
                    }
                }
            })
        });
    }

    pub fn show(&self, video: Option<String>) {
        self.visible.set(true);
        self.read_config();
        self.refresh_history();
        if let Some(video) = video {
            let current = self.current.get_untracked();
            if current.as_ref().is_none_or(|chat| {
                chat.video_id.as_deref() != Some(&video) || chat.transcript.is_none()
            }) {
                self.open_video(video);
            }
        }
    }

    pub fn close(&self) {
        self.visible.set(false);
        if let Some(load) = self.load.borrow_mut().take() {
            load.cancel();
            self.loading.set(false);
        }
    }

    pub fn new_chat(&self) {
        self.stop();
        self.draft_revision.update(|revision| *revision += 1);
        self.draft_reasoning.set(None);
        self.current.set(None);
        self.matches.set(None);
        self.error.set(None);
    }

    pub fn open_video(&self, video: String) {
        self.open(json!({"op":"ai_chat_video_open","videoId":video}), None);
    }

    pub fn open_saved(&self, id: String, query: Option<String>) {
        if !self
            .current
            .with_untracked(|chat| chat.as_ref().is_some_and(|chat| chat.id == id))
        {
            self.open(json!({"op":"ai_chat_get","chatId":id}), query);
        } else if let Some(query) = query {
            self.find_messages(query, None);
        }
    }

    fn open(&self, request: Value, query: Option<String>) {
        self.new_chat();
        self.loading.set(true);
        let epoch = self.epoch.get();
        let chat = self.clone();
        let load = self.owner.run(|| {
            spawn(async move {
                let result = chat.load_context(request, epoch).await;
                if !chat.is_current(epoch) {
                    return;
                }
                chat.loading.set(false);
                chat.load.borrow_mut().take();
                match result {
                    Ok(conversation) => {
                        if let Some(task) = conversation.active_task.clone() {
                            chat.track_task(task, conversation.id.clone(), epoch);
                        }
                        chat.current.set(Some(Rc::new(conversation)));
                        if let Some(query) = query {
                            chat.find_messages(query, None);
                        }
                        chat.refresh_history();
                    }
                    Err(error) => chat.error.set(Some(error.to_string())),
                }
            })
        });
        *self.load.borrow_mut() = Some(load);
    }

    async fn load_context(&self, request: Value, epoch: u64) -> Result<Conversation, CoreError> {
        let chat: Conversation = self.core.ai(request).await?;
        if !self.is_current(epoch) {
            return Err(CoreError::Request("The chat changed.".into()));
        }
        self.current.set(Some(Rc::new(chat.clone())));
        let Some(video) = chat.video_id.as_ref().filter(|_| chat.transcript.is_none()) else {
            return Ok(chat);
        };
        let transcript = self
            .core
            .resolve_transcript(video.clone(), "default".into())
            .await?;
        self.core
            .ai(
                json!({"op":"ai_chat_video_open","videoId":video,"chatId":chat.id,
            "transcript":{"videoId":transcript.video_id,"trackId":transcript.track_id}}),
            )
            .await
    }

    pub fn find_chats(&self, query: String, archived: bool) {
        self.history_filter.set((query.trim().into(), archived));
        self.history.set(Vec::new());
        self.refresh_history();
    }

    fn refresh_history(&self) {
        let (query, archived) = self.history_filter.get_untracked();
        let epoch = self.history_epoch.get() + 1;
        self.history_epoch.set(epoch);
        self.history_pending.set(true);
        self.history_error.set(None);
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                if !query.is_empty() {
                    timer(Duration::from_millis(150)).await;
                }
                if chat.history_epoch.get() != epoch {
                    return;
                }
                let result = chat
                    .core
                    .ai::<History>(json!({"op":"ai_chats","query":query,"archived":archived}))
                    .await;
                if chat.history_epoch.get() == epoch {
                    chat.history_pending.set(false);
                    match result {
                        Ok(result) => chat.history.set(result.chats),
                        Err(error) => chat.history_error.set(Some(error.to_string())),
                    }
                }
            })
        });
    }

    pub async fn update_saved(&self, id: String, mut request: Value) -> Result<(), CoreError> {
        request["op"] = json!("ai_chat_update");
        request["chatId"] = json!(id);
        let saved: Conversation = self.core.ai(request).await?;
        self.current.update(|current| {
            if let Some(current) = current.as_mut().filter(|current| current.id == id) {
                Rc::make_mut(current).title = saved.title;
            }
        });
        self.refresh_history();
        Ok(())
    }

    pub async fn delete_saved(&self, id: String) -> Result<(), CoreError> {
        let _: Value = self
            .core
            .ai(json!({"op":"ai_chat_delete","chatId":id}))
            .await?;
        if self
            .current
            .with_untracked(|current| current.as_ref().is_some_and(|chat| chat.id == id))
        {
            self.new_chat();
        }
        self.refresh_history();
        Ok(())
    }

    pub fn read_config(&self) {
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                match chat.core.ai(json!({"op":"ai_config"})).await {
                    Ok(config) => chat.config.set(config),
                    Err(error) => chat.error.set(Some(error.to_string())),
                }
            })
        });
    }

    pub fn choose_provider(&self, id: String, model: Option<String>) {
        let chat = self.clone();
        let epoch = self.epoch.get();
        let current = self.current.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                let mut request = json!({"op":"ai_select","id":id});
                if let Some(model) = &model {
                    request["model"] = json!(model);
                }
                let config = chat.core.ai(request).await;
                let Ok(config) = config else {
                    chat.error.set(config.err().map(|error| error.to_string()));
                    return;
                };
                chat.config.set(config);
                if !chat.is_current(epoch) {
                    return;
                }
                if let Some(current) = current.filter(|c| c.active_task.is_none()) {
                    if current.video_id.is_none() {
                        if current.provider_id != id
                            || model.as_ref().is_some_and(|model| *model != current.model)
                        {
                            chat.new_chat();
                        }
                        return;
                    }
                    match chat
                        .core
                        .ai::<Conversation>(
                            json!({"op":"ai_chat_model","chatId":current.id,"id":id}),
                        )
                        .await
                    {
                        Ok(saved) if chat.is_current(epoch) => {
                            chat.current.set(Some(Rc::new(saved)))
                        }
                        Err(error) if chat.is_current(epoch) => {
                            chat.error.set(Some(error.to_string()))
                        }
                        _ => {}
                    }
                }
            })
        });
    }

    pub fn send(
        &self,
        prompt: String,
        attachments: Vec<String>,
        edit: Option<String>,
        accepted: impl FnOnce() + 'static,
    ) {
        let prompt = prompt.trim();
        if prompt.is_empty() && attachments.is_empty() {
            return;
        }
        let mut command = json!({"prompt":prompt,"attachments":attachments});
        if let Some(id) = edit {
            if self.busy() {
                return;
            }
            command["editMessageId"] = json!(id);
        }
        self.send_turn(command, accepted);
    }

    pub fn send_queued(&self, id: String) {
        if !self.busy()
            && let Some(current) = self.current.get_untracked()
            && let Some(message) = current.pending_messages.iter().find(|m| m.id == id)
        {
            self.send_turn(
                json!({"prompt":message.content,"queuedMessageId":id}),
                || {},
            );
        }
    }

    pub fn retry(&self, id: String) {
        if !self.busy()
            && self.current.with_untracked(|chat| {
                chat.as_ref().is_some_and(|chat| {
                    chat.messages.iter().any(|message| {
                        message.id == id
                            && message.role == "assistant"
                            && message.status != "streaming"
                    })
                })
            })
        {
            self.send_turn(json!({"prompt":"","retry":true,"retryMessageId":id}), || {});
        }
    }

    fn send_turn(&self, mut command: Value, accepted: impl FnOnce() + 'static) {
        if self.sending.get_untracked()
            || self.loading.get_untracked()
            || self.settings_busy.get_untracked()
        {
            return;
        }
        let current = self.current.get_untracked();
        if current
            .as_ref()
            .is_some_and(|chat| chat.video_id.is_some() && chat.transcript.is_none())
        {
            return;
        }
        let id = current
            .as_ref()
            .map(|chat| chat.provider_id.clone())
            .filter(|id| !id.is_empty())
            .or_else(|| {
                self.config
                    .with_untracked(|config| config["activeId"].as_str().map(str::to_owned))
            });
        let Some(id) = id else {
            self.error
                .set(Some("Choose a model to start chatting.".into()));
            return;
        };
        let epoch = self.epoch.get();
        let queue = self.task.get_untracked().is_some();
        command["id"] = json!(id);
        command["chatId"] = json!(current.as_ref().map(|chat| &chat.id));
        command["reasoningEffort"] = json!(self.reasoning());
        let clear_feedback =
            command.get("retryMessageId").is_some() || command.get("editMessageId").is_some();
        let request = json!({"op":"ai_chat_session_send","sessionId":SESSION,"version":epoch,"queue":queue,
            "clearFeedback":clear_feedback,"command":command});
        self.sending.set(true);
        self.error.set(None);
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = chat.core.ai::<Started>(request).await;
                if !chat.is_current(epoch) {
                    return;
                }
                chat.sending.set(false);
                match result {
                    Ok(started) => {
                        accepted();
                        let existing = chat.task.get_untracked();
                        if existing.as_ref() != Some(&started.task_id) {
                            chat.track_task(
                                started.task_id,
                                started.conversation.id.clone(),
                                epoch,
                            );
                            chat.current.set(Some(Rc::new(started.conversation)));
                        }
                        chat.sent_revision.update(|revision| *revision += 1);
                    }
                    Err(error) => chat.error.set(Some(error.to_string())),
                }
            })
        });
    }

    pub fn branch(&self, id: String) {
        if self.busy() || self.sending() || self.loading() {
            return;
        }
        let Some(current) = self.current.get_untracked() else {
            return;
        };
        let epoch = self.epoch.get();
        self.loading.set(true);
        self.error.set(None);
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = chat
                    .core
                    .ai::<Conversation>(
                        json!({"op":"ai_chat_branch","chatId":current.id,"messageId":id}),
                    )
                    .await;
                if chat.is_current(epoch) {
                    chat.loading.set(false);
                    match result {
                        Ok(conversation) => {
                            chat.new_chat();
                            chat.current.set(Some(Rc::new(conversation)));
                            chat.refresh_history();
                        }
                        Err(error) => chat.error.set(Some(error.to_string())),
                    }
                }
            });
        });
    }

    pub fn stop(&self) {
        if let Some(load) = self.load.borrow_mut().take() {
            load.cancel();
        }
        let epoch = REVISION.fetch_add(1, Ordering::Relaxed);
        self.epoch.set(epoch);
        self.task.set(None);
        self.loading.set(false);
        self.sending.set(false);
        let chat = self.clone();
        let id = self.current.get_untracked().map(|chat| chat.id.clone());
        self.owner.run(|| {
            spawn(async move {
                let result = chat
                    .core
                    .ai::<Value>(json!({"op":"ai_chat_session_cancel","sessionId":SESSION,
                "version":epoch,"chatId":id,"clearFeedback":true}))
                    .await;
                if chat.is_current(epoch)
                    && let Ok(value) = result
                    && let Ok(saved) = Conversation::deserialize(&value["conversation"])
                    && chat.current.with_untracked(|current| {
                        current
                            .as_ref()
                            .is_some_and(|current| current.id == saved.id)
                    })
                {
                    chat.current.set(Some(Rc::new(saved)));
                }
            })
        });
    }

    fn track_task(&self, task: String, id: String, epoch: u64) {
        self.task.set(Some(task.clone()));
        let chat = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = chat.watch(&task, &id, epoch).await;
                if chat.owns(&task, epoch) {
                    chat.task.set(None);
                    if let Err(error) = result {
                        chat.error.set(Some(error.to_string()));
                    }
                    chat.refresh_history();
                }
            })
        });
    }

    async fn watch(&self, task: &str, id: &str, epoch: u64) -> Result<(), CoreError> {
        let mut updates = self.core.subscribe();
        let channel = format!("ai:{task}");
        while self.owns(task, epoch) {
            let status: Poll = self
                .core
                .ai(json!({"op":"ai_chat_poll","taskId":task}))
                .await?;
            if !self.owns(task, epoch) {
                return Ok(());
            }
            if status.state.status != "running" {
                let revision = self.reasoning_revision.get();
                let mut saved: Conversation = self
                    .core
                    .ai(json!({"op":"ai_chat_get","chatId":id}))
                    .await?;
                let _ = self
                    .core
                    .ai::<Value>(json!({"op":"ai_chat_session_forget","sessionId":SESSION,
                    "version":epoch,"taskId":task}))
                    .await;
                if self.owns(task, epoch) {
                    if revision != self.reasoning_revision.get() {
                        saved.reasoning_effort = self.reasoning();
                    }
                    self.current.set(Some(Rc::new(saved)));
                }
                return Ok(());
            }
            let reply = status.state.reply_id.as_deref();
            let current = match self.current.get_untracked() {
                Some(current)
                    if reply.is_none_or(|reply| {
                        current.messages.last().is_some_and(|m| m.id == reply)
                    }) =>
                {
                    current
                }
                _ => Rc::new(
                    self.core
                        .ai::<Conversation>(json!({"op":"ai_chat_get","chatId":id}))
                        .await?,
                ),
            };
            if !self.owns(task, epoch) {
                return Ok(());
            }
            let mut updated = (*current).clone();
            if let Some(pending) = status.state.pending_messages {
                updated.pending_messages = pending;
            }
            if let Some(message) = updated
                .messages
                .last_mut()
                .filter(|m| m.role == "assistant" && reply.is_none_or(|reply| m.id == reply))
            {
                message.content = status.state.text;
                message.reasoning = status.state.reasoning.unwrap_or_default();
                if let Some(blocks) = status.state.blocks {
                    message.blocks = blocks;
                }
                if let Some(model) = status.state.model {
                    message.model = model;
                }
                if let Some(provider) = status.state.provider_label {
                    message.provider_label = provider;
                }
                message.usage = status.state.usage;
                message.usage_details = status.state.usage_details;
            }
            self.current.set(Some(Rc::new(updated)));
            tokio::select! {
                () = timer(Duration::from_secs(2)) => {},
                () = async {
                    while let Ok(update) = updates.recv().await {
                        if update == channel { break; }
                    }
                } => {},
            }
        }
        Ok(())
    }

    fn is_current(&self, epoch: u64) -> bool {
        self.epoch.get() == epoch
    }

    fn owns(&self, task: &str, epoch: u64) -> bool {
        self.is_current(epoch)
            && self
                .task
                .with_untracked(|selected| selected.as_deref() == Some(task))
    }
}
