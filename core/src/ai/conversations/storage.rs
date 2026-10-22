//! Saved chat validation, atomic writes and interrupted-turn recovery.
use super::index;
use super::index::Metadata;
use super::model::{Conversation, MAX_CHAT, MAX_CHATS};
use crate::ai::http::MAX_OUTPUT;
use crate::ai::{activity, catalog, games, jobs, practice, providers, usage};
use crate::{Error, Result};
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

pub(super) static CHATS: Mutex<()> = Mutex::new(());

pub(super) fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if !providers::valid_id(id) {
        return Err("Invalid conversation identity.".into());
    }
    Ok(root.join("ai/chats").join(format!("{id}.json")))
}
pub(super) fn read(root: &Path, id: &str) -> Result<Conversation> {
    let file = fs::File::open(path(root, id)?).map_err(|cause| {
        Error::caused_by(
            "This chat is no longer available.",
            "open saved chat",
            cause,
        )
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_CHAT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|cause| Error::caused_by("Could not read this chat.", "read saved chat", cause))?;
    if bytes.len() > MAX_CHAT {
        return Err("Saved chat is too large.".into());
    }
    let mut chat: Conversation = serde_json::from_slice(&bytes).map_err(|cause| {
        Error::caused_by("This saved chat is unreadable.", "decode saved chat", cause)
    })?;
    if chat.id != id
        || chat.branched_from.as_ref().is_some_and(|branch| {
            !providers::valid_id(&branch.chat_id)
                || branch.chat_id == chat.id
                || !providers::valid_id(&branch.message_id)
        })
        || !(providers::valid_id(&chat.provider_id)
            || chat.provider_id.is_empty() && chat.messages.is_empty())
        || chat
            .video_id
            .as_ref()
            .is_some_and(|id| !crate::video_id(id))
        || chat
            .video_id
            .as_ref()
            .is_some_and(|id| chat.transcript.as_ref().is_some_and(|t| &t.video_id != id))
        || chat.model.len() > 200
        || chat.title.len() > 1000
        || chat
            .reasoning_effort
            .as_ref()
            .is_some_and(|v| !catalog::valid_reasoning(v))
        || chat.pending_messages.len() > 8
        || chat.pending_messages.iter().any(|m| {
            !providers::valid_id(&m.id) || m.content.trim().is_empty() || m.content.len() > 16000
        })
        || chat
            .pending_messages
            .iter()
            .map(|m| m.content.len() + 2)
            .sum::<usize>()
            > 16002
        || chat.messages.len() > 200
        || !chat.messages.len().is_multiple_of(2)
        || chat.messages.iter().enumerate().any(|(i, m)| {
            !providers::valid_id(&m.id)
                || m.usage_chat_id
                    .as_ref()
                    .is_some_and(|id| m.role != "assistant" || !providers::valid_id(id))
                || m.role != if i % 2 == 0 { "user" } else { "assistant" }
                || !matches!(
                    m.status.as_str(),
                    "completed" | "streaming" | "failed" | "stopped"
                )
                || m.content.len() > MAX_OUTPUT
                || m.prepared_input.as_ref().is_some_and(|input| {
                    m.role != "user"
                        || input.content.len() > 480_000
                        || input.content.trim().is_empty()
                        || input.transcript.as_ref().is_some_and(|hash| {
                            hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                })
                || m.games.len() > 3
                || m.games.iter().any(|game| !game.validate())
                || !activity::valid(&m.blocks)
                || m.reasoning.len() > MAX_OUTPUT
                || m.model.len() > 200
                || m.provider_label.len() > 100
                || !catalog::valid_reasoning(&m.reasoning_effort)
                || (!m.provider_id.is_empty() && !providers::valid_id(&m.provider_id))
                || m.model_name.chars().count() > 200
                || m.completed_at > usage::MAX_COUNT
                || m.context_window.is_some_and(|n| n > usage::MAX_COUNT)
                || m.max_output_tokens.is_some_and(|n| n > usage::MAX_COUNT)
                || !usage::valid(&m.usage)
        })
    {
        return Err("This saved chat is invalid.".into());
    }
    let source = chat
        .transcript
        .as_ref()
        .map(|t| games::Source {
            video_id: t.video_id.clone(),
            title: t.title.clone(),
            start: None,
            track_id: Some(t.track_id.clone()),
            language: Some(t.language.clone()),
        })
        .or_else(|| {
            chat.video_id.as_ref().map(|id| games::Source {
                video_id: id.clone(),
                title: chat.title.clone(),
                start: None,
                track_id: None,
                language: None,
            })
        });
    for game in chat.messages.iter_mut().flat_map(|m| &mut m.games) {
        if game.source.is_none() {
            game.source = source.clone().map(|mut source| {
                source.start = game.cues.iter().map(|c| c.start).min_by(f64::total_cmp);
                source
            });
        }
        practice::restore(root, game)?;
    }
    Ok(chat)
}
pub(super) fn save(root: &Path, chat: &Conversation) -> Result<()> {
    let bytes = serde_json::to_vec(chat).map_err(|cause| {
        Error::caused_by("Could not encode this chat.", "encode saved chat", cause)
    })?;
    if bytes.len() > MAX_CHAT {
        return Err("This chat is full. Start a new chat to continue.".into());
    }
    index::update(root, &chat.id, Some(Metadata::from(chat)), || {
        atomic_write(&path(root, &chat.id)?, &bytes)
    })
}

pub(super) fn delete(root: &Path, id: &str) -> Result<()> {
    let target = path(root, id)?;
    index::update(root, id, None, || match fs::remove_file(&target) {
        Ok(()) => sync_directory(target.parent().ok_or("Invalid chat storage path.")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(Error::caused_by(
            "Could not delete this chat.",
            "delete saved chat",
            cause,
        )),
    })
}

pub(super) fn storage_error(cause: std::io::Error) -> Error {
    Error::caused_by("Could not save chat history.", "commit chat storage", cause)
}

pub(super) fn sync_directory(dir: &Path) -> Result<()> {
    fs::File::open(dir)
        .and_then(|file| file.sync_all())
        .map_err(storage_error)
}

pub(super) fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let dir = target.parent().ok_or("Invalid chat storage path.")?;
    fs::create_dir_all(dir).map_err(storage_error)?;
    let temp = target.with_file_name(format!(
        "{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid chat storage path.")?
    ));
    let mut file = fs::File::create(&temp).map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)?;
    fs::rename(temp, target).map_err(storage_error)?;
    sync_directory(dir)
}
pub(super) fn running(id: &str) -> Result<bool> {
    let all = jobs::jobs()
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let job = all.get(id).cloned();
    drop(all);
    let Some(job) = job else {
        return Ok(false);
    };
    Ok(!job.cancelled.load(Ordering::Relaxed)
        && job
            .state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?["status"]
            == "running")
}

pub(super) fn recover(root: &Path, chat: &mut Conversation) -> Result<()> {
    if let Some(id) = chat.active_task.as_deref()
        && !running(id)?
    {
        let partial = jobs::jobs()
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .get(id)
            .map(|j| -> Result<_> {
                let state = j
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                Ok((
                    state["text"].as_str().unwrap_or_default().to_owned(),
                    state["reasoning"].as_str().unwrap_or_default().to_owned(),
                    j.activity
                        .lock()
                        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                        .blocks
                        .clone(),
                ))
            })
            .transpose()?;
        chat.active_task = None;
        if let Some(m) = chat.messages.last_mut().filter(|m| m.status == "streaming") {
            if let Some((text, reasoning, blocks)) = partial {
                m.content = text;
                m.reasoning = reasoning;
                m.blocks = blocks;
            }
            activity::settle(&mut m.blocks, activity::Status::Stopped);
            m.status = "stopped".into();
            m.error = Some("Response interrupted. You can try again.".into());
        }
        save(root, chat)?;
    }
    Ok(())
}
pub(super) fn ids(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("ai/chats");
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(cause) => {
            return Err(Error::caused_by(
                "Could not open chat history.",
                "list saved chats",
                cause,
            ));
        }
    };
    let mut ids = Vec::new();
    for entry in entries.take(MAX_CHATS * 2).flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(id) = p
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|id| providers::valid_id(id))
        {
            ids.push(id.to_owned());
        }
    }
    Ok(ids)
}

// Only operations that need message bodies use this path.
pub(super) fn load_all(root: &Path) -> Result<Vec<Conversation>> {
    let mut rows = Vec::new();
    for id in ids(root)? {
        if let Ok(mut chat) = read(root, &id) {
            recover(root, &mut chat)?;
            rows.push(chat);
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    Ok(rows)
}
