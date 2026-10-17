//! Native HLS track metadata is transient. Never accept or store rendition URLs
//! here: Expo owns the manifest, and the platform adapter pins a native track.
use super::Result;
use serde_json::{Value, json};

fn tracks(command: &Value) -> Result<Vec<Value>> {
    let raw = command["tracks"]
        .as_array()
        .filter(|v| v.len() <= 500)
        .ok_or("Invalid live tracks.")?;
    let mut tracks = Vec::new();
    for (index, track) in raw.iter().enumerate() {
        let Some(height) = track["height"].as_u64().filter(|n| *n > 0 && *n <= 8640) else {
            continue;
        };
        if track["supported"] == false {
            continue;
        }
        let fps = track["fps"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 240.0)
            .unwrap_or(0.0)
            .round() as u64;
        let bitrate = track["bitrate"].as_u64().unwrap_or(0).min(1_000_000_000);
        tracks.push(json!({"height":height,"fps":fps,"index":index,"bitrate":bitrate,"label":format!("{height}p{}",if fps>30 {fps.to_string()}else{String::new()})}));
    }
    tracks.sort_by_key(|t| {
        std::cmp::Reverse((
            t["height"].as_u64(),
            t["fps"].as_u64(),
            t["bitrate"].as_u64(),
        ))
    });
    tracks.dedup_by_key(|t| (t["height"].as_u64(), t["fps"].as_u64()));
    tracks.truncate(16);
    Ok(tracks)
}
pub(super) fn choices(command: &Value) -> Result<Value> {
    Ok(json!(tracks(command)?))
}
pub(super) fn select(command: &Value) -> Result<Value> {
    let height = command["height"]
        .as_u64()
        .filter(|n| *n <= 8640)
        .ok_or("Invalid live resolution.")?;
    let fps = command["fps"].as_u64().filter(|n| *n <= 240).unwrap_or(0);
    if height == 0 {
        return Ok(json!({"height":0,"fps":0,"index":-1}));
    }
    tracks(command)?
        .into_iter()
        .find(|t| t["height"] == height && (fps == 0 || t["fps"] == fps))
        .ok_or("This live resolution is unavailable. Try Auto.".into())
}
