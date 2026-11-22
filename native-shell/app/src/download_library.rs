use std::rc::Rc;

use govue_shell_runtime::WatchPlayback;
use nsh_core::{Checked, PressEvent, element::ImageSrc, prelude::*};
use serde_json::{Value, json};

use crate::{
    downloads::{Artifact, Batch, Destination, Downloads, Entry},
    ui::{self, Palette, press},
};

pub const DOWNLOAD: &str = "M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5";
const SETTINGS: &str = "M12 2v3m0 14v3M2 12h3m14 0h3M19 12a7 7 0 1 1-14 0 7 7 0 0 1 14 0zm-4 0a3 3 0 1 1-6 0 3 3 0 0 1 6 0z";
const BACK: &str = "m12 19-7-7 7-7M5 12h14";
const MORE: &str = "M12 4v1m0 6v1m0 6v1";
const PAUSE: &str = "M8 4h2v16H8zM15 4h2v16h-2z";

#[derive(Clone)]
pub struct DownloadLibrary {
    downloads: Downloads,
    watch: WatchPlayback,
    palette: Palette,
    start: Rc<dyn Fn()>,
    open: Rc<dyn Fn(String)>,
}

impl DownloadLibrary {
    pub fn new(
        downloads: Downloads,
        watch: WatchPlayback,
        palette: Palette,
        start: impl Fn() + 'static,
        open: impl Fn(String) + 'static,
    ) -> Self {
        Self {
            downloads,
            watch,
            palette,
            start: Rc::new(start),
            open: Rc::new(open),
        }
    }

    pub fn view(&self) -> If {
        let screen = self.downloads.destination;
        let library = self.clone();
        when(
            move || screen.with(Option::is_some),
            move || library.panel(),
        )
    }

    fn panel(&self) -> View {
        let p = self.palette;
        let screen = self.downloads.destination;
        let back = self.downloads.clone();
        let selected = self.downloads.selected;
        let library = self.clone();
        let details = self.clone();
        let preferences = self.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .bg(move || p.background())
            .child(
                view()
                    .flex_row()
                    .h(px(48.0))
                    .px(px(8.0))
                    .items_center()
                    .child(ui::icon_button(p, BACK, "Back from downloads", move || {
                        back.back();
                    }))
                    .child(
                        text(move || match screen.get() {
                            Some(Destination::Settings) => "Background & downloads",
                            Some(Destination::History) => "History",
                            Some(Destination::Batch(_)) => "Batch details",
                            _ => "Downloads",
                        })
                        .flex_1()
                        .pl(px(8.0))
                        .text_size(px(20.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || p.ink()),
                    )
                    .child(when(
                        move || screen.get() == Some(Destination::Library),
                        move || {
                            ui::icon_button(p, SETTINGS, "Download settings", move || {
                                screen.set(Some(Destination::Settings))
                            })
                        },
                    )),
            )
            .child(when(
                move || screen.get() == Some(Destination::Settings),
                move || preferences.settings(),
            ))
            .child(when(
                move || screen.get() != Some(Destination::Settings),
                move || library.list(),
            ))
            .child(when(
                move || selected.with(Option::is_some),
                move || {
                    let details = details.clone();
                    dynamic(move || {
                        details
                            .downloads
                            .active_entry()
                            .map(|entry| details.details(entry))
                    })
                },
            ))
    }

    fn list(&self) -> View {
        let p = self.palette;
        let destination = self.downloads.destination;
        let filter = self.downloads.filter;
        let listing = self.downloads.listing;
        let state = self.downloads.state;
        let error = self.downloads.error;
        let rows = self.clone();
        let header = self.clone();
        view()
            .flex_1()
            .child(dynamic(move || match destination.get() {
                Some(Destination::History) => {
                    let downloads = header.downloads.clone();
                    row_action(p, "Clear history".into(), move || {
                        let ids = downloads.state.with_untracked(|state| {
                            state
                                .entries
                                .iter()
                                .filter(|entry| {
                                    matches!(
                                        entry.state.as_str(),
                                        "complete" | "failed" | "cancelled" | "skipped"
                                    )
                                })
                                .map(|entry| entry.id.clone())
                                .collect()
                        });
                        downloads.remove(
                            "Clear history?",
                            "Remove completed, failed and cancelled download records?".into(),
                            ids,
                        );
                    })
                }
                Some(Destination::Batch(_)) => listing.with(|listing| {
                    listing
                        .batch
                        .as_ref()
                        .map(|batch| header.batch(batch.clone()))
                        .unwrap_or_else(view)
                }),
                _ => view()
                    .flex_row()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .pt(px(4.0))
                    .pb(px(12.0))
                    .children(
                        [
                            ("all", "All"),
                            ("active", "In progress"),
                            ("complete", "Completed"),
                        ]
                        .map(|(key, title)| {
                            chip(
                                p,
                                title,
                                move || filter.get() == key,
                                move || filter.set(key),
                            )
                        }),
                    ),
            }))
            .child(divider())
            .child(when(
                move || error.with(Option::is_some),
                move || {
                    view().p(px(16.0)).child(
                        text(move || error.get().unwrap_or_default()).text_color(move || p.muted()),
                    )
                },
            ))
            .child(when(
                move || error.with(Option::is_none) && !state.with(|state| state.ready),
                move || {
                    view()
                        .p(px(16.0))
                        .items_center()
                        .child(text("Loading…").text_color(move || p.muted()))
                },
            ))
            .child(when(
                move || {
                    state.with(|state| state.ready) && listing.with(|view| view.items.is_empty())
                },
                move || {
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap(px(12.0))
                        .p(px(24.0))
                        .child(ui::icon(DOWNLOAD, 44.0, || rgb(0x666666).into()))
                        .child(help(p, "No downloads yet".into()))
                },
            ))
            .child(when(
                move || {
                    state.with(|state| state.ready) && listing.with(|view| !view.items.is_empty())
                },
                move || {
                    let rows = rows.clone();
                    scroll().flex_1().child(each(
                        move || listing.with(|view| view.items.clone()),
                        |entry| entry.id.clone(),
                        move |entry, _| {
                            let rows = rows.clone();
                            dynamic(move || rows.entry(entry.get()))
                        },
                    ))
                },
            ))
    }

    fn entry(&self, entry: Entry) -> View {
        let p = self.palette;
        let selected = self.downloads.selected;
        let id = entry.id.clone();
        let details = entry.id.clone();
        let action = self.clone();
        let pausable = entry.pausable();
        let progress = entry.total > 0 && entry.state != "complete";
        let downloading = self.downloads.state.with(|state| {
            entry.state == "queued" && state.preferences["wifiOnly"] == true
                || entry.state == "downloading" && state.waiting_for_wifi.contains(&entry.id)
        });
        let label = if downloading {
            "Waiting for Wi-Fi".to_owned()
        } else if entry.state == "complete" {
            "Completed".to_owned()
        } else {
            let mut chars = entry.state.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        };
        let label = if entry.label.is_empty() {
            label
        } else {
            format!("{label} · {}", entry.label)
        };
        let play = entry.clone();
        let commands = self.downloads.clone();
        let pause_id = entry.id.clone();
        view()
            .child(
                view()
                    .flex_row()
                    .gap(px(12.0))
                    .p(px(16.0))
                    .child(
                        press()
                            .label(format!("Details for {}", entry.title))
                            .on_press(move || selected.set(Some(id.clone())))
                            .child(
                                image(ImageSrc::from(format!(
                                    "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                                    entry.video_id
                                )))
                                .w(px(112.0))
                                .h(px(63.0))
                                .rounded(px(8.0))
                                .bg(move || p.raised()),
                            ),
                    )
                    .child(
                        view()
                            .flex_1()
                            .child(
                                press()
                                    .on_press(move || {
                                        if play.watch_page_downloaded {
                                            action.play(play.clone());
                                        } else {
                                            selected.set(Some(play.id.clone()));
                                        }
                                    })
                                    .child(
                                        text(entry.title.clone())
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(move || p.ink()),
                                    ),
                            )
                            .child(help(p, entry.author))
                            .child(help(p, label))
                            .children(progress.then(|| {
                                view()
                                    .child(progress_bar(
                                        p,
                                        entry.received as f32 / entry.total as f32,
                                    ))
                                    .child(help(
                                        p,
                                        format!(
                                            "{} / {}",
                                            format_bytes(entry.received),
                                            format_bytes(entry.total)
                                        ),
                                    ))
                            }))
                            .children(entry.error.map(|error| {
                                text(error)
                                    .mt(px(4.0))
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xcc3333))
                            })),
                    )
                    .child(
                        view()
                            .children(pausable.then(|| {
                                let paused = entry.state == "paused";
                                ui::icon_button(
                                    p,
                                    if paused { ui::PLAY } else { PAUSE },
                                    if paused {
                                        "Resume download"
                                    } else {
                                        "Pause download"
                                    },
                                    move || {
                                        commands.run(
                                            json!({"op":"downloads_runner_pause","id":pause_id}),
                                        );
                                    },
                                )
                            }))
                            .child(
                                press()
                                    .size(px(40.0))
                                    .items_center()
                                    .justify_center()
                                    .label(format!("Download options for {}", entry.title))
                                    .on_press(move || selected.set(Some(details.clone())))
                                    .child(ui::icon(MORE, 22.0, move || p.ink())),
                            ),
                    ),
            )
            .child(divider())
    }

    fn play(&self, entry: Entry) {
        let start = self.start.clone();
        self.downloads
            .play(entry, self.watch.clone(), move || start());
    }

    fn details(&self, entry: Entry) -> View {
        let p = self.palette;
        let selected = self.downloads.selected;
        let screen = self.downloads.destination;
        let play = entry.clone();
        let action = self.clone();
        let download = self.downloads.clone();
        let remove = self.downloads.clone();
        let id = entry.id.clone();
        let artifacts = if entry.artifacts.is_empty() {
            vec![Artifact {
                file: entry.file.clone(),
                label: entry.label.clone(),
                mime_type: entry.mime_type.clone(),
            }]
        } else {
            entry.artifacts.clone()
        };
        let active = entry.pausable() || entry.state == "combining";
        let pausable = entry.pausable();
        let resume = entry.state == "paused";
        let title = entry.title.clone();
        let batch = entry.batch_id.and_then(|batch| {
            self.downloads
                .state
                .with(|state| {
                    state
                        .entries
                        .iter()
                        .filter(|entry| entry.batch_id.as_ref() == Some(&batch))
                        .count()
                        > 1
                })
                .then_some(batch)
        });
        let open = self.open.clone();
        let library = self.downloads.clone();
        let video = entry.video_id.clone();
        let clipboard = video.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000066))
                    .label("Close download details")
                    .on_press(move || selected.set(None)),
            )
            .child(
                view()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .max_h(percentage(0.75))
                    .rounded(px(16.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .px(px(16.0))
                            .h(px(48.0))
                            .child(
                                text(entry.title.clone())
                                    .flex_1()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(
                                p,
                                ui::CLOSE,
                                "Close download details",
                                move || selected.set(None),
                            )),
                    )
                    .child(
                        scroll().child(
                            view()
                                .child(view().p(px(16.0)).child(help(
                                    p,
                                    format!(
                                        "{}\n{} · {}\n{} / {}",
                                        entry.author,
                                        entry.label,
                                        entry.state,
                                        format_bytes(entry.received),
                                        format_bytes(entry.total)
                                    ),
                                )))
                                .children(entry.watch_page_downloaded.then(|| {
                                    row_action(p, "Play".into(), move || action.play(play.clone()))
                                }))
                                .children((entry.state == "complete").then(|| {
                                    view().children(
                                        artifacts
                                            .into_iter()
                                            .map(|artifact| {
                                                view().children(
                                                    [("Save", true), ("Share", false)].map(
                                                        |(action, save)| {
                                                            let downloads = self.downloads.clone();
                                                            let id = entry.id.clone();
                                                            let title = entry.title.clone();
                                                            let artifact = artifact.clone();
                                                            let label = if artifact.label.is_empty()
                                                            {
                                                                "file"
                                                            } else {
                                                                &artifact.label
                                                            };
                                                            row_action(
                                                                p,
                                                                format!("{action} {label}"),
                                                                move || {
                                                                    downloads.export(
                                                                        id.clone(),
                                                                        artifact.clone(),
                                                                        title.clone(),
                                                                        save,
                                                                    );
                                                                },
                                                            )
                                                        },
                                                    ),
                                                )
                                            })
                                            .collect::<Vec<_>>(),
                                    )
                                }))
                                .children(pausable.then(|| {
                                    let downloads = download.clone();
                                    let id = id.clone();
                                    row_action(
                                        p,
                                        if resume { "Resume" } else { "Pause" }.into(),
                                        move || {
                                            selected.set(None);
                                            downloads.run(
                                                json!({"op":"downloads_runner_pause","id":id}),
                                            );
                                        },
                                    )
                                }))
                                .child(row_action(
                                    p,
                                    if active {
                                        "Cancel download"
                                    } else {
                                        "Download again"
                                    }
                                    .into(),
                                    {
                                        let id = id.clone();
                                        move || {
                                            selected.set(None);
                                            let op = if active {
                                                "downloads_runner_cancel"
                                            } else {
                                                "downloads_runner_retry"
                                            };
                                            download.run(json!({"op":op,"id":id}));
                                        }
                                    },
                                ))
                                .children((!active).then(|| {
                                    row_action(p, "Remove".into(), move || {
                                        remove.remove(
                                            "Remove download?",
                                            title.clone(),
                                            vec![id.clone()],
                                        );
                                    })
                                }))
                                .children(batch.map(|batch| {
                                    row_action(p, "View batch".into(), move || {
                                        selected.set(None);
                                        screen.set(Some(Destination::Batch(batch.clone())));
                                    })
                                }))
                                .child(row_action(p, "Copy video ID".into(), move || {
                                    selected.set(None);
                                    nsh_core::element::set_clipboard(clipboard.clone());
                                }))
                                .child(row_action(p, "Open on YouTube".into(), move || {
                                    library.close();
                                    open(format!("https://m.youtube.com/watch?v={video}"));
                                })),
                        ),
                    ),
            )
    }

    fn batch(&self, batch: Batch) -> View {
        let p = self.palette;
        let downloads = self.downloads.clone();
        let description = if batch.failed > 0 {
            format!(
                "{} of {} completed · {} failed",
                batch.completed, batch.total, batch.failed
            )
        } else {
            format!("{} of {} completed", batch.completed, batch.total)
        };
        view().px(px(16.0)).py(px(12.0)).gap(px(8.0))
            .child(text(batch.name).text_size(px(14.0)).text_color(move || p.ink()))
            .child(help(p, description))
            .child(progress_bar(p, batch.completed as f32 / batch.total.max(1) as f32))
            .child(view().flex_row().flex_wrap().gap(px(8.0))
                .children([(batch.pause_ids, true, "Pause all"), (batch.resume_ids, false, "Resume all")]
                    .into_iter().filter(|(ids, _, _)| !ids.is_empty()).map(|(ids, paused, label)| {
                        let downloads = downloads.clone();
                        chip(p, label, || false, move || {
                            for id in &ids {
                                downloads.run(json!({"op":"downloads_runner_pause","id":id,"paused":paused}));
                            }
                        })
                    }).collect::<Vec<_>>())
                .child(chip(p, "Remove batch", || false, move || {
                    downloads.remove("Remove batch?", "Remove these downloads from the manager?".into(), batch.ids.clone());
                })))
    }

    fn settings(&self) -> View {
        let p = self.palette;
        let state = self.downloads.state;
        let screen = self.downloads.destination;
        let wifi = computed(move || state.with(|state| state.preferences["wifiOnly"] == true));
        let connections = computed(move || {
            state.with(|state| state.preferences["maxConnections"].as_u64().unwrap_or(4))
        });
        let quality = computed(move || {
            state.with(|state| state.preferences["defaultQuality"].as_u64().unwrap_or(1080))
        });
        let update = self.downloads.clone();
        let set_quality = self.downloads.clone();
        let set_connections = self.downloads.clone();
        let width = signal(1.0_f32);
        view().flex_1().child(scroll().flex_1().child(view().px(px(16.0)).pb(px(16.0))
            .child(press().h(px(48.0)).flex_row().items_center()
                .role(AccessibilityRole::Switch).label("Download over Wi-Fi only")
                .accessibility_state(move || AccessibilityState {
                    checked: Some(if wifi.get() {Checked::True} else {Checked::False}),
                    ..Default::default()
                }).on_press(move || update.run(json!({"op":"downloads_config","settings":{"wifiOnly":!wifi.get_untracked()}})))
                .child(text("Download over Wi-Fi only").flex_1().text_size(px(14.0)).text_color(move || p.ink()))
                .child(view().relative().w(px(42.0)).h(px(24.0)).rounded(px(12.0))
                    .bg(move || if wifi.get() {p.ink()} else {p.muted()})
                    .child(view().absolute().top(px(3.0)).left(move || px(if wifi.get(){21.0}else{3.0}))
                        .size(px(18.0)).rounded(px(9.0)).bg(move || p.background()))))
            .child(divider())
            .child(text("Download quality").mt(px(16.0)).text_size(px(14.0)).text_color(move || p.ink()))
            .child(help(p, "Higher quality uses more data and storage".into()))
            .children([144, 240, 360, 480, 720, 1080, 1440, 2160].map(|height| {
                let set_quality = set_quality.clone();
                press().h(px(48.0)).px(px(14.0)).gap(px(16.0)).flex_row().items_center()
                    .label(format!("{height}p"))
                    .on_press(move || {
                        let mut config = state.with_untracked(|s| s.preferences["lastConfig"].clone());
                        config["qualityHeight"] = json!(height);
                        config["videoKey"] = Value::String(String::new());
                        set_quality.run(json!({"op":"downloads_config","settings":{"defaultQuality":height,"lastConfig":config}}));
                    })
                    .child(view().size(px(20.0)).rounded(px(10.0)).border_2()
                        .border_color(move || if quality.get() == height {rgb(0x3ea6ff).into()} else {p.muted()})
                        .items_center().justify_center()
                        .child(when(move || quality.get() == height, || view().size(px(10.0)).rounded(px(5.0)).bg(rgb(0x3ea6ff)))))
                    .child(text(format!("{height}p")).text_size(px(14.0)).text_color(move || p.ink()))
            }))
            .child(divider())
            .child(text(move || format!("Connections: {}",connections.get()))
                .mt(px(16.0)).text_size(px(14.0)).text_color(move || p.ink()))
            .child(press().h(px(40.0)).justify_center().label("Connections per download")
                .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0.max(1.0)))
                .on_press(move |event: &PressEvent| {
                    let value = (1.0 + 15.0 * event.position.x.0 / width.get_untracked()).clamp(1.0, 16.0).round() as u64;
                    set_connections.run(json!({"op":"downloads_config","settings":{"maxConnections":value}}));
                }).child(dynamic(move || progress_bar(p, (connections.get() - 1) as f32 / 15.0))))
            .child(divider())
            .child(row_action(p, "History".into(), move || screen.set(Some(Destination::History))))
            .child(help(p, "Delete downloads".into()))))
    }
}

fn divider() -> View {
    view().h(px(0.5)).bg(rgba(0x42424266))
}

fn row_action(p: Palette, title: String, action: impl Fn() + 'static) -> View {
    press()
        .min_h(px(48.0))
        .px(px(16.0))
        .justify_center()
        .on_press(action)
        .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
}

fn chip(
    p: Palette,
    title: &'static str,
    selected: impl Fn() -> bool + Copy + 'static,
    action: impl Fn() + 'static,
) -> View {
    press()
        .min_h(px(32.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .justify_center()
        .on_press(action)
        .bg(move || if selected() { p.ink() } else { p.raised() })
        .child(
            text(title)
                .text_size(px(13.0))
                .text_color(move || if selected() { p.background() } else { p.ink() }),
        )
}

fn help(p: Palette, value: String) -> Text {
    text(value)
        .mt(px(4.0))
        .text_size(px(12.0))
        .text_color(move || p.muted())
}

fn progress_bar(p: Palette, value: f32) -> View {
    view()
        .mt(px(8.0))
        .h(px(3.0))
        .rounded(px(2.0))
        .bg(move || p.raised())
        .child(
            view()
                .h_full()
                .w(percentage(value.clamp(0.0, 1.0)))
                .rounded(px(2.0))
                .bg(move || p.ink()),
        )
}

pub fn format_bytes(number: u64) -> String {
    match number {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
        n if n > 0 => format!("{} KB", n.div_ceil(1024)),
        _ => "—".into(),
    }
}
