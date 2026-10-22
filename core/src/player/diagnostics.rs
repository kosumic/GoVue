//! Transient renderer samples. No addresses or raw error messages are accepted.
use crate::Result;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Default)]
struct Monitor {
    id: String,
    epoch: u64,
    last_at: u64,
    grace: u64,
    frames: VecDeque<(u64, u64)>,
    buffers: VecDeque<u64>,
    buffer_at: Option<u64>,
}
static MONITOR: Mutex<Option<Monitor>> = Mutex::new(None);

fn number(value: &Value, key: &str, max: f64) -> Option<f64> {
    value[key]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= max)
}
fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 256
                && !s.contains("://")
                && s.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        })
        .map(str::to_owned)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Frame rates and drop ratios are approximate display measurements."
)]
pub(super) fn sample(command: &Value) -> Result<Value> {
    let id = command["videoId"]
        .as_str()
        .filter(|s| crate::video_id(s))
        .ok_or("Invalid diagnostic video identity.")?;
    let raw = &command["sample"];
    let at = raw["sampledAt"]
        .as_u64()
        .filter(|v| *v <= 9_007_199_254_740_991)
        .ok_or("Invalid diagnostic sample time.")?;
    let epoch = raw["epoch"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= at)
        .ok_or("Invalid diagnostic source generation.")?;
    let state = raw["state"]
        .as_str()
        .filter(|s| {
            matches!(
                *s,
                "idle" | "buffering" | "playing" | "paused" | "ended" | "error"
            )
        })
        .ok_or("Invalid diagnostic playback state.")?;
    let rendered = raw["renderedFrames"]
        .as_u64()
        .unwrap_or(0)
        .min(1_000_000_000);
    let dropped = raw["droppedFrames"]
        .as_u64()
        .unwrap_or(0)
        .min(1_000_000_000);
    let mut guard = MONITOR
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    let monitor = guard.get_or_insert_with(Monitor::default);
    if monitor.id != id || monitor.epoch != epoch {
        *monitor = Monitor {
            id: id.into(),
            epoch,
            grace: at.saturating_add(3000),
            ..Monitor::default()
        };
    }
    if at < monitor.last_at {
        return Err("The diagnostic sample was superseded.".into());
    }
    monitor.last_at = at;
    if state == "buffering" {
        if monitor.buffer_at.is_none() {
            monitor.buffer_at = Some(at);
            monitor.buffers.push_back(at);
        }
    } else {
        monitor.buffer_at = None;
    }
    while monitor.buffers.front().is_some_and(|t| at - t > 60_000) {
        monitor.buffers.pop_front();
    }
    // Bound storage even if a caller supplies repeated transition timestamps.
    while monitor.buffers.len() > 120 {
        monitor.buffers.pop_front();
    }
    let eligible = state == "playing" && raw["visible"] == true;
    if !eligible || at < monitor.grace {
        monitor.frames.clear();
    } else {
        if monitor.frames.back().is_some_and(|(_, n)| rendered < *n) {
            monitor.frames.clear();
        }
        if monitor.frames.back().is_none_or(|(t, _)| at > *t) {
            monitor.frames.push_back((at, rendered));
        }
        while monitor.frames.len() > 1 && monitor.frames.get(1).is_some_and(|(t, _)| at - t >= 5000)
        {
            monitor.frames.pop_front();
        }
        if monitor.frames.len() > 120 {
            monitor.frames.pop_front();
        }
    }
    let fps = monitor.frames.front().and_then(|(t, n)| {
        (at - t >= 5000).then(|| (rendered - n) as f64 * 1000.0 / (at - t) as f64)
    });
    let nominal = number(raw, "nominalFps", 240.0);
    let expected = nominal.map(|n| {
        (n * number(raw, "speed", 16.0).unwrap_or(1.0))
            .min(number(raw, "refreshRate", 1000.0).unwrap_or(60.0))
    });
    let mut result = json!({
        "nominalFps":nominal,"renderedFps":fps,"expectedFps":expected,
        "renderedFrames":rendered,"droppedFrames":dropped,
        "droppedRatio":(rendered+dropped>0).then(||dropped as f64/(rendered+dropped) as f64),
        "firstFrameMs":raw["firstFrameMs"].as_u64().filter(|v|*v<=604_800_000),
        "bufferingMs":monitor.buffer_at.map_or(0,|t|at-t),
        "bufferCount":monitor.buffers.len(),"playbackState":state
    });
    drop(guard);
    for key in [
        "videoCodec",
        "audioCodec",
        "videoDecoder",
        "audioDecoder",
        "errorCode",
    ] {
        result[key] = json!(text(raw, key));
    }
    for (key, max) in [
        ("width", 15360.0),
        ("height", 8640.0),
        ("videoBitrate", 1_000_000_000.0),
        ("audioBitrate", 100_000_000.0),
        ("audioSampleRate", 384_000.0),
        ("audioChannels", 32.0),
    ] {
        result[key] = json!(number(raw, key, max));
    }
    Ok(result)
}
