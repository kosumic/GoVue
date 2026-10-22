//! AI command validation, dispatch and worker ownership.
use crate::Result;
use crate::ai::providers::{CONFIG_LOCK, Provider};
use crate::ai::{
    accounts, activity, audio_requests, catalog, chat_sessions, client, connections, conversations,
    credentials, games, jobs, providers, usage, word_assistance,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

pub(super) static FOREGROUND: AtomicBool = AtomicBool::new(true);

pub(super) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    if c["op"]
        .as_str()
        .is_some_and(|op| op.starts_with("ai_chat_session_"))
    {
        return chat_sessions::dispatch(&c, &root);
    }
    if matches!(
        c["op"].as_str(),
        Some("ai_caption_audio" | "ai_caption_audio_cancel")
    ) {
        return audio_requests::dispatch(&c, &root);
    }
    if crate::string(&c, "op")?.starts_with("ai_practice_") {
        return conversations::dispatch(c, root);
    }
    if catalog::operation(crate::string(&c, "op")?) {
        return catalog::dispatch(c, root);
    }
    match crate::string(&c, "op")? {
        "ai_word_assistance" => word_assistance::start(c, root),
        "ai_word_audio" => word_assistance::audio(&c, &root),
        "ai_game_audio_release" => games::audio::release(crate::string(&c, "token")?),
        "ai_chats"
        | "ai_chat_find"
        | "ai_chat_update"
        | "ai_chat_get"
        | "ai_chat_branch"
        | "ai_chat_delete"
        | "ai_chat_send"
        | "ai_chat_reasoning"
        | "ai_chat_video_open"
        | "ai_chat_model"
        | "ai_chat_speech_hints"
        | "ai_chat_audio"
        | "ai_game_action"
        | "ai_game_export"
        | "ai_game_audio"
        | "ai_usage" => conversations::dispatch(c, root),
        "ai_foreground" => {
            FOREGROUND.store(c["active"] == true, Ordering::Relaxed);
            Ok(Value::Null)
        }
        "ai_config" => {
            let _g = CONFIG_LOCK
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            providers::public_config(&root)
        }
        "ai_settings" => {
            let agent_md = crate::string(&c, "agentMd")?;
            if agent_md.len() > 16_000 || agent_md.contains('\0') {
                return Err(
                    "Enter AGENT.md text of up to 16,000 bytes without null characters.".into(),
                );
            }
            let _g = CONFIG_LOCK
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            let mut cfg = providers::config(&root)?;
            cfg.agent_md = agent_md.into();
            providers::save(&root, &cfg)?;
            providers::public_config(&root)
        }
        "ai_save" => connections::save_connection(&c, &root),
        "ai_connection_preview" => {
            let p: Provider = serde_json::from_value(c["provider"].clone())
                .map_err(|_| "Invalid provider settings.")?;
            Ok(connections::preview(&p))
        }
        "ai_select" => {
            let id = crate::string(&c, "id")?;
            let _g = CONFIG_LOCK
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
            let mut cfg = providers::config(&root)?;
            if let Some(p) = cfg.providers.iter_mut().find(|p| p.id == id) {
                if !p.enabled {
                    return Err("Enable this provider before selecting a model.".into());
                }
                if let Some(model) = c["model"].as_str() {
                    if !p.models.iter().any(|m| m.id == model && !m.hidden) {
                        return Err("Choose a visible saved model.".into());
                    }
                    p.model = model.into();
                }
            } else if !cfg.groups.iter().any(|g| g.id == id) {
                return Err("Provider or model group was removed.".into());
            }
            cfg.active_id = Some(id.into());
            providers::save(&root, &cfg)?;
            providers::public_config(&root)
        }
        "ai_login" => {
            let p = providers::provider(&root, crate::string(&c, "id")?)?;
            if p.kind == "chatgpt" {
                credentials::login(root, p)
            } else {
                accounts::login(root, p, c["consent"] == true)
            }
        }
        "ai_start" => {
            let p = providers::provider(&root, crate::string(&c, "id")?)?;
            let kind = crate::string(&c, "kind")?.to_owned();
            if !matches!(kind.as_str(), "chat" | "models" | "disconnect" | "remove") {
                return Err("Unsupported AI request.".into());
            }
            if matches!(kind.as_str(), "disconnect" | "remove") {
                jobs::cancel_owner(&p.id)?;
            }
            let job = jobs::new_job(&p.id)?;
            let worker = job.clone();
            thread::spawn(move || {
                let result = match kind.as_str() {
                    "models" => client::models(&root, &p, &worker),
                    "chat" => client::chat(&root, &p, &c, &worker),
                    _ => credentials::disconnect(&root, &p, kind == "remove", &worker),
                };
                worker.finish(result);
            });
            Ok(json!({"taskId":job.id}))
        }
        "ai_chat_poll" | "ai_poll" | "ai_cancel" | "ai_forget" => {
            let id = crate::string(&c, "taskId")?;
            let job = jobs::jobs()
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                .get(id)
                .cloned()
                .ok_or("AI request is no longer available.")?;
            if c["op"] == "ai_chat_poll" {
                let mut cursor = job
                    .updates
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                let state = job
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                return Ok(cursor.poll(&state, usage::count(&c["afterRevision"])));
            }
            if c["op"] == "ai_poll" {
                return Ok(job
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .clone());
            }
            job.cancelled.store(true, Ordering::Relaxed);
            activity::settle(
                &mut job
                    .activity
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .blocks,
                activity::Status::Stopped,
            );
            let interrupted = conversations::interrupt(&job)?;
            job.publish_activity()?;
            if c["op"] == "ai_cancel" {
                job.state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?["status"] =
                    json!("cancelled");
                job.notifications.finish();
            } else {
                jobs::jobs()
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .remove(id);
                job.notifications.finish();
            }
            Ok(interrupted)
        }
        _ => Err("Unsupported AI operation.".into()),
    }
}
/// Validates an AI JSON command and returns a JSON success or failure envelope.
/// Long-running effects are dispatched to Rust workers by the command handler.
#[uniffi::export]
pub fn ai_execute(request: String, storage_path: String) -> String {
    let result = if request.len() > 1_000_000 {
        Err("AI command is too large.".into())
    } else {
        serde_json::from_str(&request)
            .map_err(|_| "Unreadable AI command.".into())
            .and_then(|v| dispatch(v, PathBuf::from(storage_path)))
    };
    match result {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}
