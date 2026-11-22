use std::{collections::HashSet, rc::Rc};

use govue_shell_runtime::{ReaderResults, ReaderSnapshot, TranscriptReader, TranscriptTrack};
use nsh_core::{Checked, prelude::*};

use crate::ui::{self, Palette, press};

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Captions,
    Machine,
}

impl Menu {
    fn title(self) -> &'static str {
        match self {
            Self::Captions => "Caption translations",
            Self::Machine => "Machine translations",
        }
    }

    fn close(self) -> &'static str {
        match self {
            Self::Captions => "Close caption translations",
            Self::Machine => "Close machine translations",
        }
    }

    fn chip(self) -> &'static str {
        match self {
            Self::Captions => "Translations…",
            Self::Machine => "Auto-translate…",
        }
    }

    fn tracks(self, result: &ReaderResults) -> &[TranscriptTrack] {
        match self {
            Self::Captions => &result.track_choices.caption_tracks,
            Self::Machine => &result.track_choices.translations,
        }
    }
}

#[derive(Clone)]
pub struct ReaderLanguages {
    p: Palette,
    reader: TranscriptReader,
    documents: Memo<Option<Rc<ReaderSnapshot>>>,
    results: Signal<Option<Rc<ReaderResults>>>,
    selected: Memo<Vec<String>>,
    menu: Signal<Option<Menu>>,
}

impl ReaderLanguages {
    pub fn new(
        p: Palette,
        reader: TranscriptReader,
        documents: Memo<Option<Rc<ReaderSnapshot>>>,
        results: Signal<Option<Rc<ReaderResults>>>,
    ) -> Self {
        let selected = computed(move || {
            documents.with(|state| {
                state.as_ref().map_or_else(Vec::new, |state| {
                    state
                        .track_ids
                        .iter()
                        .enumerate()
                        .map(|(index, id)| {
                            state.document(index).map_or_else(
                                || id.clone(),
                                |document| document.data.track_id.clone(),
                            )
                        })
                        .collect()
                })
            })
        });
        let menu = signal(None);
        let video = computed(move || {
            documents.with(|state| state.as_ref().map(|state| state.video_id.clone()))
        });
        effect(move || {
            video.get();
            menu.set(None);
        });
        Self {
            p,
            reader,
            documents,
            results,
            selected,
            menu,
        }
    }

    pub fn close(&self) -> bool {
        if self.menu.with_untracked(Option::is_some) {
            self.menu.set(None);
            true
        } else {
            false
        }
    }

    pub fn bar(&self) -> Scroll {
        let p = self.p;
        let results = self.results;
        let documents = self.documents;
        let selected = self.selected;
        let menu = self.menu;
        let reader = self.reader.clone();
        let bar = element_ref();
        effect(move || {
            selected.get();
            bar.scroll_to(
                Point {
                    x: px(0.0),
                    y: px(0.0),
                },
                true,
            );
        });
        let options = move || {
            let Some(state) = documents.get() else {
                return Vec::new();
            };
            let choices = results.get();
            let selected = selected.get();
            let mut tracks = Vec::new();
            for id in &selected {
                if let Some(track) = state
                    .documents
                    .iter()
                    .flat_map(|document| &document.data.tracks)
                    .find(|track| &track.id == id)
                {
                    tracks.push(track.clone());
                }
            }
            if let Some(choices) = choices {
                tracks.extend(choices.track_choices.source_tracks.iter().cloned());
            }
            let mut seen = HashSet::new();
            tracks.retain(|track| !track.id.is_empty() && seen.insert(track.id.clone()));
            tracks
        };
        scroll()
            .element_ref(&bar)
            .horizontal()
            .h(px(48.0))
            .scrollbar(false)
            .px(px(20.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(each(
                        options,
                        |track| track.id.clone(),
                        move |track, _| {
                            let reader = reader.clone();
                            let order = computed(move || {
                                track.with(|track| {
                                    selected.with(|selected| {
                                        selected.iter().position(|id| id == &track.id)
                                    })
                                })
                            });
                            language_choice(
                                selected,
                                order,
                                move || track.with(track_title),
                                move || {
                                    reader.toggle_track(track.with(|track| track.id.clone()));
                                },
                            )
                            .h(px(38.0))
                            .px(px(12.0))
                            .rounded(px(10.0))
                            .bg(move || {
                                if order.with(Option::is_some) {
                                    p.raised()
                                } else {
                                    p.background()
                                }
                            })
                            .border(px(0.5))
                            .border_color(move || p.raised())
                            .child(
                                text(move || {
                                    track.with(|track| {
                                        let name = track_title(track);
                                        order.get().map_or_else(
                                            || name.clone(),
                                            |order| format!("{} · {name}", order + 1),
                                        )
                                    })
                                })
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                            )
                        },
                    ))
                    .children([Menu::Captions, Menu::Machine].into_iter().map(|kind| {
                        when(
                            move || {
                                results.with(|result| {
                                    result
                                        .as_ref()
                                        .is_some_and(|result| !kind.tracks(result).is_empty())
                                })
                            },
                            move || {
                                press()
                                    .h(px(38.0))
                                    .rounded(px(10.0))
                                    .px(px(12.0))
                                    .justify_center()
                                    .bg(move || p.raised())
                                    .label(kind.title())
                                    .on_press(move || menu.set(Some(kind)))
                                    .child(
                                        text(kind.chip())
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(move || p.ink()),
                                    )
                            },
                        )
                    })),
            )
    }

    pub fn sheet(&self) -> If {
        let menu = self.menu;
        let p = self.p;
        let results = self.results;
        let selected = self.selected;
        let reader = self.reader.clone();
        when(
            move || menu.with(Option::is_some),
            move || {
                let reader = reader.clone();
                dynamic(move || {
                    let reader = reader.clone();
                    let Some(kind) = menu.get() else {
                        return view();
                    };
                    view().absolute().inset(px(0.0)).z_index(14).bg(rgba(0x00000088)).justify_end()
                    .child(press().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0))
                        .h(relative(0.25)).label(kind.close()).on_press(move || menu.set(None)))
                    .child(view().h(relative(0.75)).rounded_tl(px(20.0)).rounded_tr(px(20.0)).overflow_hidden().bg(move || p.background())
                        .child(view().flex_row().items_center().justify_between().pl(px(20.0)).pr(px(8.0)).pt(px(10.0))
                            .child(text(kind.title()).text_size(px(19.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
                            .child(ui::icon_button(p, ui::CLOSE, kind.close(), move || menu.set(None))))
                        .child(text(move || {
                            let scope = match kind {
                                Menu::Captions => "Caption tracks supplied with this video",
                                Menu::Machine => "Automatic captions and YouTube machine translations",
                            };
                            let instructions = if selected.with(Vec::len) >= 2 {
                                "Remove a selected language before adding another."
                            } else { "Select a language to compare with the source." };
                            format!("{scope} · {instructions}")
                        }).px(px(20.0)).py(px(12.0)).text_size(px(13.0)).line_height(px(20.0)).text_color(move || p.muted()))
                        .child(scroll().flex_1().child(view().pb(px(40.0)).child(each(
                            move || results.with(|result| result.as_ref().map_or_else(Vec::new, |result| kind.tracks(result).to_vec())),
                            |track| track.id.clone(),
                            move |track, _| {
                                let reader = reader.clone();
                                let order = computed(move || track.with(|track| selected.with(|selected| {
                                    selected.iter().position(|id| id == &track.id)
                                })));
                                language_choice(selected, order, move || track.with(track_title), move || {
                                    reader.toggle_track(track.with(|track| track.id.clone()));
                                    menu.set(None);
                                }).min_h(px(52.0)).px(px(20.0)).border_b(px(0.5)).border_color(move || p.raised())
                                    .child(text(move || track.with(|track| {
                                        let name = track_title(track);
                                        if order.with(Option::is_some) { format!("✓ {name}") } else { name }
                                    })).text_size(px(15.0)).line_height(px(22.0)).text_color(move || p.ink()))
                            },
                        )))))
                })
            },
        )
    }
}

fn track_title(track: &TranscriptTrack) -> String {
    if track.automatic && track.translated_from.is_none() {
        format!("{} · Auto", track.label)
    } else {
        track.label.clone()
    }
}

fn language_choice(
    selected: Memo<Vec<String>>,
    order: Memo<Option<usize>>,
    name: impl Fn() -> String + 'static,
    choose: impl Fn() + 'static,
) -> View {
    let disabled = computed(move || order.with(Option::is_none) && selected.with(Vec::len) >= 2);
    press()
        .role(AccessibilityRole::Checkbox)
        .label(name)
        .justify_center()
        .accessibility_state(move || AccessibilityState {
            checked: Some(if order.with(Option::is_some) {
                Checked::True
            } else {
                Checked::False
            }),
            disabled: disabled.get(),
            ..Default::default()
        })
        .opacity(move || if disabled.get() { 0.4_f32 } else { 1.0_f32 })
        .disabled(disabled)
        .on_press(choose)
}
