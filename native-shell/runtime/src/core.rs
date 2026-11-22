use std::fmt;
use std::path::Path;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use govue_core::CoreEvents;
use nsh_core::element::blocking;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::broadcast;

static UPDATES: OnceLock<broadcast::Sender<String>> = OnceLock::new();

struct NativeUpdates(broadcast::Sender<String>);

impl CoreEvents for NativeUpdates {
    fn changed(&self, channel: String) {
        let _ = self.0.send(channel);
    }
}

#[derive(Debug)]
pub enum CoreError {
    StorageUnavailable,
    WorkerStopped,
    InvalidResponse(serde_json::Error),
    Request(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StorageUnavailable => f.write_str("App storage is unavailable."),
            Self::WorkerStopped => f.write_str("The GoVue core worker stopped."),
            Self::InvalidResponse(_) => f.write_str("Unreadable GoVue core response."),
            Self::Request(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidResponse(error) => Some(error),
            _ => None,
        }
    }
}

/// GoVue's existing Rust core and app-private storage, shared by the reader and chat.
#[derive(Clone)]
pub struct Core {
    storage: String,
}

impl Core {
    /// Resolves the original `Context.getFilesDir()/transcripts` from the
    /// standard Android `NshView` data directory, `files/nsh/<app id>`.
    #[cfg(target_os = "android")]
    pub fn android() -> Result<Self, CoreError> {
        use std::ffi::OsStr;

        let data = nsh_core::element::data_dir().ok_or(CoreError::StorageUnavailable)?;
        let nsh = data
            .parent()
            .filter(|parent| parent.file_name() == Some(OsStr::new("nsh")))
            .ok_or(CoreError::StorageUnavailable)?;
        let files = nsh.parent().ok_or(CoreError::StorageUnavailable)?;
        Self::with_storage(&files.join("transcripts"))
    }

    /// Opens the app's existing GoVue core root. The host resolves its sandbox path.
    pub fn with_storage(path: &Path) -> Result<Self, CoreError> {
        let storage = path
            .to_str()
            .filter(|_| path.is_absolute())
            .ok_or(CoreError::StorageUnavailable)?
            .to_owned();
        Ok(Self { storage })
    }

    /// Receives channel names when core state changes. A lagging receiver
    /// should refresh its current snapshot before continuing to listen.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        UPDATES
            .get_or_init(|| {
                let (sender, _) = broadcast::channel(256);
                govue_core::install_core_events(Box::new(NativeUpdates(sender.clone())));
                sender
            })
            .subscribe()
    }

    pub fn state<T: DeserializeOwned>(&self, request: Value) -> Result<T, CoreError> {
        decode(&govue_core::state_execute(request.to_string()))
    }

    pub(crate) fn register_caption_page(
        &self,
        adapter: &str,
        video: Option<&str>,
    ) -> Result<(), CoreError> {
        govue_core::register_caption_page(adapter, video, &self.storage).map_err(CoreError::Request)
    }

    /// Resolve the watch video using the shared extraction jobs. Dropping this
    /// future cancels the job even after its worker has started.
    pub async fn resolve_media(
        &self,
        video_id: String,
        user_agent: String,
        fresh: bool,
    ) -> Result<Value, CoreError> {
        self.extract(move |storage, active| {
            govue_core::resolve_media_catalog(storage, &video_id, &user_agent, fresh, active)
        })
        .await
    }

    #[cfg(target_os = "android")]
    pub(crate) async fn resolve_caption(
        &self,
        video: String,
        track: String,
    ) -> Result<(), CoreError> {
        self.extract(move |storage, active| {
            govue_core::resolve_caption_track(storage, &video, &track, active)
        })
        .await
    }

    async fn extract<T: Send + 'static>(
        &self,
        run: impl FnOnce(&str, &dyn Fn() -> bool) -> Result<T, String> + Send + 'static,
    ) -> Result<T, CoreError> {
        let cancel = ExtractionCancellation(Arc::new(AtomicBool::new(false)));
        let stopped = cancel.0.clone();
        let storage = self.storage.clone();
        blocking(move || {
            run(&storage, &|| !stopped.load(Ordering::Acquire)).map_err(CoreError::Request)
        })
        .await
        .map_err(|_| CoreError::WorkerStopped)?
    }

    pub async fn execute<T: DeserializeOwned + Send + 'static>(
        &self,
        request: Value,
    ) -> Result<T, CoreError> {
        self.run(request, govue_core::execute).await
    }

    pub async fn ai<T: DeserializeOwned + Send + 'static>(
        &self,
        request: Value,
    ) -> Result<T, CoreError> {
        self.run(request, govue_core::ai_execute).await
    }

    async fn run<T: DeserializeOwned + Send + 'static>(
        &self,
        request: Value,
        execute: fn(String, String) -> String,
    ) -> Result<T, CoreError> {
        let storage = self.storage.clone();
        blocking(move || decode(&execute(request.to_string(), storage)))
            .await
            .map_err(|_| CoreError::WorkerStopped)?
    }
}

struct ExtractionCancellation(Arc<AtomicBool>);

impl Drop for ExtractionCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    value: Value,
    error: Option<String>,
}

fn decode<T: DeserializeOwned>(response: &str) -> Result<T, CoreError> {
    let reply: Reply = serde_json::from_str(response).map_err(CoreError::InvalidResponse)?;
    if reply.ok {
        serde_json::from_value(reply.value).map_err(CoreError::InvalidResponse)
    } else {
        let message = reply
            .error
            .unwrap_or_else(|| "The GoVue core request failed.".into());
        Err(CoreError::Request(message))
    }
}
