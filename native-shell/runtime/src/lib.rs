//! Rust adapters for services hosted by GoVue's Native Shell platform code.

mod browser;
mod caption_overlay;
mod captions;
mod core;
mod player;
mod tokens;
mod video;

pub use browser::{BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab};
pub use caption_overlay::{
    CaptionDisplay, CaptionFrame, CaptionLayout, CaptionStyle, caption_overlay,
};
pub use captions::CaptionPage;
pub use core::{Core, CoreError};
pub use player::{PlayerRuntime, PlayerRuntimeError, PlayerSignatures};
pub use tokens::TokenRuntime;
pub use video::{
    EmbeddedVideoFrame, PageVideoLayout, PlaybackEvent, PlaybackFit, PlaybackRequest,
    PlaybackSource, PlaybackStatus, VideoPlayer, VideoTrack,
};
