//! Rebuildable metadata index. Callers hold CHATS throughout each transaction.
use super::{atomic_write, ids, read, storage_error, sync_directory};
use crate::{
    Result,
    ai::conversations::{BranchRef, Conversation, MAX_CHATS},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Read, path::Path};

const VERSION: u32 = 1;
const MAX_INDEX: usize = 1_000_000;
const INDEX: &str = "ai/chat-index.json";
const DIRTY: &str = "ai/chat-index.dirty";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::ai::conversations) struct TranscriptMetadata {
    pub video_id: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::ai::conversations) struct Metadata {
    pub id: String,
    pub branched_from: Option<BranchRef>,
    pub pinned: bool,
    pub archived: bool,
    pub video_id: Option<String>,
    pub title: String,
    pub provider_id: String,
    pub provider_label: String,
    pub model: String,
    pub updated_at: u64,
    pub transcript: Option<TranscriptMetadata>,
}

impl From<&Conversation> for Metadata {
    fn from(chat: &Conversation) -> Self {
        Self {
            id: chat.id.clone(),
            branched_from: chat.branched_from.clone(),
            pinned: chat.pinned,
            archived: chat.archived,
            video_id: chat.video_id.clone(),
            title: chat.title.clone(),
            provider_id: chat.provider_id.clone(),
            provider_label: chat.provider_label.clone(),
            model: chat.model.clone(),
            updated_at: chat.updated_at,
            transcript: chat.transcript.as_ref().map(|t| TranscriptMetadata {
                video_id: t.video_id.clone(),
                title: t.title.clone(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    chats: Vec<Metadata>,
}

fn encode(chats: Vec<Metadata>) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&Index {
        version: VERSION,
        chats,
    })
    .map_err(|_| "Could not encode chat history.")?;
    if bytes.len() > MAX_INDEX {
        return Err("Chat history index is too large.".into());
    }
    Ok(bytes)
}

fn cached(root: &Path) -> Option<Vec<Metadata>> {
    if root.join(DIRTY).try_exists().ok()? {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(root.join(INDEX))
        .ok()?
        .take((MAX_INDEX + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_INDEX {
        return None;
    }
    let index: Index = serde_json::from_slice(&bytes).ok()?;
    let mut seen = HashSet::new();
    if index.version != VERSION
        || index.chats.len() > MAX_CHATS * 2
        || index
            .chats
            .iter()
            .any(|chat| !crate::ai::valid_id(&chat.id) || !seen.insert(&chat.id))
    {
        return None;
    }
    Some(index.chats)
}

fn clean(root: &Path) -> Result<()> {
    match fs::remove_file(root.join(DIRTY)) {
        Ok(()) => sync_directory(&root.join("ai")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(storage_error(cause)),
    }
}

pub(in crate::ai::conversations) fn list(root: &Path) -> Result<Vec<Metadata>> {
    let mut rows = if let Some(chats) = cached(root) {
        chats
    } else {
        // One-time migration or interrupted commit: inspect at most one bounded
        // record at a time, without retaining messages or recovering active jobs.
        let mut chats = Vec::new();
        for id in ids(root)? {
            if let Ok(chat) = read(root, &id) {
                chats.push(Metadata::from(&chat));
            }
        }
        atomic_write(&root.join(INDEX), &encode(chats.clone())?)?;
        clean(root)?;
        chats
    };
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    Ok(rows)
}

pub(super) fn update(
    root: &Path,
    id: &str,
    metadata: Option<Metadata>,
    mutate: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut rows = list(root)?;
    rows.retain(|chat| chat.id != id);
    rows.extend(metadata);
    let bytes = encode(rows)?;
    // Persist invalidation before the record changes. If any later step fails,
    // the next index access rebuilds from committed records rather than serving
    // stale metadata. The mutex hides the intermediate state from other callers.
    atomic_write(&root.join(DIRTY), b"1")?;
    mutate()?;
    atomic_write(&root.join(INDEX), &bytes)?;
    clean(root)
}
