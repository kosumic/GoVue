//! Correlated turn transactions, queued messages and worker finalization.
use super::model::{Conversation, MAX_CHATS, Message, PreparedInput, QueuedMessage};
use super::storage::CHATS;
use super::{index, library, storage};
use crate::Result;
use crate::ai::jobs::Job;
use crate::ai::{activity, identity, jobs, providers, usage};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

/// Only completed user/assistant pairs become context for the next turn.
pub(in crate::ai) fn history(value: &Value) -> Result<Vec<Value>> {
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
        let role = crate::string(row, "role")?;
        let content = crate::string(row, "content")?;
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
pub(in crate::ai) fn record_input(job: &Job, input: PreparedInput) -> Result<()> {
    let Some((root, id)) = job.conversation.get() else {
        return Ok(());
    };
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = storage::read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This response is no longer active.".into());
    }
    let index = chat
        .messages
        .len()
        .checked_sub(2)
        .ok_or("Missing chat prompt.")?;
    chat.messages[index].prepared_input = Some(input);
    storage::save(root, &chat)
}
pub(super) fn finish(
    root: &Path,
    id: &str,
    job: &Arc<Job>,
    result: &Result<Value>,
) -> Result<Value> {
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    // Deleting a chat or retrying after cancellation must never resurrect an old turn.
    if !storage::path(root, id)?.exists() {
        return Ok(Value::Null);
    }
    let mut chat = storage::read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        // Terminal steering already commits a successful reply while holding the
        // queue lock. Recognize only that exact reply; a newer turn or deletion
        // must never be mistaken for this worker's completion.
        if chat.active_task.is_none() && result.is_ok() && !job.cancelled.load(Ordering::Relaxed) {
            let state = job
                .state
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            let committed = chat
                .messages
                .last()
                .is_some_and(|reply| reply.status == "completed" && state["replyId"] == reply.id);
            drop(state);
            if committed {
                return Ok(json!({"conversation":library::presentation(&chat)?}));
            }
        }
        return Ok(Value::Null);
    }
    capture_reply(&mut chat, job, result)?;
    chat.active_task = None;
    chat.updated_at = identity::now();
    storage::save(root, &chat)?;
    Ok(json!({"conversation":library::presentation(&chat)?}))
}
pub(super) fn capture_reply(
    chat: &mut Conversation,
    job: &Job,
    result: &Result<Value>,
) -> Result<()> {
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
    message.completed_at = identity::now();
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
pub(in crate::ai) fn steer(job: &Job, terminal: bool) -> Result<Option<(String, String)>> {
    let Some((root, id)) = job.conversation.get() else {
        return Ok(None);
    };
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = storage::read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This response is no longer active.".into());
    }
    if chat.pending_messages.is_empty() {
        if terminal {
            capture_reply(&mut chat, job, &Ok(Value::Null))?;
            chat.active_task = None;
            chat.updated_at = identity::now();
            storage::save(root, &chat)?;
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
    let mut user = Message::try_new("user", content.clone())?;
    user.id.clone_from(&chat.pending_messages[0].id);
    user.prepared_input = Some(PreparedInput {
        content: content.clone(),
        transcript: chat.messages.iter().rev().find_map(|m| {
            m.prepared_input
                .as_ref()
                .and_then(|input| input.transcript.clone())
        }),
    });
    let reply = Message::try_new("assistant", String::new())?;
    let reply_id = reply.id.clone();
    capture_reply(&mut chat, job, &Ok(Value::Null))?;
    chat.messages.extend([user, reply]);
    chat.pending_messages.clear();
    chat.updated_at = identity::now();
    storage::save(root, &chat)?;
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
    job.notifications.changed();
    Ok(Some((content, reply_id)))
}

pub(super) fn enqueue(root: &Path, chat: &mut Conversation, c: &Value) -> Result<Value> {
    if c["retry"] == true
        || c.get("editMessageId").is_some()
        || c.get("retryMessageId").is_some()
        || c.get("intent").is_some()
    {
        return Err("Stop this response before editing or regenerating messages.".into());
    }
    let content = crate::string(c, "prompt")?.trim();
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
    let job = jobs::jobs()
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
        id: identity::random()?,
        content: content.into(),
    });
    chat.updated_at = identity::now();
    storage::save(root, chat)?;
    job.state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?["pendingMessages"] =
        json!(chat.pending_messages);
    job.notifications.changed();
    Ok(json!({"taskId":job.id,"conversation":library::presentation(chat)?}))
}
pub(in crate::ai) fn interrupt(job: &Arc<Job>) -> Result<Value> {
    if let Some((root, id)) = job.conversation.get() {
        return finish(root, id, job, &Err("Response stopped.".into()));
    }
    Ok(Value::Null)
}

#[allow(
    clippy::too_many_lines,
    reason = "The send transaction validates and persists one correlated conversation update before spawning its worker."
)]
pub(super) fn send(c: Value, root: PathBuf) -> Result<Value> {
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let existing = c["chatId"].as_str().filter(|id| !id.is_empty());
    let mut chat = if let Some(id) = existing {
        let mut chat = storage::read(&root, id)?;
        storage::recover(&root, &mut chat)?;
        if chat.active_task.is_some() {
            return enqueue(&root, &mut chat, &c);
        }
        chat
    } else if c["transcript"]["videoId"].is_string() {
        library::open_video(
            &root,
            &json!({"videoId":c["transcript"]["videoId"],"transcript":c["transcript"]}),
        )?
    } else {
        if index::list(&root)?.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let p = providers::provider(&root, crate::string(&c, "id")?)?;
        Conversation {
            id: identity::random()?,
            branched_from: None,
            pinned: false,
            archived: false,
            title_edited: false,
            video_id: None,
            title: String::new(),
            provider_id: p.id,
            provider_label: p.label,
            model: p.model,
            created_at: identity::now(),
            updated_at: identity::now(),
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
        let selected = providers::provider(&root, crate::string(&c, "id")?)?;
        chat.provider_id = selected.id;
        chat.provider_label = selected.label;
        chat.model = selected.model;
    }
    let mut p = providers::provider(&root, &chat.provider_id)?;
    p.model.clone_from(&chat.model);
    if p.model.is_empty() {
        return Err("Choose a model in Providers first.".into());
    }
    if let Some(value) = c.get("reasoningEffort") {
        chat.reasoning_effort = library::parse_reasoning(value)?;
    }
    let retry = c["retry"] == true;
    let retry_message = c
        .get("retryMessageId")
        .map(|value| {
            let id = value
                .as_str()
                .filter(|id| providers::valid_id(id))
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
                .filter(|id| providers::valid_id(id))
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
        crate::string(&c, "prompt")?
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
        let transcript = library::caption(&root, value)?;
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
            if index::list(&root)?.iter().any(|other| {
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
    let mut user = Message::try_new("user", prompt)?;
    if let Some(id) = prompt_id {
        user.id = id;
    }
    chat.messages
        .extend([user, Message::try_new("assistant", String::new())?]);
    chat.updated_at = identity::now();
    wire["replyId"] = json!(chat.messages.last().ok_or("Missing assistant reply.")?.id);
    let job = jobs::new_job(&p.id)?;
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
    if let Err(error) = storage::save(&root, &chat) {
        jobs::jobs()
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .remove(&job.id);
        return Err(error);
    }
    let worker = job.clone();
    let id = chat.id.clone();
    crate::ai::notifications::started(c["notificationSession"].as_str(), &id);
    thread::spawn(move || {
        let result = crate::ai::client::chat(&root, &p, &wire, &worker);
        let saved = finish(&root, &id, &worker, &result);
        crate::ai::notifications::completed(&saved);
        worker.finish(match result {
            Ok(_) => saved,
            Err(e) => saved.and(Err(e)),
        });
    });
    Ok(json!({"taskId":job.id,"conversation":library::presentation(&chat)?}))
}
