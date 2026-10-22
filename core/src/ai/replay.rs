//! Provider wire history is memory-only: signed reasoning never enters saved chats.
use crate::Result;
use crate::ai::jobs::Job;
use crate::ai::providers;
use crate::ai::providers::Provider;
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

const MAX_BYTES: usize = 8_000_000;
const MAX_ENTRIES: usize = 12;
static HISTORY: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

#[derive(PartialEq, Eq)]
struct Scope {
    root: PathBuf,
    chat: String,
    provider: String,
    model: String,
    protocol: String,
}
struct Entry {
    scope: Scope,
    reply: String,
    rows: Vec<Value>,
    bytes: usize,
}
fn scope(p: &Provider, job: &Job, protocol: &str) -> Option<Scope> {
    let (root, chat) = job.conversation.get()?;
    Some(Scope {
        root: root.clone(),
        chat: chat.clone(),
        provider: p.id.clone(),
        model: p.model.clone(),
        protocol: protocol.into(),
    })
}
pub(super) fn field(protocol: &str) -> &'static str {
    match protocol {
        "responses" => "input",
        "gemini" => "contents",
        _ => "messages",
    }
}
pub(super) fn restore(p: &Provider, c: &Value, job: &Job, protocol: &str) -> Option<Vec<Value>> {
    let scope = scope(p, job, protocol)?;
    let reply = c["historyTail"].as_str()?;
    let mut history = HISTORY.lock().ok()?;
    let index = history
        .iter()
        .position(|e| e.scope == scope && e.reply == reply)?;
    let entry = history.remove(index)?;
    let rows = entry.rows.clone();
    history.push_back(entry);
    drop(history);
    Some(rows)
}
pub(super) fn remember(p: &Provider, c: &Value, job: &Job, protocol: &str, body: &Value) {
    let Some(scope) = scope(p, job, protocol) else {
        return;
    };
    let Some(reply) = c["replyId"].as_str().filter(|id| providers::valid_id(id)) else {
        return;
    };
    let Some(rows) = body[field(protocol)].as_array() else {
        return;
    };
    let bytes = body[field(protocol)].to_string().len();
    if bytes > MAX_BYTES {
        return;
    }
    let Ok(mut history) = HISTORY.lock() else {
        return;
    };
    if job.cancelled.load(Ordering::Relaxed) {
        return;
    }
    history.retain(|e| e.scope != scope || e.reply != reply);
    while history.len() >= MAX_ENTRIES
        || history.iter().map(|e| e.bytes).sum::<usize>() + bytes > MAX_BYTES
    {
        history.pop_front();
    }
    history.push_back(Entry {
        scope,
        reply: reply.into(),
        rows: rows.clone(),
        bytes,
    });
}
pub(super) fn forget_provider(id: &str) -> Result<()> {
    HISTORY
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .retain(|e| e.scope.provider != id);
    Ok(())
}
pub(super) fn forget_chat(root: &Path, id: &str) -> Result<()> {
    HISTORY
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .retain(|e| e.scope.root != root || e.scope.chat != id);
    Ok(())
}
