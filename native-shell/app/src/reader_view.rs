use std::rc::Rc;

use govue_shell_runtime::{ReaderResults, ReaderSnapshot, TranscriptReader, TranscriptTrack};
use nsh_core::{element::input, prelude::*, share::ShareRequest};

use crate::ui::{self, Palette, press};

const REFRESH: &str = "M20 11a8 8 0 1 0-2.3 6m2.3-13v7h-7";
const SHARE: &str = "M8.7 10.4l6.7-3.8M8.7 13.6l6.7 3.8M9 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0zm12-7a3 3 0 1 1-6 0 3 3 0 0 1 6 0zm0 14a3 3 0 1 1-6 0 3 3 0 0 1 6 0z";

#[derive(Clone)]
pub struct ReaderView {
    reader: TranscriptReader,
    palette: Palette,
    query: Signal<String>,
    search: Signal<bool>,
    error: Signal<Option<String>>,
    documents: Memo<Option<Rc<ReaderSnapshot>>>,
    results: Signal<Option<Rc<ReaderResults>>>,
    seek: Rc<dyn Fn(f64)>,
}

impl ReaderView {
    pub fn new(reader: TranscriptReader, palette: Palette) -> Self {
        let documents = computed({
            let reader = reader.clone();
            move || reader.snapshot().filter(|_| reader.visible())
        });
        let panel = Self {
            reader,
            palette,
            documents,
            query: signal(String::new()),
            search: signal(false),
            results: signal(None),
            error: signal(None),
            seek: Rc::new(|_| {}),
        };
        effect({
            let panel = panel.clone();
            move || {
                let document = panel.documents.get();
                let query = panel.query.get();
                let Some(document) = document.filter(|doc| doc.document(0).is_some()) else {
                    panel.results.set(None);
                    return;
                };
                let panel = panel.clone();
                spawn(async move {
                    match panel.reader.rows(&document, &query).await {
                        Ok(results) => {
                            panel.results.set(Some(Rc::new(results)));
                            panel.error.set(None);
                        }
                        Err(error) => panel.error.set(Some(error.to_string())),
                    }
                });
            }
        });
        panel
    }

    pub fn on_seek(mut self, seek: impl Fn(f64) + 'static) -> Self {
        self.seek = Rc::new(seek);
        self
    }

    pub fn view(&self, top: impl Fn() -> f32 + 'static) -> If {
        let reader = self.reader.clone();
        let panel = self.clone();
        let top = Rc::new(top);
        when(
            move || reader.visible(),
            move || {
                let top = top.clone();
                let p = panel.palette;
                view()
                    .absolute()
                    .top(move || px(top()))
                    .bottom(px(0.0))
                    .left(px(0.0))
                    .right(px(0.0))
                    .bg(move || p.background())
                    .flex_col()
                    .child(panel.header())
                    .child(panel.language_bar())
                    .child(panel.lines())
            },
        )
    }

    fn header(&self) -> View {
        let p = self.palette;
        let documents = self.documents;
        let search = self.search;
        let query = self.query;
        let refresh = self.reader.clone();
        let close = self.reader.clone();
        let copy = self.clone();
        let share = self.clone();
        view()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .px(px(8.0))
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(36.0))
                            .pl(px(4.0))
                            .child(
                                text("Transcript")
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(
                                text(move || {
                                    documents.with(|state| {
                                        state
                                            .as_ref()
                                            .and_then(|doc| doc.document(0))
                                            .map(|doc| doc.data.title.clone())
                                            .unwrap_or_default()
                                    })
                                })
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(move || p.muted()),
                            ),
                    )
                    .child(ui::icon_button(
                        p,
                        ui::SEARCH,
                        "Search transcript",
                        move || {
                            let open = !search.get();
                            search.set(open);
                            if !open {
                                query.set(String::new());
                            }
                        },
                    ))
                    .child(ui::icon_button(
                        p,
                        REFRESH,
                        "Refresh transcript",
                        move || refresh.retry(),
                    ))
                    .child(ui::icon_button(p, ui::COPY, "Copy transcript", move || {
                        copy.export(false)
                    }))
                    .child(ui::icon_button(p, SHARE, "Share transcript", move || {
                        share.export(true)
                    }))
                    .child(ui::icon_button(
                        p,
                        ui::CLOSE,
                        "Close transcript",
                        move || {
                            search.set(false);
                            query.set(String::new());
                            close.close();
                        },
                    )),
            )
            .child(when(search, move || {
                input()
                    .h(px(40.0))
                    .mx(px(12.0))
                    .my(px(4.0))
                    .px(px(8.0))
                    .bg(move || p.raised())
                    .rounded(px(8.0))
                    .label("Search transcript")
                    .placeholder("Search transcript")
                    .value(move || query.get())
                    .text_size(px(14.0))
                    .text_color(move || p.ink())
                    .placeholder_color(move || p.muted())
                    .on_change(move |value: &str| query.set(value.into()))
            }))
    }

    fn language_bar(&self) -> View {
        let p = self.palette;
        let results = self.results;
        let documents = self.documents;
        let reader = self.reader.clone();
        let mut_error = self.error;
        let options = move || {
            let Some(choices) = results.get() else {
                return Vec::new();
            };
            let selected = documents.with(|state| {
                state
                    .as_ref()
                    .map(|state| state.track_ids.clone())
                    .unwrap_or_default()
            });
            let tracks = &choices.track_choices;
            let mut all = tracks.source_tracks.clone();
            all.extend(tracks.caption_tracks.iter().cloned());
            all.extend(tracks.translations.iter().cloned());
            all.retain(|track| !track.id.is_empty());
            let mut seen = std::collections::HashSet::new();
            all.retain(|track| seen.insert(track.id.clone()));
            all.sort_by_key(|track| selected.iter().position(|id| id == &track.id).unwrap_or(2));
            all
        };
        view()
            .child(scroll().horizontal().h(px(48.0)).px(px(12.0)).child(
                view().flex_row().items_center().gap(px(8.0)).child(each(
                    options,
                    |track| track.id.clone(),
                    move |track, _| {
                        let reader = reader.clone();
                        press()
                            .h(px(38.0))
                            .rounded(px(10.0))
                            .px(px(12.0))
                            .bg(move || p.raised())
                            .justify_center()
                            .on_press(move || {
                                reader.toggle_track(track.with(|track| track.id.clone()))
                            })
                            .child(
                                text(move || track.with(|track| label(track, &documents.get())))
                                    .text_color(move || p.ink())
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                    },
                )),
            ))
            .child(when(
                move || {
                    mut_error.get().is_some()
                        || documents.with(|doc| {
                            doc.as_ref()
                                .is_some_and(|doc| doc.error.is_some() || doc.loading)
                        })
                },
                move || {
                    text(move || {
                        if let Some(error) = mut_error.get() {
                            return error;
                        }
                        documents.get().map_or_else(String::new, |doc| {
                            if let Some(seconds) = doc.retry_after_seconds {
                                format!(
                                    "YouTube is limiting this translation. Retrying in {seconds}s…"
                                )
                            } else if let Some(error) = &doc.error {
                                error.clone()
                            } else {
                                "Loading transcript…".into()
                            }
                        })
                    })
                    .px(px(16.0))
                    .py(px(8.0))
                    .text_size(px(13.0))
                    .text_color(move || p.muted())
                },
            ))
    }

    fn lines(&self) -> View {
        let first = self.clone();
        let second = self.clone();
        let documents = self.documents;
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(first.pane(0))
            .child(when(
                move || {
                    documents.with(|state| {
                        state
                            .as_ref()
                            .is_some_and(|state| state.document(1).is_some())
                    })
                },
                move || second.pane(1),
            ))
    }

    fn pane(&self, track: usize) -> View {
        let p = self.palette;
        let documents = self.documents;
        let results = self.results;
        let seek = self.seek.clone();
        let indices = computed(move || {
            results.with(|results| {
                results
                    .as_ref()
                    .map(|results| {
                        results
                            .rows
                            .iter()
                            .flat_map(|row| row.lines.iter())
                            .filter(|line| line.track == track)
                            .map(|line| line.index)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        });
        let segment = move |position: usize| {
            let index = indices.with(|rows| rows.get(position).copied())?;
            let state = documents.get()?;
            state.document(track)?.data.segments.get(index).cloned()
        };
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(
                text(move || {
                    documents.with(|state| {
                        state
                            .as_ref()
                            .and_then(|state| state.document(track))
                            .map_or_else(String::new, |doc| doc.data.language.clone())
                    })
                })
                .text_size(px(11.0))
                .text_color(move || p.muted())
                .px(px(20.0)),
            )
            .child(
                virtual_list(
                    move || indices.with(Vec::len),
                    move |row| {
                        let seek = seek.clone();
                        press()
                            .px(px(20.0))
                            .py(px(7.0))
                            .gap(px(4.0))
                            .on_press(move || {
                                if let Some(line) = segment(row.index()) {
                                    seek(line.start);
                                }
                            })
                            .child(
                                text(move || {
                                    segment(row.index())
                                        .map_or_else(String::new, |line| timestamp(line.start))
                                })
                                .text_size(px(10.0))
                                .text_color(move || p.muted()),
                            )
                            .child(
                                text(move || {
                                    segment(row.index()).map_or_else(String::new, |line| line.text)
                                })
                                .text_size(px(15.0))
                                .line_height(px(23.0))
                                .text_color(move || p.ink()),
                            )
                    },
                )
                .flex_1(),
            )
    }

    fn export(&self, system_share: bool) {
        let Some(state) = self.documents.get_untracked() else {
            return;
        };
        let reader = self.reader.clone();
        let error = self.error;
        spawn(async move {
            match reader.export(&state).await {
                Ok(content) if system_share => {
                    if let Err(reason) = share(ShareRequest::new().text(content)).await {
                        error.set(Some(reason.to_string()));
                    }
                }
                Ok(content) => set_clipboard(content),
                Err(reason) => error.set(Some(reason.to_string())),
            }
        });
    }
}

fn label(track: &TranscriptTrack, snapshot: &Option<Rc<ReaderSnapshot>>) -> String {
    let label = &track.label;
    let number = snapshot
        .as_ref()
        .and_then(|doc| doc.track_ids.iter().position(|id| id == &track.id));
    number.map_or_else(|| label.clone(), |index| format!("{} · {label}", index + 1))
}

pub fn timestamp(value: f64) -> String {
    let seconds = value as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
