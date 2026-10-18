//! Shared caption color conversion for the LiTube HSV editor.
use crate::{Result, string};
use serde_json::{Value, json};

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Validated HSV components produce rounded channels in the 0..=255 range."
)]
fn rgb(h: f64, s: f64, v: f64) -> String {
    let channel = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        (v * (1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0)) * 255.0).round() as u8
    };
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(5.0),
        channel(3.0),
        channel(1.0)
    )
}

#[allow(
    clippy::float_cmp,
    reason = "The maximum is selected directly from the same RGB operands to identify its channel."
)]
pub(super) fn color(command: &Value) -> Result<Value> {
    let (h, s, v) = if command["hex"].is_string() {
        let text = string(command, "hex")?.trim();
        let raw = text.strip_prefix('#').unwrap_or(text);
        if !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Enter a hexadecimal color.".into());
        }
        let raw = match raw.len() {
            3 => raw.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => raw.into(),
            // Android's LiTube backups encode alpha before RGB.
            8 if raw.is_ascii() => raw[2..].into(),
            _ => return Err("Enter a hexadecimal color.".into()),
        };
        let packed = u32::from_str_radix(&raw, 16).map_err(|_| "Enter a hexadecimal color.")?;
        let r = f64::from((packed >> 16) & 255) / 255.0;
        let g = f64::from((packed >> 8) & 255) / 255.0;
        let b = f64::from(packed & 255) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        (h, if max == 0.0 { 0.0 } else { delta / max }, max)
    } else {
        let number = |key: &str, max: f64| {
            command[key]
                .as_f64()
                .filter(|n| n.is_finite() && (0.0..=max).contains(n))
                .ok_or_else(|| "Invalid caption color.".to_owned())
        };
        (
            number("hue", 360.0)?,
            number("saturation", 1.0)?,
            number("value", 1.0)?,
        )
    };
    Ok(json!({"color":rgb(h,s,v),"hue":h,"saturation":s,"value":v,"hueColor":rgb(h,1.0,1.0)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hsv_round_trips_caption_colors_and_preserves_gray_brightness() {
        for hex in [
            "#FFFFFF", "#000000", "#808080", "#FF0000", "#00FF00", "#0000FF", "#2BB6B6",
        ] {
            let parsed = color(&json!({"hex":hex})).unwrap();
            assert_eq!(parsed["color"], hex);
            let converted = color(&parsed).unwrap();
            assert_eq!(converted["color"], hex);
        }
        assert_eq!(color(&json!({"hex":"#abc"})).unwrap()["color"], "#AABBCC");
        assert_eq!(
            color(&json!({"hex":"FF2BB6B6"})).unwrap()["color"],
            "#2BB6B6"
        );
        for hex in ["#", "##fff", "ZZFFFFFF", "NaN", "#12", "#123456789"] {
            assert!(color(&json!({"hex":hex})).is_err());
        }
        for hue in [-1.0, 361.0] {
            assert!(color(&json!({"hue":hue,"saturation":1.0,"value":1.0})).is_err());
        }
    }
}
