//! Local conversations and provider-independent turn construction.
mod activities;
mod commands;
mod index;
mod library;
mod model;
mod storage;
mod turns;

pub(super) use activities::{create_game, read_game_library};
pub(super) use commands::dispatch;
pub(super) use model::PreparedInput;
pub(super) use turns::{history, interrupt, record_input, steer};

pub(super) fn attachment_storage_lock() -> crate::Result<std::sync::MutexGuard<'static, ()>> {
    storage::CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.".into())
}
pub(super) fn save_attachment(path: &std::path::Path, bytes: &[u8]) -> crate::Result<()> {
    storage::atomic_write(path, bytes)
}
pub(super) fn release_attachments(root: &std::path::Path, ids: &[String]) -> crate::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut candidates: std::collections::HashSet<_> = ids.iter().collect();
    for id in storage::ids(root)? {
        let chat = storage::read(root, &id)?;
        for image in chat
            .messages
            .iter()
            .flat_map(|m| &m.attachments)
            .chain(chat.pending_messages.iter().flat_map(|m| &m.attachments))
        {
            candidates.remove(&image.id);
        }
    }
    for id in candidates {
        super::attachments::remove(root, id)?;
    }
    Ok(())
}
