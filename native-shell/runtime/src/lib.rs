//! Rust adapters for services hosted by GoVue's Native Shell platform code.

#[cfg(target_os = "android")]
mod ai_android;
mod browser;
mod caption_overlay;
mod captions;
mod core;
mod player;
mod reader;
#[cfg(target_os = "android")]
mod subtitles;
mod tokens;
mod video;
#[cfg(target_os = "android")]
mod watch;

pub use browser::{BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab};
pub use caption_overlay::{
    CaptionDisplay, CaptionFrame, CaptionLayout, CaptionStyle, caption_overlay,
};
pub use captions::CaptionPage;
pub use core::{Core, CoreError};
pub use player::{PlayerRuntime, PlayerRuntimeError, PlayerSignatures};
pub use reader::{
    ReaderDocument, ReaderLine, ReaderResults, ReaderRow, ReaderSnapshot, ReaderTracks, Transcript,
    TranscriptReader, TranscriptSegment, TranscriptTrack,
};
#[cfg(target_os = "android")]
pub use subtitles::WatchSubtitles;
pub use tokens::TokenRuntime;
pub use video::{
    EmbeddedVideoFrame, PageVideoLayout, PlaybackEvent, PlaybackFit, PlaybackRequest,
    PlaybackSource, PlaybackStatus, VideoPlayer, VideoTrack,
};
#[cfg(target_os = "android")]
pub use watch::{
    AudioTrack, PlaybackSelection, SubtitleTrack, VideoQuality, WatchPlayback, WatchVideo,
};
