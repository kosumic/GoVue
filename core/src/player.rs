//! Shared player state. UI concepts follow LiTube; all persisted records omit
//! signed media/caption URLs and browser session data.
use crate::transcript::with_transcript;
use crate::{Result, clean, media, string, video_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;

static WRITES: Mutex<()> = Mutex::new(());
mod caption;
mod caption_color;
mod cast;
mod diagnostics;
mod gestures;
mod mini;
mod orientation;
mod pip;
pub(crate) mod preferences;
pub(crate) mod session;
pub(crate) mod sponsor;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Item {
    video_id: String,
    title: String,
    author: String,
    position: f64,
    playlist_id: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct PageContext {
    video_id: String,
    next: Option<Item>,
    previous: Option<Item>,
    autoplay: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Library {
    queue: Vec<Item>,
    enabled: bool,
    loop_mode: u8,
    history: Vec<Item>,
    page: PageContext,
}

fn item(value: &Value) -> Result<Item> {
    let id = string(value, "videoId")?;
    if !video_id(id) {
        return Err("Invalid video identity.".into());
    }
    Ok(Item {
        video_id: id.into(),
        title: clean(value["title"].as_str().unwrap_or("Video")),
        author: clean(value["author"].as_str().unwrap_or("")),
        position: value["position"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 604_800.0)
            .unwrap_or(0.0),
        playlist_id: value["playlistId"]
            .as_str()
            .filter(|s| {
                s.len() <= 200
                    && !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            .map(str::to_owned),
    })
}

fn load(root: &str) -> Library {
    let mut library: Library =
        serde_json::from_value(media::read(root, "player-library.json", json!({})))
            .unwrap_or_default();
    library.queue.retain(|i| video_id(&i.video_id));
    library.history.retain(|i| video_id(&i.video_id));
    library.queue.truncate(500);
    library.history.truncate(200);
    library.loop_mode %= 4;
    library
}

fn snapshot(library: &Library, current: &str, next: Option<&Item>) -> Value {
    let index = library.queue.iter().position(|i| i.video_id == current);
    let mode = ["next", "repeat", "pause", "shuffle"][library.loop_mode as usize];
    let page_current = library.page.video_id == current;
    json!({"queue":library.queue,"enabled":library.enabled,"loopMode":mode,
        "history":library.history,"hasPrevious":if library.enabled {index.is_some_and(|i| i>0)} else {(page_current&&library.page.previous.is_some())||library.history.iter().any(|i| i.video_id!=current)},
        "hasNext":if library.enabled {if library.loop_mode==3 {library.queue.iter().any(|i|i.video_id!=current)}else{index.map_or(!library.queue.is_empty(),|i|i+1<library.queue.len())}}else{page_current&&library.page.next.is_some()},"next":next})
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep each operation and its shared state validation and persistence in one dispatcher."
)]
pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    if command["op"]
        .as_str()
        .is_some_and(|op| op.starts_with("player_session_"))
    {
        return session::dispatch(command);
    }
    if command["op"] == "player_chapter" {
        let starts = command["starts"]
            .as_array()
            .filter(|v| v.len() <= 300)
            .ok_or("Invalid chapter list.")?;
        let starts = starts
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|n| n.is_finite() && (0.0..604_800.0).contains(n))
                    .ok_or("Invalid chapter position.")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if starts.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err("Chapter positions are out of order.".into());
        }
        let position = command["position"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or("Invalid playback position.")?;
        return Ok(json!(starts.iter().rposition(|start| *start <= position)));
    }
    if command["op"] == "player_pip" {
        return pip::control(command, root);
    }
    if command["op"] == "player_gesture" {
        return gestures::drag(command);
    }
    if command["op"] == "player_double_tap" {
        return gestures::double_tap(command);
    }
    if command["op"] == "player_diagnostics" {
        return diagnostics::sample(command);
    }
    if command["op"] == "player_cast_devices" {
        return cast::devices(command);
    }
    if matches!(
        command["op"].as_str(),
        Some("player_orientation" | "player_fullscreen" | "player_surface_reset")
    ) {
        return orientation::dispatch(command, root);
    }
    if command["op"] == "player_caption_color" {
        return caption_color::color(command);
    }
    if command["op"] == "player_live_seek" {
        let offset = command["offset"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=604_800.0).contains(v))
            .ok_or("The live position is not available yet.")?;
        let target = command["target"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=60.0).contains(v))
            .unwrap_or(3.0);
        return Ok(json!((offset - target).max(0.0)));
    }
    if command["op"] == "player_mini" {
        let _guard = WRITES
            .lock()
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        return mini::layout(command, root);
    }
    let _lock = WRITES
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    let op = string(command, "op")?;
    let current = command["videoId"].as_str().unwrap_or("");
    let mut library = load(root);
    let mut next: Option<Item> = None;
    match op {
        "player_resume" => {
            // The web player's live timeline can have a different origin from
            // the native HLS window. Keep the native default live position.
            if command["live"] == true {
                return Ok(Value::Null);
            }
            let settings = preferences::settings(&json!({}), root)?;
            let requested = command["position"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(0.0);
            let duration = command["duration"].as_f64().unwrap_or(0.0);
            let saved = library
                .history
                .iter()
                .find(|i| i.video_id == current)
                .map_or(0.0, |i| i.position);
            return Ok(json!(if requested <= 0.25
                && settings["rememberPosition"] == true
                && saved > 0.0
                && saved < duration - 5.0
            {
                saved
            } else {
                requested
            }));
        }
        "player_caption" | "player_caption_frame" => {
            let position = command["position"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .ok_or("Invalid caption position.")?;
            return with_transcript(root, current, string(command, "trackId")?, |data| {
                let after = data.segments.partition_point(|s| s.start <= position);
                let index = after.checked_sub(1).filter(|&i| {
                    let s = &data.segments[i];
                    let end = if s.duration > 0.0 {
                        s.start + s.duration
                    } else {
                        data.segments.get(i + 1).map_or(f64::INFINITY, |n| n.start)
                    };
                    position < end
                });
                if op == "player_caption_frame" {
                    return index.map_or_else(
                        || json!({"text":"","sourceText":"","segmentIndex":-1}),
                        |index| {
                            let segment = &data.segments[index];
                            let next = data
                                .segments
                                .get(index + 1)
                                .map_or(f64::INFINITY, |s| s.start);
                            let end = if segment.duration > 0.0 {
                                next.min(segment.start + segment.duration)
                            } else {
                                next
                            };
                            caption::frame(segment, index, position, end, &command["layout"])
                        },
                    );
                }
                json!(index.map_or("", |index| data.segments[index].text.as_str()))
            });
        }
        "player_state" => {}
        "player_context" => {
            if !video_id(current) {
                return Err("Invalid page playback identity.".into());
            }
            library.page = PageContext {
                video_id: current.into(),
                next: command
                    .get("next")
                    .filter(|v| !v.is_null())
                    .map(item)
                    .transpose()?,
                previous: command
                    .get("previous")
                    .filter(|v| !v.is_null())
                    .map(item)
                    .transpose()?,
                autoplay: command["autoplay"] == true,
            };
        }
        "player_add" | "player_add_batch" => {
            let items = if op == "player_add" {
                vec![&command["item"]]
            } else {
                command["items"]
                    .as_array()
                    .ok_or("Missing queue items.")?
                    .iter()
                    .collect()
            };
            if items.len() > 500 {
                return Err("A queue can contain at most 500 videos.".into());
            }
            let validated: Vec<_> = items.into_iter().map(item).collect::<Result<_>>()?;
            for entry in validated {
                if !library.queue.iter().any(|i| i.video_id == entry.video_id) {
                    if library.queue.len() >= 500 {
                        return Err("The queue is full.".into());
                    }
                    library.queue.push(entry);
                }
            }
        }
        "player_remove" => {
            let id = string(command, "id")?;
            library.queue.retain(|i| i.video_id != id);
        }
        "player_clear" => {
            library.queue.clear();
            library.enabled = false;
        }
        "player_queue_enabled" => {
            library.enabled = command["enabled"]
                .as_bool()
                .ok_or("Invalid queue setting.")?;
        }
        "player_move" => {
            let from = if let Some(id) = command["id"].as_str() {
                library
                    .queue
                    .iter()
                    .position(|item| item.video_id == id)
                    .ok_or("This queue item was removed.")?
            } else {
                usize::try_from(command["from"].as_u64().ok_or("Invalid queue position.")?)
                    .map_err(|_| "Invalid queue position.")?
            };
            let to = if let Some(offset) = command["offset"]
                .as_i64()
                .filter(|offset| (-500..=500).contains(offset))
            {
                from.saturating_add_signed(
                    isize::try_from(offset).map_err(|_| "Invalid queue offset.")?,
                )
                .min(library.queue.len().saturating_sub(1))
            } else {
                usize::try_from(command["to"].as_u64().ok_or("Invalid queue position.")?)
                    .map_err(|_| "Invalid queue position.")?
            };
            if from >= library.queue.len() || to >= library.queue.len() {
                return Err("Queue position changed.".into());
            }
            let entry = library.queue.remove(from);
            library.queue.insert(to, entry);
        }
        "player_loop" => {
            library.loop_mode = (library.loop_mode + 1) % 4;
        }
        "player_history" => {
            let mut entry = item(&command["item"])?;
            if command["item"].get("position").is_none()
                && let Some(previous) = library
                    .history
                    .iter()
                    .find(|i| i.video_id == entry.video_id)
            {
                entry.position = previous.position;
            }
            library.history.retain(|i| i.video_id != entry.video_id);
            library.history.insert(0, entry);
            library.history.truncate(200);
        }
        "player_next" | "player_previous" | "player_ended" => {
            if op == "player_ended" && library.loop_mode == 1 {
                next = library
                    .history
                    .iter()
                    .find(|i| i.video_id == current)
                    .cloned();
                if let Some(entry) = &mut next {
                    entry.position = 0.0;
                }
            } else if op != "player_ended" || library.loop_mode != 2 {
                if library.enabled && !library.queue.is_empty() {
                    let index = library.queue.iter().position(|i| i.video_id == current);
                    let at = if op == "player_previous" {
                        index.and_then(|i| i.checked_sub(1))
                    } else if library.loop_mode == 3 && library.queue.len() > 1 {
                        let mut random = [0; 4];
                        getrandom::getrandom(&mut random)
                            .map_err(|_| "Could not shuffle queue.")?;
                        let step =
                            1 + u32::from_le_bytes(random) as usize % (library.queue.len() - 1);
                        Some(index.map_or(
                            u32::from_le_bytes(random) as usize % library.queue.len(),
                            |i| (i + step) % library.queue.len(),
                        ))
                    } else {
                        Some(index.map_or(0, |i| i + 1))
                    };
                    next = at.and_then(|i| library.queue.get(i)).cloned();
                } else if op == "player_previous" {
                    next = if library.page.video_id == current {
                        library.page.previous.clone()
                    } else {
                        None
                    };
                    if next.is_none() {
                        next = library
                            .history
                            .iter()
                            .find(|i| i.video_id != current)
                            .cloned();
                    }
                } else if library.page.video_id == current
                    && (op != "player_ended" || library.page.autoplay)
                {
                    next.clone_from(&library.page.next);
                }
            }
        }
        "player_position" => {
            let position = command["position"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 604_800.0)
                .ok_or("Invalid playback position.")?;
            if let Some(entry) = library.history.iter_mut().find(|i| i.video_id == current) {
                entry.position = position;
            }
        }
        _ => return Err("Unknown player action.".into()),
    }
    if op != "player_state" {
        media::write(
            root,
            "player-library.json",
            &serde_json::to_value(&library).map_err(|_| "Could not save queue.")?,
        )?;
    }
    Ok(snapshot(&library, current, next.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_resume_preserves_the_native_window_default() {
        let directory =
            std::env::temp_dir().join(format!("govue-live-resume-{}", std::process::id()));
        let root = directory.to_str().unwrap();
        assert!(dispatch(&json!({"op":"player_resume","videoId":"M3HKLzjvKPc","position":50000.0,"duration":0,"live":true}),root).unwrap().is_null());
        assert_eq!(dispatch(&json!({"op":"player_resume","videoId":"jNQXAC9IVRw","position":7.5,"duration":19,"live":false}),root).unwrap(), json!(7.5));
    }

    #[test]
    fn queue_persistence_validates_batches_and_excludes_source_secrets() {
        let directory = std::env::temp_dir().join(format!(
            "govue-queue-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = directory.to_str().unwrap();
        let entry = json!({"videoId":"abcdefghijk","title":"First video","author":"Uploader","uri":"https://r1.googlevideo.com/private?sig=secret","sessionToken":"secret"});
        dispatch(&json!({"op":"player_add","item":entry}), root).unwrap();
        dispatch(&json!({"op":"player_add","item":entry}), root).unwrap();
        let state = dispatch(&json!({"op":"player_state"}), root).unwrap();
        assert_eq!(state["queue"].as_array().unwrap().len(), 1);
        assert!(dispatch(&json!({"op":"player_add_batch","items":[{"videoId":"lmnopqrstuv","title":"Second"},{"videoId":"bad"}]}),root).is_err());
        assert_eq!(
            dispatch(&json!({"op":"player_state"}), root).unwrap()["queue"],
            state["queue"]
        );
        let persisted =
            std::fs::read_to_string(directory.join("media/player-library.json")).unwrap();
        assert!(!persisted.contains("googlevideo"));
        assert!(!persisted.contains("sessionToken"));
        assert!(!persisted.contains("secret"));
        dispatch(
            &json!({"op":"player_add","item":{"videoId":"lmnopqrstuv","title":"Second"}}),
            root,
        )
        .unwrap();
        dispatch(&json!({"op":"player_queue_enabled","enabled":true}), root).unwrap();
        let next = dispatch(&json!({"op":"player_next","videoId":"abcdefghijk"}), root).unwrap();
        assert_eq!(next["next"]["videoId"], "lmnopqrstuv");
        assert!(!dispatch(&json!({"op":"player_state","videoId":"lmnopqrstuv"}),root).unwrap()["hasNext"].as_bool().unwrap());
        dispatch(&json!({"op":"player_move","from":1,"to":0}), root).unwrap();
        assert_eq!(
            dispatch(
                &json!({"op":"player_previous","videoId":"abcdefghijk"}),
                root
            )
            .unwrap()["next"]["videoId"],
            "lmnopqrstuv"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
