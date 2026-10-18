//! PiP controls share the queue and persisted player preferences. The temporary
//! audio-only mode is process memory only; platform adapters own window changes.
use crate::{Result, video_id};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
struct Mode {
    video: String,
    audio_only: bool,
}
static MODE: Mutex<Mode> = Mutex::new(Mode {
    video: String::new(),
    audio_only: false,
});

pub(super) fn control(command: &Value, root: &str) -> Result<Value> {
    let id = command["videoId"].as_str().unwrap_or("");
    let active = command["active"] == true && video_id(id);
    let settings = super::preferences::settings(&json!({}), root)?;
    let queue = super::snapshot(&super::load(root), id, None);
    let cast = crate::cast::dispatch(&json!({"op":"cast_status"}))?;
    let remote = cast["active"] == true && cast["link"] != true;
    let enabled = active && settings["autoPip"] == true && !remote;
    let playing = command["playing"] == true;
    let background = settings["backgroundPlayback"] == true;
    let mut mode = MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    id.clone_into(&mut mode.video);
    if !active {
        mode.audio_only = false;
    }
    let action = command["action"].as_str().unwrap_or("snapshot");
    let perform = match action {
        "snapshot" => "",
        "foreground" => {
            if command["handoff"] != true {
                mode.audio_only = false;
            }
            ""
        }
        "audio_failed" => {
            mode.audio_only = false;
            ""
        }
        "audio" if enabled && background && command["pip"] == true => {
            mode.audio_only = true;
            "backgroundAudio"
        }
        "toggle" if enabled && command["pip"] == true => {
            if playing {
                "pause"
            } else if command["ended"] == true {
                "replay"
            } else {
                "play"
            }
        }
        "previous" if enabled && command["pip"] == true && queue["hasPrevious"] == true => {
            "previous"
        }
        "next" if enabled && command["pip"] == true && queue["hasNext"] == true => "next",
        _ => return Err("This picture-in-picture action is unavailable.".into()),
    };
    let mut actions = Vec::new();
    if enabled {
        actions.push(if background {
            json!({"id":"audio","label":"Listen in background","icon":"headphones","enabled":true})
        } else {
            json!({"id":"previous","label":"Previous","icon":"previous","enabled":queue["hasPrevious"]})
        });
        actions.push(json!({"id":"toggle","label":if playing {"Pause"}else{"Play"},"icon":if playing {"pause"}else{"play"},"enabled":true}));
        actions.push(json!({"id":"next","label":"Next","icon":"next","enabled":queue["hasNext"]}));
    }
    let result = Ok(
        json!({"videoId":mode.video,"available":enabled,"autoEnter":enabled&&playing&&!mode.audio_only,
        "audioOnly":mode.audio_only,"actions":actions,"perform":perform}),
    );
    drop(mode);
    result
}
