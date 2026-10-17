//! Gesture policy and short-lived double-tap accumulation. No media addresses
//! or pointer samples are persisted.
use crate::{Result, string, video_id};
use serde_json::{Value, json};
use std::sync::Mutex;

fn number(command: &Value, key: &str, maximum: f64) -> Result<f64> {
    command[key]
        .as_f64()
        .filter(|n| n.is_finite() && n.abs() <= maximum)
        .ok_or_else(|| format!("Invalid gesture {key}."))
}

pub(super) fn drag(command: &Value) -> Result<Value> {
    let width = number(command, "width", 10000.0)?;
    let height = number(command, "height", 10000.0)?;
    if width <= 0.0 || height <= 0.0 {
        return Err("Missing player size.".into());
    }
    let dx = number(command, "dx", 100000.0)?;
    let dy = number(command, "dy", 100000.0)?;
    let x = number(command, "x", 10000.0)?;
    let mode = command["mode"].as_str().filter(|s| !s.is_empty());
    let kind = mode.unwrap_or_else(|| {
        if dx.abs() > dy.abs() {
            "seek"
        } else if x < width * 0.35 {
            "brightness"
        } else if x > width * 0.65 {
            "volume"
        } else {
            "fullscreen"
        }
    });
    Ok(match kind {
        "seek" => {
            let delta = (dx / width * 120000.0).trunc() / 1000.0;
            let position = number(command, "position", 604800.0)?.max(0.0);
            let duration = number(command, "duration", 604800.0)?.max(0.0);
            json!({"kind":kind,"delta":delta,"target":(position+delta).clamp(0.0,if duration>0.0{duration}else{604800.0})})
        }
        "brightness" => {
            let delta = (-dy / height * 1.5).clamp(-1.0, 1.0);
            json!({"kind":kind,"delta":delta,"target":(number(command,"brightness",1.0)?+delta).clamp(0.01,1.0)})
        }
        "volume" => {
            let delta = (-dy / height * 1.2).clamp(-1.0, 1.0);
            json!({"kind":kind,"delta":delta,"target":(number(command,"volume",1.0)?+delta).clamp(0.0,1.0)})
        }
        "fullscreen" => {
            json!({"kind":kind,"enabled":dy<0.0,"commit":dy.abs()>height*0.08&&dy.abs()>dx.abs()})
        }
        _ => return Err("Invalid gesture mode.".into()),
    })
}

#[derive(Default)]
struct SeekTaps {
    owner: String,
    video_id: String,
    at: u64,
    delta: i64,
}
static TAPS: Mutex<Option<SeekTaps>> = Mutex::new(None);

pub(super) fn double_tap(command: &Value) -> Result<Value> {
    let id = string(command, "videoId")?;
    let owner = string(command, "owner")?;
    if !video_id(id)
        || owner.is_empty()
        || owner.len() > 80
        || !owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("Invalid gesture owner.".into());
    }
    let width = number(command, "width", 10000.0)?;
    let x = number(command, "x", 10000.0)?;
    if width <= 0.0 || !(0.0..=width).contains(&x) {
        return Err("Invalid gesture position.".into());
    }
    let delta = if x < width / 3.0 {
        -10
    } else if x > width * 2.0 / 3.0 {
        10
    } else {
        return Ok(json!({"kind":"toggle"}));
    };
    let at = command["at"]
        .as_u64()
        .filter(|v| *v <= 9_007_199_254_740_991)
        .ok_or("Invalid gesture time.")?;
    let position = number(command, "position", 604800.0)?.max(0.0);
    let duration = number(command, "duration", 604800.0)?.max(0.0);
    let mut guard = TAPS.lock().unwrap_or_else(|e| e.into_inner());
    let taps = guard.get_or_insert_with(SeekTaps::default);
    if taps.owner != owner
        || taps.video_id != id
        || at < taps.at
        || at - taps.at > 600
        || (taps.delta > 0) != (delta > 0)
    {
        *taps = SeekTaps {
            owner: owner.into(),
            video_id: id.into(),
            at,
            delta: 0,
        };
    }
    taps.at = at;
    taps.delta = (taps.delta + delta).clamp(-604800, 604800);
    let target =
        (position + delta as f64).clamp(0.0, if duration > 0.0 { duration } else { 604800.0 });
    Ok(json!({"kind":"seek","target":target,"delta":delta,"accumulated":taps.delta}))
}
