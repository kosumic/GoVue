use super::{BrowserDocument, BrowserTab, PlaybackSource, Spawned, VideoQuality, WatchPlayback};
use nsh_core::prelude::*;
use serde_json::{Value, json};
use std::time::Duration;

type Media = (Value, PlaybackSource, Vec<VideoQuality>);

pub(super) struct Prediction {
    document: BrowserDocument,
    video_id: String,
    task: Spawned<()>,
    prepared: Option<Prepared>,
}

struct Prepared {
    media: Media,
    quality: (u32, u32),
    audio_key: String,
}

impl WatchPlayback {
    pub fn preload_watch(&self, tab: &BrowserTab, generation: u64, video_id: String) {
        let Some(document) = tab
            .document
            .get_untracked()
            .map(|state| state.document)
            .filter(|document| document.generation == generation)
        else {
            return;
        };
        if self.video.with_untracked(Option::is_some)
            || !self.foreground.get_untracked()
            || !self
                .page
                .borrow()
                .tab
                .as_ref()
                .is_some_and(|active| active.is_same(tab))
            || self.preload.borrow().as_ref().is_some_and(|prediction| {
                prediction.video_id == video_id && prediction.document.same_document(&document)
            })
        {
            return;
        }
        self.cancel_preload();
        let watch = self.clone();
        let candidate = video_id.clone();
        let mut updates = self.core.subscribe();
        let task = self.owner.run(|| {
            spawn(async move {
                let wait = timer(Duration::from_secs(6));
                let mut wait = std::pin::pin!(wait);
                let catalog = loop {
                    if let Some(catalog) = watch.core.prepared_media(&candidate) {
                        break catalog;
                    }
                    tokio::select! {
                        () = &mut wait => {
                            watch.preload.borrow_mut().take();
                            return;
                        },
                        _ = updates.recv() => {}
                    }
                };
                watch.stage_preload(catalog).await;
                timer(Duration::from_secs(20)).await;
                watch.preload.borrow_mut().take();
                watch.player.set_preload(None);
            })
        });
        *self.preload.borrow_mut() = Some(Prediction {
            document,
            video_id,
            task,
            prepared: None,
        });
    }

    async fn stage_preload(&self, catalog: Value) {
        let Some(settings) = self.settings.get_untracked() else {
            return;
        };
        if catalog["live"] == true || !self.foreground.get_untracked() {
            return;
        }
        let height = if settings.get("rememberQuality") == Some(&Value::Bool(true)) {
            settings
                .get("qualityHeight")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0)
        } else {
            0
        };
        let fps = settings
            .get("qualityFps")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
        let quality = (height, fps);
        let audio_key = settings
            .get("audioKey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let position = self.core.execute::<Option<f64>>(json!({
            "op":"player_resume", "videoId":catalog["videoId"],
            "duration":catalog["duration"], "position":0,
        }));
        let agent = self.page.borrow().user_agent.clone();
        let (position, media) = tokio::join!(
            position,
            self.plan_catalog(catalog, &agent, quality, &audio_key, true)
        );
        if let (Ok(Some(position)), Ok(media)) = (position, media)
            && self.video.with_untracked(Option::is_none)
            && let Some(prediction) = self.preload.borrow_mut().as_mut()
            && media.0["videoId"] == prediction.video_id
        {
            self.player.set_preload(Some((media.1.clone(), position)));
            prediction.prepared = Some(Prepared {
                media,
                quality,
                audio_key,
            });
        }
    }

    pub(super) fn preloaded_media(
        &self,
        video_id: &str,
        quality: (u32, u32),
        audio_key: &str,
    ) -> Option<Media> {
        let prediction = self.preload.borrow();
        let prediction = prediction.as_ref()?;
        let prepared = prediction.prepared.as_ref()?;
        if prediction.video_id != video_id
            || prepared.quality != quality
            || prepared.audio_key != audio_key
        {
            return None;
        }
        Some(prepared.media.clone())
    }

    pub(super) fn probe_preloaded_position(&self) {
        let mut page = self.page.borrow_mut();
        let Some(watch) = page.watch.clone() else {
            return;
        };
        if page.media_ready
            || page.startup_probe.is_some()
            || self.video.with_untracked(Option::is_some)
            || !self.preload.borrow().as_ref().is_some_and(|prediction| {
                prediction.video_id == watch.video_id && prediction.prepared.is_some()
            })
        {
            return;
        }
        let request = format!(
            "native-start:{}:{}",
            self.session_id, watch.document.generation
        );
        page.startup_probe = Some(request.clone());
        drop(page);
        let request = json!(request);
        let video = json!(watch.video_id);
        watch.inject(&format!("window.__govue?.probeStart({request},{video});"));
    }

    pub(super) fn keep_preload(&self, document: &BrowserDocument) {
        let matches = self.preload.borrow().as_ref().is_none_or(|prediction| {
            prediction.document.same_document(document)
                || self
                    .core
                    .state::<Value>(json!({
                        "op":"browser_identity", "url":document.url,
                    }))
                    .is_ok_and(|identity| identity["videoId"] == prediction.video_id)
        });
        if !matches {
            self.cancel_preload();
        }
    }

    pub(super) fn cancel_preload(&self) {
        if let Some(prediction) = self.preload.borrow_mut().take() {
            prediction.task.cancel();
            self.player.set_preload(None);
        }
    }
}
