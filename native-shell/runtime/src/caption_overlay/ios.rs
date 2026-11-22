use nsh_core::{Bounds, PointerEvents, Points, element::text_measurer, prelude::*};
use unicode_segmentation::UnicodeSegmentation;

use super::{CaptionDisplay, CaptionLayout};

pub(super) fn overlay(
    display: impl Fn() -> Option<CaptionDisplay> + 'static,
    on_layout: impl Fn(CaptionLayout) + 'static,
) -> View {
    let display = computed(display);
    let size = signal((0.0_f32, 0.0_f32));
    let cue_height = signal(0.0_f32);
    let scale = computed(move || {
        display.with(|display| {
            display
                .as_ref()
                .map_or(1.0_f32, |cue| cue.style.scale as f32)
        })
    });
    let bold = computed(move || {
        display.with(|display| display.as_ref().is_some_and(|cue| cue.style.bold))
    });
    let source = computed(move || {
        display.with(|display| {
            display.as_ref().map(|display| {
                (
                    display.frame.source_text.clone(),
                    display.frame.segment_index,
                )
            })
        })
    });
    let measure = text_measurer();
    effect(move || {
        let width = 0.92 * size.get().0 - 16.0 * scale.get();
        if width <= 0.0 {
            return;
        }
        if let Some((source_text, segment_index)) = source.get() {
            let weight = if bold.get() { 700 } else { 400 };
            let font = format!("{weight} {}px system-ui", 14.0 * scale.get());
            let lines = wrap(&source_text, width, |row| measure(&font, row).0);
            on_layout(CaptionLayout {
                source_text,
                segment_index,
                lines,
            });
        }
    });
    view()
        .absolute()
        .inset(px(0.0))
        .pointer_events(PointerEvents::None)
        .on_layout(move |bounds: &Bounds<Points>| {
            size.set((bounds.size.width.0, bounds.size.height.0))
        })
        .child(when(
            move || display.with(Option::is_some),
            move || {
                view()
                    .absolute()
                    .items_center()
                    .left(move || {
                        px(display.with(|cue| {
                            cue.as_ref().map_or(0.0, |cue| {
                                size.get().0 * 0.04 * (cue.style.horizontal as f32 + 1.0)
                            })
                        }))
                    })
                    .w(percentage(0.92))
                    .top(move || {
                        px(display.with(|cue| {
                            cue.as_ref().map_or(0.0, |cue| {
                                let clear = if cue.controls { 52.0 } else { 12.0 };
                                clear
                                    + (size.get().1 - 2.0 * clear - cue_height.get()).max(0.0)
                                        * cue.style.vertical as f32
                            })
                        }))
                    })
                    .on_layout(move |bounds: &Bounds<Points>| cue_height.set(bounds.size.height.0))
                    .child(each(
                        move || vec![bold.get()],
                        |weight| *weight,
                        move |weight, _| {
                            text(move || {
                                display.with(|cue| {
                                    cue.as_ref()
                                        .map_or_else(String::new, |cue| cue.frame.text.clone())
                                })
                            })
                            .max_w(percentage(1.0))
                            .text_center()
                            .rounded(px(4.0))
                            .px(move || px(8.0 * scale.get()))
                            .py(move || px(2.0 * scale.get()))
                            .text_size(move || px(14.0 * scale.get()))
                            .font_weight(if weight.get_untracked() {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(move || -> Hsla {
                                display.with(|cue| {
                                    let color = cue
                                        .as_ref()
                                        .and_then(|cue| cue.style.color.strip_prefix('#'))
                                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                                        .unwrap_or(0xffffff);
                                    rgb(color).into()
                                })
                            })
                            .bg(move || {
                                display.with(|cue| {
                                    match cue.as_ref().map(|cue| cue.style.background.as_str()) {
                                        Some("none") => rgba(0),
                                        Some("opaque") => rgba(0x000000ff),
                                        _ => rgba(0x00000099),
                                    }
                                })
                            })
                        },
                    ))
            },
        ))
}

fn wrap(source: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut rows = Vec::new();
    for paragraph in source.split('\n') {
        let mut row = String::new();
        for token in paragraph.split_word_bounds() {
            if token.trim().is_empty() {
                if !row.is_empty() {
                    row.push_str(token);
                }
                continue;
            }
            if measure(token) <= width {
                append(&mut rows, &mut row, token, width, &measure);
            } else {
                for grapheme in token.graphemes(true) {
                    append(&mut rows, &mut row, grapheme, width, &measure);
                }
            }
        }
        rows.push(row.trim_end().to_owned());
    }
    rows
}

fn append(
    rows: &mut Vec<String>,
    row: &mut String,
    part: &str,
    width: f32,
    measure: &impl Fn(&str) -> f32,
) {
    let previous = row.len();
    row.push_str(part);
    if previous > 0 && measure(row) > width {
        row.truncate(previous);
        rows.push(row.trim_end().to_owned());
        row.clear();
        row.push_str(part);
    }
}
