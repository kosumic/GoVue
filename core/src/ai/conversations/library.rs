//! Conversation presentation, branching, search and usage queries.
use super::index::Metadata;
use super::model::{BranchRef, Conversation, MAX_CHATS, TranscriptRef};
use super::storage::CHATS;
use super::{index, storage};
use crate::Result;
use crate::ai::providers::CONFIG_LOCK;
use crate::ai::{catalog, identity, jobs, providers, search, usage};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn presentation(chat: &Conversation) -> Result<Value> {
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
pub(super) fn refresh_games(chat: &Conversation) -> Result<()> {
    let job = if let Some(task) = &chat.active_task {
        jobs::jobs()
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
pub(super) fn visible_chats(root: &Path) -> Result<Vec<Metadata>> {
    let rows = index::list(root)?;
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
pub(super) fn caption(root: &Path, value: &Value) -> Result<Option<TranscriptRef>> {
    if value.is_null() {
        return Ok(None);
    }
    let id = crate::string(value, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid transcript identity.".into());
    }
    let _g = crate::REQUEST_LOCK
        .lock(c"GoVue core request: chat.caption")
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let transcript = crate::transcript::load(
        root.to_str().ok_or("Invalid transcript storage path.")?,
        id,
        crate::string(value, "trackId")?,
    )?;
    Ok(Some(TranscriptRef {
        video_id: transcript.video_id.clone(),
        track_id: transcript.track_id.clone(),
        title: transcript.title.clone(),
        language: transcript.language.clone(),
    }))
}

// CHATS serializes lookup and creation, including simultaneous opens on both entry points.
pub(super) fn open_video(root: &Path, c: &Value) -> Result<Conversation> {
    let id = crate::string(c, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid video identity.".into());
    }
    let rows = index::list(root)?;
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
        let mut chat = storage::read(root, &chat.id)?;
        storage::recover(root, &mut chat)?;
        chat
    } else {
        if rows.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let selected = {
            let _g = CONFIG_LOCK
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            providers::config(root)?.active_id
        };
        let p = selected
            .as_deref()
            .and_then(|id| providers::provider(root, id).ok());
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
            created_at: identity::now(),
            updated_at: identity::now(),
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
    storage::save(root, &chat)?;
    Ok(chat)
}

pub(super) fn branch(root: &Path, c: &Value) -> Result<Value> {
    let mut chat = storage::read(root, crate::string(c, "chatId")?)?;
    storage::recover(root, &mut chat)?;
    if chat.active_task.is_some() {
        return Err("Stop the response before branching this chat.".into());
    }
    let message_id = crate::string(c, "messageId")?;
    let index = chat
        .messages
        .iter()
        .position(|m| m.id == message_id && m.role == "assistant" && m.status != "streaming")
        .ok_or("Choose a finished response to branch from.")?;
    if index::list(root)?.len() >= MAX_CHATS {
        return Err("Chat history is full. Delete an older chat first.".into());
    }
    let source_id = chat.id.clone();
    chat.branched_from = Some(BranchRef {
        chat_id: source_id.clone(),
        message_id: message_id.into(),
    });
    chat.id = identity::random()?;
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
    chat.created_at = identity::now();
    chat.updated_at = chat.created_at;
    storage::save(root, &chat)?;
    presentation(&chat)
}
pub(super) fn usage_report(command: &Value, root: &Path) -> Result<Value> {
    let selected = command
        .get("chatId")
        .map(|v| v.as_str().ok_or("Invalid conversation identity."))
        .transpose()?;
    let chats = storage::load_all(root)?;
    if selected.is_some_and(|id| !chats.iter().any(|chat| chat.id == id)) {
        return Err("This chat is no longer available.".into());
    }
    let cfg = providers::config(root)?;
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

pub(super) fn speech_hints(c: &Value, root: &Path) -> Result<Value> {
    let chat = c["chatId"]
        .as_str()
        .map(|id| storage::read(root, id))
        .transpose()?;
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
        let video_id = crate::string(reference, "videoId")?;
        let track_id = crate::string(reference, "trackId")?;
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

pub(super) fn chat_list(c: &Value, root: &Path) -> Result<Value> {
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
                    if let Ok(mut saved) = storage::read(root, &chat.id) {
                        storage::recover(root, &mut saved)?;
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

pub(super) fn parse_reasoning(value: &Value) -> Result<Option<String>> {
    if value.is_null() || value == "" {
        return Ok(None);
    }
    let level = value
        .as_str()
        .filter(|v| catalog::valid_reasoning(v))
        .ok_or("Invalid reasoning effort.")?;
    Ok(Some(level.into()))
}
