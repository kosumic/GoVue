//! LiTube mini-player geometry and persisted placement. Coordinates are relative
//! to the inset-safe React Native workspace; only the settled frame is stored.
use crate::{Result, media};
use serde_json::{Value, json};

pub(super) fn layout(command: &Value, root: &str) -> Result<Value> {
    let dimension = |key: &str| {
        command[key]
            .as_f64()
            .filter(|n| n.is_finite() && (32.0..10000.0).contains(n))
            .ok_or_else(|| format!("Invalid window {key}."))
    };
    let width = dimension("width")?;
    let height = dimension("height")?;
    let inset = command["bottomInset"]
        .as_f64()
        .filter(|n| n.is_finite() && (0.0..=512.0).contains(n))
        .unwrap_or(0.0);
    let compact = width < 600.0;
    let minimum = if compact { 190.0_f64 } else { 240.0 };
    let maximum = (width - 24.0).min(if compact { 320.0 } else { 420.0 });
    let minimum = minimum.min(maximum);
    let mut saved = media::read(root, "mini-player.json", json!({}));
    if !saved.is_object() {
        saved = json!({});
    }
    if let Some(values) = command["frame"].as_object() {
        for key in ["x", "y", "width"] {
            if let Some(value) = values.get(key) {
                if !value
                    .as_f64()
                    .is_some_and(|n| n.is_finite() && n.abs() < 10000.0)
                {
                    return Err("Invalid mini-player position.".into());
                }
                saved[key] = value.clone();
            }
        }
    }
    let mode = command["mode"].as_str().unwrap_or("settle");
    if !matches!(mode, "drag" | "settle" | "release") {
        return Err("Invalid mini-player action.".into());
    }
    let player_width = saved["width"]
        .as_f64()
        .unwrap_or(minimum)
        .round()
        .clamp(minimum, maximum);
    let player_height = (player_width * 9.0 / 16.0).floor();
    let right = 12.0_f64;
    let bottom = 12.0 + (56.0 - inset).max(0.0);
    let left = width - right - player_width;
    let top = height - bottom - player_height;
    let min_x = -left;
    let max_x = right.max(min_x);
    let min_y = -top;
    let max_y = if mode != "settle" && command["pinch"] != true {
        (bottom + player_height / 2.0 - 52.0).max(min_y)
    } else {
        bottom.max(min_y)
    };
    let mut x = saved["x"].as_f64().unwrap_or(0.0).clamp(min_x, max_x);
    let mut y = saved["y"].as_f64().unwrap_or(0.0).clamp(min_y, max_y);
    let center_x = left + x + player_width / 2.0;
    let center_y = top + y + player_height / 2.0;
    let close_left = (width - 96.0) / 2.0;
    let close_top = height - 88.0;
    let close_armed = command["pinch"] != true
        && mode != "settle"
        && (close_left..=close_left + 96.0).contains(&center_x)
        && (close_top..=close_top + 72.0).contains(&center_y);
    let dismiss = mode == "release" && close_armed;
    if !dismiss && (mode != "drag" || command["pinch"] == true) {
        x = if (x - min_x).abs() <= (max_x - x).abs() {
            min_x
        } else {
            max_x
        };
        y = y.clamp(min_y, bottom.max(min_y));
    }
    let frame = json!({"x":x,"y":y,"width":player_width});
    if command["frame"].is_object() && mode != "drag" && !dismiss {
        media::write(root, "mini-player.json", &frame)?;
    }
    Ok(
        json!({"x":x,"y":y,"width":player_width,"height":player_height,"right":right,"bottom":bottom,
        "minimumWidth":minimum,"maximumWidth":maximum,"closeArmed":close_armed,"dismiss":dismiss,
        "close":{"left":close_left,"top":close_top,"width":96,"height":72}}),
    )
}
