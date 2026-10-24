//! Reader document loading, cache policy, and cancellation shared by all platforms.
use crate::identity::SessionId;
use crate::{Result, json};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

struct Reader {
    version: u64,
    cancelled: AtomicBool,
    state: Mutex<Value>,
}
#[derive(Clone, Eq, Hash, PartialEq)]
struct ReaderKey {
    root: String,
    session: SessionId,
}
static READERS: OnceLock<Mutex<HashMap<ReaderKey, Arc<Reader>>>> = OnceLock::new();
static CLOSED: OnceLock<Mutex<HashMap<ReaderKey, u64>>> = OnceLock::new();
fn channel(id: &str) -> String {
    format!("transcript:{id}")
}
fn check(reader: &Reader) -> Result<()> {
    if reader.cancelled.load(Ordering::Acquire) {
        Err("Transcript loading cancelled.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn snapshot(command: &Value, root: &str) -> Result<Value> {
    let session = SessionId::parse(crate::string(command, "sessionId")?, 100)
        .map_err(|_| "Invalid reader session.")?;
    let key = ReaderKey {
        root: root.into(),
        session,
    };
    let reader = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Transcript reader is busy.")?
        .get(&key)
        .cloned()
        .ok_or("This reader has closed.")?;
    let state = reader
        .state
        .lock()
        .map_err(|_| "Transcript reader is busy.")?
        .clone();
    Ok(state)
}

fn document(
    reader: &Reader,
    root: &str,
    video: &str,
    audio: &str,
    track: &str,
    fresh: bool,
    documents: &[Value],
) -> Result<Value> {
    check(reader)?;
    if let Some(retained) = documents
        .iter()
        .find(|d| track == "default" || d["data"]["trackId"] == track)
    {
        return Ok(retained.clone());
    }
    let saved = if fresh {
        Value::Null
    } else {
        crate::request(
            &json!({"op":"restore","videoId":video,"audioKey":audio,
                "trackId":if track=="default" {None}else{Some(track)}}),
            root,
        )
        .unwrap_or(Value::Null)
    };
    check(reader)?;
    if !saved.is_null() {
        return Ok(json!({"data":saved,"cached":true}));
    }
    Ok(
        json!({"data":crate::extraction::resolve(root,"transcript",video,track,audio,fresh,&||check(reader))?,"cached":false}),
    )
}

pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let id = SessionId::parse(crate::string(command, "sessionId")?, 100)
        .map_err(|_| "Invalid reader session.")?;
    let key = ReaderKey {
        root: root.into(),
        session: id.clone(),
    };
    let mut readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Transcript reader is busy.")?;
    let op = crate::string(command, "op")?;
    let version = command["version"]
        .as_u64()
        .ok_or("Missing reader revision.")?;
    let mut closed = CLOSED
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Transcript reader is busy.")?;
    if closed.get(&key).is_some_and(|last| {
        version <= *last && !(op == "transcript_session_dispose" && version == *last)
    }) {
        return Ok(Value::Null);
    }
    if readers.get(&key).is_some_and(|r| r.version > version) {
        return Ok(Value::Null);
    }
    if op == "transcript_session_release" {
        if let Some(reader) = readers.get(&key) {
            reader.cancelled.store(true, Ordering::Release);
        }
        // Native async calls may reach Rust out of order. Remember a release
        // even if its load has not arrived, while retaining documents for reuse.
        if closed.len() >= 128
            && let Some(first) = closed.keys().next().cloned()
        {
            closed.remove(&first);
        }
        closed.insert(key, version);
        return Ok(Value::Null);
    }
    if op == "transcript_session_dispose" {
        if let Some(reader) = readers.remove(&key) {
            reader.cancelled.store(true, Ordering::Release);
        }
        if closed.len() >= 128
            && let Some(first) = closed.keys().next().cloned()
        {
            closed.remove(&first);
        }
        closed.insert(key, version);
        return Ok(Value::Null);
    }
    closed.remove(&key);
    drop(closed);
    if op != "transcript_session_load" {
        return Err("Unknown reader action.".into());
    }
    let video = crate::string(command, "videoId")?;
    let audio = command["audioKey"].as_str().unwrap_or("");
    let tracks = command["trackIds"]
        .as_array()
        .filter(|t| !t.is_empty() && t.len() <= 2)
        .ok_or("Select one or two transcript languages.")?;
    let restore_preferences = tracks.len() == 1 && tracks[0] == "default";
    if !crate::video_id(video)
        || audio.len() > 512
        || tracks
            .iter()
            .any(|t| !t.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 512))
    {
        return Err("Invalid transcript selection.".into());
    }
    if !readers.contains_key(&key) && readers.len() >= 16 {
        return Err("Too many transcript readers.".into());
    }
    let fresh = command["fresh"] == true;
    let mut documents = Vec::new();
    if let Some(previous) = readers.get(&key) {
        previous.cancelled.store(true, Ordering::Release);
        let state = previous
            .state
            .lock()
            .map_err(|_| "Transcript reader is busy.")?;
        if !fresh && state["videoId"] == video && state["audioKey"] == audio {
            documents = state["documents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| restore_preferences || tracks.contains(&d["data"]["trackId"]))
                .cloned()
                .collect();
        }
    }
    if !fresh
        && let Ok(mut seed) =
            serde_json::from_value::<crate::Transcript>(command["initial"].clone())
        && seed.video_id == video
        && (restore_preferences || tracks.contains(&json!(seed.track_id)))
        && !documents
            .iter()
            .any(|d| d["data"]["trackId"] == seed.track_id)
        && crate::transcript::normalize(&mut seed).is_ok()
    {
        crate::transcript::remember(root, &seed)?;
        documents.push(json!({"data":seed,"cached":true}));
    }
    let reader = Arc::new(Reader {
        version,
        cancelled: AtomicBool::new(false),
        state: Mutex::new(json!({
            "version":version,"videoId":video,"audioKey":audio,"trackIds":tracks,
            "documents":if restore_preferences {Vec::new()} else {documents.clone()},"loading":true,"error":null
        })),
    });
    readers.insert(key, reader.clone());
    let initial = reader
        .state
        .lock()
        .map_err(|_| "Transcript reader is busy.")?
        .clone();
    drop(readers);
    let (root, id, video, audio, tracks) = (
        root.to_owned(),
        id,
        video.to_owned(),
        audio.to_owned(),
        tracks.clone(),
    );
    thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut tracks = tracks;
            if restore_preferences {
                // Resolve portable language preferences from this video's track
                // metadata, including when its cached document is a translation.
                let base = document(&reader, &root, &video, &audio, "default", fresh, &documents)?;
                let data = serde_json::from_value::<crate::Transcript>(base["data"].clone())
                    .map_err(|_| "Invalid transcript metadata.")?;
                tracks = crate::transcript::preferences::resolve(&root, &data)
                    .into_iter()
                    .map(Value::String)
                    .collect();
                documents.push(base);
                check(&reader)?;
                reader
                    .state
                    .lock()
                    .map_err(|_| "Transcript reader is busy.")?["trackIds"] = json!(tracks);
            }
            let mut loaded = Vec::new();
            for track in &tracks {
                let document = document(
                    &reader,
                    &root,
                    &video,
                    &audio,
                    track.as_str().ok_or("Invalid transcript track.")?,
                    fresh,
                    &documents,
                )?;
                check(&reader)?;
                loaded.push(document);
                let mut state = reader
                    .state
                    .lock()
                    .map_err(|_| "Transcript reader is busy.")?;
                state["documents"] = json!(loaded);
                state["loading"] = json!(loaded.len() < tracks.len());
                drop(state);
                crate::changed(&channel(id.as_str()));
            }
            Ok(())
        })();
        if let Err(error) = result
            && check(&reader).is_ok()
        {
            if let Ok(mut state) = reader.state.lock() {
                state["loading"] = json!(false);
                state["error"] = json!(error);
            }
            crate::changed(&channel(id.as_str()));
        }
    });
    Ok(initial)
}
