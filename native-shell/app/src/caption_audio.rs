use std::time::Duration;

use govue_shell_runtime::{
    Core, CoreError, EmbeddedVideoFrame, PlaybackEvent, PlaybackFit, PlaybackSource, VideoPlayer,
    WatchPlayback,
};
use nsh_core::{PointerEvents, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Loading,
    Playing,
}

#[derive(Deserialize)]
struct Clip {
    uri: String,
    token: String,
    start: f64,
    end: f64,
}

#[derive(Clone)]
pub struct CaptionAudio {
    core: Core,
    watch: WatchPlayback,
    player: VideoPlayer,
    request: Signal<Option<String>>,
    playing: Signal<Option<(u64, f64)>>,
    pub phase: Signal<Phase>,
    pub error: Signal<Option<String>>,
}

impl CaptionAudio {
    pub fn new(core: Core, watch: WatchPlayback) -> Self {
        Self {
            core,
            watch,
            player: VideoPlayer::new(),
            request: signal(None),
            playing: signal(None),
            phase: signal(Phase::Idle),
            error: signal(None),
        }
    }

    pub fn view(&self) -> View {
        let audio = self.clone();
        view()
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .child(self.player.embedded_view(
                || EmbeddedVideoFrame {
                    left: 0.0,
                    top: 0.0,
                    width: 1.0,
                    height: 1.0,
                    visible: true,
                },
                || PlaybackFit::Contain,
                || 0.0,
                || 1.0,
                move |event| audio.receive(event),
            ))
    }

    fn receive(&self, event: PlaybackEvent) {
        if let Some((owner, end)) = self.playing.get_untracked()
            && let PlaybackEvent::Status { generation, status } = event
            && owner == generation
        {
            if status.state == "error" {
                self.stop();
                self.error
                    .set(Some("Caption audio could not be played.".into()));
            } else if status.state == "ended" || status.position >= end {
                self.stop();
            } else if status.playing {
                self.phase.set(Phase::Playing);
            }
        }
    }

    pub fn stop(&self) {
        if let Some(request) = self.request.get_untracked() {
            self.request.set(None);
            let core = self.core.clone();
            spawn(async move {
                let _: Result<Value, _> = core
                    .ai(json!({"op":"ai_caption_audio_cancel","requestId":request}))
                    .await;
            });
        }
        if self.playing.with_untracked(Option::is_some) {
            self.playing.set(None);
            self.player.clear();
        }
        self.phase.set(Phase::Idle);
        self.error.set(None);
    }

    pub async fn play(&self, command: Value, before_play: impl FnOnce()) -> Result<(), CoreError> {
        self.stop();
        let request: String = self.core.state(json!({"op":"ai_game_session"}))?;
        self.request.set(Some(request.clone()));
        self.phase.set(Phase::Loading);
        let result = self
            .core
            .ai::<Clip>(json!({"op":"ai_caption_audio","requestId":request,"command":command}))
            .await;
        if self
            .request
            .with_untracked(|current| current.as_deref() != Some(&request))
        {
            if let Ok(clip) = result {
                govue_core::release_caption_audio(&clip.token);
            }
            return Ok(());
        }
        self.request.set(None);
        let source = result.and_then(|clip| {
            if !clip.start.is_finite()
                || !clip.end.is_finite()
                || clip.start < 0.0
                || clip.end <= clip.start
            {
                govue_core::release_caption_audio(&clip.token);
                return Err(CoreError::Request("Invalid caption audio range.".into()));
            }
            let source = PlaybackSource::caption_audio(clip.uri, clip.token)?;
            Ok((source, clip.start, clip.end))
        });
        let (source, start, end) = match source {
            Ok(source) => source,
            Err(error) => {
                self.phase.set(Phase::Idle);
                return Err(error);
            }
        };
        before_play();
        if let Some(video) = self.watch.video() {
            self.watch.player().pause(video.generation);
        }
        let generation = self.player.replace(source, Some(start), true, 1.0);
        self.playing.set(Some((generation, end)));
        let audio = self.clone();
        spawn(async move {
            timer(Duration::from_secs(60)).await;
            if audio
                .playing
                .with_untracked(|playing| playing.is_some_and(|(owner, _)| owner == generation))
            {
                audio.stop();
                audio.error.set(Some(
                    "Audio playback timed out. Try replaying the clip.".into(),
                ));
            }
        });
        Ok(())
    }
}
