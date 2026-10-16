use super::{Result, Transcript};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize)]
struct Cursor {
    position: f64,
    end: Option<f64>,
    playing: bool,
}

// The next line takes over overlapping captions. A missing duration lasts until
// that next line; the final undated caption has no known pause boundary.
fn next_end(data: &Transcript, position: f64) -> Option<f64> {
    let after = data.segments.partition_point(|s| s.start <= position);
    let index = after.saturating_sub(1);
    for i in index..data.segments.len() {
        let segment = &data.segments[i];
        let next = data.segments.get(i + 1).map(|s| s.start);
        let end = if segment.duration > 0.0 {
            let end = segment.start + segment.duration;
            Some(next.map_or(end, |start| start.min(end)))
        } else {
            next
        };
        if let Some(end) = end.filter(|end| end.is_finite() && *end > position) {
            return Some(end);
        }
    }
    None
}

pub(super) fn step(data: &Transcript, command: &Value) -> Result<Value> {
    let Some(position) = command["position"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
    else {
        return Ok(json!({"pause":false,"cursor":null}));
    };
    let playing = command["playing"] == true;
    let rate = command["rate"]
        .as_f64()
        .filter(|n| *n > 0.0 && *n <= 16.0)
        .ok_or("Invalid playback rate.")?;
    let elapsed = command["elapsed"].as_f64().unwrap_or(0.0);
    let previous = serde_json::from_value::<Cursor>(command["cursor"].clone()).ok();
    // A seek, ad, or delayed background sample re-arms at the new position
    // instead of pausing for lines the viewer skipped.
    let pause = previous.is_some_and(|previous| {
        playing
            && previous.playing
            && command["seeking"] != true
            && (0.0..=1.0).contains(&elapsed)
            && position >= previous.position
            && position - previous.position <= elapsed * rate + 0.15
            && previous
                .end
                .is_some_and(|end| previous.position < end && position >= end)
    });
    Ok(json!({"pause":pause,"cursor":Cursor {
        position, end: next_end(data, position), playing: playing && command["seeking"] != true,
    }}))
}
