//! Reader document loading, cache policy, and cancellation shared by all platforms.
use crate::{Result, json, request, string};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Reader {
    version: u64,
    cancelled: AtomicBool,
    state: Mutex<Value>,
}
type Key = (String, String);
static READERS: OnceLock<Mutex<HashMap<Key, Arc<Reader>>>> = OnceLock::new();
static CLOSED: OnceLock<Mutex<HashMap<Key, u64>>> = OnceLock::new();
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
pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let id = string(command, "sessionId")?;
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err("Invalid reader session.".into());
    }
    let key = (root.to_owned(), id.to_owned());
    let mut readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Transcript reader is busy.")?;
    let op = string(command, "op")?;
    if op == "transcript_session_snapshot" {
        let reader = readers.get(&key).ok_or("This reader has closed.")?;
        return Ok(reader
            .state
            .lock()
            .map_err(|_| "Transcript reader is busy.")?
            .clone());
    }
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
    let video = string(command, "videoId")?;
    let audio = command["audioKey"].as_str().unwrap_or("");
    let tracks = command["trackIds"]
        .as_array()
        .filter(|t| !t.is_empty() && t.len() <= 2)
        .ok_or("Select one or two transcript languages.")?;
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
                .filter(|d| tracks.contains(&d["data"]["trackId"]))
                .cloned()
                .collect();
        }
    }
    if !fresh
        && let Ok(mut seed) =
            serde_json::from_value::<crate::Transcript>(command["initial"].clone())
        && seed.video_id == video
        && tracks.contains(&json!(seed.track_id))
        && !documents
            .iter()
            .any(|d| d["data"]["trackId"] == seed.track_id)
        && crate::normalize(&mut seed).is_ok()
    {
        crate::remember(
            crate::cache_path(root, &seed.video_id, &seed.track_id)?,
            &seed,
        );
        documents.push(json!({"data":seed,"cached":true}));
    }
    let reader = Arc::new(Reader {
        version,
        cancelled: AtomicBool::new(false),
        state: Mutex::new(json!({
            "version":version,"videoId":video,"audioKey":audio,"trackIds":tracks,"documents":documents,"loading":true,"error":null
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
        id.to_owned(),
        video.to_owned(),
        audio.to_owned(),
        tracks.clone(),
    );
    thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut loaded = Vec::new();
            for track in &tracks {
                check(&reader)?;
                let retained = documents
                    .iter()
                    .find(|d| d["data"]["trackId"] == *track)
                    .cloned();
                let saved = if fresh || retained.is_some() {
                    Value::Null
                } else {
                    request(
                        &json!({"op":"restore","videoId":video,"audioKey":audio,
                        "trackId":if track=="default" {Value::Null}else{track.clone()}}),
                        &root,
                    )
                    .unwrap_or(Value::Null)
                };
                check(&reader)?;
                let document = if let Some(retained) = retained {
                    retained
                } else if !saved.is_null() {
                    json!({"data":saved,"cached":true})
                } else {
                    json!({"data":crate::extraction::resolve(&root,"transcript",&video,track.as_str().ok_or("Invalid transcript track.")?,&audio,fresh,&||check(&reader))?,"cached":false})
                };
                check(&reader)?;
                loaded.push(document);
                let mut state = reader
                    .state
                    .lock()
                    .map_err(|_| "Transcript reader is busy.")?;
                state["documents"] = json!(loaded);
                state["loading"] = json!(loaded.len() < tracks.len());
                drop(state);
                crate::changed(&channel(&id));
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
            crate::changed(&channel(&id));
        }
    });
    Ok(initial)
}
