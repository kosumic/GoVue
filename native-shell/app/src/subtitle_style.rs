use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use govue_shell_runtime::CaptionStyle;
use nsh_core::{
    Checked, PointerEvents, ResizeMode,
    element::{ImageSrc, ResponderEvent, TouchClaim, input},
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    settings_view::Preferences,
    ui::{self, Palette, press},
};

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Color {
    color: String,
    hue: f64,
    saturation: f64,
    value: f64,
    hue_color: String,
}

#[derive(Clone)]
struct Editor {
    p: Palette,
    preferences: Preferences,
    style: Signal<CaptionStyle>,
    color: Signal<Color>,
    hex: Signal<String>,
    last_saved: Signal<Option<Instant>>,
}

pub fn subtitle_style(
    p: Palette,
    preferences: Preferences,
    video: Option<String>,
    workspace_height: impl Fn() -> f32 + Copy + 'static,
    close: impl Fn() + 'static,
) -> Option<View> {
    let style = preferences.caption_style()?;
    let color: Color = preferences.caption_color(json!({"hex":style.color}))?;
    let editor = Editor {
        p,
        preferences,
        style: signal(style),
        hex: signal(color.color.clone()),
        color: signal(color),
        last_saved: signal(None),
    };
    let close: Rc<dyn Fn()> = Rc::new(close);
    let finish = {
        let editor = editor.clone();
        Rc::new(move || {
            editor.save();
            close();
        })
    };
    Some(
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(2)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000066))
                    .label("Close subtitle style")
                    .on_press({
                        let finish = finish.clone();
                        move || finish()
                    }),
            )
            .child(
                view()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .max_h(percentage(0.90))
                    .rounded(px(16.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .min_h(px(48.0))
                            .px(px(20.0))
                            .child(
                                text("Subtitle style")
                                    .flex_1()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(p, ui::CLOSE, "Close subtitle style", {
                                let finish = finish.clone();
                                move || finish()
                            })),
                    )
                    .child(scroll().flex_shrink(1.0).child(editor.body(
                        video,
                        workspace_height,
                        {
                            let finish = finish.clone();
                            move || finish()
                        },
                    ))),
            ),
    )
}

impl Editor {
    fn save(&self) {
        self.preferences
            .update(json!({"subtitleStyle":self.style.get_untracked()}));
        self.last_saved.set(Some(Instant::now()));
    }

    fn change(&self, update: impl FnOnce(&mut CaptionStyle)) {
        self.style.update(update);
        if self.last_saved.with_untracked(|last| {
            last.is_none_or(|last| last.elapsed() >= Duration::from_millis(500))
        }) {
            self.save();
        }
    }

    fn body(
        &self,
        video: Option<String>,
        workspace_height: impl Fn() -> f32 + Copy + 'static,
        close: impl Fn() + 'static,
    ) -> View {
        let p = self.p;
        let style = self.style;
        let bold = self.clone();
        let reset = self.clone();
        view()
            .px(px(20.0))
            .pb(px(12.0))
            .child(self.preview(video, workspace_height))
            .child(
                text("Drag the preview to position subtitles")
                    .mt(px(6.0))
                    .mb(px(4.0))
                    .text_size(px(12.0))
                    .text_color(move || p.muted()),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .mt(px(8.0))
                    .child(label(p, "Text size").flex_1())
                    .child(
                        text(move || format!("{:.0}%", (style.with(|v| v.scale) * 100.0).floor()))
                            .text_size(px(12.0))
                            .text_color(move || p.ink()),
                    ),
            )
            .child(self.size_slider())
            .child(label(p, "Background"))
            .child(
                view().flex_row().gap(px(8.0)).mt(px(4.0)).children(
                    [
                        ("None", "none"),
                        ("Translucent", "semi"),
                        ("Opaque", "opaque"),
                    ]
                    .map(|(title, key)| {
                        let edit = self.clone();
                        let selected = move || style.with(|style| style.background == key);
                        press()
                            .flex_1()
                            .py(px(8.0))
                            .items_center()
                            .rounded(px(8.0))
                            .bg(move || if selected() { p.ink() } else { p.raised() })
                            .on_press(move || {
                                edit.change(|style| style.background = key.into());
                                edit.save();
                            })
                            .child(text(title).text_size(px(13.0)).text_color(move || {
                                if selected() { p.background() } else { p.ink() }
                            }))
                    }),
                ),
            )
            .child(
                press()
                    .flex_row()
                    .items_center()
                    .mt(px(8.0))
                    .h(px(44.0))
                    .role(AccessibilityRole::Switch)
                    .label("Bold subtitles")
                    .accessibility_state(move || AccessibilityState {
                        checked: Some(if style.with(|style| style.bold) {
                            Checked::True
                        } else {
                            Checked::False
                        }),
                        ..Default::default()
                    })
                    .on_press(move || {
                        bold.change(|style| style.bold = !style.bold);
                        bold.save();
                    })
                    .child(label(p, "Bold").flex_1())
                    .child(
                        view()
                            .relative()
                            .w(px(42.0))
                            .h(px(24.0))
                            .rounded(px(12.0))
                            .bg(move || {
                                if style.with(|style| style.bold) {
                                    p.ink()
                                } else {
                                    p.raised()
                                }
                            })
                            .child(
                                view()
                                    .absolute()
                                    .top(px(3.0))
                                    .left(move || {
                                        px(if style.with(|style| style.bold) {
                                            21.0
                                        } else {
                                            3.0
                                        })
                                    })
                                    .size(px(18.0))
                                    .rounded(px(9.0))
                                    .bg(move || p.background()),
                            ),
                    ),
            )
            .child(self.color_picker())
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .pt(px(8.0))
                    .child(action("Reset", move || {
                        if let Some(style) = reset.preferences.default_caption_style() {
                            let color = reset
                                .preferences
                                .caption_color::<Color>(json!({"hex":style.color}));
                            reset.style.set(style);
                            if let Some(color) = color {
                                reset.hex.set(color.color.clone());
                                reset.color.set(color);
                            }
                            reset.save();
                        }
                    }))
                    .child(action("Close", close)),
            )
    }

    fn preview(
        &self,
        video: Option<String>,
        workspace_height: impl Fn() -> f32 + Copy + 'static,
    ) -> View {
        let p = self.p;
        let style = self.style;
        let editor = self.clone();
        let frame = signal((1.0_f32, 1.0_f32));
        let caption_size = signal((0.0_f32, 0.0_f32));
        let origin = signal((0.0, 1.0));
        let caption_position = computed(move || {
            let (width, height) = frame.get();
            let (caption_width, caption_height) = caption_size.get();
            style.with(|style| {
                (
                    10.0 + (width - caption_width - 20.0).max(0.0)
                        * (style.horizontal as f32 + 1.0)
                        / 2.0,
                    10.0 + (height - caption_height - 20.0).max(0.0) * style.vertical as f32,
                )
            })
        });
        let release = self.clone();
        let terminate = self.clone();
        let bold = computed(move || style.with(|style| style.bold));
        view()
            .h(move || {
                px(if workspace_height() < 500.0 {
                    100.0
                } else {
                    168.0
                })
            })
            .rounded(px(12.0))
            .overflow_hidden()
            .bg(move || p.raised())
            .on_layout(move |bounds: &Bounds<Points>| {
                frame.set((bounds.size.width.0.max(1.0), bounds.size.height.0.max(1.0)))
            })
            .claim_touches(TouchClaim::on_start())
            .keep_touches(true)
            .on_responder_grant(move |_: &ResponderEvent| {
                origin.set(style.with_untracked(|style| (style.horizontal, style.vertical)))
            })
            .on_responder_move(move |event: &ResponderEvent| {
                let (x, y) = origin.get_untracked();
                let (width, height) = frame.get_untracked();
                editor.change(|style| {
                    style.horizontal =
                        (x + f64::from(event.gesture.delta.x.0 / width * 2.0)).clamp(-1.0, 1.0);
                    style.vertical =
                        (y + f64::from(event.gesture.delta.y.0 / height)).clamp(0.0, 1.0);
                });
            })
            .on_responder_release(move |_: &ResponderEvent| release.save())
            .on_responder_terminate(move |_: &ResponderEvent| terminate.save())
            .children(video.map(|id| {
                image(ImageSrc::from(format!(
                    "https://i.ytimg.com/vi/{id}/hqdefault.jpg"
                )))
                .absolute()
                .inset(px(0.0))
                .w_full()
                .h_full()
                .pointer_events(PointerEvents::None)
            }))
            .child(dynamic(move || {
                text("Subtitle preview")
                    .absolute()
                    .left(move || px(caption_position.get().0))
                    .top(move || px(caption_position.get().1))
                    .max_w(move || px((frame.get().0 - 20.0).max(1.0)))
                    .on_layout(move |bounds: &Bounds<Points>| {
                        caption_size.set((bounds.size.width.0, bounds.size.height.0))
                    })
                    .px(px(5.0))
                    .py(px(2.0))
                    .pointer_events(PointerEvents::None)
                    .text_center()
                    .text_size(move || px(14.0 * style.with(|style| style.scale as f32)))
                    .font_weight(if bold.get() {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(move || style.with(|style| hex_color(&style.color)))
                    .bg(move || {
                        style.with(|style| match style.background.as_str() {
                            "none" => rgba(0x00000000),
                            "opaque" => rgba(0x000000ff),
                            _ => rgba(0x00000099),
                        })
                    })
            }))
    }

    fn size_slider(&self) -> View {
        let p = self.p;
        let style = self.style;
        let width = signal(1.0_f32);
        let slider = self.clone();
        let drag = Rc::new(move |event: &ResponderEvent| {
            let fraction = f64::from((event.location.x.0 / width.get_untracked()).clamp(0.0, 1.0));
            slider.change(|style| style.scale = (50.0 + 250.0 * fraction).round() / 100.0);
        });
        let fill = move || percentage(style.with(|style| (style.scale as f32 - 0.5) / 2.5));
        let start = drag.clone();
        let release = self.clone();
        let terminate = self.clone();
        view()
            .h(px(32.0))
            .justify_center()
            .label("Subtitle text size")
            .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0.max(1.0)))
            .claim_touches(TouchClaim::on_start())
            .keep_touches(true)
            .on_responder_grant(move |event: &ResponderEvent| start(event))
            .on_responder_move(move |event: &ResponderEvent| drag(event))
            .on_responder_release(move |_: &ResponderEvent| release.save())
            .on_responder_terminate(move |_: &ResponderEvent| terminate.save())
            .child(
                view()
                    .h(px(3.0))
                    .bg(move || p.raised())
                    .pointer_events(PointerEvents::None)
                    .child(view().h_full().w(fill).bg(rgb(0x3ea6ff))),
            )
            .child(
                view()
                    .absolute()
                    .top(px(10.0))
                    .left(fill)
                    .ml(px(-6.0))
                    .size(px(12.0))
                    .rounded(px(6.0))
                    .bg(rgb(0x3ea6ff))
                    .pointer_events(PointerEvents::None),
            )
    }

    fn color_picker(&self) -> View {
        let p = self.p;
        let color = self.color;
        let hex = self.hex;
        let editor = self.clone();
        let blur = self.clone();
        let hue = computed(move || color.with(|color| color.hue_color.clone()));
        view()
            .pt(px(10.0))
            .child(label(p, "Color"))
            .child(
                self.color_drag(false)
                    .h(px(132.0))
                    .mt(px(8.0))
                    .rounded(px(8.0))
                    .overflow_hidden()
                    .child(
                        image(move || saturation_image(&hue.get()))
                            .absolute()
                            .inset(px(0.0))
                            .w_full()
                            .h_full()
                            .object_fit(ResizeMode::Stretch)
                            .pointer_events(PointerEvents::None),
                    )
                    .child(
                        view()
                            .absolute()
                            .left(move || percentage(color.with(|color| color.saturation as f32)))
                            .top(move || percentage(color.with(|color| 1.0 - color.value as f32)))
                            .mt(px(-8.0))
                            .ml(px(-8.0))
                            .size(px(16.0))
                            .rounded(px(8.0))
                            .border_2()
                            .border_color(rgb(0xffffff))
                            .pointer_events(PointerEvents::None),
                    ),
            )
            .child(
                self.color_drag(true)
                    .h(px(20.0))
                    .mt(px(10.0))
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .child(
                        image(hue_image())
                            .absolute()
                            .inset(px(0.0))
                            .w_full()
                            .h_full()
                            .object_fit(ResizeMode::Stretch)
                            .pointer_events(PointerEvents::None),
                    )
                    .child(
                        view()
                            .absolute()
                            .top(px(0.0))
                            .bottom(px(0.0))
                            .w(px(4.0))
                            .ml(px(-2.0))
                            .rounded(px(2.0))
                            .bg(rgb(0xffffff))
                            .left(move || percentage(color.with(|color| color.hue as f32 / 360.0)))
                            .pointer_events(PointerEvents::None),
                    ),
            )
            .child(
                view()
                    .relative()
                    .mt(px(12.0))
                    .border_1()
                    .border_color(move || p.muted())
                    .rounded(px(4.0))
                    .flex_row()
                    .items_center()
                    .child(
                        text("Hex color")
                            .absolute()
                            .top(px(-9.0))
                            .left(px(12.0))
                            .px(px(4.0))
                            .bg(move || p.background())
                            .text_size(px(12.0))
                            .text_color(move || p.muted()),
                    )
                    .child(
                        input()
                            .label("Subtitle color hex")
                            .value(move || hex.get())
                            .on_change(move |text: &str| {
                                let text = text.chars().take(9).collect::<String>();
                                editor.hex.set(text.clone());
                                editor.update_color(json!({"hex":text}), false);
                                editor.save();
                            })
                            .on_blur(move || {
                                hex.set(color.with_untracked(|color| color.color.clone()));
                                blur.save();
                            })
                            .flex_1()
                            .min_h(px(48.0))
                            .px(px(16.0))
                            .text_size(px(16.0))
                            .text_color(move || p.ink()),
                    )
                    .child(
                        view()
                            .size(px(24.0))
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(move || p.muted())
                            .mr(px(12.0))
                            .bg(move || color.with(|color| hex_color(&color.color))),
                    ),
            )
    }

    fn color_drag(&self, hue: bool) -> View {
        let size = signal((1.0_f32, 1.0_f32));
        let editor = self.clone();
        let color = self.color;
        let drag = Rc::new(move |event: &ResponderEvent| {
            let (width, height) = size.get_untracked();
            let x = f64::from((event.location.x.0 / width).clamp(0.0, 1.0));
            let y = f64::from((event.location.y.0 / height).clamp(0.0, 1.0));
            let old = color.get_untracked();
            let values = if hue {
                json!({"hue":360.0*x,"saturation":old.saturation,"value":old.value})
            } else {
                json!({"hue":old.hue,"saturation":x,"value":1.0-y})
            };
            editor.update_color(values, true);
        });
        let start = drag.clone();
        let release = self.clone();
        let terminate = self.clone();
        view()
            .label(if hue {
                "Subtitle color hue"
            } else {
                "Subtitle saturation and brightness"
            })
            .on_layout(move |bounds: &Bounds<Points>| {
                size.set((bounds.size.width.0.max(1.0), bounds.size.height.0.max(1.0)))
            })
            .claim_touches(TouchClaim::on_start())
            .keep_touches(true)
            .on_responder_grant(move |event: &ResponderEvent| start(event))
            .on_responder_move(move |event: &ResponderEvent| drag(event))
            .on_responder_release(move |_: &ResponderEvent| release.save())
            .on_responder_terminate(move |_: &ResponderEvent| terminate.save())
    }

    fn update_color(&self, values: Value, reset_hex: bool) {
        if let Some(color) = self.preferences.caption_color::<Color>(values) {
            if reset_hex {
                self.hex.set(color.color.clone());
            }
            self.change(|style| style.color = color.color.clone());
            self.color.set(color);
        }
    }
}

fn label(p: Palette, label: &'static str) -> Text {
    text(label)
        .text_size(px(14.0))
        .text_color(move || p.muted())
}

fn action(label: &'static str, click: impl Fn() + 'static) -> View {
    press().px(px(16.0)).py(px(12.0)).on_press(click).child(
        text(label)
            .text_size(px(14.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(0x3ea6ff)),
    )
}

fn hex_color(value: &str) -> Hsla {
    rgb(u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0xffffff)).into()
}

// Use the same color ramps on both hosts, including software-rendered video
// devices that cannot composite the platform gradient layers over a player.
fn saturation_image(hue: &str) -> ImageSrc {
    const SIZE: u32 = 128;
    let color = u32::from_str_radix(hue.trim_start_matches('#'), 16).unwrap_or(0xff0000);
    let channels = [16, 8, 0].map(|shift| (color >> shift) & 255);
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        let value = (SIZE - 1 - y) as f32 / (SIZE - 1) as f32;
        for x in 0..SIZE {
            let saturation = x as f32 / (SIZE - 1) as f32;
            pixels.extend(channels.map(|channel| {
                ((255.0 + (channel as f32 - 255.0) * saturation) * value).round() as u8
            }));
            pixels.push(255);
        }
    }
    ImageSrc::rgba(SIZE, SIZE, pixels)
}

fn hue_image() -> ImageSrc {
    const STEPS: u32 = 48;
    const STOPS: [[u32; 3]; 7] = [
        [255, 0, 0],
        [255, 255, 0],
        [0, 255, 0],
        [0, 255, 255],
        [0, 0, 255],
        [255, 0, 255],
        [255, 0, 0],
    ];
    let mut pixels = Vec::with_capacity((STEPS * 6 * 4) as usize);
    for (start, end) in STOPS.iter().zip(STOPS.iter().skip(1)) {
        for index in 0..STEPS {
            pixels.extend(std::array::from_fn::<_, 3, _>(|channel| {
                ((start[channel] * (STEPS - index) + end[channel] * index) / STEPS) as u8
            }));
            pixels.push(255);
        }
    }
    ImageSrc::rgba(STEPS * 6, 1, pixels)
}
