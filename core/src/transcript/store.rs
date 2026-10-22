//! Bounded normalized documents in memory and in the native sandbox.
use super::{Transcript, normalize};
use crate::{
    Error, MAX_BYTES, Result, caption_tracks,
    identity::{TrackId, VideoId},
    json, trace,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const CACHE_BYTES: u64 = 24_000_000;
// At most three documents remain in memory. Disk storage is bounded separately.
static DOCUMENTS: OnceLock<Mutex<VecDeque<(PathBuf, Transcript)>>> = OnceLock::new();

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
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        Path::new(root).join(format!("{hash}.json"))
    }
}

pub(crate) fn remember(root: &str, data: &Transcript) -> Result<()> {
    let path = TranscriptKey::parse(&data.video_id, &data.track_id)?.path(root);
    let mut docs = DOCUMENTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    docs.retain(|(p, _)| p != &path);
    docs.push_back((path, data.clone()));
    while docs.len() > 3 {
        docs.pop_front();
    }
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
    remember(root, data)?;
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

pub(crate) fn load(root: &str, id: &str, track: &str) -> Result<Transcript> {
    let path = TranscriptKey::parse(id, track)?.path(root);
    {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        if let Some((_, data)) = docs.iter().find(|(p, _)| p == &path) {
            return Ok(data.clone());
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
    normalize(&mut data)?;
    remember(root, &data)?;
    Ok(data)
}

/// Caption sampling borrows the bounded cache without cloning every cue.
pub(crate) fn with_transcript<T>(
    root: &str,
    id: &str,
    track: &str,
    read: impl FnOnce(&Transcript) -> T,
) -> Result<T> {
    let path = TranscriptKey::parse(id, track)?.path(root);
    {
        let docs = DOCUMENTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        if let Some((_, data)) = docs.iter().find(|(p, _)| p == &path) {
            return Ok(read(data));
        }
    }
    Ok(read(&load(root, id, track)?))
}

pub(super) fn restore(root: &str, id: &str, audio_key: &str) -> Result<Value> {
    let _span = trace::Section::begin(c"GoVue transcript: rust.cache_restore");
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(Value::Null);
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
    for (path, _) in files.into_iter().take(20) {
        let Some(mut data) = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Transcript>(&bytes).ok())
        else {
            continue;
        };
        if data.video_id == id
            && normalize(&mut data).is_ok()
            && caption_tracks::matches_audio(&data, audio_key)
        {
            remember(root, &data)?;
            return Ok(json!(data));
        }
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::load;
    use std::{error::Error as _, io};

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
