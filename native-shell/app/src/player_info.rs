use govue_shell_runtime::{Core, EmbeddedVideoFrame, WatchPlayback};
use nsh_core::prelude::*;
use serde_json::{Value, json};

use crate::reader_view::timestamp;
use crate::ui::{self, Palette, press};

const UNAVAILABLE: &str = "Unavailable";
pub const INFORMATION: &str = "M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0zm-10-1v5m0-8h.01";

pub fn monitor(
    core: Core,
    watch: WatchPlayback,
    bounds: Memo<EmbeddedVideoFrame>,
    fullscreen: Memo<bool>,
) -> Signal<Value> {
    let result = signal(Value::Null);
    effect(move || {
        let (Some(video), Some(status)) = (watch.video(), watch.status()) else {
            result.set(Value::Null);
            return;
        };
        let Some(mut sample) = status.diagnostics.as_object().cloned() else {
            result.set(Value::Null);
            return;
        };
        sample.extend([
            ("epoch".into(), json!(video.generation)),
            ("state".into(), json!(status.state)),
            ("speed".into(), json!(status.rate)),
            (
                "visible".into(),
                json!(bounds.get().visible || fullscreen.get()),
            ),
            ("errorCode".into(), json!(status.error_code)),
        ]);
        if let Ok(sample) =
            core.state(json!({"op":"player_diagnostics","videoId":video.video_id,"sample":sample}))
        {
            result.set(sample);
        }
    });
    result
}

pub fn dialog(
    p: Palette,
    watch: WatchPlayback,
    sample: Signal<Value>,
    close: impl Fn() + Clone + 'static,
) -> View {
    let rows = computed(move || entries(&watch, &sample.get()));
    let copied = signal(0_u64);
    let scrim = close.clone();
    let done = close.clone();
    view()
        .absolute()
        .inset(px(0.0))
        .z_index(2)
        .justify_center()
        .px(px(20.0))
        .child(
            press()
                .absolute()
                .inset(px(0.0))
                .bg(rgba(0x00000073))
                .label("Close info")
                .on_press(scrim),
        )
        .child(
            view()
                .w_full()
                .max_w(px(480.0))
                .h(percentage(0.72))
                .self_center()
                .rounded(px(18.0))
                .bg(move || p.background())
                .overflow_hidden()
                .child(
                    view()
                        .h(px(52.0))
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .pl(px(18.0))
                        .pr(px(4.0))
                        .child(
                            text("Info")
                                .text_size(px(19.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                        )
                        .child(ui::icon_button(p, ui::CLOSE, "Close info", close)),
                )
                .child(
                    scroll()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(view().px(px(16.0)).child(each(
                            rows,
                            |(label, _)| *label,
                            move |item, _| {
                                view()
                                    .pb(px(12.0))
                                    .mb(px(8.0))
                                    .border_b(px(0.5))
                                    .border_color(move || p.raised())
                                    .child(
                                        view()
                                            .flex_row()
                                            .items_center()
                                            .child(
                                                text(move || item.with(|(label, _)| *label))
                                                    .flex_1()
                                                    .text_size(px(12.0))
                                                    .text_color(move || p.muted()),
                                            )
                                            .child(
                                                press()
                                                    .size(px(48.0))
                                                    .items_center()
                                                    .justify_center()
                                                    .label(move || {
                                                        item.with(|(label, _)| {
                                                            format!("Copy {label}")
                                                        })
                                                    })
                                                    .on_press(move || {
                                                        copy(
                                                            item.with_untracked(
                                                                |(label, value)| {
                                                                    format!("{label}: {value}")
                                                                },
                                                            ),
                                                            copied,
                                                        );
                                                    })
                                                    .child(ui::icon(ui::COPY, 20.0, move || {
                                                        p.muted()
                                                    })),
                                            ),
                                    )
                                    .child(
                                        text(move || item.with(|(_, value)| value.clone()))
                                            .selectable(true)
                                            .text_size(px(14.0))
                                            .text_color(move || p.ink()),
                                    )
                            },
                        ))),
                )
                .child(
                    view()
                        .px(px(16.0))
                        .pb(px(10.0))
                        .flex_row()
                        .justify_end()
                        .child(button(p, "Close", done))
                        .child(button(p, "Copy all", move || {
                            copy(
                                rows.with_untracked(|rows| {
                                    rows.iter()
                                        .map(|(name, value)| format!("{name}: {value}"))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                }),
                                copied,
                            );
                        })),
                )
                .child(when(
                    move || copied.get() != 0,
                    move || {
                        text("Copied")
                            .px(px(16.0))
                            .pb(px(10.0))
                            .text_size(px(13.0))
                            .text_color(move || p.muted())
                    },
                )),
        )
}

fn entries(watch: &WatchPlayback, sample: &Value) -> Vec<(&'static str, String)> {
    let Some(video) = watch.video() else {
        return Vec::new();
    };
    let status = watch.status();
    let live = watch.live();
    let label = |key: &str| sample[key].as_str().unwrap_or(UNAVAILABLE).to_owned();
    let fps = |key: &str| {
        sample[key]
            .as_f64()
            .map_or_else(|| UNAVAILABLE.into(), |n| format!("{n:.1} fps"))
    };
    let bitrate = |key: &str| {
        sample[key].as_f64().map_or_else(
            || UNAVAILABLE.into(),
            |value| format!("{:.0} kbps", (value / 1000.0).floor()),
        )
    };
    let resolution = sample["width"]
        .as_f64()
        .zip(sample["height"].as_f64())
        .map_or_else(|| UNAVAILABLE.into(), |(w, h)| format!("{w:.0}×{h:.0}"));
    let rendered = sample["renderedFrames"].as_u64().unwrap_or_default();
    let dropped = sample["droppedFrames"].as_u64().unwrap_or_default();
    let frames = sample["droppedRatio"].as_f64().map_or_else(
        || UNAVAILABLE.into(),
        |ratio| format!("{dropped} / {} ({:.1}%)", rendered + dropped, ratio * 100.0),
    );
    let buffering = sample["playbackState"].as_str().map_or_else(
        || {
            status
                .as_ref()
                .map_or_else(|| UNAVAILABLE.into(), |status| status.state.clone())
        },
        |state| {
            format!(
                "{state} · {} ms · Buffers/min: {}",
                sample["bufferingMs"], sample["bufferCount"]
            )
        },
    );
    let quality = if video.quality_height == 0 {
        "Auto".into()
    } else if video.quality_fps > 30 {
        format!("{}p{}", video.quality_height, video.quality_fps)
    } else {
        format!("{}p", video.quality_height)
    };
    let mut rows = vec![
        ("Media FPS", fps("nominalFps")),
        ("Rendered FPS · 5 s", fps("renderedFps")),
        ("Expected FPS", fps("expectedFps")),
        ("Video decoder", label("videoDecoder")),
        ("Audio decoder", label("audioDecoder")),
        ("Video codec", label("videoCodec")),
        ("Audio codec", label("audioCodec")),
        ("Dropped frames / total", frames),
        (
            "Playback speed",
            status
                .as_ref()
                .map_or_else(|| UNAVAILABLE.into(), |status| format!("{}×", status.rate)),
        ),
        ("Playback / buffering", buffering),
        (
            "First frame time",
            sample["firstFrameMs"]
                .as_u64()
                .map_or_else(|| UNAVAILABLE.into(), |n| format!("{n} ms")),
        ),
        ("Title", video.title),
        (
            "Author",
            if video.author.is_empty() {
                UNAVAILABLE.into()
            } else {
                video.author
            },
        ),
        ("Quality", quality),
        (
            "Duration",
            if live {
                "LIVE".into()
            } else {
                status
                    .as_ref()
                    .and_then(|status| status.duration)
                    .map_or_else(|| UNAVAILABLE.into(), timestamp)
            },
        ),
    ];
    rows.extend(live.then(|| ("Type", "LIVE".into())));
    rows.extend([
        ("Video ID", video.video_id),
        ("Resolution", resolution),
        ("Video bitrate", bitrate("videoBitrate")),
        ("Audio bitrate", bitrate("audioBitrate")),
    ]);
    rows.extend(
        sample["audioSampleRate"]
            .as_f64()
            .map(|value| ("Sample rate", format!("{value:.0} Hz"))),
    );
    rows.extend(
        sample["audioChannels"]
            .as_f64()
            .map(|value| ("Channels", format!("{value:.0}"))),
    );
    rows.extend(
        sample["errorCode"]
            .as_str()
            .map(|value| ("Error", value.to_owned())),
    );
    rows
}

fn button(p: Palette, label: &'static str, action: impl Fn() + 'static) -> View {
    press()
        .h(px(48.0))
        .min_w(px(64.0))
        .px(px(12.0))
        .items_center()
        .justify_center()
        .label(label)
        .on_press(action)
        .child(text(label).text_size(px(14.0)).text_color(move || p.ink()))
}

fn copy(text: String, feedback: Signal<u64>) {
    set_clipboard(text);
    let version = feedback.get_untracked() + 1;
    feedback.set(version);
    spawn(async move {
        timer(std::time::Duration::from_secs(2)).await;
        if feedback.get_untracked() == version {
            feedback.set(0);
        }
    });
}
