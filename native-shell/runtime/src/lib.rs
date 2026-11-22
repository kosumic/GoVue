//! Rust adapters for services hosted by GoVue's Native Shell platform code.

mod core;
mod player;

pub use core::{Core, CoreError};
pub use player::{PlayerRuntime, PlayerRuntimeError, PlayerSignatures};
