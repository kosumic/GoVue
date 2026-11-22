use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::{BrowserDocument, BrowserEvent, Core, CoreError};

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    uri: String,
    content_type: String,
    quality_height: u32,
    #[serde(default)]
    quality_fps: u32,
    #[serde(default)]
    startup_height: u32,
    #[serde(default)]
    headers: BTreeMap<String, Option<String>>,
}

impl Drop for Source {
    fn drop(&mut self) {
        if self.uri.starts_with("http://127.0.0.1:") {
            govue_core::release_playback_route(&self.uri);
        }
    }
}

pub struct PlaybackRequest<'a> {
    pub catalog: &'a Value,
    pub height: u32,
    pub fps: u32,
    pub audio_key: &'a str,
    pub user_agent: &'a str,
    pub allow_fallback: bool,
}

/// A planned source keeps its loopback route alive until it is discarded or
/// retired by the native player. Source addresses and headers stay in memory.
pub struct PlaybackSource(Rc<Source>);

impl PlaybackSource {
    pub async fn prepare_android(
        core: &Core,
        request: PlaybackRequest<'_>,
    ) -> Result<Self, CoreError> {
        let source: Source = core
            .execute(json!({"op": "media_playback", "manifest": "dash",
            "catalog": request.catalog, "height": request.height, "fps": request.fps,
            "audioKey": request.audio_key, "userAgent": request.user_agent,
            "allowFallback": request.allow_fallback}))
            .await?;
        Ok(Self(Rc::new(source)))
    }

    /// Plays an existing local download or a media file published by Android.
    pub fn offline(uri: String) -> Result<Self, CoreError> {
        let valid = Url::parse(&uri).is_ok_and(|url| {
            (url.scheme() == "file" && url.host_str().is_none() && url.path().starts_with('/'))
                || (url.scheme() == "content"
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none())
        });
        if !valid {
            return Err(CoreError::Request("Invalid local media address.".into()));
        }
        Ok(Self(Rc::new(Source {
            uri,
            content_type: "progressive".into(),
            quality_height: 0,
            quality_fps: 0,
            startup_height: 0,
            headers: BTreeMap::new(),
        })))
    }

    pub fn quality(&self) -> (u32, u32) {
        (self.0.quality_height, self.0.quality_fps)
    }
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackFit {
    Contain,
    Cover,
    Fill,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageVideoLayout {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    viewport_width: f32,
    visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmbeddedVideoFrame {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub visible: bool,
}

impl PageVideoLayout {
    /// Accept layout updates from the retained watch tab for the current
    /// document and video. The browser bridge authenticates the main frame.
    pub fn receive(
        core: &Core,
        event: &BrowserEvent,
        watch: &BrowserDocument,
        video_id: &str,
    ) -> Option<Self> {
        let BrowserEvent::PageMessage { document, raw, .. } = event else {
            return None;
        };
        if !document.same_document(watch) || raw.len() > 8192 {
            return None;
        }
        let message: Value = serde_json::from_str(raw).ok()?;
        if message["type"] != "playerLayout" || message["videoId"] != video_id {
            return None;
        }
        let identity: Value = core
            .state(json!({"op":"browser_identity", "url":document.url}))
            .ok()?;
        if identity["videoId"] != video_id {
            return None;
        }
        core.state::<Option<Self>>(json!({"op":"browser_layout", "layout":message["layout"]}))
            .ok()
            .flatten()
    }

    pub fn frame(self, workspace_width: f32) -> EmbeddedVideoFrame {
        let width = if workspace_width.is_finite() {
            workspace_width.max(0.0)
        } else {
            0.0
        };
        let scale = width / self.viewport_width;
        EmbeddedVideoFrame {
            left: self.left * scale,
            top: self.top * scale,
            width: (self.width * scale).max(1.0),
            height: (self.height * scale).max(1.0),
            visible: self.visible && width > 0.0,
        }
    }
}

#[derive(Clone, Default, PartialEq)]
struct Load {
    generation: u64,
    source: Option<Rc<Source>>,
    position: Option<f64>,
    playing: bool,
    rate: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    pub state: String,
    pub position: f64,
    pub buffered_position: f64,
    pub duration: Option<f64>,
    pub playing: bool,
    pub rate: f64,
    pub error_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoTrack {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bitrate: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaybackEvent {
    Applied {
        generation: u64,
    },
    FirstFrame {
        generation: u64,
    },
    Status {
        generation: u64,
        status: PlaybackStatus,
    },
    Tracks {
        generation: u64,
        tracks: Vec<VideoTrack>,
    },
}

impl PlaybackEvent {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Applied { generation }
            | Self::FirstFrame { generation }
            | Self::Status { generation, .. }
            | Self::Tracks { generation, .. } => *generation,
        }
    }
}

/// Owner of one mounted texture surface. Rust orders replacements and keeps
/// previous media routes alive until Android acknowledges the replacement.
#[derive(Clone)]
pub struct VideoPlayer {
    element: ElementRef,
    load: Signal<Load>,
    retired: Rc<RefCell<BTreeMap<u64, Rc<Source>>>>,
}

impl VideoPlayer {
    pub fn new() -> Self {
        Self {
            element: element_ref(),
            load: signal(Load::default()),
            retired: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    pub fn view(
        &self,
        fit: impl Fn() -> PlaybackFit + 'static,
        on_event: impl Fn(PlaybackEvent) + 'static,
    ) -> NativeView {
        let load = self.load;
        let retired = self.retired.clone();
        nsh_core::element::native("govue/video-player")
            .element_ref(&self.element)
            .pointer_events(PointerEvents::None)
            .prop("fit", move || json!(fit()))
            .prop("source", move || {
                load.with(|load| {
                    let source = load.source.as_deref();
                    json!({"generation": load.generation, "source": source,
                    "position": load.position, "playing": load.playing, "rate": load.rate})
                })
            })
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() != "playback" {
                    return;
                }
                let Ok(event) = PlaybackEvent::deserialize(&event.data) else {
                    return;
                };
                if let PlaybackEvent::Applied { generation } = &event {
                    retired
                        .borrow_mut()
                        .retain(|previous, _| previous >= generation);
                }
                if event.generation() == load.with(|current| current.generation) {
                    on_event(event);
                }
            })
    }

    /// Use the same mounted view as page controls move or cover the video.
    pub fn embedded_view(
        &self,
        frame: impl Fn() -> EmbeddedVideoFrame + 'static,
        fit: impl Fn() -> PlaybackFit + 'static,
        on_event: impl Fn(PlaybackEvent) + 'static,
    ) -> NativeView {
        let frame = computed(frame);
        self.view(fit, on_event)
            .absolute()
            .left(move || px(frame.with(|frame| frame.left)))
            .top(move || px(frame.with(|frame| frame.top)))
            .w(move || px(frame.with(|frame| frame.width)))
            .h(move || px(frame.with(|frame| frame.height)))
            .opacity(move || {
                if frame.with(|frame| frame.visible) {
                    1.0_f32
                } else {
                    0.0_f32
                }
            })
    }

    /// Omit the position to use the manifest's default, including the live edge.
    pub fn replace(
        &self,
        source: PlaybackSource,
        position: Option<f64>,
        playing: bool,
        rate: f64,
    ) -> u64 {
        self.set_load(
            Some(source.0),
            position.filter(|value| value.is_finite() && *value >= 0.0),
            playing,
            rate,
        )
    }

    pub fn clear(&self) -> u64 {
        self.set_load(None, None, false, 1.0)
    }

    fn set_load(
        &self,
        source: Option<Rc<Source>>,
        position: Option<f64>,
        playing: bool,
        rate: f64,
    ) -> u64 {
        let previous = self.load.get_untracked();
        if let Some(source) = previous.source {
            self.retired
                .borrow_mut()
                .insert(previous.generation, source);
        }
        let generation = previous.generation + 1;
        let rate = if rate.is_finite() && (0.25..=4.0).contains(&rate) {
            rate
        } else {
            1.0
        };
        self.load.set(Load {
            generation,
            source,
            position,
            playing,
            rate,
        });
        generation
    }

    pub fn play(&self, generation: u64) {
        self.command(generation, "play", Value::Null);
    }

    pub fn pause(&self, generation: u64) {
        self.command(generation, "pause", Value::Null);
    }

    pub fn seek(&self, generation: u64, seconds: f64) {
        if seconds.is_finite() && seconds >= 0.0 {
            self.command(generation, "seek", json!(seconds));
        }
    }

    pub fn rate(&self, generation: u64, value: f64) {
        if (0.25..=4.0).contains(&value) {
            self.command(generation, "rate", json!(value));
        }
    }

    pub fn volume(&self, generation: u64, value: f64) {
        if (0.0..=1.0).contains(&value) {
            self.command(generation, "volume", json!(value));
        }
    }

    pub fn select_video_track(&self, generation: u64, track_id: Option<&str>) {
        self.command(generation, "videoTrack", json!(track_id));
    }

    pub fn enable_video(&self, generation: u64, enabled: bool) {
        self.command(generation, "videoEnabled", json!(enabled));
    }

    fn command(&self, generation: u64, action: &str, value: Value) {
        if self
            .load
            .with_untracked(|current| current.source.is_some() && current.generation == generation)
        {
            self.element
                .command(action, json!({"generation": generation, "value": value}));
        }
    }
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self::new()
    }
}
