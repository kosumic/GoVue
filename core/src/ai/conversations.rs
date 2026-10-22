//! Local conversations and provider-independent turn construction.
use super::{
    CONFIG_LOCK, Job, activity, catalog, chat_sessions, config, games, jobs, new_job, now,
    practice, provider, random, replay, search, usage, valid_id,
};
use crate::{Result, string};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

const MAX_CHAT: usize = 4_000_000;
const MAX_CHATS: usize = 100;

mod store;
use store::{Metadata, list, load_all, path, read, recover, save};
static CHATS: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptRef {
    video_id: String,
    track_id: String,
    title: String,
    language: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PreparedInput {
    pub content: String,
    pub transcript: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage_chat_id: Option<String>,
    role: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prepared_input: Option<PreparedInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    games: Vec<games::Game>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<activity::Block>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    usage: Value,
    // Projected histories can be replayed; derived counts are always recomputed.
    #[serde(default, rename = "usageDetails", skip_serializing)]
    _usage_details: serde::de::IgnoredAny,
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider_label: String,
    #[serde(default)]
    reasoning_effort: String,
    #[serde(default)]
    provider_id: String,
    #[serde(default)]
    model_name: String,
    #[serde(default)]
    completed_at: u64,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<u64>,
}
impl Message {
    fn new(role: &str, content: String) -> Result<Self> {
        Ok(Self {
            id: random()?,
            usage_chat_id: None,
            role: role.into(),
            content,
            prepared_input: None,
            games: Vec::new(),
            blocks: Vec::new(),
            status: if role == "user" {
                "completed"
            } else {
                "streaming"
            }
            .into(),
            error: None,
            reasoning: String::new(),
            usage: Value::Null,
            _usage_details: serde::de::IgnoredAny,
            model: String::new(),
            provider_label: String::new(),
            reasoning_effort: String::new(),
            provider_id: String::new(),
            model_name: String::new(),
            completed_at: 0,
            context_window: None,
            max_output_tokens: None,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueuedMessage {
    id: String,
    content: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BranchRef {
    chat_id: String,
    message_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Conversation {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branched_from: Option<BranchRef>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    title_edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_id: Option<String>,
    title: String,
    provider_id: String,
    provider_label: String,
    model: String,
    created_at: u64,
    updated_at: u64,
    transcript: Option<TranscriptRef>,
    messages: Vec<Message>,
    active_task: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pending_messages: Vec<QueuedMessage>,
}

fn presentation(chat: &Conversation) -> Result<Value> {
    refresh_games(chat)?;
    let mut value = serde_json::to_value(chat).map_err(|_| "Could not open this chat.")?;
    for message in value["messages"]
        .as_array_mut()
        .ok_or("Invalid chat messages.")?
    {
        let fields = message.as_object_mut().ok_or("Invalid chat message.")?;
        fields.remove("preparedInput");
        fields.remove("usageChatId");
        message["usageDetails"] = usage::details(&message["usage"]);
    }
    Ok(value)
}
fn refresh_games(chat: &Conversation) -> Result<()> {
    let job = if let Some(task) = &chat.active_task {
        jobs()
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .get(task)
            .cloned()
    } else {
        None
    };
    if let Some(job) = job {
        let mut state = job
            .state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        if let Some(message) = chat.messages.iter().find(|m| state["replyId"] == m.id) {
            state["games"] = json!(message.games);
        }
    }
    Ok(())
}
fn visible_chats(root: &Path) -> Result<Vec<Metadata>> {
    let rows = list(root)?;
    let bound: std::collections::HashSet<_> = rows
        .iter()
        .filter(|chat| chat.branched_from.is_none())
        .filter_map(|chat| chat.video_id.as_ref())
        .collect();
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .iter()
        .filter(|chat| {
            !(chat.branched_from.is_none()
                && chat.video_id.is_none()
                && chat
                    .transcript
                    .as_ref()
                    .is_some_and(|t| bound.contains(&t.video_id)))
        })
        .filter(|chat| {
            if chat.branched_from.is_some() {
                return true;
            }
            let video = chat
                .video_id
                .as_ref()
                .or_else(|| chat.transcript.as_ref().map(|t| &t.video_id));
            video.is_none_or(|id| seen.insert(id.clone()))
        })
        .cloned()
        .collect())
}
fn caption(root: &Path, value: &Value) -> Result<Option<TranscriptRef>> {
    if value.is_null() {
        return Ok(None);
    }
    let id = string(value, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid transcript identity.".into());
    }
    let _g = crate::REQUEST_LOCK
        .lock(c"GoVue core request: chat.caption")
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let transcript = crate::transcript::load(
        root.to_str().ok_or("Invalid transcript storage path.")?,
        id,
        string(value, "trackId")?,
    )?;
    Ok(Some(TranscriptRef {
        video_id: transcript.video_id,
        track_id: transcript.track_id,
        title: transcript.title,
        language: transcript.language,
    }))
}

// CHATS serializes lookup and creation, including simultaneous opens on both entry points.
fn open_video(root: &Path, c: &Value) -> Result<Conversation> {
    let id = string(c, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid video identity.".into());
    }
    let rows = list(root)?;
    let candidates = rows.iter().filter(|chat| chat.branched_from.is_none());
    let existing = if let Some(value) = c.get("chatId") {
        let chat = rows
            .iter()
            .find(|chat| Some(chat.id.as_str()) == value.as_str())
            .ok_or("This chat is no longer available.")?;
        if chat.video_id.as_deref() != Some(id)
            && chat.transcript.as_ref().is_none_or(|t| t.video_id != id)
        {
            return Err("This chat belongs to a different video.".into());
        }
        Some(chat)
    } else {
        candidates
            .clone()
            .find(|chat| chat.video_id.as_deref() == Some(id))
            .or_else(|| {
                candidates.clone().find(|chat| {
                    chat.video_id.is_none()
                        && chat.transcript.as_ref().is_some_and(|t| t.video_id == id)
                })
            })
    };
    let mut chat = if let Some(chat) = existing {
        let mut chat = read(root, &chat.id)?;
        recover(root, &mut chat)?;
        chat
    } else {
        if rows.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let selected = {
            let _g = CONFIG_LOCK
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            config(root)?.active_id
        };
        let p = selected.as_deref().and_then(|id| provider(root, id).ok());
        Conversation {
            id: format!("video_{id}"),
            branched_from: None,
            pinned: false,
            archived: false,
            title_edited: false,
            video_id: Some(id.into()),
            title: c["title"]
                .as_str()
                .unwrap_or("Video chat")
                .chars()
                .take(100)
                .collect(),
            provider_id: p.as_ref().map(|p| p.id.clone()).unwrap_or_default(),
            provider_label: p.as_ref().map(|p| p.label.clone()).unwrap_or_default(),
            model: p.map(|p| p.model).unwrap_or_default(),
            created_at: now(),
            updated_at: now(),
            transcript: None,
            messages: Vec::new(),
            active_task: None,
            reasoning_effort: None,
            pending_messages: Vec::new(),
        }
    };
    chat.video_id = Some(id.into());
    if !c["transcript"].is_null() {
        let transcript = caption(root, &c["transcript"])?;
        if transcript.as_ref().is_none_or(|t| t.video_id != id) {
            return Err("This transcript belongs to a different video.".into());
        }
        chat.transcript = transcript;
        if chat.messages.is_empty() && !chat.title_edited {
            chat.title = chat
                .transcript
                .as_ref()
                .ok_or("Missing video transcript.")?
                .title
                .chars()
                .take(100)
                .collect();
        }
    }
    save(root, &chat)?;
    Ok(chat)
}

fn branch(root: &Path, c: &Value) -> Result<Value> {
    let mut chat = read(root, string(c, "chatId")?)?;
    recover(root, &mut chat)?;
    if chat.active_task.is_some() {
        return Err("Stop the response before branching this chat.".into());
    }
    let message_id = string(c, "messageId")?;
    let index = chat
        .messages
        .iter()
        .position(|m| m.id == message_id && m.role == "assistant" && m.status != "streaming")
        .ok_or("Choose a finished response to branch from.")?;
    if list(root)?.len() >= MAX_CHATS {
        return Err("Chat history is full. Delete an older chat first.".into());
    }
    let source_id = chat.id.clone();
    chat.branched_from = Some(BranchRef {
        chat_id: source_id.clone(),
        message_id: message_id.into(),
    });
    chat.id = random()?;
    chat.messages.truncate(index + 1);
    for message in &mut chat.messages {
        if message.role == "assistant" {
            message
                .usage_chat_id
                .get_or_insert_with(|| source_id.clone());
        }
    }
    chat.pending_messages.clear();
    chat.active_task = None;
    chat.pinned = false;
    chat.archived = false;
    chat.title = format!(
        "Branch of {}",
        chat.title.chars().take(190).collect::<String>()
    );
    chat.created_at = now();
    chat.updated_at = chat.created_at;
    save(root, &chat)?;
    presentation(&chat)
}
/// Only completed user/assistant pairs become context for the next turn.
pub(super) fn history(value: &Value) -> Result<Vec<Value>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let rows = value.as_array().ok_or("Invalid conversation history.")?;
    if rows.len() > 198 || rows.len() % 2 != 0 {
        return Err("Invalid conversation history.".into());
    }
    let mut total = 0;
    let mut result = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let role = string(row, "role")?;
        let content = string(row, "content")?;
        total += content.len();
        if role != if i % 2 == 0 { "user" } else { "assistant" }
            || content.trim().is_empty()
            || total > 300_000
        {
            return Err(
                "This conversation has reached its context limit. Start a new chat to continue."
                    .into(),
            );
        }
        result.push(json!({"role":role,"content":content}));
    }
    Ok(result)
}
pub(super) fn record_input(job: &Job, input: PreparedInput) -> Result<()> {
    let Some((root, id)) = job.conversation.get() else {
        return Ok(());
    };
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This response is no longer active.".into());
    }
    let index = chat
        .messages
        .len()
        .checked_sub(2)
        .ok_or("Missing chat prompt.")?;
    chat.messages[index].prepared_input = Some(input);
    save(root, &chat)
}
fn finish(root: &Path, id: &str, job: &Arc<Job>, result: &Result<Value>) -> Result<Value> {
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    // Deleting a chat or retrying after cancellation must never resurrect an old turn.
    if !path(root, id)?.exists() {
        return Ok(Value::Null);
    }
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Ok(Value::Null);
    }
    capture_reply(&mut chat, job, result)?;
    chat.active_task = None;
    chat.updated_at = now();
    save(root, &chat)?;
    Ok(json!({"conversation":presentation(&chat)?}))
}
fn capture_reply(chat: &mut Conversation, job: &Job, result: &Result<Value>) -> Result<()> {
    let message = chat.messages.last_mut().ok_or("Missing chat reply.")?;
    message.content = job
        .state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?["text"]
        .as_str()
        .unwrap_or_default()
        .into();
    {
        let state = job
            .state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        message.reasoning = state["reasoning"].as_str().unwrap_or_default().into();
        message.usage = state["usage"].clone();
        message.model = state["model"].as_str().unwrap_or_default().into();
        message.provider_label = state["providerLabel"].as_str().unwrap_or_default().into();
        message.reasoning_effort = state["reasoningEffort"].as_str().unwrap_or_default().into();
        message.provider_id = state["providerId"].as_str().unwrap_or_default().into();
        message.model_name = state["modelName"].as_str().unwrap_or_default().into();
        message.context_window = usage::count(&state["contextWindow"]);
        message.max_output_tokens = usage::count(&state["maxOutputTokens"]);
    }
    message.status = if job.cancelled.load(Ordering::Relaxed) {
        "stopped"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .into();
    message.error = if message.status == "failed" {
        result.as_ref().err().map(ToString::to_string)
    } else {
        None
    };
    message.completed_at = now();
    {
        let mut tape = job
            .activity
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        activity::settle(
            &mut tape.blocks,
            match message.status.as_str() {
                "stopped" => activity::Status::Stopped,
                "failed" => activity::Status::Failed,
                _ => activity::Status::Completed,
            },
        );
        message.blocks.clone_from(&tape.blocks);
    }
    Ok(())
}

/// Deliver steering only after the complete tool result batch, or a text-only reply.
/// Finalization shares the queue lock so a message arriving at completion starts a new turn.
pub(super) fn steer(job: &Job, terminal: bool) -> Result<Option<(String, String)>> {
    let Some((root, id)) = job.conversation.get() else {
        return Ok(None);
    };
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This response is no longer active.".into());
    }
    if chat.pending_messages.is_empty() {
        if terminal {
            capture_reply(&mut chat, job, &Ok(Value::Null))?;
            chat.active_task = None;
            chat.updated_at = now();
            save(root, &chat)?;
        }
        return Ok(None);
    }
    if chat.messages.len() >= 198 {
        return Err("This chat is full. Start a new chat to continue.".into());
    }
    let content = chat
        .pending_messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut user = Message::new("user", content.clone())?;
    user.id.clone_from(&chat.pending_messages[0].id);
    user.prepared_input = Some(PreparedInput {
        content: content.clone(),
        transcript: chat.messages.iter().rev().find_map(|m| {
            m.prepared_input
                .as_ref()
                .and_then(|input| input.transcript.clone())
        }),
    });
    let reply = Message::new("assistant", String::new())?;
    let reply_id = reply.id.clone();
    capture_reply(&mut chat, job, &Ok(Value::Null))?;
    chat.messages.extend([user, reply]);
    chat.pending_messages.clear();
    chat.updated_at = now();
    save(root, &chat)?;
    *job.activity
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")? = activity::Tape::default();
    let mut state = job
        .state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    for field in ["text", "textPrefix", "reasoning", "reasoningPrefix"] {
        state[field] = json!("");
    }
    for field in ["usage", "usagePrefix"] {
        state[field] = Value::Null;
    }
    state["blocks"] = json!([]);
    state["games"] = json!([]);
    state["replyId"] = json!(reply_id);
    state["pendingMessages"] = json!([]);
    state["steeringCommitted"] = json!(true);
    drop(state);
    Ok(Some((content, reply_id)))
}

fn enqueue(root: &Path, chat: &mut Conversation, c: &Value) -> Result<Value> {
    if c["retry"] == true
        || c.get("editMessageId").is_some()
        || c.get("retryMessageId").is_some()
        || c.get("intent").is_some()
    {
        return Err("Stop this response before editing or regenerating messages.".into());
    }
    let content = string(c, "prompt")?.trim();
    let bytes = chat
        .pending_messages
        .iter()
        .map(|m| m.content.len() + 2)
        .sum::<usize>();
    if content.is_empty() || content.len() > 16000 {
        return Err("Enter a question of up to 16,000 bytes.".into());
    }
    if chat.pending_messages.len() >= 8
        || bytes + content.len() > 16000
        || chat.messages.len() >= 198
    {
        return Err("The message queue is full. Wait for the assistant to continue.".into());
    }
    let job = jobs()
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .get(
            chat.active_task
                .as_ref()
                .ok_or("This response has finished.")?,
        )
        .cloned()
        .ok_or("This response has finished.")?;
    job.pending()?;
    chat.pending_messages.push(QueuedMessage {
        id: random()?,
        content: content.into(),
    });
    chat.updated_at = now();
    save(root, chat)?;
    job.state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?["pendingMessages"] =
        json!(chat.pending_messages);
    Ok(json!({"taskId":job.id,"conversation":presentation(chat)?}))
}
pub(super) fn interrupt(job: &Arc<Job>) -> Result<Value> {
    if let Some((root, id)) = job.conversation.get() {
        return finish(root, id, job, &Err("Response stopped.".into()));
    }
    Ok(Value::Null)
}

/// Commit before continuing the model so a failed final reply cannot lose a game.
pub(super) fn create_game(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let (root, id) = job
        .conversation
        .get()
        .ok_or("Games require a saved conversation.")?;
    let call_id = string(call, "id")?;
    if call_id.is_empty() || call_id.len() > 200 {
        return Err("Invalid tool call identity.".into());
    }
    let definition: games::Definition = serde_json::from_str(string(call, "arguments")?)
        .map_err(|_| "Invalid game arguments. Supply all fields described by the tool schema.")?;
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This game request is no longer active.".into());
    }
    let transcript = if let Some(reference) = &chat.transcript {
        let _guard = crate::REQUEST_LOCK
            .lock(c"GoVue core request: chat.create_game")
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        Some(crate::transcript::load(
            root.to_str().ok_or("Invalid storage path.")?,
            &reference.video_id,
            &reference.track_id,
        )?)
    } else {
        None
    };
    let reply = chat
        .messages
        .last_mut()
        .ok_or("Missing assistant message.")?;
    if let Some(game) = reply.games.iter().find(|game| game.call_id == call_id) {
        return Ok(json!({"gameId":game.id,"title":game.definition.title,"status":"created"}));
    }
    if reply.games.len() >= 3 {
        return Err("At most three games can be created in one reply.".into());
    }
    let mut game = games::Game::create(definition, call_id, transcript.as_ref())?;
    if game.source.is_none() {
        game.source = chat.video_id.as_ref().map(|id| games::Source {
            video_id: id.clone(),
            title: chat.title.clone(),
            start: None,
            track_id: None,
            language: None,
        });
    }
    job.pending()?;
    let game = practice::capture(root, &game)?;
    let result = json!({"gameId":game.id,"title":game.definition.title,"status":"created","savedToPractice":true,"message":"The playable activity is in chat and saved in Practice in the sidebar. Briefly invite the user to play; do not repeat its HTML or answers."});
    reply.games.push(game);
    reply.blocks.clone_from(
        &job.activity
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .blocks,
    );
    let snapshot = json!(reply.games);
    job.pending()?;
    save(root, &chat)?;
    let mut state = job
        .state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    state["gameCommitted"] = json!(true);
    state["games"] = snapshot;
    drop(state);
    Ok(result)
}

pub(super) fn read_game_library(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let (root, _) = job
        .conversation
        .get()
        .ok_or("Games require a saved conversation.")?;
    job.pending()?;
    let mut command: Value = serde_json::from_str(string(call, "arguments")?)
        .map_err(|_| "Game library arguments must be a JSON object.")?;
    let fields = command
        .as_object_mut()
        .ok_or("Game library arguments must be an object.")?;
    let op = match string(call, "name")? {
        "list_interactive_games" => "ai_practice_catalog",
        "read_interactive_game" => "ai_practice_get",
        _ => return Err("Unknown game library tool.".into()),
    };
    fields.insert("op".into(), json!(op));
    dispatch(command, root.clone())
}

fn game_export(c: &Value, root: &Path) -> Result<Value> {
    let chat = read(root, string(c, "chatId")?)?;
    let game = chat
        .messages
        .iter()
        .filter(|m| m.id == c["messageId"])
        .flat_map(|m| &m.games)
        .find(|g| g.id == c["gameId"])
        .ok_or("This game is no longer available.")?;
    let source = game
        .source
        .as_ref()
        .map(|source| json!(source))
        .or_else(|| {
            chat.transcript
                .as_ref()
                .map(|reference| json!(reference))
                .or_else(|| chat.video_id.as_ref().map(|id| json!({"videoId":id})))
        });
    // Export the original recipe and frozen captions, never play history or
    // conversation/provider identities. A reused game starts from its rules.
    let text = serde_json::to_string_pretty(&json!({
        "format":"lingoplay-game", "version":1,
        "definition":game.definition, "source":source, "cues":game.cues,
    }))
    .map_err(|_| "Could not copy this game.")?;
    Ok(json!({"text":text}))
}

fn game_action(c: &Value, root: &Path) -> Result<Value> {
    let mut chat = read(root, string(c, "chatId")?)?;
    let message = chat
        .messages
        .iter_mut()
        .find(|m| m.id == c["messageId"])
        .ok_or("This game message is no longer available.")?;
    let game = message
        .games
        .iter_mut()
        .find(|g| g.id == c["gameId"])
        .ok_or("This game is no longer available.")?;
    practice::capture(root, game)?;
    let result = practice::apply(root, c)?;
    *game = serde_json::from_value(result["game"].clone())
        .map_err(|_| "Could not restore game progress.")?;
    chat.updated_at = now();
    save(root, &chat)?;
    refresh_games(&chat)?;
    if result["notify"] == true {
        chat_sessions::feedback(
            root,
            &chat.id,
            string(&result["game"], "id")?,
            result["game"]["revision"].as_u64().unwrap_or(0),
        )?;
    }
    Ok(result)
}

fn game_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let chat = read(root, string(c, "chatId")?)?;
        let game = chat
            .messages
            .iter()
            .filter(|m| m.id == c["messageId"])
            .flat_map(|m| &m.games)
            .find(|g| g.id == c["gameId"])
            .ok_or("This game is no longer available.")?;
        let cue = game
            .cues
            .iter()
            .find(|cue| cue.id == c["cueId"])
            .ok_or("This game cannot play that caption.")?
            .clone();
        let video_id = game
            .source
            .as_ref()
            .map(|s| s.video_id.clone())
            .or_else(|| {
                chat.video_id
                    .or_else(|| chat.transcript.map(|t| t.video_id))
            })
            .ok_or("This game has no source video.")?;
        (video_id, cue)
    };
    if c["prefetch"] == true {
        games::audio::prefetch(root, &video_id, &cue, &c["catalog"])
    } else {
        games::audio::prepare(root, &video_id, &cue, &c["catalog"])
    }
}

fn transcript_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let chat = read(root, string(c, "chatId")?)?;
        let reference = chat.transcript.ok_or("This chat has no transcript.")?;
        if c["videoId"] != reference.video_id {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let track_id = string(c, "trackId")?;
        let _guard = crate::REQUEST_LOCK
            .lock(c"GoVue core request: chat.transcript_audio")
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let allowed = crate::transcript::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            &reference.track_id,
            |data| data.tracks.iter().any(|track| track.id == track_id),
        )?;
        if !allowed {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let cue = crate::transcript::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            track_id,
            |data| games::Cue::from_transcript(data, string(c, "cueId")?),
        )??;
        (reference.video_id, cue)
    };
    if c["prefetch"] == true {
        games::audio::prefetch(root, &video_id, &cue, &c["catalog"])
    } else {
        games::audio::prepare(root, &video_id, &cue, &c["catalog"])
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "The send transaction validates and persists one correlated conversation update before spawning its worker."
)]
fn send(c: Value, root: PathBuf) -> Result<Value> {
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let existing = c["chatId"].as_str().filter(|id| !id.is_empty());
    let mut chat = if let Some(id) = existing {
        let mut chat = read(&root, id)?;
        recover(&root, &mut chat)?;
        if chat.active_task.is_some() {
            return enqueue(&root, &mut chat, &c);
        }
        chat
    } else if c["transcript"]["videoId"].is_string() {
        open_video(
            &root,
            &json!({"videoId":c["transcript"]["videoId"],"transcript":c["transcript"]}),
        )?
    } else {
        if list(&root)?.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let p = provider(&root, string(&c, "id")?)?;
        Conversation {
            id: random()?,
            branched_from: None,
            pinned: false,
            archived: false,
            title_edited: false,
            video_id: None,
            title: String::new(),
            provider_id: p.id,
            provider_label: p.label,
            model: p.model,
            created_at: now(),
            updated_at: now(),
            transcript: None,
            messages: Vec::new(),
            active_task: None,
            reasoning_effort: None,
            pending_messages: Vec::new(),
        }
    };
    if chat.active_task.is_some() {
        return enqueue(&root, &mut chat, &c);
    }
    if chat.provider_id.is_empty() {
        let selected = provider(&root, string(&c, "id")?)?;
        chat.provider_id = selected.id;
        chat.provider_label = selected.label;
        chat.model = selected.model;
    }
    let mut p = provider(&root, &chat.provider_id)?;
    p.model.clone_from(&chat.model);
    if p.model.is_empty() {
        return Err("Choose a model in Providers first.".into());
    }
    if let Some(value) = c.get("reasoningEffort") {
        chat.reasoning_effort = parse_reasoning(value)?;
    }
    let retry = c["retry"] == true;
    let retry_message = c
        .get("retryMessageId")
        .map(|value| {
            let id = value
                .as_str()
                .filter(|id| valid_id(id))
                .ok_or("Invalid response to regenerate.")?;
            if !retry || existing.is_none() {
                return Err("Choose an existing response to regenerate.");
            }
            chat.messages
                .iter()
                .position(|message| message.id == id && message.role == "assistant")
                .ok_or("This response is no longer available to regenerate.")
        })
        .transpose()?;
    let retry_prompt = if retry {
        let index = retry_message
            .or_else(|| chat.messages.len().checked_sub(1))
            .filter(|&index| chat.messages[index].role == "assistant")
            .and_then(|index| index.checked_sub(1))
            .filter(|&index| chat.messages[index].role == "user")
            .ok_or("There is no message to retry.")?;
        Some(index)
    } else {
        None
    };
    let edit = c
        .get("editMessageId")
        .map(|value| {
            let id = value
                .as_str()
                .filter(|id| valid_id(id))
                .ok_or("Invalid message to edit.")?;
            if retry || existing.is_none() {
                return Err("Choose an existing user message to edit.");
            }
            chat.messages
                .iter()
                .position(|message| message.id == id && message.role == "user")
                .ok_or("This message is no longer available to edit.")
        })
        .transpose()?;
    let prompt = if let Some(index) = retry_prompt {
        chat.messages[index].content.as_str()
    } else {
        string(&c, "prompt")?
    }
    .trim()
    .to_owned();
    if prompt.is_empty() || prompt.len() > 16_000 {
        return Err("Enter a message of up to 16,000 bytes.".into());
    }
    let prompt_id = edit
        .or(retry_prompt)
        .map(|index| chat.messages[index].id.clone());
    if let Some(id) = c.get("queuedMessageId") {
        let index = chat
            .pending_messages
            .iter()
            .position(|m| id == &m.id)
            .filter(|&index| chat.pending_messages[index].content == prompt)
            .ok_or("This queued message is no longer available.")?;
        chat.pending_messages.remove(index);
    }
    if let Some(index) = edit {
        chat.messages.truncate(index);
        if index == 0 && chat.video_id.is_none() && !chat.title_edited {
            chat.title.clear();
        }
    } else if let Some(index) = retry_prompt {
        chat.messages.truncate(index);
    }
    if chat.messages.len() >= 198 {
        return Err("This chat is full. Start a new chat to continue.".into());
    }
    if let Some(value) = c.get("transcript")
        && (!value.is_null() || chat.video_id.is_none())
    {
        let transcript = caption(&root, value)?;
        if chat
            .video_id
            .as_ref()
            .is_some_and(|id| transcript.as_ref().is_none_or(|t| &t.video_id != id))
        {
            return Err("This transcript belongs to a different video.".into());
        }
        if chat.video_id.is_none()
            && let Some(t) = &transcript
        {
            if list(&root)?.iter().any(|other| {
                other.id != chat.id
                    && (other.video_id.as_deref() == Some(t.video_id.as_str())
                        || other
                            .transcript
                            .as_ref()
                            .is_some_and(|r| r.video_id == t.video_id))
            }) {
                return Err("This video already has a chat. Open it from the video.".into());
            }
            chat.video_id = Some(t.video_id.clone());
        }
        chat.transcript = transcript;
    }
    if chat.video_id.is_some() && chat.transcript.is_none() {
        return Err("Load this video's transcript before sending a message.".into());
    }
    let history_rows = chat
        .messages
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| pair[1].status == "completed")
        .flat_map(|pair| {
            pair.iter().map(|m| {
                let content = m
                    .prepared_input
                    .as_ref()
                    .map_or(m.content.as_str(), |input| input.content.as_str());
                let content = if content.trim().is_empty()
                    && m.blocks
                        .iter()
                        .any(|block| matches!(block, activity::Block::Tool { .. }))
                {
                    "[Completed tool activity.]"
                } else {
                    content
                };
                json!({"role":m.role,"content":content})
            })
        })
        .collect::<Vec<_>>();
    history(&json!(history_rows))?;
    let mut wire = json!({"prompt":prompt,"history":history_rows,"intent":c["intent"],"includeTranscript":chat.transcript.is_some()});
    let previous = chat
        .messages
        .as_chunks::<2>()
        .0
        .iter()
        .rfind(|pair| pair[1].status == "completed");
    if let Some(previous) = previous {
        wire["historyTail"] = json!(previous[1].id);
        wire["lastTranscript"] = json!(
            previous[0]
                .prepared_input
                .as_ref()
                .and_then(|input| input.transcript.as_deref())
        );
    }
    wire["reasoningEffort"] = json!(chat.reasoning_effort);
    let mut recent_games = chat
        .messages
        .iter()
        .flat_map(|m| &m.games)
        .rev()
        .collect::<Vec<_>>();
    recent_games.sort_by_key(|game| {
        std::cmp::Reverse((
            Some(game.id.as_str()) == c["focusGameId"].as_str(),
            game.last_activity(),
        ))
    });
    let mut activity = Vec::new();
    let mut activity_bytes = 0;
    for game in recent_games.into_iter().take(8) {
        let snapshot = game.context();
        let size = snapshot.to_string().len();
        if activity_bytes + size <= 160_000 {
            activity_bytes += size;
            activity.push(snapshot);
        }
    }
    wire["gameActivity"] =
        json!(serde_json::to_string(&activity).map_err(|_| "Could not read game activity.")?);
    if let Some(t) = &chat.transcript {
        wire["videoId"] = json!(t.video_id);
        wire["trackId"] = json!(t.track_id);
    }
    if chat.title.is_empty() {
        chat.title = if c["intent"] == "summary" {
            chat.transcript
                .as_ref()
                .map(|t| t.title.chars().take(100).collect())
                .unwrap_or_else(|| "Video summary".into())
        } else {
            prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect()
        };
    }
    let mut user = Message::new("user", prompt)?;
    if let Some(id) = prompt_id {
        user.id = id;
    }
    chat.messages
        .extend([user, Message::new("assistant", String::new())?]);
    chat.updated_at = now();
    wire["replyId"] = json!(chat.messages.last().ok_or("Missing assistant reply.")?.id);
    let job = new_job(&p.id)?;
    {
        let mut state = job
            .state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        state["replyId"] = wire["replyId"].clone();
        state["pendingMessages"] = json!(chat.pending_messages);
    }
    let _ = job.conversation.set((root.clone(), chat.id.clone()));
    chat.active_task = Some(job.id.clone());
    if let Err(error) = save(&root, &chat) {
        jobs()
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .remove(&job.id);
        return Err(error);
    }
    let worker = job.clone();
    let id = chat.id.clone();
    thread::spawn(move || {
        let result = super::chat(&root, &p, &wire, &worker);
        let saved = finish(&root, &id, &worker, &result);
        worker.finish(match result {
            Ok(_) => saved,
            Err(e) => saved.and(Err(e)),
        });
    });
    Ok(json!({"taskId":job.id,"conversation":presentation(&chat)?}))
}
fn usage_report(command: &Value, root: &Path) -> Result<Value> {
    let selected = command
        .get("chatId")
        .map(|v| v.as_str().ok_or("Invalid conversation identity."))
        .transpose()?;
    let chats = load_all(root)?;
    if selected.is_some_and(|id| !chats.iter().any(|chat| chat.id == id)) {
        return Err("This chat is no longer available.".into());
    }
    let cfg = config(root)?;
    let mut turns = Vec::new();
    let mut counted = std::collections::HashSet::new();
    let mut context = Value::Null;
    for chat in chats
        .iter()
        .filter(|chat| selected.is_none_or(|id| chat.id == id))
    {
        for message in chat
            .messages
            .iter()
            .filter(|m| m.role == "assistant" && m.status != "streaming")
        {
            let model_id = if message.model.is_empty() {
                &chat.model
            } else {
                &message.model
            };
            let label = if message.provider_label.is_empty() {
                &chat.provider_label
            } else {
                &message.provider_label
            };
            if selected.is_some() && usage::count(&message.usage["inputTokens"]).is_some() {
                let model = cfg
                    .providers
                    .iter()
                    .filter(|p| {
                        p.id == message.provider_id
                            || (message.provider_id.is_empty()
                                && (p.id == chat.provider_id || p.label == *label))
                    })
                    .flat_map(|p| &p.models)
                    .find(|m| m.id == *model_id);
                let window = message
                    .context_window
                    .or_else(|| model.and_then(|m| m.context_window));
                let group_limit = cfg
                    .groups
                    .iter()
                    .find(|g| g.id == chat.provider_id)
                    .and_then(|g| g.context_limit);
                let window = match (window, group_limit) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                context = json!({"used":message.usage["latestContextTokens"].as_u64().or_else(|| message.usage["inputTokens"].as_u64()),
                    "window":window,"maxOutputTokens":message.max_output_tokens.or_else(|| model.and_then(|m| m.max_output_tokens)),
                    "reasoningEffort":message.reasoning_effort,"model":model_id,"providerLabel":label});
            }
            // Only branch copies share usage; legacy chats may reuse message identities.
            if !counted.insert((
                message.usage_chat_id.as_ref().unwrap_or(&chat.id),
                &message.id,
            )) {
                continue;
            }
            turns.push(usage::Turn {
                chat_id: chat.id.clone(),
                provider_id: message.provider_id.clone(),
                provider_label: label.clone(),
                model: model_id.clone(),
                model_name: if message.model_name.is_empty() {
                    model_id.clone()
                } else {
                    message.model_name.clone()
                },
                estimated: message.model.is_empty() || message.provider_label.is_empty(),
                completed_at: message.completed_at,
                usage: message.usage.clone(),
            });
        }
    }
    let mut report = usage::report(&turns, &cfg)?;
    report["chatId"] = json!(selected);
    report["context"] = context;
    report["title"] = json!(
        selected
            .and_then(|id| chats.iter().find(|c| c.id == id))
            .map(|c| c.title.as_str())
    );
    report["chats"] = json!(chats.iter().map(|chat| json!({"id":chat.id,"title":chat.title,"model":chat.model,"providerLabel":chat.provider_label})).collect::<Vec<_>>());
    Ok(report)
}

fn speech_hints(c: &Value, root: &Path) -> Result<Value> {
    let chat = c["chatId"].as_str().map(|id| read(root, id)).transpose()?;
    let draft = c["draft"].as_str().unwrap_or_default();
    if draft.chars().count() > 8000 {
        return Err("Message is too long.".into());
    }
    let texts = std::iter::once(draft)
        .chain(chat.iter().map(|chat| chat.title.as_str()))
        .chain(chat.iter().flat_map(|chat| {
            chat.messages
                .iter()
                .rev()
                .take(20)
                .map(|m| m.content.as_str())
        }));
    let transcript = if c["transcript"].is_null() {
        chat.as_ref().and_then(|chat| chat.transcript.clone())
    } else {
        let reference = &c["transcript"];
        let video_id = string(reference, "videoId")?;
        let track_id = string(reference, "trackId")?;
        if !crate::video_id(video_id) || track_id.len() > 1000 {
            return Err("Invalid transcript identity.".into());
        }
        Some(TranscriptRef {
            video_id: video_id.into(),
            track_id: track_id.into(),
            title: String::new(),
            language: String::new(),
        })
    };
    if let Some(transcript) = transcript {
        if chat
            .as_ref()
            .and_then(|chat| chat.video_id.as_ref())
            .is_some_and(|id| *id != transcript.video_id)
        {
            return Err("This transcript belongs to a different video.".into());
        }
        // Evicted captions should not prevent dictation using the conversation vocabulary.
        let _g = crate::REQUEST_LOCK
            .lock(c"GoVue core request: chat.dictation_hints")
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let hints = crate::transcript::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &transcript.video_id,
            &transcript.track_id,
            |data| {
                crate::speech::hints(
                    texts.clone(),
                    std::iter::once(data.title.as_str())
                        .chain(data.segments.iter().map(|s| s.text.as_str())),
                )
            },
        );
        return Ok(json!(hints.unwrap_or_else(|_| crate::speech::hints(
            texts,
            std::iter::empty()
        ))));
    }
    Ok(json!(crate::speech::hints(texts, std::iter::empty())))
}

fn chat_list(c: &Value, root: &Path) -> Result<Value> {
    let query = search::query(c)?;
    let archived = c.get("archived").map_or(Ok(false), |value| {
        value.as_bool().ok_or("Invalid archive filter.")
    })?;
    let mut rows = {
        let _guard = CHATS
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        visible_chats(root)?
    };
    rows.sort_by_key(|chat| !chat.pinned);
    let mut chats = Vec::new();
    for chat in rows.iter().filter(|chat| chat.archived == archived) {
        let mut hit = None;
        if !query.is_empty() {
            hit = search::first(&chat.title, &query).or_else(|| {
                chat.transcript
                    .as_ref()
                    .and_then(|t| search::first(&t.title, &query))
            });
            if hit.is_none() {
                // Content search is bounded to one MAX_CHAT record at a time.
                // Release CHATS before scanning text and between record reads so
                // saves/deletes never wait for an entire history search.
                let saved = {
                    let _guard = CHATS
                        .lock()
                        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                    if let Ok(mut saved) = read(root, &chat.id) {
                        recover(root, &mut saved)?;
                        Some(saved)
                    } else {
                        None
                    }
                };
                hit = saved.as_ref().and_then(|saved| {
                    saved.messages.iter().find_map(|message| {
                        search::first(&message.content, &query).map(|mut hit| {
                            hit.message_id = Some(message.id.clone());
                            hit.role = Some(message.role.clone());
                            hit
                        })
                    })
                });
            }
            if hit.is_none() {
                continue;
            }
        }
        chats.push(json!({"id":chat.id,"videoId":chat.video_id.as_ref().or_else(|| chat.transcript.as_ref().map(|t| &t.video_id)),"title":chat.title,"providerId":chat.provider_id,"providerLabel":chat.provider_label,"model":chat.model,"updatedAt":chat.updated_at,"videoTitle":chat.transcript.as_ref().map(|t| &t.title),"pinned":chat.pinned,"archived":chat.archived,"searchMatch":hit}));
    }
    Ok(json!({"query":query,"chats":chats}))
}

pub(super) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    if string(&c, "op")?.starts_with("ai_practice_") {
        if matches!(
            c["op"].as_str(),
            Some(
                "ai_practice_list"
                    | "ai_practice_catalog"
                    | "ai_practice_get"
                    | "ai_practice_rename"
            )
        ) {
            let _guard = CHATS
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            for chat in load_all(&root)? {
                for game in chat.messages.iter().flat_map(|m| &m.games) {
                    practice::capture(&root, game)?;
                }
            }
        }
        return practice::dispatch(&c, &root);
    }
    if c["op"] == "ai_chat_send" {
        return send(c, root);
    }
    if c["op"] == "ai_game_audio" {
        return game_audio(&c, &root);
    }
    if c["op"] == "ai_chat_audio" {
        return transcript_audio(&c, &root);
    }
    if c["op"] == "ai_chats" {
        return chat_list(&c, &root);
    }
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    match string(&c, "op")? {
        "ai_game_action" => game_action(&c, &root),
        "ai_game_export" => game_export(&c, &root),
        "ai_chat_speech_hints" => speech_hints(&c, &root),
        "ai_usage" => usage_report(&c, &root),
        "ai_chat_update" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            if let Some(value) = c.get("title") {
                let title = value.as_str().ok_or("Invalid chat name.")?.trim();
                if title.is_empty()
                    || title.chars().count() > 200
                    || title.chars().any(char::is_control)
                {
                    return Err("Enter a chat name of 1–200 characters.".into());
                }
                chat.title = title.into();
                chat.title_edited = true;
            }
            if let Some(value) = c.get("pinned") {
                chat.pinned = value.as_bool().ok_or("Invalid pin setting.")?;
            }
            if let Some(value) = c.get("archived") {
                chat.archived = value.as_bool().ok_or("Invalid archive setting.")?;
            }
            save(&root, &chat)?;
            presentation(&chat)
        }
        "ai_chat_find" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            // Include the currently rendered reply without persisting partial output or queries.
            let job = if let Some(id) = chat.active_task.as_ref() {
                jobs()
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .get(id)
                    .cloned()
            } else {
                None
            };
            if let Some(job) = job
                && let Some(message) = chat.messages.last_mut().filter(|m| m.status == "streaming")
            {
                let state = job
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                message.content = state["text"].as_str().unwrap_or_default().into();
            }
            search::find(
                &c,
                chat.messages
                    .iter()
                    .map(|m| (m.id.as_str(), m.role.as_str(), m.content.as_str())),
            )
        }
        "ai_chat_video_open" => presentation(&open_video(&root, &c)?),
        "ai_chat_branch" => branch(&root, &c),
        "ai_chat_model" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            if chat.active_task.is_some() {
                return Err("Stop the response before changing models.".into());
            }
            let selected = provider(&root, string(&c, "id")?)?;
            chat.provider_id = selected.id;
            chat.provider_label = selected.label;
            chat.model = selected.model;
            save(&root, &chat)?;
            presentation(&chat)
        }
        "ai_chat_get" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            presentation(&chat)
        }
        "ai_chat_reasoning" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            // Running requests own their wire settings; this preference affects the next turn.
            chat.reasoning_effort = parse_reasoning(&c["reasoningEffort"])?;
            save(&root, &chat)?;
            presentation(&chat)
        }
        "ai_chat_delete" => {
            let id = string(&c, "chatId")?;
            if let Ok(chat) = read(&root, id) {
                for game in chat.messages.iter().flat_map(|message| &message.games) {
                    practice::capture(&root, game)?;
                }
            }
            if let Ok(chat) = read(&root, id)
                && let Some(task) = chat.active_task
                && let Some(job) = jobs()
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .get(&task)
            {
                job.cancelled.store(true, Ordering::Relaxed);
            }
            replay::forget_chat(&root, id)?;
            store::delete(&root, id)?;
            Ok(Value::Null)
        }
        _ => Err("Unsupported conversation operation.".into()),
    }
}

fn parse_reasoning(value: &Value) -> Result<Option<String>> {
    if value.is_null() || value == "" {
        return Ok(None);
    }
    let level = value
        .as_str()
        .filter(|v| catalog::valid_reasoning(v))
        .ok_or("Invalid reasoning effort.")?;
    Ok(Some(level.into()))
}

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::fs;

    #[test]
    fn poisoned_partial_reply_is_never_saved_as_recovered_history() {
        for activity in [false, true] {
            let job = super::super::tests::job();
            let root = std::env::temp_dir().join(format!("govue-poison-{}", job.id));
            let mut chat: Conversation = serde_json::from_value(json!({
                "id":"fixture","title":"Fixture","providerId":"fixture",
                "providerLabel":"Fixture","model":"fixture","createdAt":0,
                "updatedAt":0,"messages":[],"activeTask":job.id,
            }))
            .unwrap();
            jobs().lock().unwrap().insert(job.id.clone(), job.clone());
            job.cancelled.store(true, Ordering::Relaxed);
            if activity {
                crate::poison(&job.activity);
            } else {
                crate::poison(&job.state);
            }
            let result = recover(&root, &mut chat);
            jobs().lock().unwrap().remove(&job.id);
            let saved = path(&root, &chat.id).unwrap().exists();
            let _ = fs::remove_dir_all(&root);
            assert_eq!(
                result.unwrap_err().to_string(),
                "AI state is unavailable. Restart GoVue."
            );
            assert_eq!(chat.active_task.as_deref(), Some(job.id.as_str()));
            assert!(!saved);
        }
    }
}
