//! Rust adapters for services hosted by GoVue's Native Shell platform code.

mod browser;
mod core;
mod player;

pub use browser::{BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab};
pub use core::{Core, CoreError};
pub use player::{PlayerRuntime, PlayerRuntimeError, PlayerSignatures};
