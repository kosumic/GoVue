//! Saved conversation, message and transcript reference data.
use crate::Result;
use crate::ai::{activity, attachments, games, identity};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const MAX_CHAT: usize = 4_000_000;
pub(super) const MAX_CHATS: usize = 100;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TranscriptRef {
    pub(super) video_id: String,
    pub(super) track_id: String,
    pub(super) title: String,
    pub(super) language: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::ai) struct PreparedInput {
    pub content: String,
    pub transcript: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Message {
    pub(super) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) usage_chat_id: Option<String>,
    pub(super) role: String,
    pub(super) content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) attachments: Vec<attachments::Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) prepared_input: Option<PreparedInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) games: Vec<games::Game>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) blocks: Vec<activity::Block>,
    pub(super) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    #[serde(default)]
    pub(super) reasoning: String,
    #[serde(default)]
    pub(super) usage: Value,
    // Projected histories can be replayed; derived counts are always recomputed.
    #[serde(default, rename = "usageDetails", skip_serializing)]
    pub(super) _usage_details: serde::de::IgnoredAny,
    #[serde(default)]
    pub(super) model: String,
    #[serde(default)]
    pub(super) provider_label: String,
    #[serde(default)]
    pub(super) reasoning_effort: String,
    #[serde(default)]
    pub(super) provider_id: String,
    #[serde(default)]
    pub(super) model_name: String,
    #[serde(default)]
    pub(super) completed_at: u64,
    #[serde(default)]
    pub(super) context_window: Option<u64>,
    #[serde(default)]
    pub(super) max_output_tokens: Option<u64>,
}
impl Message {
    pub(super) fn try_new(role: &str, content: String) -> Result<Self> {
        Ok(Self {
            id: identity::random()?,
            usage_chat_id: None,
            role: role.into(),
            content,
            attachments: Vec::new(),
            prepared_input: None,
            games: Vec::new(),
            blocks: Vec::new(),
            status: if role == "user" {
                "completed"
            } else {
                "streaming"
            }
            .into(),
            error: None,
            reasoning: String::new(),
            usage: Value::Null,
            _usage_details: serde::de::IgnoredAny,
            model: String::new(),
            provider_label: String::new(),
            reasoning_effort: String::new(),
            provider_id: String::new(),
            model_name: String::new(),
            completed_at: 0,
            context_window: None,
            max_output_tokens: None,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QueuedMessage {
    pub(super) id: String,
    pub(super) content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) attachments: Vec<attachments::Attachment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BranchRef {
    pub(super) chat_id: String,
    pub(super) message_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Conversation {
    pub(super) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) branched_from: Option<BranchRef>,
    #[serde(default)]
    pub(super) pinned: bool,
    #[serde(default)]
    pub(super) archived: bool,
    #[serde(default)]
    pub(super) title_edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) video_id: Option<String>,
    pub(super) title: String,
    pub(super) provider_id: String,
    pub(super) provider_label: String,
    pub(super) model: String,
    pub(super) created_at: u64,
    pub(super) updated_at: u64,
    pub(super) transcript: Option<TranscriptRef>,
    pub(super) messages: Vec<Message>,
    pub(super) active_task: Option<String>,
    #[serde(default)]
    pub(super) reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) pending_messages: Vec<QueuedMessage>,
}
