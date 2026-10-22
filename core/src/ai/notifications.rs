//! Reply notification policy is shared; native adapters only present OS alerts.
use super::valid_id;
use crate::{Result, string};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

#[uniffi::export(callback_interface)]
pub trait ChatNotificationPlatform: Send + Sync {
    /// Revisions let adapters discard effects overtaken by opening the chat.
    fn update(&self, chat_id: String, show: bool, revision: u64);
}

static PLATFORM: OnceLock<Box<dyn ChatNotificationPlatform>> = OnceLock::new();
static ATTENTION: Mutex<Attention> = Mutex::new(Attention {
    foreground: true,
    chat: None,
    session: None,
    revision: 0,
});

#[uniffi::export]
pub fn install_chat_notification_platform(platform: Box<dyn ChatNotificationPlatform>) {
    let _ = PLATFORM.set(platform);
}

struct Attention {
    foreground: bool,
    chat: Option<String>,
    session: Option<String>,
    revision: u64,
}
impl Attention {
    fn effect(&mut self, chat: String, show: bool) -> (String, bool, u64) {
        self.revision += 1;
        (chat, show, self.revision)
    }
    fn completed(&mut self, chat: &str) -> Option<(String, bool, u64)> {
        if self.foreground && self.chat.as_deref() == Some(chat) {
            None
        } else {
            Some(self.effect(chat.into(), true))
        }
    }
    fn dismiss_visible(&mut self) -> Option<(String, bool, u64)> {
        self.chat
            .clone()
            .filter(|_| self.foreground)
            .map(|chat| self.effect(chat, false))
    }
}
fn deliver(effect: Option<(String, bool, u64)>) {
    if let (Some(platform), Some((chat, show, revision))) = (PLATFORM.get(), effect) {
        platform.update(chat, show, revision);
    }
}
pub(crate) fn foreground(active: bool) {
    let effect = ATTENTION.lock().ok().and_then(|mut state| {
        state.foreground = active;
        state.dismiss_visible()
    });
    deliver(effect);
}
pub(crate) fn visible(command: &Value) -> Result<Value> {
    let session = command["sessionId"]
        .as_str()
        .map(|id| crate::identity::SessionId::parse(id, 150).map(|id| id.as_str().to_owned()))
        .transpose()?;
    let chat = if command["chatId"].is_null() {
        None
    } else {
        let id = string(command, "chatId")?;
        if !valid_id(id) {
            return Err("Invalid chat notification target.".into());
        }
        Some(id.into())
    };
    let effect = {
        let mut state = ATTENTION
            .lock()
            .map_err(|_| "Chat notifications are unavailable.")?;
        state.chat = chat;
        state.session = session;
        state.dismiss_visible()
    };
    deliver(effect);
    Ok(Value::Null)
}
pub(super) fn started(session: Option<&str>, chat: &str) {
    if let Ok(mut state) = ATTENTION.lock()
        && session.is_some()
        && state.session.as_deref() == session
    {
        // Correlate before spawning: a fast first reply may finish before React
        // receives its new conversation ID and renders the next snapshot.
        state.chat = Some(chat.into());
    }
}
pub(super) fn completed(saved: &Result<Value>) {
    // A stale/deleted turn has no saved conversation. Failed and stopped replies
    // are never eligible, including when the provider returned partial text.
    let Some(chat) = saved
        .as_ref()
        .ok()
        .and_then(|saved| saved.get("conversation"))
    else {
        return;
    };
    if chat["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .is_none_or(|reply| reply["status"] != "completed")
    {
        return;
    }
    let Some(id) = chat["id"].as_str() else {
        return;
    };
    let effect = ATTENTION
        .lock()
        .ok()
        .and_then(|mut state| state.completed(id));
    // No conversation, provider, or storage lock is held during platform effects.
    deliver(effect);
}
