use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nsh_core::{element::Spawned, prelude::*};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use tokio::sync::oneshot;
use url::Url;

use crate::{
    BrowserDocument, BrowserEvent, BrowserTab, Core, CoreError, EmbeddedVideoFrame,
    PageVideoLayout, PlaybackEvent, PlaybackFit, PlaybackRequest, PlaybackSource, PlaybackStatus,
    VideoPlayer,
};

#[derive(Clone)]
struct WatchDocument {
    tab: BrowserTab,
    document: BrowserDocument,
    video_id: String,
    origin: String,
}

impl WatchDocument {
    fn inject(&self, script: &str) {
        self.tab.inject(&self.document, script);
    }

    fn native_player(&self, enabled: bool) {
        let video = json!(self.video_id);
        self.inject(&format!("window.__govueMedia?.native({video},{enabled});"));
    }

    fn request_position(&self, request: &str) {
        let request = json!(request);
        let video = json!(self.video_id);
        self.inject(&format!(
            "window.__govue?.position({request},{video},true);"
        ));
    }
}

#[derive(Deserialize)]
struct Attempt {
    token: u64,
    height: u32,
    fps: u32,
    settings: Map<String, Value>,
}

struct PositionRequest {
    id: String,
    send: oneshot::Sender<Option<f64>>,
}

#[derive(Default)]
struct Page {
    tab: Option<BrowserTab>,
    watch: Option<WatchDocument>,
    user_agent: String,
    pending: Option<(u64, Spawned<()>)>,
    position: Option<PositionRequest>,
    displayed: bool,
    bridge_ready: bool,
    media_ready: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WatchVideo {
    pub video_id: String,
    pub title: String,
    pub author: String,
    pub generation: u64,
    pub offline: bool,
    pub quality_height: u32,
    pub quality_fps: u32,
    pub qualities: Rc<[VideoQuality]>,
    pub audio_key: String,
    pub audio_tracks: Rc<[AudioTrack]>,
    pub subtitles: Rc<[SubtitleTrack]>,
    pub chapters: Rc<[Chapter]>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub start: f64,
    pub title: String,
    pub preview_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SponsorSegment {
    pub start: f64,
    pub end: f64,
    pub category: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VideoQuality {
    pub height: u32,
    pub fps: Option<u32>,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AudioTrack {
    pub key: String,
    pub label: String,
}

pub enum PlaybackSelection {
    Quality { height: u32, fps: u32 },
    Audio(String),
}

type StreamResult = Box<dyn FnOnce(Result<(), String>)>;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SubtitleTrack {
    pub id: String,
    pub language: String,
    pub label: String,
    pub automatic: bool,
}

/// Owns the handoff from the active YouTube document to one retained player.
#[derive(Clone)]
pub struct WatchPlayback {
    core: Core,
    owner: Owner,
    session_id: u64,
    player: VideoPlayer,
    page: Rc<RefCell<Page>>,
    sponsorship: Signal<Option<(String, Value)>>,
    layout: Signal<Option<PageVideoLayout>>,
    presentation: Signal<Option<EmbeddedVideoFrame>>,
    settings: Signal<Option<Map<String, Value>>>,
    foreground: Signal<bool>,
    queue_target: Signal<Option<(String, Instant)>>,
    fit: Signal<PlaybackFit>,
    video: Signal<Option<WatchVideo>>,
    status: Signal<Option<Rc<PlaybackStatus>>>,
    remote_status: Signal<Option<(u64, Rc<PlaybackStatus>)>>,
    catalog: Signal<Option<(u64, Rc<Value>)>>,
    seek_intent: Signal<Option<(u64, f64)>>,
    preparing: Signal<bool>,
    error: Signal<Option<String>>,
}

impl WatchPlayback {
    pub fn new(core: Core) -> Result<Self, CoreError> {
        let session_id = core.state(json!({"op":"player_session_create"}))?;
        let watch = Self {
            core,
            owner: Owner::current(),
            session_id,
            player: VideoPlayer::new(),
            page: Rc::new(RefCell::new(Page::default())),
            sponsorship: signal(None),
            layout: signal(None),
            presentation: signal(None),
            settings: signal(None),
            foreground: signal(false),
            queue_target: signal(None),
            fit: signal(PlaybackFit::Contain),
            video: signal(None),
            status: signal(None),
            remote_status: signal(None),
            catalog: signal(None),
            seek_intent: signal(None),
            preparing: signal(false),
            error: signal(None),
        };
        cleanup({
            let watch = watch.clone();
            move || {
                watch.cancel_pending();
                let _ = watch.session::<Value>("dispose", json!({}));
            }
        });
        effect({
            let watch = watch.clone();
            move || {
                if let Some(video) = watch.video() {
                    let watch = watch.clone();
                    spawn(async move {
                        watch.remember_playback(video).await;
                    });
                }
            }
        });
        Ok(watch)
    }

    pub fn select(&self, tab: Option<&BrowserTab>) {
        let current = self.page.borrow().tab.clone();
        if current.as_ref().zip(tab).is_some_and(|(a, b)| a.is_same(b)) {
            return;
        }
        self.page.borrow_mut().tab = tab.cloned();
        let document = tab.and_then(|tab| {
            tab.document
                .get_untracked()
                .map(|state| (tab, state.document))
        });
        self.change_document(document, tab.is_some());
        if let Some(watch) = self.page.borrow().watch.clone() {
            watch.inject("document.dispatchEvent(new Event('yt-navigate-finish'));");
        }
    }

    /// Use the persisted settings snapshot from the app root.
    pub fn configure(&self, settings: Map<String, Value>, foreground: bool) {
        self.settings.set(Some(settings));
        self.foreground.set(foreground);
        self.maybe_start();
    }

    pub fn video(&self) -> Option<WatchVideo> {
        self.video.get()
    }

    pub fn offline(&self) -> bool {
        self.video
            .with(|video| video.as_ref().is_some_and(|video| video.offline))
    }

    pub fn live(&self) -> bool {
        self.catalog.with(|catalog| {
            catalog
                .as_ref()
                .is_some_and(|(_, catalog)| catalog["live"] == true)
        })
    }

    /// Replace the mounted player with a file resolved by the download runner.
    pub fn play_offline(
        &self,
        video_id: String,
        title: String,
        author: String,
        uri: String,
    ) -> Result<(), CoreError> {
        let source = PlaybackSource::offline(uri)?;
        self.cancel_pending();
        self.session::<Value>("offline", json!({}))?;
        self.session::<Value>(
            "commit",
            json!({"playback":{"videoId":video_id,"offline":true,"qualityHeight":0,"qualityFps":0}}),
        )?;
        if let Some(watch) = &self.page.borrow().watch {
            watch.native_player(true);
        }
        self.fit.set(PlaybackFit::Contain);
        self.status.set(None);
        self.remote_status.set(None);
        self.catalog.set(None);
        self.error.set(None);
        let generation = self.player.replace(source, Some(0.0), true, 1.0);
        self.video.set(Some(WatchVideo {
            video_id,
            title,
            author,
            generation,
            offline: true,
            quality_height: 0,
            quality_fps: 0,
            qualities: Rc::from([]),
            audio_key: String::new(),
            audio_tracks: Rc::from([]),
            subtitles: Rc::from([]),
            chapters: Rc::from([]),
        }));
        Ok(())
    }

    pub(crate) fn settings(&self) -> Option<Map<String, Value>> {
        self.settings.get()
    }

    pub fn status(&self) -> Option<Rc<PlaybackStatus>> {
        self.remote_status
            .with(|remote| {
                remote
                    .as_ref()
                    .filter(|(generation, _)| {
                        self.video.with(|video| {
                            video
                                .as_ref()
                                .is_some_and(|video| video.generation == *generation)
                        })
                    })
                    .map(|(_, status)| status.clone())
            })
            .or_else(|| self.status.get())
    }

    pub fn remote(&self) -> bool {
        self.remote_status.with(|remote| {
            remote.as_ref().is_some_and(|(generation, _)| {
                self.video.with(|video| {
                    video
                        .as_ref()
                        .is_some_and(|video| video.generation == *generation)
                })
            })
        })
    }

    pub fn set_remote_status(&self, generation: u64, status: Option<PlaybackStatus>) {
        if self.video.with_untracked(|video| {
            video
                .as_ref()
                .is_some_and(|video| video.generation == generation)
        }) {
            self.remote_status
                .set(status.map(|status| (generation, Rc::new(status))));
        }
    }

    pub fn cast_catalog(&self, generation: u64) -> Option<(Rc<Value>, String)> {
        let catalog = self.catalog.with_untracked(|catalog| {
            catalog
                .as_ref()
                .filter(|(owner, _)| *owner == generation)
                .map(|(_, data)| data.clone())
        })?;
        Some((catalog, self.page.borrow().user_agent.clone()))
    }

    pub fn cast_selection(&self, generation: u64, height: u32, fps: u32, audio: String) {
        self.video.update(|video| {
            if let Some(video) = video
                .as_mut()
                .filter(|video| video.generation == generation)
            {
                video.quality_height = height;
                video.quality_fps = fps;
                video.audio_key = audio;
            }
        });
    }

    pub fn preparing(&self) -> bool {
        self.preparing.get()
    }

    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    pub fn player(&self) -> &VideoPlayer {
        &self.player
    }

    pub fn seek(&self, generation: u64, seconds: f64) {
        if seconds.is_finite()
            && seconds >= 0.0
            && self.video.with_untracked(|video| {
                video
                    .as_ref()
                    .is_some_and(|video| video.generation == generation)
            })
        {
            self.seek_intent.set(Some((generation, seconds)));
            self.player.seek(generation, seconds);
        }
    }

    pub fn seek_intent(&self) -> Option<(u64, f64)> {
        self.seek_intent.get()
    }

    pub fn fit(&self) -> PlaybackFit {
        self.fit.get()
    }

    pub fn resize(&self, fit: PlaybackFit) {
        self.fit.set(fit);
    }

    pub fn rate(&self, rate: f64) {
        if let Some(video) = self.video.get_untracked() {
            self.player.rate(video.generation, rate);
        }
    }

    pub async fn speeds(&self) -> Result<Vec<f64>, CoreError> {
        self.core.execute(json!({"op":"media_speeds"})).await
    }

    pub fn set_sponsor_segments(&self, video_id: String, segments: Value) {
        self.sponsorship.set(Some((video_id, segments)));
        self.sponsor_page();
    }

    pub fn sponsor_segments(&self, video_id: &str) -> Vec<SponsorSegment> {
        self.sponsorship.with(|value| {
            value
                .as_ref()
                .filter(|(id, _)| id == video_id)
                .and_then(|(_, segments)| Vec::deserialize(segments).ok())
                .unwrap_or_default()
        })
    }

    fn sponsor_page(&self) {
        let page = self.page.borrow();
        let sponsorship = self.sponsorship.get_untracked();
        if let Some((watch, (id, segments))) = page.watch.as_ref().zip(sponsorship.as_ref())
            && watch.video_id == *id
            && page.media_ready
        {
            let id = json!(id);
            watch.inject(&format!("window.__govueMedia?.sponsors({id},{segments});"));
        }
    }

    pub fn frame(&self, width: f32) -> EmbeddedVideoFrame {
        let frame = self
            .presentation
            .get()
            .or_else(|| self.layout.get().map(|layout| layout.frame(width)));
        let mounted = self.video.with(Option::is_some) || self.preparing.get();
        frame.map_or(
            EmbeddedVideoFrame {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height: 1.0,
                visible: false,
            },
            |frame| EmbeddedVideoFrame {
                visible: frame.visible && mounted,
                ..frame
            },
        )
    }

    pub fn present(&self, frame: Option<EmbeddedVideoFrame>) {
        self.presentation.set(frame);
    }

    /// Mount beside the retained browser, inside the same clipping container.
    pub fn view(
        &self,
        width: impl Fn() -> f32 + 'static,
        corner_radius: impl Fn() -> f32 + 'static,
        zoom: impl Fn() -> f32 + 'static,
        on_event: impl Fn(PlaybackEvent) + 'static,
    ) -> impl IntoElement {
        let watch = self.clone();
        let fit = self.fit;
        let status = self.status;
        let video = self.video;
        let recovery = self.clone();
        self.player.embedded_view(
            move || {
                let mut frame = watch.frame(width());
                if watch.remote() {
                    frame.visible = false;
                }
                frame
            },
            move || fit.get(),
            corner_radius,
            zoom,
            move |event| {
                if let PlaybackEvent::Status {
                    generation,
                    status: latest,
                } = &event
                    && video.with_untracked(|video| {
                        video
                            .as_ref()
                            .is_some_and(|video| video.generation == *generation)
                    })
                {
                    status.set(Some(Rc::new(latest.clone())));
                    if let Some(current) = video.get_untracked() {
                        recovery.finish_queue_target(&current.video_id);
                    }
                    if latest.state == "error" {
                        recovery.recover(*generation);
                    }
                }
                on_event(event);
            },
        )
    }

    async fn remember_playback(&self, video: WatchVideo) {
        let _ = self
            .core
            .execute::<Value>(json!({"op":"player_history","videoId":video.video_id,
            "item":{"videoId":video.video_id,"title":video.title,"author":video.author}}))
            .await;
        loop {
            timer(Duration::from_secs(5)).await;
            if self.video.with_untracked(|current| {
                current
                    .as_ref()
                    .is_none_or(|current| current.generation != video.generation)
            }) {
                break;
            }
            let remember = self.settings.with_untracked(|settings| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.get("rememberPosition"))
                    == Some(&Value::Bool(true))
            });
            if remember && let Some(status) = self.status.get_untracked() {
                let _ = self
                    .core
                    .execute::<Value>(json!({"op":"player_position",
                    "videoId":video.video_id,"position":status.position}))
                    .await;
            }
        }
    }

    fn recover(&self, generation: u64) {
        if !self
            .session("recover", json!({"remote":false}))
            .unwrap_or(false)
        {
            return;
        }
        let watch = self.clone();
        spawn(async move {
            timer(Duration::from_millis(750)).await;
            if watch.video.with_untracked(|video| {
                video
                    .as_ref()
                    .is_some_and(|video| video.generation == generation)
            }) && watch.status.with_untracked(|status| {
                status
                    .as_ref()
                    .is_some_and(|status| status.state == "error")
            }) {
                watch.start();
            }
        });
    }

    pub fn receive(&self, tab: &BrowserTab, event: &BrowserEvent) {
        if !self
            .page
            .borrow()
            .tab
            .as_ref()
            .is_some_and(|active| active.is_same(tab))
        {
            return;
        }
        match event {
            BrowserEvent::Navigation { document, .. } => {
                self.change_document(Some((tab, document.clone())), false);
            }
            BrowserEvent::Ready { document } => {
                self.change_document(Some((tab, document.clone())), false);
                self.page.borrow_mut().bridge_ready = true;
                self.maybe_start();
            }
            BrowserEvent::Error { .. } => self.change_document(None, false),
            BrowserEvent::PageMessage {
                document,
                origin,
                raw,
            } => {
                let Some(watch) = self.page.borrow().watch.clone() else {
                    return;
                };
                if origin != &watch.origin
                    || !watch.document.same_document(document)
                    || raw.len() > 8192
                {
                    return;
                }
                if let Ok(message) = serde_json::from_str::<Value>(raw)
                    && message["videoId"] == watch.video_id
                {
                    self.message(&watch, event, message);
                }
            }
            _ => {}
        }
    }

    /// Lets a native queue action start its target in the background.
    /// The next validated document must match the target before extraction starts.
    pub fn expect_queue_target(&self, generation: u64, address: &str) {
        if !self.settings.with_untracked(|settings| {
            settings
                .as_ref()
                .and_then(|settings| settings.get("backgroundPlayback"))
                == Some(&Value::Bool(true))
        }) || !self.video.with_untracked(|video| {
            video
                .as_ref()
                .is_some_and(|video| !video.offline && video.generation == generation)
        }) {
            return;
        }
        let identity: Value = match self
            .core
            .state(json!({"op":"browser_identity","url":address}))
        {
            Ok(identity) => identity,
            Err(_) => return,
        };
        if let Some(target) = identity["videoId"].as_str() {
            self.arm_queue_target(target.to_owned(), Duration::from_secs(20));
        }
    }

    fn arm_queue_target(&self, video: String, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        self.queue_target.set(Some((video, deadline)));
        let queued = self.queue_target;
        self.owner.run(|| {
            spawn(async move {
                timer(timeout).await;
                if queued.with_untracked(|target| {
                    target.as_ref().is_some_and(|(_, time)| *time == deadline)
                }) {
                    queued.set(None);
                }
            });
        });
    }

    fn finish_queue_target(&self, video: &str) {
        if self
            .queue_target
            .with_untracked(|target| target.as_ref().is_some_and(|(target, _)| target == video))
        {
            self.queue_target.set(None);
        }
    }

    pub fn queue_transition(&self) -> bool {
        self.queue_target.with(Option::is_some)
    }

    fn message(&self, watch: &WatchDocument, event: &BrowserEvent, message: Value) {
        match message["type"].as_str() {
            Some("page") => {
                if let Some(agent) = message["userAgent"].as_str()
                    && let Ok(agent) = self
                        .core
                        .state(json!({"op":"browser_user_agent","agent":agent}))
                {
                    self.page.borrow_mut().user_agent = agent;
                }
            }
            Some("mediaReady") => {
                let (displayed, pending_position) = {
                    let mut page = self.page.borrow_mut();
                    page.media_ready = true;
                    (
                        page.displayed || self.offline(),
                        page.position.as_ref().map(|request| request.id.clone()),
                    )
                };
                if let Some(request) = pending_position {
                    // A probe started before the web player was available.
                    watch.request_position(&request);
                }
                if displayed {
                    watch.native_player(true);
                }
                self.sponsor_page();
                self.maybe_start();
            }
            Some("playerLayout") => {
                if let Some(layout) =
                    PageVideoLayout::receive(&self.core, event, &watch.document, &watch.video_id)
                {
                    self.layout.set(Some(layout));
                }
            }
            Some("playbackPosition") => self.receive_position(watch, message),
            _ => {}
        }
    }

    fn change_document(&self, next: Option<(&BrowserTab, BrowserDocument)>, selecting_tab: bool) {
        let waiting_for_document = selecting_tab && next.is_none();
        let next = next.and_then(|(tab, document)| {
            let identity: Value = self
                .core
                .state(json!({"op":"browser_identity","url":document.url}))
                .ok()?;
            let video_id = identity["videoId"].as_str()?.to_owned();
            let origin = Url::parse(&document.url)
                .ok()?
                .origin()
                .ascii_serialization();
            Some(WatchDocument {
                tab: tab.clone(),
                document,
                video_id,
                origin,
            })
        });
        let previous = self.page.borrow().watch.clone();
        if previous
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(a, b)| a.tab.is_same(&b.tab) && a.document.same_document(&b.document))
            || (previous.is_none() && next.is_none())
        {
            return;
        }
        let offline = self
            .video
            .with_untracked(|video| video.as_ref().is_some_and(|video| video.offline));
        let retain = !offline
            && previous
                .as_ref()
                .zip(next.as_ref())
                .is_some_and(|(a, b)| a.tab.is_same(&b.tab) && a.video_id == b.video_id)
            && self.video.with_untracked(Option::is_some);
        self.cancel_pending();
        if !retain {
            if let Some(previous) = &previous {
                previous.native_player(false);
            }
            if !offline {
                self.reset_player(false);
            }
        }
        let id = next
            .as_ref()
            .map(|watch| watch.video_id.as_str())
            .unwrap_or_default();
        let _ = self.session::<Value>("select", json!({"videoId":id}));
        self.layout.set(None);
        self.error.set(None);
        let mut page = self.page.borrow_mut();
        if !waiting_for_document
            && self.queue_target.with_untracked(|target| {
                target
                    .as_ref()
                    .is_some_and(|(target, deadline)| target != id || Instant::now() >= *deadline)
            })
        {
            self.queue_target.set(None);
        }
        page.watch = next;
        page.bridge_ready = false;
        page.media_ready = false;
        page.displayed = retain;
        if offline && let Some(watch) = &page.watch {
            watch.native_player(true);
        }
        drop(page);
        if self.queue_transition() {
            self.maybe_start();
        }
    }

    fn maybe_start(&self) {
        if self
            .video
            .with_untracked(|video| video.as_ref().is_some_and(|video| video.offline))
        {
            return;
        }
        let Some(settings) = self.settings.get_untracked() else {
            return;
        };
        let (id, pending, ready, bridge_ready) = {
            let page = self.page.borrow();
            let Some(watch) = page.watch.as_ref() else {
                return;
            };
            (
                watch.video_id.clone(),
                page.pending.is_some(),
                page.media_ready,
                page.bridge_ready,
            )
        };
        let ready = ready || (bridge_ready && self.core.playback_preloaded(&id));
        let requested = self.queue_target.with_untracked(|target| {
            target
                .as_ref()
                .is_some_and(|(target, deadline)| target == &id && Instant::now() < *deadline)
        });
        let foreground = self.foreground.get_untracked()
            || (requested && settings.get("backgroundPlayback") == Some(&Value::Bool(true)));
        if self
            .session(
                "auto",
                json!({"owner":id,"ready":ready || requested,"foreground":foreground,
            "pending":pending,"settings":settings}),
            )
            .unwrap_or(false)
        {
            if requested {
                self.arm_queue_target(id, Duration::from_secs(110));
            }
            self.start();
        }
    }

    pub fn start(&self) {
        self.start_source(None, None);
    }

    pub fn select_stream(
        &self,
        selection: PlaybackSelection,
        complete: impl FnOnce(Result<(), String>) + 'static,
    ) {
        if self.video.with_untracked(Option::is_some) && !self.preparing.get_untracked() {
            self.start_source(Some(selection), Some(Box::new(complete)));
        }
    }

    fn start_source(&self, selection: Option<PlaybackSelection>, complete: Option<StreamResult>) {
        if self
            .video
            .with_untracked(|video| video.as_ref().is_some_and(|video| video.offline))
        {
            return;
        }
        let Some(settings) = self.settings.get_untracked() else {
            return;
        };
        let Some(watch) = self.page.borrow().watch.clone() else {
            return;
        };
        let attempt = self.session::<Option<Attempt>>("begin", json!({"settings":settings}));
        let Ok(Some(attempt)) = attempt else {
            return;
        };
        let token = attempt.token;
        let fresh = self.video.with_untracked(Option::is_some)
            || self.error.with_untracked(Option::is_some);
        self.preparing.set(true);
        self.error.set(None);
        let controller = self.clone();
        let task = self.owner.run(|| {
            spawn(async move {
                let result = controller
                    .prepare(&watch, attempt, settings, selection.as_ref(), fresh)
                    .await;
                if !controller
                    .session("finish", json!({"token":token}))
                    .unwrap_or(false)
                {
                    return;
                }
                if result.is_err() {
                    controller.finish_queue_target(&watch.video_id);
                }
                controller.page.borrow_mut().pending = None;
                controller.preparing.set(false);
                match result {
                    Ok(()) => {
                        if let Some(complete) = complete {
                            complete(Ok(()));
                        }
                    }
                    Err(error) => {
                        let displayed = controller.video.with_untracked(Option::is_some);
                        controller.page.borrow_mut().displayed = displayed;
                        watch.native_player(displayed);
                        let message = error.to_string();
                        if let Some(complete) = complete {
                            complete(Err(message));
                        } else {
                            controller.error.set(Some(message));
                        }
                    }
                }
            })
        });
        self.page.borrow_mut().pending = Some((token, task));
    }

    async fn prepare(
        &self,
        watch: &WatchDocument,
        attempt: Attempt,
        mut settings: Map<String, Value>,
        selection: Option<&PlaybackSelection>,
        fresh: bool,
    ) -> Result<(), CoreError> {
        let previous = self.status.get_untracked();
        let agent = self.page.borrow().user_agent.clone();
        settings.extend(attempt.settings);
        let mut quality = (attempt.height, attempt.fps);
        let current = self.video.get_untracked();
        let mut audio_key = current
            .as_ref()
            .map(|video| video.audio_key.as_str())
            .or_else(|| settings.get("audioKey").and_then(Value::as_str))
            .unwrap_or_default();
        match selection {
            Some(PlaybackSelection::Quality { height, fps }) => quality = (*height, *fps),
            Some(PlaybackSelection::Audio(key)) => audio_key = key,
            None => {}
        }
        let (position, prepared) = tokio::join!(
            async {
                let position = if let Some(previous) = &previous {
                    Some(previous.position)
                } else {
                    self.probe_position(watch, attempt.token).await
                };
                self.page.borrow_mut().displayed = true;
                watch.native_player(true);
                position
            },
            self.prepare_source(&watch.video_id, &agent, quality, audio_key, selection.is_none(), fresh)
        );
        let (catalog, source, qualities) = prepared?;
        let live = catalog["live"] == true;
        let previous = self.status.get_untracked().or(previous);
        let position = if previous.is_some() && !live {
            previous.as_ref().map(|status| status.position)
        } else {
            self.core.execute(json!({"op":"player_resume","videoId":watch.video_id,
                "position":position.or(catalog["position"].as_f64()),"duration":catalog["duration"],"live":live})).await?
        };
        if !self.session("current", json!({"token":attempt.token}))? {
            return Ok(());
        }
        let (height, fps) = source.quality();
        self.session::<Value>(
            "commit",
            json!({"playback":{"videoId":watch.video_id,"offline":false,
            "qualityHeight":height,"qualityFps":fps}}),
        )?;
        if previous.is_none() {
            self.fit.set(PlaybackFit::from_setting(
                settings
                    .get("resizeMode")
                    .and_then(Value::as_str)
                    .unwrap_or("fit"),
            ));
        }
        self.status.set(None);
        let playing = previous
            .as_ref()
            .is_none_or(|status| status.playing || status.state == "error");
        let rate = previous.as_ref().map_or_else(
            || {
                settings
                    .get("playbackSpeed")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0)
            },
            |status| status.rate,
        );
        let generation = self.player.replace(source, position, playing, rate);
        self.video.set(Some(WatchVideo {
            video_id: watch.video_id.clone(),
            title: catalog["title"].as_str().unwrap_or_default().to_owned(),
            author: catalog["author"].as_str().unwrap_or_default().to_owned(),
            generation,
            offline: false,
            quality_height: height,
            quality_fps: fps,
            qualities: qualities.into(),
            audio_key: audio_key.to_owned(),
            audio_tracks: Vec::<AudioTrack>::deserialize(&catalog["audioTracks"])
                .unwrap_or_default()
                .into(),
            subtitles: Vec::<SubtitleTrack>::deserialize(&catalog["subtitles"])
                .unwrap_or_default()
                .into(),
            chapters: Vec::<Chapter>::deserialize(&catalog["chapters"])
                .unwrap_or_default()
                .into(),
        }));
        self.catalog.set(Some((generation, Rc::new(catalog))));
        Ok(())
    }

    async fn prepare_source(
        &self,
        video_id: &str,
        agent: &str,
        quality: (u32, u32),
        audio_key: &str,
        allow_fallback: bool,
        fresh: bool,
    ) -> Result<(Value, PlaybackSource, Vec<VideoQuality>), CoreError> {
        let catalog = tokio::select! {
            catalog = self.core.resolve_media(video_id.to_owned(), agent.to_owned(), fresh) => catalog?,
            () = timer(Duration::from_secs(95)) => {
                return Err(CoreError::Request("YouTube extraction timed out. Try again.".into()));
            }
        };
        if catalog["videoId"] != video_id {
            return Err(CoreError::Request(
                "The video changed during extraction.".into(),
            ));
        }
        let request = PlaybackRequest {
            catalog: &catalog,
            height: quality.0,
            fps: quality.1,
            audio_key,
            user_agent: agent,
            allow_fallback,
        };
        let source = if cfg!(target_os = "ios") {
            PlaybackSource::prepare_ios(&self.core, request).await?
        } else {
            PlaybackSource::prepare_android(&self.core, request).await?
        };
        let qualities = if catalog["live"] == true {
            Vec::new()
        } else {
            self.core
                .execute::<Vec<VideoQuality>>(
                    json!({"op":"media_playback_choices","catalog":catalog}),
                )
                .await
                .unwrap_or_default()
        };
        Ok((catalog, source, qualities))
    }

    async fn probe_position(&self, watch: &WatchDocument, token: u64) -> Option<f64> {
        let id = format!("native:{}:{token}", self.session_id);
        let (send, receive) = oneshot::channel();
        self.page.borrow_mut().position = Some(PositionRequest {
            id: id.clone(),
            send,
        });
        watch.request_position(&id);
        let position = tokio::select! {
            response = receive => response.ok().flatten(),
            () = timer(Duration::from_millis(500)) => None,
        };
        self.page.borrow_mut().position = None;
        let escaped_id = json!(id);
        watch.inject(&format!("window.__govue?.cancelPosition({escaped_id});"));
        position
    }

    fn receive_position(&self, watch: &WatchDocument, message: Value) {
        #[derive(Deserialize)]
        struct Position {
            position: Option<f64>,
        }

        let mut page = self.page.borrow_mut();
        let Some(request) = page.position.as_ref() else {
            return;
        };
        if let Ok(Some(position)) = self.core.state::<Option<Position>>(json!({"op":"browser_position",
            "origin":watch.origin,"videoId":watch.video_id,"requestId":request.id,"message":message,"playback":true}))
            && let Some(request) = page.position.take()
        {
            let _ = request.send.send(position.position);
        }
    }

    pub fn close(&self) {
        self.queue_target.set(None);
        self.cancel_pending();
        let offline = self
            .video
            .with_untracked(|video| video.as_ref().is_some_and(|video| video.offline));
        let position = self
            .status
            .get_untracked()
            .map_or(0.0, |status| status.position);
        self.reset_player(true);
        let mut page = self.page.borrow_mut();
        page.displayed = false;
        if let Some(watch) = &page.watch {
            watch.native_player(false);
            if !offline {
                let video = json!(watch.video_id);
                let position = json!(position);
                watch.inject(&format!("window.__govue?.seek({video},{position});"));
            }
        }
    }

    fn reset_player(&self, manual: bool) {
        let _ = self.session::<Value>("close", json!({"manual":manual}));
        if self.video.get_untracked().is_some() {
            self.player.clear();
        }
        self.video.set(None);
        self.status.set(None);
        self.remote_status.set(None);
        self.catalog.set(None);
        self.error.set(None);
    }

    fn cancel_pending(&self) {
        let (pending, position, watch) = {
            let mut page = self.page.borrow_mut();
            (
                page.pending.take(),
                page.position.take(),
                page.watch.clone(),
            )
        };
        if let Some((token, task)) = pending {
            task.cancel();
            let _ = self.session::<bool>("finish", json!({"token":token}));
            self.preparing.set(false);
        }
        if let Some((position, watch)) = position.zip(watch) {
            let request = json!(position.id);
            watch.inject(&format!("window.__govue?.cancelPosition({request});"));
        }
    }

    fn session<T: DeserializeOwned>(
        &self,
        action: &str,
        mut request: Value,
    ) -> Result<T, CoreError> {
        request["op"] = json!(format!("player_session_{action}"));
        request["sessionId"] = json!(self.session_id);
        self.core.state(request)
    }
}
