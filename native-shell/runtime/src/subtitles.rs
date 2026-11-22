use std::rc::Rc;

use nsh_core::{PointerEvents, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::{
    CaptionDisplay, CaptionFrame, CaptionLayout, CaptionStyle, Core, WatchPlayback, caption_overlay,
};

#[derive(Clone, PartialEq)]
struct Selection {
    video: String,
    track: String,
}

/// Draws the selected captions at the native player position. Track selection
/// and style use the same persisted settings as the rest of the player.
#[derive(Clone)]
pub struct WatchSubtitles {
    watch: WatchPlayback,
    loaded: Signal<Option<Selection>>,
    loading: Signal<bool>,
    error: Signal<Option<String>>,
    layout: Signal<Option<Rc<CaptionLayout>>>,
    frame: Signal<Option<CaptionFrame>>,
}

impl WatchSubtitles {
    pub fn new(core: Core, watch: WatchPlayback) -> Self {
        let subtitles = Self {
            watch,
            loaded: signal(None),
            loading: signal(false),
            error: signal(None),
            layout: signal(None),
            frame: signal(None),
        };

        let request = computed({
            let subtitles = subtitles.clone();
            move || {
                let track = subtitles.key();
                let video = subtitles.watch.video()?;
                video
                    .subtitles
                    .iter()
                    .any(|option| option.id == track)
                    .then_some(Selection {
                        video: video.video_id,
                        track,
                    })
            }
        });
        effect({
            let subtitles = subtitles.clone();
            let core = core.clone();
            move || {
                let selected = request.get();
                subtitles.loading.set(selected.is_some());
                subtitles.error.set(None);
                subtitles.loaded.set(None);
                subtitles.layout.set(None);
                subtitles.frame.set(None);
                if let Some(selected) = selected {
                    let subtitles = subtitles.clone();
                    let core = core.clone();
                    spawn(async move {
                        match core
                            .resolve_transcript(selected.video.clone(), selected.track.clone())
                            .await
                        {
                            Ok(_) => subtitles.loaded.set(Some(selected)),
                            Err(error) => subtitles.error.set(Some(error.to_string())),
                        }
                        subtitles.loading.set(false);
                    });
                }
            }
        });

        let sample = Rc::new(Notify::new());
        effect({
            let subtitles = subtitles.clone();
            let sample = sample.clone();
            move || {
                if subtitles.loaded.get().is_some() {
                    let _ = subtitles.watch.status();
                    let _ = subtitles.layout.get();
                    sample.notify_one();
                }
            }
        });
        spawn(subtitles.clone().sample(core, sample));
        subtitles
    }

    pub fn key(&self) -> String {
        self.watch
            .settings()
            .and_then(|settings| {
                settings
                    .get("subtitleKey")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    }

    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    pub fn selected(&self) -> bool {
        self.loaded.get().is_some()
    }

    pub fn display(&self, controls: bool) -> Option<CaptionDisplay> {
        let frame = self.frame.get()?;
        if frame.text.is_empty() {
            return None;
        }
        let settings = self.watch.settings()?;
        let style = CaptionStyle::deserialize(settings.get("subtitleStyle")?).ok()?;
        Some(CaptionDisplay {
            frame,
            style,
            controls,
        })
    }

    /// Mount in the same workspace as the watch player, above its texture.
    pub fn view(
        &self,
        width: impl Fn() -> f32 + 'static,
        controls: impl Fn() -> bool + 'static,
    ) -> View {
        let bounds = computed({
            let watch = self.watch.clone();
            move || watch.frame(width())
        });
        let display = self.clone();
        let received = self.clone();
        view()
            .absolute()
            .left(move || px(bounds.get().left))
            .top(move || px(bounds.get().top))
            .w(move || px(bounds.get().width))
            .h(move || px(bounds.get().height))
            .pointer_events(PointerEvents::None)
            .child(caption_overlay(
                move || {
                    bounds
                        .get()
                        .visible
                        .then(|| display.display(controls()))
                        .flatten()
                },
                move |layout| {
                    if received.frame.with_untracked(|frame| {
                        frame.as_ref().is_some_and(|frame| {
                            frame.source_text == layout.source_text
                                && frame.segment_index == layout.segment_index
                        })
                    }) {
                        received.layout.set(Some(Rc::new(layout)));
                    }
                },
            ))
    }

    async fn sample(self, core: Core, wake: Rc<Notify>) {
        loop {
            wake.notified().await;
            let Some(selection) = self.loaded.get_untracked() else {
                continue;
            };
            let Some(status) = self.watch.status() else {
                continue;
            };
            let layout = self.layout.get_untracked();
            let result = core
                .execute::<CaptionFrame>(json!({"op":"player_caption_frame",
                "videoId":selection.video,"trackId":selection.track,
                "position":status.position,"layout":layout.as_deref()}))
                .await;
            if let Ok(frame) = result
                && self
                    .loaded
                    .with_untracked(|current| current.as_ref() == Some(&selection))
            {
                self.frame.set(Some(frame));
            }
        }
    }
}
