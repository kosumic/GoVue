use std::rc::Rc;
use std::time::{Duration, Instant};

use govue_shell_runtime::{Core, TranscriptReader, WatchPlayback};
use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct ReaderClock {
    watch: WatchPlayback,
    enabled: Memo<bool>,
    change: Rc<dyn Fn(bool)>,
    reset: Signal<u64>,
}

#[derive(Deserialize)]
struct Step {
    pause: bool,
    cursor: Value,
}

impl ReaderClock {
    pub fn new(
        core: Core,
        reader: TranscriptReader,
        watch: WatchPlayback,
        enabled: impl Fn() -> bool + 'static,
        change: impl Fn(bool) + 'static,
    ) -> Self {
        let clock = Self {
            watch,
            enabled: computed(enabled),
            change: Rc::new(change),
            reset: signal(0),
        };
        let target = computed({
            let clock = clock.clone();
            move || {
                if !clock.enabled.get() || !reader.visible() {
                    return None;
                }
                let snapshot = reader.snapshot()?;
                let document = snapshot.document(0)?;
                Some((snapshot.video_id.clone(), document.data.track_id.clone()))
            }
        });
        effect({
            let clock = clock.clone();
            move || {
                let _ = clock.reset.get();
                if let Some((video, track)) = target.get() {
                    let clock = clock.clone();
                    let core = core.clone();
                    spawn(async move {
                        clock.pause_at_line_end(core, &video, &track).await;
                    });
                }
            }
        });
        clock
    }

    pub fn position(&self, video: &str) -> Option<f64> {
        self.watch
            .video()
            .filter(|current| current.video_id == video)?;
        self.watch.status().map(|status| status.position)
    }

    pub fn paused_after_line(&self) -> bool {
        self.enabled.get()
    }

    pub fn toggle(&self) {
        (self.change)(!self.enabled.get_untracked());
    }

    pub fn seek(&self, time: f64) {
        self.reset.update(|revision| *revision += 1);
        if let Some(video) = self.watch.video() {
            self.watch.player().seek(video.generation, time);
            self.watch.player().play(video.generation);
        }
    }

    async fn pause_at_line_end(self, core: Core, video: &str, track: &str) {
        let mut cursor = Value::Null;
        let mut last = None;
        let mut generation = None;
        loop {
            let sample = self
                .watch
                .video()
                .filter(|current| current.video_id == video)
                .zip(self.watch.status());
            if let Some((current, status)) = sample {
                let now = Instant::now();
                if generation != Some(current.generation) {
                    generation = Some(current.generation);
                    cursor = Value::Null;
                    last = None;
                }
                let elapsed = last
                    .replace(now)
                    .map_or(0.0, |previous| now.duration_since(previous).as_secs_f64());
                let step = core.execute::<Step>(json!({"op":"transcript_playback","videoId":video,
                    "trackId":track,"position":status.position,"playing":status.playing,"seeking":false,
                    "rate":status.rate,"elapsed":elapsed,"cursor":cursor})).await;
                if let Ok(step) = step {
                    cursor = step.cursor;
                    if step.pause
                        && self
                            .watch
                            .video()
                            .is_some_and(|video| video.generation == current.generation)
                    {
                        self.watch.player().pause(current.generation);
                    }
                }
            }
            timer(Duration::from_millis(200)).await;
        }
    }
}
