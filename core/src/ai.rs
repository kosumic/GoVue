//! Provider protocols, OAuth and transcript context are shared by both platforms.
//! Native adapters only handle HTTPS bytes and protected credential storage.
mod accounts;
mod activity;
mod audio_requests;
mod catalog;
mod chat_sessions;
mod client;
mod commands;
mod connections;
mod conversations;
mod credentials;
mod games;
mod http;
mod identity;
mod jobs;
mod notifications;
mod plan;
mod practice;
mod providers;
mod replay;
mod search;
mod sse;
mod tools;
mod updates;
mod usage;
mod word_assistance;

#[cfg(test)]
mod tests;

pub use commands::ai_execute;
pub use http::{AiHeader, AiHttpRequest, AiPlatform, AiResponse, AiSecret, install_ai_platform};
pub(crate) use notifications::foreground as chat_notification_foreground;
pub(crate) use notifications::visible as chat_notification_visibility;
pub use notifications::{ChatNotificationPlatform, install_chat_notification_platform};
