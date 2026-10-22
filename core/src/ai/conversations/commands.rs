//! Saved conversation command dispatch.
use super::storage::CHATS;
use super::{activities, library, storage, turns};
use crate::Result;
use crate::ai::{jobs, practice, providers, replay, search};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

pub(in crate::ai) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    if crate::string(&c, "op")?.starts_with("ai_practice_") {
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
            for chat in storage::load_all(&root)? {
                for game in chat.messages.iter().flat_map(|m| &m.games) {
                    practice::capture(&root, game)?;
                }
            }
        }
        return practice::dispatch(&c, &root);
    }
    if c["op"] == "ai_chat_send" {
        return turns::send(c, root);
    }
    if c["op"] == "ai_game_audio" {
        return activities::game_audio(&c, &root);
    }
    if c["op"] == "ai_chat_audio" {
        return activities::transcript_audio(&c, &root);
    }
    if c["op"] == "ai_chats" {
        return library::chat_list(&c, &root);
    }
    let _g = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    match crate::string(&c, "op")? {
        "ai_game_action" => activities::game_action(&c, &root),
        "ai_game_export" => activities::game_export(&c, &root),
        "ai_chat_speech_hints" => library::speech_hints(&c, &root),
        "ai_usage" => library::usage_report(&c, &root),
        "ai_chat_update" => {
            let mut chat = storage::read(&root, crate::string(&c, "chatId")?)?;
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
            storage::save(&root, &chat)?;
            library::presentation(&chat)
        }
        "ai_chat_find" => {
            let mut chat = storage::read(&root, crate::string(&c, "chatId")?)?;
            // Include the currently rendered reply without persisting partial output or queries.
            let job = if let Some(id) = chat.active_task.as_ref() {
                jobs::jobs()
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
        "ai_chat_video_open" => library::presentation(&library::open_video(&root, &c)?),
        "ai_chat_branch" => library::branch(&root, &c),
        "ai_chat_model" => {
            let mut chat = storage::read(&root, crate::string(&c, "chatId")?)?;
            storage::recover(&root, &mut chat)?;
            if chat.active_task.is_some() {
                return Err("Stop the response before changing models.".into());
            }
            let selected = providers::provider(&root, crate::string(&c, "id")?)?;
            chat.provider_id = selected.id;
            chat.provider_label = selected.label;
            chat.model = selected.model;
            storage::save(&root, &chat)?;
            library::presentation(&chat)
        }
        "ai_chat_get" => {
            let mut chat = storage::read(&root, crate::string(&c, "chatId")?)?;
            storage::recover(&root, &mut chat)?;
            library::presentation(&chat)
        }
        "ai_chat_reasoning" => {
            let mut chat = storage::read(&root, crate::string(&c, "chatId")?)?;
            storage::recover(&root, &mut chat)?;
            // Running requests own their wire settings; this preference affects the next turn.
            chat.reasoning_effort = library::parse_reasoning(&c["reasoningEffort"])?;
            storage::save(&root, &chat)?;
            library::presentation(&chat)
        }
        "ai_chat_delete" => {
            let id = crate::string(&c, "chatId")?;
            if let Ok(chat) = storage::read(&root, id) {
                for game in chat.messages.iter().flat_map(|message| &message.games) {
                    practice::capture(&root, game)?;
                }
            }
            if let Ok(chat) = storage::read(&root, id)
                && let Some(task) = chat.active_task
                && let Some(job) = jobs::jobs()
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .get(&task)
            {
                job.cancelled.store(true, Ordering::Relaxed);
            }
            replay::forget_chat(&root, id)?;
            storage::delete(&root, id)?;
            Ok(Value::Null)
        }
        _ => Err("Unsupported conversation operation.".into()),
    }
}

#[cfg(test)]
mod poison_tests {
    use super::super::model::Conversation;
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn poisoned_partial_reply_is_never_saved_as_recovered_history() {
        for activity in [false, true] {
            let job = crate::ai::tests::job();
            let root = std::env::temp_dir().join(format!("govue-poison-{}", job.id));
            let mut chat: Conversation = serde_json::from_value(json!({
                "id":"fixture","title":"Fixture","providerId":"fixture",
                "providerLabel":"Fixture","model":"fixture","createdAt":0,
                "updatedAt":0,"messages":[],"activeTask":job.id,
            }))
            .unwrap();
            jobs::jobs()
                .lock()
                .unwrap()
                .insert(job.id.clone(), job.clone());
            job.cancelled.store(true, Ordering::Relaxed);
            if activity {
                crate::poison(&job.activity);
            } else {
                crate::poison(&job.state);
            }
            let result = storage::recover(&root, &mut chat);
            jobs::jobs().lock().unwrap().remove(&job.id);
            let saved = storage::path(&root, &chat.id).unwrap().exists();
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
