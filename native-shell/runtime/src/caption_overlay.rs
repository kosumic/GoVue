use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionFrame {
    pub text: String,
    pub source_text: String,
    pub segment_index: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionStyle {
    pub scale: f64,
    pub horizontal: f64,
    pub vertical: f64,
    pub background: String,
    pub bold: bool,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionLayout {
    pub source_text: String,
    pub segment_index: i64,
    pub lines: Vec<String>,
}

#[derive(Clone, PartialEq)]
pub struct CaptionDisplay {
    pub frame: CaptionFrame,
    pub style: CaptionStyle,
    pub controls: bool,
}

/// Draw over the existing player bounds. Send layout events back in the
/// `player_caption_frame` command so Rust can page long captions by time.
pub fn caption_overlay(
    display: impl Fn() -> Option<CaptionDisplay> + 'static,
    on_layout: impl Fn(CaptionLayout) + 'static,
) -> NativeView {
    nsh_core::element::native("govue/caption-overlay")
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .w_full()
        .h_full()
        .pointer_events(PointerEvents::None)
        .prop("display", move || {
            display().map_or(Value::Null, |display| {
                let style = display.style;
                // Match the original caption's working width and its margins
                // in both the visible-controls and the unobstructed player.
                json!({"frame": display.frame, "style": style,
                    "maxWidth": 0.92, "left": 0.08 * (style.horizontal + 1.0) / 2.0,
                    "clearance": if display.controls { 52 } else { 12 }})
            })
        })
        .on_native_event(move |event: &NativeEvent| {
            if event.name.as_ref() == "layout"
                && let Ok(layout) = CaptionLayout::deserialize(&event.data)
            {
                on_layout(layout);
            }
        })
}
