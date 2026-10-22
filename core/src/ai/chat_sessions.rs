//! Conversation/task ownership and deferred game feedback live beside saved chats.
use crate::{Result, identity::SessionId, string};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
struct Session {
    version: u64,
    task: Option<String>,
    chat: Option<String>,
}
#[derive(Clone, Eq, Hash, PartialEq)]
struct ControllerKey {
    root: PathBuf,
    session: SessionId,
}
#[derive(Eq, Hash, PartialEq)]
struct FeedbackKey {
    root: PathBuf,
    chat: String,
}
impl FeedbackKey {
    fn new(root: &Path, chat: &str) -> Self {
        Self {
            root: root.into(),
            chat: chat.into(),
        }
    }
}
static CONTROLLERS: OnceLock<Mutex<HashMap<ControllerKey, Arc<Mutex<Session>>>>> = OnceLock::new();
static FEEDBACK: OnceLock<Mutex<HashMap<FeedbackKey, (String, u64)>>> = OnceLock::new();

pub(super) fn feedback(root: &Path, chat: &str, game: &str, revision: u64) -> Result<()> {
    FEEDBACK
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .insert(FeedbackKey::new(root, chat), (game.into(), revision));
    crate::changed("chat-feedback");
    Ok(())
}
fn clear_feedback(root: &Path, chat: Option<&str>) -> Result<()> {
    if let Some(chat) = chat {
        FEEDBACK
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .remove(&FeedbackKey::new(root, chat));
    }
    Ok(())
}
fn cancel(session: &mut Session, root: &Path) -> Value {
    let Some(task) = session.task.take() else {
        return Value::Null;
    };
    let result = super::dispatch(json!({"op":"ai_cancel","taskId":task}), root.into())
        .unwrap_or(Value::Null);
    let _ = super::dispatch(json!({"op":"ai_forget","taskId":task}), root.into());
    result
}
pub(super) fn dispatch(c: &Value, root: &Path) -> Result<Value> {
    let id = SessionId::parse(string(c, "sessionId")?, 150).map_err(|_| "Invalid chat session.")?;
    let key = ControllerKey {
        root: root.into(),
        session: id,
    };
    let mut controllers = CONTROLLERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Chats are busy.")?;
    if !controllers.contains_key(&key) && controllers.len() >= 16 {
        return Err("Too many open chat sessions.".into());
    }
    let controller = controllers.entry(key).or_default().clone();
    drop(controllers);
    let mut session = controller.lock().map_err(|_| "This chat is busy.")?;
    let version = c["version"].as_u64().ok_or("Missing chat revision.")?;
    if c["op"] == "ai_chat_session_forget" {
        let task = string(c, "taskId")?;
        if session.task.as_deref() == Some(task) {
            session.task = None;
        }
        return super::dispatch(json!({"op":"ai_forget","taskId":task}), root.into());
    }
    if c["op"] == "ai_chat_session_cancel" {
        // A delayed cancellation acknowledges its generation without cancelling
        // a newer send that already started in that same generation.
        if version <= session.version {
            return Ok(Value::Null);
        }
        session.version = version;
        if c["clearFeedback"] == true {
            clear_feedback(root, c["chatId"].as_str().or(session.chat.as_deref()))?;
        }
        // A view may adopt an already-running saved conversation without sending.
        if session.task.is_none()
            && let Some(chat) = c["chatId"].as_str()
            && let Ok(saved) =
                super::dispatch(json!({"op":"ai_chat_get","chatId":chat}), root.into())
        {
            session.task = saved["activeTask"].as_str().map(str::to_owned);
            session.chat = Some(chat.into());
        }
        return Ok(cancel(&mut session, root));
    }
    if version < session.version {
        return Err("This chat request was superseded.".into());
    }
    if version > session.version {
        cancel(&mut session, root);
        session.version = version;
    }
    if c["clearFeedback"] == true {
        clear_feedback(
            root,
            c["command"]["chatId"].as_str().or(session.chat.as_deref()),
        )?;
    }
    let mut command = if c["op"] == "ai_chat_session_feedback" {
        let chat = string(c, "chatId")?;
        let pending = FEEDBACK
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Game feedback is busy.")?
            .get(&FeedbackKey::new(root, chat))
            .cloned();
        let Some((game, revision)) = pending else {
            return Ok(Value::Null);
        };
        let saved = super::dispatch(json!({"op":"ai_chat_get","chatId":chat}), root.into())?;
        if saved["activeTask"].is_string() {
            return Ok(Value::Null);
        }
        let mut command = json!({"op":"ai_chat_send","id":saved["providerId"],"chatId":chat,
            "prompt":"Give me feedback on my latest interaction with the first game in the attached game activity.",
            "focusGameId":game});
        command["feedbackRevision"] = json!(revision);
        command
    } else if c["op"] == "ai_chat_session_send" {
        let mut command = c["command"]
            .as_object()
            .cloned()
            .map(Value::Object)
            .ok_or("Invalid chat command.")?;
        command["op"] = json!("ai_chat_send");
        if c["queue"] == true {
            let chat = command["chatId"]
                .as_str()
                .or(session.chat.as_deref())
                .ok_or("This chat has not started yet.")?;
            let saved = super::dispatch(json!({"op":"ai_chat_get","chatId":chat}), root.into())?;
            command["chatId"] = json!(chat);
            command["id"] = saved["providerId"].clone();
        }
        command
    } else {
        return Err("Unknown chat session action.".into());
    };
    let feedback = command
        .as_object_mut()
        .and_then(|c| c.remove("feedbackRevision"));
    let chat = command["chatId"].as_str().map(str::to_owned);
    let game = command["focusGameId"].as_str().map(str::to_owned);
    command["notificationSession"] = c["sessionId"].clone();
    let result = super::dispatch(command, root.into())?;
    session.task = result["taskId"].as_str().map(str::to_owned);
    session.chat = result["conversation"]["id"].as_str().map(str::to_owned);
    drop(session);
    if let (Some(chat), Some(game), Some(revision)) =
        (chat, game, feedback.and_then(|v| v.as_u64()))
    {
        let mut pending = FEEDBACK
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Game feedback is busy.")?;
        if pending.get(&FeedbackKey::new(root, &chat)) == Some(&(game, revision)) {
            pending.remove(&FeedbackKey::new(root, &chat));
        }
    }
    Ok(result)
}
