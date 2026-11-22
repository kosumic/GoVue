use std::collections::HashSet;

use nsh_core::{
    Checked,
    element::{ImageSrc, input},
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    download_library::{DOWNLOAD, format_bytes},
    downloads::{Downloads, Picker, Target, alert},
    ui::{self, Palette, press},
};

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    qualities: Vec<Value>,
    audio_tracks: Vec<Value>,
    subtitles: Vec<Value>,
    bitrates: Vec<u64>,
    preview: Value,
    error: Option<String>,
}

#[derive(Clone)]
struct Confirmation {
    p: Palette,
    downloads: Downloads,
    target: Target,
    catalog: Signal<Option<Value>>,
    config: Signal<Value>,
    options: Signal<Options>,
    selected: Signal<HashSet<String>>,
    loading: Signal<bool>,
    submitting: Signal<bool>,
    retry: Signal<u64>,
    error: Signal<Option<String>>,
}

pub fn download_confirm(
    p: Palette,
    downloads: Downloads,
    width: Signal<f32>,
    height: Signal<f32>,
) -> impl IntoElement {
    dynamic(move || {
        let target = downloads.target.get()?;
        let sheet = Confirmation::new(p, downloads.clone(), target);
        Some(sheet.view(computed(move || width.get() > height.get())))
    })
}

impl Confirmation {
    fn new(p: Palette, downloads: Downloads, target: Target) -> Self {
        let selected = signal(
            target
                .items
                .iter()
                .map(|item| item.video_id.clone())
                .collect(),
        );
        let sheet = Self {
            p,
            downloads,
            target,
            selected,
            catalog: signal(None),
            config: signal(Value::Null),
            options: signal(Options::default()),
            loading: signal(true),
            submitting: signal(false),
            retry: signal(0),
            error: signal(None),
        };
        effect({
            let sheet = sheet.clone();
            move || {
                let revision = sheet.retry.get();
                let sheet = sheet.clone();
                sheet.loading.set(true);
                sheet.error.set(None);
                spawn(async move {
                    let result = sheet.load(revision > 0).await;
                    if sheet.retry.get_untracked() != revision {
                        return;
                    }
                    match result {
                        Ok(catalog) => sheet.catalog.set(catalog),
                        Err(error) => sheet.error.set(Some(error.to_string())),
                    }
                    sheet.loading.set(false);
                });
            }
        });
        effect({
            let sheet = sheet.clone();
            move || {
                let Some(catalog) = sheet.catalog.get() else {
                    return;
                };
                let config = sheet.config.get();
                let sheet = sheet.clone();
                spawn(async move {
                    let result = sheet
                        .downloads
                        .core
                        .execute(json!({
                            "op":"downloads_options","catalog":catalog,"config":config
                        }))
                        .await;
                    if sheet.config.get_untracked() == config {
                        match result {
                            Ok(options) => sheet.options.set(options),
                            Err(error) => {
                                let error: govue_shell_runtime::CoreError = error;
                                sheet.error.set(Some(error.to_string()));
                            }
                        }
                    }
                });
            }
        });
        sheet
    }

    fn batch(&self) -> bool {
        self.target.items.len() > 1
    }

    async fn load(&self, fresh: bool) -> Result<Option<Value>, govue_shell_runtime::CoreError> {
        if self.config.with_untracked(Value::is_null) {
            let prefs = self
                .downloads
                .core
                .execute::<Value>(json!({"op":"downloads_config"}))
                .await?;
            let mut config = prefs["lastConfig"].clone();
            config["videoKey"] = json!("");
            config["fileName"] = json!("");
            if config["qualityHeight"].as_u64().unwrap_or(0) == 0 {
                config["qualityHeight"] = prefs["defaultQuality"].clone();
            }
            self.config.set(config);
        }
        if self.batch() {
            return Ok(None);
        }
        self.downloads
            .core
            .resolve_media(self.target.items[0].video_id.clone(), String::new(), fresh)
            .await
            .map(Some)
    }

    fn update(&self, fields: Value) {
        self.config.update(|config| {
            if let (Some(config), Some(fields)) = (config.as_object_mut(), fields.as_object()) {
                config.extend(fields.clone());
            }
        });
    }

    fn view(&self, landscape: Memo<bool>) -> View {
        let p = self.p;
        let batch = self.batch();
        let target = self.downloads.target;
        let picker = self.downloads.picker;
        let sheet = self.clone();
        let edit = self.clone();
        let config = self.config;
        let loading = self.loading;
        let options = self.options;
        let error = self.error;
        let retry = self.retry;
        let selected = self.selected;
        let more = signal(false);
        let next = self.clone();
        let submitting = self.submitting;
        let available = computed(move || {
            !loading.get()
                && !submitting.get()
                && config.with(|config| config.is_object())
                && selected.with(|items| !items.is_empty())
                && (batch || options.with(|options| options.preview.is_object()))
        });
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(3)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000066))
                    .label("Close download")
                    .on_press(move || target.set(None)),
            )
            .child(
                view()
                    .absolute()
                    .bottom(px(0.0))
                    .left(px(0.0))
                    .right(px(0.0))
                    .max_h(percentage(0.90))
                    .rounded(px(16.0))
                    .bg(move || p.background())
                    .pt(px(12.0))
                    .children(batch.then(|| {
                        view()
                            .flex_row()
                            .px(px(16.0))
                            .items_center()
                            .child(
                                text("Download playlist")
                                    .flex_1()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(p, ui::CLOSE, "Close download", move || {
                                target.set(None)
                            }))
                    }))
                    .child(
                        scroll().flex_shrink(1.0).child(
                            view()
                                .px(px(16.0))
                                .child(dynamic({
                                    let sheet = self.clone();
                                    move || sheet.identity(landscape.get())
                                }))
                                .child(view().flex_row().gap(px(8.0)).py(px(14.0)).children(
                                    [(false, "Video"), (true, "Audio")].map(|(audio, label)| {
                                        let sheet = sheet.clone();
                                        let active = move || {
                                            config.with(|value| {
                                                value["audioOnly"].as_bool() == Some(audio)
                                            })
                                        };
                                        press()
                                            .min_h(px(48.0))
                                            .justify_center()
                                            .on_press(move || {
                                                sheet.update(json!({
                                                    "audioOnly":audio,"attachmentsOnly":false
                                                }))
                                            })
                                            .child(
                                                view()
                                                    .min_h(px(36.0))
                                                    .px(px(12.0))
                                                    .rounded(px(8.0))
                                                    .justify_center()
                                                    .bg(
                                                        move || {
                                                            if active() {
                                                                p.ink()
                                                            } else {
                                                                p.raised()
                                                            }
                                                        },
                                                    )
                                                    .child(
                                                        text(label).text_size(px(14.0)).text_color(
                                                            move || {
                                                                if active() {
                                                                    p.background()
                                                                } else {
                                                                    p.ink()
                                                                }
                                                            },
                                                        ),
                                                    ),
                                            )
                                    }),
                                ))
                                .child(when(loading, move || {
                                    text("Loading formats…")
                                        .py(px(12.0))
                                        .text_color(move || p.muted())
                                }))
                                .children(
                                    [
                                        ("Download quality", Picker::Quality),
                                        ("Audio track", Picker::Audio),
                                        ("Subtitles", Picker::Subtitles),
                                    ]
                                    .map(|(label, key)| {
                                        let sheet = self.clone();
                                        let disabled = move || {
                                            loading.get()
                                                || key != Picker::Subtitles
                                                    && config.with(|value| {
                                                        value["attachmentsOnly"] == true
                                                    })
                                        };
                                        selection_row(p, label, move || sheet.label(key))
                                            .on_press(move || {
                                                if !disabled() {
                                                    picker.set(Some(key));
                                                }
                                            })
                                            .opacity(move || if disabled() { 0.4_f32 } else { 1.0 })
                                    }),
                                )
                                .child(
                                    view()
                                        .h(px(52.0))
                                        .flex_row()
                                        .items_center()
                                        .child(
                                            text("Size")
                                                .flex_1()
                                                .text_size(px(14.0))
                                                .text_color(move || p.ink()),
                                        )
                                        .child(
                                            text(move || {
                                                if batch {
                                                    "Calculated for each video".into()
                                                } else {
                                                    options.with(|options| {
                                                        format_bytes(
                                                            options.preview["bytes"]
                                                                .as_u64()
                                                                .unwrap_or(0),
                                                        )
                                                    })
                                                }
                                            })
                                            .text_size(px(13.0))
                                            .text_color(move || p.muted()),
                                        ),
                                )
                                .child(when(
                                    move || {
                                        error.with(Option::is_some)
                                            || options.with(|options| options.error.is_some())
                                    },
                                    move || {
                                        view()
                                            .child(
                                                text(move || {
                                                    error
                                                        .get()
                                                        .or_else(|| {
                                                            options.with(|options| {
                                                                options.error.clone()
                                                            })
                                                        })
                                                        .unwrap_or_default()
                                                })
                                                .text_size(px(13.0))
                                                .text_color(rgb(0xcc3333)),
                                            )
                                            .child(
                                                selection_row(p, "Retry", String::new)
                                                    .on_press(move || retry.update(|id| *id += 1)),
                                            )
                                    },
                                ))
                                .child(
                                    selection_row(p, "More options", String::new)
                                        .on_press(move || more.update(|more| *more = !*more)),
                                )
                                .child(when(more, move || edit.advanced()))
                                .children(batch.then(|| self.batch_items())),
                        ),
                    )
                    .child(
                        view().px(px(16.0)).py(px(8.0)).child(
                            press()
                                .min_h(px(48.0))
                                .rounded(px(20.0))
                                .bg(move || p.ink())
                                .flex_row()
                                .items_center()
                                .justify_center()
                                .gap(px(8.0))
                                .label(move || {
                                    if batch {
                                        format!("Download {} videos", selected.with(HashSet::len))
                                    } else {
                                        "Start download".into()
                                    }
                                })
                                .accessibility_state(move || AccessibilityState {
                                    disabled: !available.get(),
                                    ..Default::default()
                                })
                                .opacity(move || if available.get() { 1.0_f32 } else { 0.4 })
                                .on_press(move || {
                                    if available.get_untracked() {
                                        next.submit();
                                    }
                                })
                                .child(ui::icon(DOWNLOAD, 18.0, move || p.background()))
                                .child(
                                    text(move || {
                                        if batch {
                                            format!(
                                                "Download {} videos",
                                                selected.with(HashSet::len)
                                            )
                                        } else if submitting.get() {
                                            "Downloading…".into()
                                        } else {
                                            "Download".into()
                                        }
                                    })
                                    .text_size(px(14.0))
                                    .text_color(move || p.background()),
                                ),
                        ),
                    ),
            )
            .child(when(move || picker.with(Option::is_some), {
                let sheet = self.clone();
                move || {
                    let sheet = sheet.clone();
                    dynamic(move || picker.get().map(|key| sheet.picker(key)))
                }
            }))
    }

    fn identity(&self, landscape: bool) -> View {
        let p = self.p;
        let batch = self.batch();
        let catalog = self.catalog;
        let title = self.target.items[0].title.clone();
        let author = self.target.items[0].author.clone();
        let name = self.target.name.clone();
        let count = self.target.items.len();
        let row = view().gap(px(12.0));
        let row = if landscape {
            row.flex_row().items_start()
        } else {
            row
        };
        row.children((!batch).then(|| {
            image(ImageSrc::from(format!(
                "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                self.target.items[0].video_id
            )))
            .w(percentage(if landscape { 0.42 } else { 1.0 }))
            .aspect_ratio(16.0 / 9.0)
            .rounded(px(12.0))
            .bg(move || p.raised())
        }))
        .child(
            view()
                .flex_1()
                .child(
                    text(move || {
                        if batch {
                            name.clone()
                        } else {
                            catalog.with(|catalog| field(catalog.as_ref(), "title", &title))
                        }
                    })
                    .text_size(px(20.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || p.ink()),
                )
                .child(
                    text(move || {
                        if batch {
                            format!("{count} videos")
                        } else {
                            catalog.with(|catalog| field(catalog.as_ref(), "author", &author))
                        }
                    })
                    .text_size(px(13.0))
                    .text_color(move || p.muted()),
                ),
        )
    }

    fn label(&self, picker: Picker) -> String {
        let config = self.config.get();
        let options = self.options.get();
        match picker {
            Picker::Quality if config["audioOnly"] == true => config["audioBitrate"]
                .as_u64()
                .filter(|n| *n > 0)
                .map(|n| format!("{} kbps", n.div_ceil(1000)))
                .unwrap_or_else(|| "Auto".into()),
            Picker::Quality if self.batch() => {
                format!("{}p", config["qualityHeight"].as_u64().unwrap_or(1080))
            }
            Picker::Quality => {
                let height = config["qualityHeight"]
                    .as_u64()
                    .filter(|height| *height != 0)
                    .unwrap_or(8640);
                let quality = options
                    .qualities
                    .iter()
                    .find(|quality| quality["key"] == config["videoKey"])
                    .or_else(|| {
                        options.qualities.iter().find(|quality| {
                            quality["height"]
                                .as_u64()
                                .is_some_and(|found| found <= height)
                        })
                    })
                    .or_else(|| options.qualities.last());
                field(quality, "label", "—")
            }
            Picker::Audio => field(
                options
                    .audio_tracks
                    .iter()
                    .find(|option| option["key"] == config["audioKey"]),
                "label",
                "Default",
            ),
            Picker::Subtitles if config["includeSubtitle"] != true => "Off".into(),
            Picker::Subtitles if self.batch() => {
                let track = config["subtitleKey"].as_str().unwrap_or_default();
                LANGUAGES
                    .iter()
                    .find_map(|(key, label)| (*key == track).then_some((*label).to_owned()))
                    .unwrap_or_else(|| "Default".into())
            }
            Picker::Subtitles => field(
                options.subtitles.iter().find(|option| {
                    option["id"] == config["subtitleKey"]
                        || option["language"] == config["subtitleKey"]
                }),
                "label",
                "Default",
            ),
        }
    }

    fn advanced(&self) -> View {
        let p = self.p;
        let config = self.config;
        let cover = self.clone();
        let attachments = self.clone();
        let filename = self.clone();
        view()
            .child(download_switch(
                p,
                "Cover image",
                move || config.with(|c| c["includeCover"] == true),
                move |enabled| cover.update(json!({"includeCover":enabled})),
            ))
            .child(
                input()
                    .label("Download filename")
                    .placeholder(if self.batch() {
                        "Filename template"
                    } else {
                        "Filename (use video title)"
                    })
                    .value(move || config.with(|c| c["fileName"].as_str().unwrap_or("").to_owned()))
                    .on_change(move |value: &str| {
                        filename
                            .update(json!({"fileName":value.chars().take(180).collect::<String>()}))
                    })
                    .px(px(12.0))
                    .h(px(44.0))
                    .rounded(px(8.0))
                    .border(px(1.0))
                    .border_color(move || p.muted())
                    .text_color(move || p.ink())
                    .text_size(px(14.0)),
            )
            .child(download_switch(
                p,
                "Attachments only",
                move || config.with(|c| c["attachmentsOnly"] == true),
                move |enabled| {
                    attachments.update(json!({"attachmentsOnly":enabled,"audioOnly":false}))
                },
            ))
    }

    fn batch_items(&self) -> View {
        let p = self.p;
        let items = self.target.items.clone();
        let all = self.target.items.len();
        let selected = self.selected;
        view()
            .child(
                view()
                    .flex_row()
                    .h(px(52.0))
                    .items_center()
                    .child(
                        text(move || format!("Videos · {} selected", selected.with(HashSet::len)))
                            .flex_1()
                            .text_size(px(14.0))
                            .text_color(move || p.ink()),
                    )
                    .child(
                        press()
                            .on_press(move || {
                                selected.set(if selected.with_untracked(HashSet::len) == all {
                                    HashSet::new()
                                } else {
                                    items.iter().map(|item| item.video_id.clone()).collect()
                                });
                            })
                            .child(
                                text(move || {
                                    if selected.with(HashSet::len) == all {
                                        "Deselect all"
                                    } else {
                                        "Select all"
                                    }
                                })
                                .text_size(px(13.0))
                                .text_color(move || p.muted()),
                            ),
                    ),
            )
            .children(
                self.target
                    .items
                    .iter()
                    .map(|item| {
                        let id = item.video_id.clone();
                        let target = id.clone();
                        selection_row(p, item.title.clone(), {
                            let author = item.author.clone();
                            move || author.clone()
                        })
                        .accessibility_state(move || AccessibilityState {
                            checked: Some(if selected.with(|items| items.contains(&target)) {
                                Checked::True
                            } else {
                                Checked::False
                            }),
                            ..Default::default()
                        })
                        .on_press(move || {
                            selected.update(|items| {
                                if !items.remove(&id) {
                                    items.insert(id.clone());
                                }
                            })
                        })
                    })
                    .collect::<Vec<_>>(),
            )
    }

    fn picker(&self, picker: Picker) -> View {
        let p = self.p;
        let close = self.downloads.picker;
        let title = match picker {
            Picker::Quality => "Download quality",
            Picker::Audio => "Audio track",
            Picker::Subtitles => "Subtitles",
        };
        let selected = self.label(picker);
        view()
            .absolute()
            .inset(px(0.0))
            .items_center()
            .justify_center()
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000066))
                    .label("Close selection")
                    .on_press(move || close.set(None)),
            )
            .child(
                view()
                    .w(percentage(0.88))
                    .max_w(px(380.0))
                    .max_h(px(420.0))
                    .rounded(px(16.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .h(px(52.0))
                            .flex_row()
                            .items_center()
                            .px(px(16.0))
                            .child(
                                text(title)
                                    .flex_1()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(
                                p,
                                ui::CLOSE,
                                "Close selection",
                                move || close.set(None),
                            )),
                    )
                    .child(
                        scroll().flex_shrink(1.0).child(
                            view().px(px(16.0)).children(
                                self.choices(picker)
                                    .into_iter()
                                    .map(|(label, fields)| {
                                        let sheet = self.clone();
                                        let checked = label == selected;
                                        selection_row(p, label, move || {
                                            if checked { "✓".into() } else { String::new() }
                                        })
                                        .on_press(
                                            move || {
                                                sheet.update(fields.clone());
                                                close.set(None);
                                            },
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                        ),
                    ),
            )
    }

    fn choices(&self, picker: Picker) -> Vec<(String, Value)> {
        let config = self.config.get();
        let options = self.options.get();
        let batch = self.batch();
        match picker {
            Picker::Quality if config["audioOnly"] == true => {
                let bitrates = if batch {vec![128_000, 96_000, 48_000]} else {options.bitrates};
                std::iter::once(("Auto".into(),json!({"audioBitrate":0})))
                    .chain(bitrates.into_iter().map(|rate|(format!("{} kbps",rate.div_ceil(1000)),json!({"audioBitrate":rate}))))
                    .collect()
            }
            Picker::Quality if batch => [4320,2160,1440,1080,720,480,360,240,144].into_iter()
                .map(|height|(format!("{height}p"),json!({"qualityHeight":height,"videoKey":"","qualityFps":0}))).collect(),
            Picker::Quality => options.qualities.into_iter().map(|quality| (
                field(Some(&quality),"label","Auto"),
                json!({"qualityHeight":quality["height"],"videoKey":quality["key"],"qualityFps":quality["fps"].as_u64().unwrap_or(0)}),
            )).collect(),
            Picker::Audio => std::iter::once(("Default".into(),json!({"audioKey":""})))
                .chain(options.audio_tracks.into_iter().map(|track|(field(Some(&track),"label","Audio"),json!({"audioKey":track["key"]})))).collect(),
            Picker::Subtitles => {
                let tracks: Vec<_> = if batch {
                    LANGUAGES.iter().map(|(key,label)| ((*label).to_owned(),json!({"includeSubtitle":true,"subtitleKey":key}))).collect()
                } else {
                    options.subtitles.into_iter().map(|track|(field(Some(&track),"label","Subtitles"),
                        json!({"includeSubtitle":true,"subtitleKey":track["id"]}))).collect()
                };
                std::iter::once(("Off".into(),json!({"includeSubtitle":false}))).chain(tracks).collect()
            }
        }
    }

    fn submit(&self) {
        self.submitting.set(true);
        let selected = self.selected.get_untracked();
        let items = self
            .target
            .items
            .iter()
            .filter(|item| selected.contains(&item.video_id))
            .cloned()
            .collect();
        let config = self.config.get_untracked();
        let name = self.target.name.clone();
        let downloads = self.downloads.clone();
        let submitting = self.submitting;
        spawn(async move {
            if let Err(error) = downloads.enqueue(items, &name, config).await {
                submitting.set(false);
                alert(error.to_string()).await;
            }
        });
    }
}

fn field(value: Option<&Value>, key: &str, fallback: &str) -> String {
    value
        .and_then(|value| value[key].as_str())
        .filter(|text| !text.is_empty())
        .unwrap_or(fallback)
        .into()
}

fn selection_row(
    p: Palette,
    label: impl Into<SharedString>,
    value: impl Fn() -> String + 'static,
) -> View {
    press()
        .min_h(px(52.0))
        .gap(px(16.0))
        .flex_row()
        .items_center()
        .child(
            text(label.into())
                .flex_1()
                .text_size(px(14.0))
                .text_color(move || p.ink()),
        )
        .child(
            text(value)
                .max_w(percentage(0.55))
                .text_size(px(13.0))
                .text_color(move || p.muted()),
        )
        .child(ui::icon(ui::NEXT, 18.0, move || p.muted()))
}

fn download_switch(
    p: Palette,
    label: &'static str,
    enabled: impl Fn() -> bool + Copy + 'static,
    change: impl Fn(bool) + 'static,
) -> View {
    press()
        .h(px(52.0))
        .flex_row()
        .items_center()
        .role(AccessibilityRole::Switch)
        .label(label)
        .accessibility_state(move || AccessibilityState {
            checked: Some(if enabled() {
                Checked::True
            } else {
                Checked::False
            }),
            ..Default::default()
        })
        .on_press(move || change(!enabled()))
        .child(
            text(label)
                .flex_1()
                .text_size(px(14.0))
                .text_color(move || p.ink()),
        )
        .child(
            view()
                .relative()
                .w(px(42.0))
                .h(px(24.0))
                .rounded(px(12.0))
                .bg(move || if enabled() { p.ink() } else { p.muted() })
                .child(
                    view()
                        .absolute()
                        .top(px(3.0))
                        .left(move || px(if enabled() { 21.0 } else { 3.0 }))
                        .size(px(18.0))
                        .rounded(px(9.0))
                        .bg(move || p.background()),
                ),
        )
}

const LANGUAGES: [(&str, &str); 13] = [
    ("en", "English"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("zh-Hant", "Chinese (Traditional)"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
    ("hi", "Hindi"),
    ("ar", "Arabic"),
];
