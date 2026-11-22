use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::{Core, CoreError};

static NEXT_READER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub video_id: String,
    pub track_id: String,
    pub language: String,
    pub title: String,
    pub tracks: Vec<TranscriptTrack>,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptTrack {
    pub id: String,
    pub language: String,
    pub label: String,
    pub automatic: bool,
    pub translated_from: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct TranscriptSegment {
    pub start: f64,
    pub duration: f64,
    pub text: String,
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct ReaderDocument {
    pub data: Transcript,
    pub cached: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSnapshot {
    version: u64,
    pub video_id: String,
    pub audio_key: String,
    pub track_ids: Vec<String>,
    pub documents: Vec<ReaderDocument>,
    pub loading: bool,
    pub error: Option<String>,
    pub retry_after_seconds: Option<u64>,
}

impl ReaderSnapshot {
    pub fn document(&self, index: usize) -> Option<&ReaderDocument> {
        let track = self.track_ids.get(index)?;
        self.documents
            .iter()
            .find(|doc| doc.data.track_id == *track || track == "default")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReaderLine {
    pub track: usize,
    pub index: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReaderRow {
    pub start: f64,
    pub lines: Vec<ReaderLine>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderTracks {
    pub source_tracks: Vec<TranscriptTrack>,
    pub caption_tracks: Vec<TranscriptTrack>,
    pub translations: Vec<TranscriptTrack>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderResults {
    pub rows: Vec<ReaderRow>,
    pub total: usize,
    pub track_choices: ReaderTracks,
}

/// One retained reader for a watch or chat screen. Closing the visible panel
/// keeps its documents available, while changing videos cancels the old load.
#[derive(Clone)]
pub struct TranscriptReader {
    core: Core,
    session: Rc<str>,
    version: Rc<Cell<u64>>,
    snapshot_version: Rc<Cell<u64>>,
    selection: Rc<Cell<u64>>,
    state: Signal<Option<Rc<ReaderSnapshot>>>,
    visible: Signal<bool>,
}

impl TranscriptReader {
    pub fn new(core: Core) -> Self {
        let updates = core.subscribe();
        let reader = Self {
            core,
            session: format!(
                "native-reader:{}",
                NEXT_READER.fetch_add(1, Ordering::Relaxed)
            )
            .into(),
            version: Rc::new(Cell::new(0)),
            snapshot_version: Rc::new(Cell::new(0)),
            selection: Rc::new(Cell::new(0)),
            state: signal(None),
            visible: signal(false),
        };
        spawn(reader.clone().observe(updates));
        cleanup({
            let reader = reader.clone();
            move || {
                reader
                    .core
                    .dispose_reader(&reader.session, reader.version.get())
            }
        });
        reader
    }

    pub fn select(&self, video: Option<&str>, audio_key: &str) {
        let current = self.state.get_untracked();
        if current.as_ref().map(|s| s.video_id.as_str()) != video {
            self.close();
        }
        match video {
            Some(video)
                if !current.is_some_and(|state| {
                    state.video_id == video && state.audio_key == audio_key
                }) =>
            {
                self.load(video, audio_key, vec!["default".into()], false);
            }
            None if self.state.get_untracked().is_some() => {
                self.core.dispose_reader(&self.session, self.next_version());
                self.state.set(None);
            }
            _ => {}
        }
    }

    pub fn snapshot(&self) -> Option<Rc<ReaderSnapshot>> {
        self.state.get()
    }

    pub fn select_track(&self, video: &str, track_id: &str) {
        let current = self.state.get_untracked();
        if current.as_ref().is_some_and(|state| {
            state.video_id == video && state.track_ids.first().is_some_and(|id| id == track_id)
        }) {
            return;
        }
        self.load(video, "", vec![track_id.into()], false);
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    pub fn toggle(&self) {
        if self.visible.get_untracked() {
            self.close();
        } else if let Some(state) = self.state.get_untracked() {
            self.visible.set(true);
            if state.error.is_some() && state.documents.is_empty() {
                self.retry();
            }
        }
    }

    pub fn close(&self) {
        self.selection.set(self.selection.get() + 1);
        self.visible.set(false);
    }

    pub fn retry(&self) {
        if let Some(state) = self.state.get_untracked() {
            self.load(
                &state.video_id,
                &state.audio_key,
                state.track_ids.clone(),
                state.error.is_none(),
            );
        }
    }

    pub fn toggle_track(&self, track: String) {
        let Some(state) = self.state.get_untracked() else {
            return;
        };
        let Some(doc) = state.document(0) else {
            return;
        };
        let selected = self.selection.get() + 1;
        self.selection.set(selected);
        let request = json!({"op":"transcript_select","videoId":state.video_id,
            "trackId":doc.data.track_id,"trackIds":state.track_ids,"toggleId":track});
        let reader = self.clone();
        spawn(async move {
            let result = reader.core.execute::<Vec<String>>(request).await;
            if reader.version.get() != state.version || reader.selection.get() != selected {
                return;
            }
            match result {
                Ok(tracks) => reader.load(&state.video_id, &state.audio_key, tracks, false),
                Err(error) => reader.set_error(error),
            }
        });
    }

    pub async fn rows(
        &self,
        state: &ReaderSnapshot,
        query: &str,
    ) -> Result<ReaderResults, CoreError> {
        let request = reader_request(state, "transcript_reader")?;
        if !query.trim().is_empty() {
            timer(Duration::from_millis(100)).await;
        }
        let mut request = request;
        request["query"] = json!(query);
        self.core.execute(request).await
    }

    pub async fn export(&self, state: &ReaderSnapshot) -> Result<String, CoreError> {
        let op = if state.document(1).is_some() {
            "transcript_export"
        } else {
            "export"
        };
        self.core.execute(reader_request(state, op)?).await
    }

    fn load(&self, video: &str, audio_key: &str, tracks: Vec<String>, fresh: bool) {
        let version = self.next_version();
        let documents = self
            .state
            .get_untracked()
            .filter(|state| state.video_id == video && state.audio_key == audio_key)
            .map(|state| {
                state
                    .documents
                    .iter()
                    .filter(|doc| tracks.contains(&doc.data.track_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let request = json!({"op":"transcript_session_load","sessionId":self.session.as_ref(),
            "version":version,"videoId":video,"audioKey":audio_key,"trackIds":tracks,"fresh":fresh});
        self.state.set(Some(Rc::new(ReaderSnapshot {
            version,
            video_id: video.into(),
            audio_key: audio_key.into(),
            track_ids: tracks,
            documents,
            loading: true,
            error: None,
            retry_after_seconds: None,
        })));
        let reader = self.clone();
        spawn(async move {
            let result = reader.core.execute::<Value>(request).await;
            if reader.version.get() != version {
                return;
            }
            match result {
                Ok(_) => reader.refresh().await,
                Err(error) => reader.set_error(error),
            }
        });
    }

    fn set_error(&self, error: CoreError) {
        if let Some(current) = self.state.get_untracked() {
            let mut state = (*current).clone();
            state.error = Some(error.to_string());
            state.loading = false;
            self.state.set(Some(Rc::new(state)));
        }
    }

    fn next_version(&self) -> u64 {
        let next = self.version.get() + 1;
        self.version.set(next);
        next
    }

    async fn observe(self, mut updates: broadcast::Receiver<String>) {
        let channel = format!("transcript:{}", self.session);
        loop {
            match updates.recv().await {
                Ok(received) if received == channel => self.refresh().await,
                Err(broadcast::error::RecvError::Lagged(_)) => self.refresh().await,
                Err(broadcast::error::RecvError::Closed) => return,
                _ => {}
            }
        }
    }

    async fn refresh(&self) {
        let read = self.snapshot_version.get() + 1;
        self.snapshot_version.set(read);
        if let Ok(snapshot) = self
            .core
            .execute::<ReaderSnapshot>(json!({
                "op":"transcript_session_snapshot","sessionId":self.session.as_ref()
            }))
            .await
            && self.version.get() == snapshot.version
            && self.snapshot_version.get() == read
        {
            self.state.set(Some(Rc::new(snapshot)));
        }
    }
}

fn reader_request(state: &ReaderSnapshot, op: &str) -> Result<Value, CoreError> {
    let doc = state
        .document(0)
        .ok_or_else(|| CoreError::Request("The transcript is still loading.".into()))?;
    Ok(
        json!({"op":op,"videoId":state.video_id,"trackId":doc.data.track_id,
        "comparisonTrackId":state.document(1).map(|doc| doc.data.track_id.as_str())}),
    )
}
