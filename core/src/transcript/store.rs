//! Bounded normalized documents in memory and in the native sandbox.
use super::Transcript;
use crate::identity::{TrackId, VideoId};
use crate::transcript;
use crate::{Error, MAX_BYTES, Result, caption_tracks};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const CACHE_BYTES: u64 = 24_000_000;
// At most three documents remain in memory. Disk storage is bounded separately.
static DOCUMENTS: OnceLock<Mutex<Documents>> = OnceLock::new();

#[derive(Default)]
struct Documents {
    entries: VecDeque<(PathBuf, Arc<Transcript>)>,
    // Identity, rather than a wrapping counter, prevents stale cache publication.
    revision: Arc<()>,
}

impl Documents {
    fn insert(&mut self, path: PathBuf, data: Arc<Transcript>) {
        self.entries.retain(|(p, _)| p != &path);
        self.entries.push_back((path, data));
        while self.entries.len() > 3 {
            self.entries.pop_front();
        }
        self.revision = Arc::new(());
    }
}

struct TranscriptKey {
    video: VideoId,
    track: TrackId,
}
impl TranscriptKey {
    fn parse(video: &str, track: &str) -> Result<Self> {
        Ok(Self {
            video: VideoId::parse(video).map_err(|_| "Invalid transcript identity.")?,
            track: TrackId::parse(track).map_err(|_| "Invalid transcript identity.")?,
        })
    }
    fn path(&self, root: &str) -> PathBuf {
        let key = format!("{}\0{}", self.video.as_str(), self.track.as_str());
        let hash = crate::hex(&Sha256::digest(key.as_bytes()));
        Path::new(root).join(format!("{hash}.json"))
    }
}

pub(crate) fn remember(root: &str, data: &Transcript) -> Result<()> {
    remember_shared(root, Arc::new(data.clone()))
}

pub(crate) fn remember_shared(root: &str, data: Arc<Transcript>) -> Result<()> {
    let path = TranscriptKey::parse(&data.video_id, &data.track_id)?.path(root);
    let mut docs = DOCUMENTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    docs.insert(path, data);
    drop(docs);
    Ok(())
}

pub(crate) fn save(root: &str, data: &Transcript) -> Result<()> {
    fs::create_dir_all(root).map_err(|cause| {
        Error::caused_by(
            "Could not create transcript cache.",
            "create transcript cache",
            cause,
        )
    })?;
    let path = TranscriptKey::parse(&data.video_id, &data.track_id)?.path(root);
    let bytes = serde_json::to_vec(data).map_err(|cause| {
        Error::caused_by("Could not encode transcript.", "encode transcript", cause)
    })?;
    if bytes.len() > MAX_BYTES {
        return Err("Transcript is too large to cache.".into());
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|cause| {
        Error::caused_by(
            "Could not save transcript cache.",
            "write transcript cache",
            cause,
        )
    })?;
    fs::rename(&temporary, &path).map_err(|cause| {
        Error::caused_by(
            "Could not save transcript cache.",
            "commit transcript cache",
            cause,
        )
    })?;
    let mut files: Vec<_> = fs::read_dir(root)
        .map_err(|cause| {
            Error::caused_by(
                "Could not read transcript cache.",
                "list transcript cache",
                cause,
            )
        })?
        .filter_map(|e| {
            let entry = e.ok()?;
            let meta = entry.metadata().ok()?;
            (entry.path().extension()?.to_str()? == "json" && meta.is_file()).then_some((
                entry.path(),
                meta.len(),
                meta.modified().ok()?,
            ))
        })
        .collect();
    files.sort_by_key(|f| f.2);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut count = files.len();
    for (file, bytes, _) in files {
        if count <= 20 && total <= CACHE_BYTES {
            break;
        }
        if file != path && fs::remove_file(file).is_ok() {
            total -= bytes;
            count -= 1;
        }
    }
    Ok(())
}

pub(crate) fn load(root: &str, id: &str, track: &str) -> Result<Arc<Transcript>> {
    let path = TranscriptKey::parse(id, track)?.path(root);
    {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        if let Some((_, data)) = docs.entries.iter().find(|(p, _)| p == &path) {
            return Ok(Arc::clone(data));
        }
    }
    if fs::metadata(&path)
        .map_err(|cause| {
            Error::caused_by(
                "Transcript is not cached yet.",
                "inspect transcript cache",
                cause,
            )
        })?
        .len()
        > MAX_BYTES as u64
    {
        return Err("Cached transcript is too large.".into());
    }
    let bytes = fs::read(&path).map_err(|cause| {
        Error::caused_by(
            "Could not read cached transcript.",
            "read transcript cache",
            cause,
        )
    })?;
    let mut data: Transcript = serde_json::from_slice(&bytes).map_err(|cause| {
        Error::caused_by(
            "Unreadable cached transcript.",
            "decode transcript cache",
            cause,
        )
    })?;
    if data.video_id != id || data.track_id != track {
        return Err("Cached transcript identity changed.".into());
    }
    transcript::normalize(&mut data)?;
    let data = Arc::new(data);
    remember_shared(root, Arc::clone(&data))?;
    Ok(data)
}

/// Read an immutable document without holding the cache mutex during the callback.
pub(crate) fn with_transcript<T>(
    root: &str,
    id: &str,
    track: &str,
    read: impl FnOnce(&Transcript) -> T,
) -> Result<T> {
    Ok(read(load(root, id, track)?.as_ref()))
}

pub(super) enum Snapshot {
    Memory(Arc<Transcript>),
    Files {
        files: Vec<fs::File>,
        revision: Arc<()>,
    },
}

/// Called under REQUEST_LOCK, which also serializes cache writes and pruning.
/// Open handles retain the selected file versions across atomic replacement or
/// eviction. Only the handles, never up to twenty full documents, are retained.
pub(super) fn snapshot(root: &str, id: &str, track: Option<&str>) -> Result<Snapshot> {
    let (memory, revision) = {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        let memory = track
            .and_then(|track| TranscriptKey::parse(id, track).ok())
            .map(|key| key.path(root))
            .and_then(|path| docs.entries.iter().find(|(p, _)| p == &path))
            .map(|(_, data)| Arc::clone(data));
        (memory, Arc::clone(&docs.revision))
    };
    if let Some(data) = memory {
        return Ok(Snapshot::Memory(data));
    }
    let files = if let Some(track) = track {
        TranscriptKey::parse(id, track)
            .ok()
            .and_then(|key| open(&key.path(root)))
            .into_iter()
            .collect()
    } else {
        candidates(root)
    };
    Ok(Snapshot::Files { files, revision })
}

fn open(path: &Path) -> Option<fs::File> {
    let file = fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    (meta.is_file() && meta.len() <= MAX_BYTES as u64).then_some(file)
}

fn candidates(root: &str) -> Vec<fs::File> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(|e| {
            let entry = e.ok()?;
            let meta = entry.metadata().ok()?;
            (entry.path().extension()?.to_str()? == "json"
                && meta.is_file()
                && meta.len() <= MAX_BYTES as u64)
                .then_some((entry.path(), meta.modified().ok()?))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    files
        .into_iter()
        .take(20)
        .filter_map(|(path, _)| open(&path))
        .collect()
}

impl Snapshot {
    /// Decoding, normalization, and response construction run without REQUEST_LOCK.
    pub(super) fn resolve(
        self,
        root: &str,
        id: &str,
        track: Option<&str>,
        audio_key: &str,
    ) -> Result<Option<Arc<Transcript>>> {
        let (files, revision) = match self {
            Self::Memory(data) => return Ok(Some(data)),
            Self::Files { files, revision } => (files, revision),
        };
        for file in files {
            let mut bytes = Vec::new();
            if file
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() > MAX_BYTES
            {
                continue;
            }
            let Ok(mut data) = serde_json::from_slice::<Transcript>(&bytes) else {
                continue;
            };
            if data.video_id != id
                || track.is_some_and(|track| data.track_id != track)
                || transcript::normalize(&mut data).is_err()
                || track.is_none() && !caption_tracks::matches_audio(&data, audio_key)
            {
                continue;
            }
            let data = Arc::new(data);
            let path = TranscriptKey::parse(&data.video_id, &data.track_id)?.path(root);
            let mut docs = DOCUMENTS
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
            // A newer accept, extraction, seed, or restore wins publication. The
            // caller may still use its consistent earlier snapshot for this call.
            if Arc::ptr_eq(&revision, &docs.revision) {
                docs.insert(path, Arc::clone(&data));
            }
            drop(docs);
            return Ok(Some(data));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{Transcript, load, remember_shared, save, snapshot};
    use std::error::Error as _;
    use std::sync::Arc;
    use std::{fs, io};

    #[test]
    fn snapshots_survive_replacement_without_republishing_over_newer_documents() {
        let root =
            std::env::temp_dir().join(format!("govue-transcript-snapshots-{}", std::process::id()));
        let root = root.to_str().unwrap();
        let document = |text: &str| -> Arc<Transcript> {
            Arc::new(serde_json::from_value(serde_json::json!({
                "videoId":"abcdefghijk","title":"Snapshot fixture","language":"English","trackId":".en",
                "tracks":[{"id":".en","label":"English","language":"en","automatic":false}],
                "segments":[{"start":0,"duration":1,"text":text}]
            })).unwrap())
        };
        let (warm, disk) = {
            let _guard = crate::REQUEST_LOCK
                .lock(c"GoVue core request: snapshot.fixture")
                .unwrap();
            let original = document("Original captions");
            remember_shared(root, Arc::clone(&original)).unwrap();
            save(root, &original).unwrap();
            (
                snapshot(root, "abcdefghijk", Some(".en")).unwrap(),
                snapshot(root, "abcdefghijk", None).unwrap(),
            )
        };
        {
            let _guard = crate::REQUEST_LOCK
                .lock(c"GoVue core request: snapshot.fixture")
                .unwrap();
            let replacement = document("Replacement captions");
            remember_shared(root, Arc::clone(&replacement)).unwrap();
            save(root, &replacement).unwrap();
        }
        // Atomic replacement unlinks the old file. Its open snapshot must still
        // decode the original version, and must not put it over the replacement.
        for (saved, track) in [(warm, Some(".en")), (disk, None)] {
            let restored = saved
                .resolve(root, "abcdefghijk", track, "")
                .unwrap()
                .unwrap();
            assert_eq!(restored.segments[0].text, "Original captions");
            let current = {
                let _guard = crate::REQUEST_LOCK
                    .lock(c"GoVue core request: snapshot.fixture")
                    .unwrap();
                load(root, "abcdefghijk", ".en").unwrap()
            };
            assert_eq!(current.segments[0].text, "Replacement captions");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_misses_retain_the_io_cause_and_serialize_only_the_ui_message() {
        let root = std::env::temp_dir()
            .join(format!(
                "govue-transcript-diagnostics-{}",
                std::process::id()
            ))
            .join("missing-cache");
        let error = load(root.to_str().unwrap(), "abcdefghijk", ".en")
            .err()
            .unwrap();
        let retained = error.clone();
        let cause = retained
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap();
        assert_eq!(cause.kind(), io::ErrorKind::NotFound);
        assert!(format!("{retained:?}").contains("inspect transcript cache"));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            "Transcript is not cached yet."
        );
        assert_eq!(retained.to_string(), "Transcript is not cached yet.");
    }
}
