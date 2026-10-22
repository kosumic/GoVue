//! Caption audio extraction/retry policy and shared in-flight preparation.
use crate::{Result, string};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

struct Preparation {
    subscribers: Mutex<HashSet<String>>,
    result: Mutex<Option<Result<Value>>>,
    wake: Condvar,
}
#[derive(Default)]
struct Registry {
    jobs: HashMap<(PathBuf, String), Arc<Preparation>>,
    cancelled: VecDeque<(PathBuf, String)>,
}
static REQUESTS: OnceLock<Mutex<Registry>> = OnceLock::new();
fn check(job: &Preparation) -> Result<()> {
    if job
        .subscribers
        .lock()
        .map_err(|_| "Audio preparation stopped.")?
        .is_empty()
    {
        Err("Audio preparation cancelled.".into())
    } else {
        Ok(())
    }
}
fn resolve(command: &Value, root: &Path, job: &Preparation) -> Result<Value> {
    check(job)?;
    let source = super::dispatch(command.clone(), root.into())?;
    if source["needsMedia"] != true {
        return Ok(source);
    }
    check(job)?;
    let video = string(&source, "videoId")?;
    let store = root.to_str().ok_or("Invalid audio storage.")?;
    let mut command = command.clone();
    for fresh in [false, true] {
        let catalog =
            crate::extraction::resolve(store, "media", video, "default", "", fresh, &|| {
                check(job)
            })?;
        check(job)?;
        command["catalog"] = catalog;
        let result = super::dispatch(command.clone(), root.into());
        match result {
            Ok(value) => return Ok(value),
            Err(error) if fresh => return Err(error),
            Err(_) => check(job)?,
        }
    }
    Err("Caption audio could not be prepared.".into())
}
pub(super) fn dispatch(c: &Value, root: &Path) -> Result<Value> {
    let id = string(c, "requestId")?;
    if id.is_empty()
        || id.len() > 150
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err("Invalid audio request.".into());
    }
    let mut registry = REQUESTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Audio preparation is busy.")?;
    if c["op"] == "ai_caption_audio_cancel" {
        for ((store, _), job) in &registry.jobs {
            if store == root {
                job.subscribers
                    .lock()
                    .map_err(|_| "Audio preparation is busy.")?
                    .remove(id);
            }
        }
        if registry.cancelled.len() >= 128 {
            registry.cancelled.pop_front();
        }
        registry.cancelled.push_back((root.into(), id.into()));
        return Ok(Value::Null);
    }
    if let Some(index) = registry
        .cancelled
        .iter()
        .position(|(store, request)| store == root && request == id)
    {
        registry.cancelled.remove(index);
        return Err("Audio preparation cancelled.".into());
    }
    let command = &c["command"];
    if !matches!(
        command["op"].as_str(),
        Some("ai_word_audio" | "ai_practice_audio" | "ai_game_audio" | "ai_chat_audio")
    ) || command.get("catalog").is_some()
    {
        return Err("Invalid caption audio command.".into());
    }
    let identity = if command["prefetch"] == true {
        command.to_string()
    } else {
        format!("play:{id}")
    };
    let key = (root.to_path_buf(), identity);
    let owner = !registry.jobs.contains_key(&key);
    if owner && registry.jobs.len() >= 16 {
        return Err("Too many audio preparations.".into());
    }
    let job = registry
        .jobs
        .entry(key.clone())
        .or_insert_with(|| {
            Arc::new(Preparation {
                subscribers: Mutex::new(HashSet::new()),
                result: Mutex::new(None),
                wake: Condvar::new(),
            })
        })
        .clone();
    job.subscribers
        .lock()
        .map_err(|_| "Audio preparation is busy.")?
        .insert(id.into());
    drop(registry);
    if owner {
        let result = resolve(command, root, &job);
        *job.result
            .lock()
            .map_err(|_| "Audio preparation stopped.")? = Some(result);
        job.wake.notify_all();
    }
    let mut result_guard = job
        .result
        .lock()
        .map_err(|_| "Audio preparation stopped.")?;
    while result_guard.is_none() {
        result_guard = job
            .wake
            .wait(result_guard)
            .map_err(|_| "Audio preparation stopped.")?;
    }
    let result = result_guard
        .as_ref()
        .ok_or("Audio preparation stopped.")?
        .clone();
    drop(result_guard);
    let mut registry = REQUESTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Audio preparation is busy.")?;
    let mut subscribers = job
        .subscribers
        .lock()
        .map_err(|_| "Audio preparation stopped.")?;
    let cancelled = !subscribers.remove(id);
    if subscribers.is_empty() {
        registry.jobs.remove(&key);
    }
    drop(subscribers);
    drop(registry);
    if cancelled {
        if let Ok(value) = result
            && let Some(token) = value["token"].as_str()
        {
            let _ = super::dispatch(
                json!({"op":"ai_game_audio_release","token":token}),
                root.into(),
            );
        }
        Err("Audio preparation cancelled.".into())
    } else {
        result
    }
}
