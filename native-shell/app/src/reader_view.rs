use std::rc::Rc;

use govue_shell_runtime::{ReaderResults, ReaderSnapshot, TranscriptReader, WatchPlayback};
use nsh_core::{Checked, Dialog, DialogButton, DialogButtonStyle};
use nsh_core::{
    element::{dialog, input},
    prelude::*,
    share::ShareRequest,
};

use crate::chat_transcript_audio::TranscriptAudio;
use crate::reader_clock::ReaderClock;
use crate::reader_languages::ReaderLanguages;
use crate::ui::{self, Palette, press};
use crate::word_tools::{CaptionLine, WordTools};
use crate::{download_library::DOWNLOAD, downloads::Downloads, player_info::INFORMATION};

const REFRESH: &str = "M20 11a8 8 0 1 0-2.3 6m2.3-13v7h-7";
const SHARE: &str = "M8.7 10.4l6.7-3.8M8.7 13.6l6.7 3.8M9 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0zm12-7a3 3 0 1 1-6 0 3 3 0 0 1 6 0zm0 14a3 3 0 1 1-6 0 3 3 0 0 1 6 0z";
const LOCATE: &str = "M12 2v3m0 14v3M2 12h3m14 0h3M19 12a7 7 0 1 1-14 0 7 7 0 0 1 14 0zm-5 0a2 2 0 1 1-4 0 2 2 0 0 1 4 0z";

#[derive(Clone)]
pub struct ReaderView {
    reader: TranscriptReader,
    palette: Palette,
    query: Signal<String>,
    search: Signal<bool>,
    error: Signal<Option<String>>,
    documents: Memo<Option<Rc<ReaderSnapshot>>>,
    results: Signal<Option<Rc<ReaderResults>>>,
    clock: Option<ReaderClock>,
    download: Option<(WatchPlayback, Downloads)>,
    audio: Option<TranscriptAudio>,
    languages: ReaderLanguages,
    copied: Signal<bool>,
    following: Signal<bool>,
    jump: Signal<u64>,
    words: WordTools,
}

impl ReaderView {
    pub fn new(reader: TranscriptReader, palette: Palette, words: WordTools) -> Self {
        let documents = computed({
            let reader = reader.clone();
            move || reader.snapshot().filter(|_| reader.visible())
        });
        let results = signal(None);
        let languages = ReaderLanguages::new(palette, reader.clone(), documents, results);
        let panel = Self {
            reader,
            palette,
            documents,
            query: signal(String::new()),
            search: signal(false),
            results,
            error: signal(None),
            clock: None,
            download: None,
            audio: None,
            languages,
            copied: signal(false),
            following: signal(true),
            jump: signal(0),
            words,
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
                    let result = panel.reader.rows(&document, &query).await;
                    if panel.query.get_untracked() != query
                        || !panel.documents.with_untracked(|current| {
                            current
                                .as_ref()
                                .is_some_and(|current| Rc::ptr_eq(current, &document))
                        })
                    {
                        return;
                    }
                    match result {
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

    pub fn with_playback(mut self, clock: ReaderClock) -> Self {
        self.clock = Some(clock);
        self
    }

    pub fn with_download(mut self, watch: WatchPlayback, downloads: Downloads) -> Self {
        self.download = Some((watch, downloads));
        self
    }

    pub fn with_audio(mut self, audio: TranscriptAudio) -> Self {
        self.audio = Some(audio);
        self
    }

    pub fn view(&self, top: impl Fn() -> f32 + 'static, left: impl Fn() -> f32 + 'static) -> If {
        let reader = self.reader.clone();
        let panel = self.clone();
        let top = Rc::new(top);
        let left = Rc::new(left);
        when(
            move || reader.visible(),
            move || {
                let top = top.clone();
                let left = left.clone();
                let p = panel.palette;
                view()
                    .absolute()
                    .top(move || px(top()))
                    .bottom(px(0.0))
                    .left(move || px(left()))
                    .right(px(0.0))
                    .bg(move || p.background())
                    .flex_col()
                    .child(panel.header())
                    .children(panel.audio.as_ref().map(|audio| audio.bar(p)))
                    .children(panel.playback_bar())
                    .child(panel.language_bar())
                    .child(panel.lines())
            },
        )
    }

    fn playback_bar(&self) -> Option<View> {
        let clock = self.clock.clone()?;
        let p = self.palette;
        let follow = self.following;
        let jump = self.jump;
        let query = self.query;
        let checked = computed({
            let clock = clock.clone();
            move || clock.paused_after_line()
        });
        Some(
            view()
                .flex_row()
                .items_center()
                .px(px(20.0))
                .min_h(px(48.0))
                .gap(px(12.0))
                .child(
                    view()
                        .flex_row()
                        .flex_1()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text("Pause after each line")
                                .text_size(px(13.0))
                                .text_color(move || p.ink()),
                        )
                        .child(
                            view()
                                .role(AccessibilityRole::Switch)
                                .label("Pause after each line")
                                .accessibility_state(move || AccessibilityState {
                                    checked: Some(if checked.get() {
                                        Checked::True
                                    } else {
                                        Checked::False
                                    }),
                                    ..Default::default()
                                })
                                .on_press(move || clock.toggle())
                                .relative()
                                .w(px(46.0))
                                .h(px(26.0))
                                .rounded(px(13.0))
                                .bg(move || {
                                    if checked.get() {
                                        rgb(0x606060)
                                    } else {
                                        rgb(0x4d4d4f)
                                    }
                                })
                                .child(
                                    view()
                                        .absolute()
                                        .top(px(2.0))
                                        .left(move || px(if checked.get() { 22.0 } else { 2.0 }))
                                        .size(px(22.0))
                                        .rounded(px(11.0))
                                        .bg(rgb(0xffffff)),
                                ),
                        ),
                )
                .child(when(
                    move || query.with(|query| query.trim().is_empty()),
                    move || {
                        press()
                            .label("Follow playback")
                            .h(px(44.0))
                            .rounded(px(8.0))
                            .px(px(8.0))
                            .flex_row()
                            .items_center()
                            .gap(px(5.0))
                            .bg(move || {
                                if follow.get() {
                                    p.raised()
                                } else {
                                    p.background()
                                }
                            })
                            .on_press(move || {
                                follow.set(true);
                                jump.update(|revision| *revision += 1);
                            })
                            .child(ui::icon(LOCATE, 14.0, move || p.ink()))
                            .child(
                                text(move || {
                                    if follow.get() {
                                        "Following"
                                    } else {
                                        "Follow playback"
                                    }
                                })
                                .text_size(px(11.0))
                                .text_color(move || p.ink()),
                            )
                    },
                )),
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
        let details = self.clone();
        let copied = self.copied;
        let watch_reader = self.clock.is_some();
        let ready = computed(move || {
            documents.with(|snapshot| {
                snapshot
                    .as_ref()
                    .is_some_and(|snapshot| !snapshot.loading && snapshot.document(0).is_some())
            })
        });
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
                                            .map(|doc| {
                                                if watch_reader && doc.cached {
                                                    format!("Saved · {}", doc.data.title)
                                                } else {
                                                    doc.data.title.clone()
                                                }
                                            })
                                            .unwrap_or_default()
                                    })
                                })
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(move || p.muted()),
                            ),
                    )
                    .child(
                        ui::icon_button(p, ui::SEARCH, "Search transcript", move || {
                            let open = !search.get();
                            search.set(open);
                            if !open {
                                query.set(String::new());
                            }
                        })
                        .size(px(40.0))
                        .disabled(move || !ready.get()),
                    )
                    .when(watch_reader, |header| {
                        header.child(
                            ui::icon_button(p, REFRESH, "Refresh transcript", move || {
                                copied.set(false);
                                refresh.retry();
                            })
                            .size(px(40.0))
                            .disabled(move || {
                                documents.with(|snapshot| {
                                    snapshot.as_ref().is_some_and(|snapshot| snapshot.loading)
                                })
                            }),
                        )
                    })
                    .child(
                        ui::icon_button(p, INFORMATION, "Caption details", move || {
                            details.details()
                        })
                        .size(px(40.0))
                        .disabled(move || !ready.get()),
                    )
                    .child(
                        ui::icon_button(p, ui::COPY, "Copy transcript", move || copy.export(false))
                            .size(px(40.0))
                            .disabled(move || !ready.get())
                            .label(move || {
                                if copied.get() {
                                    "Transcript copied"
                                } else {
                                    "Copy transcript"
                                }
                            }),
                    )
                    .child(
                        ui::icon_button(p, SHARE, "Share transcript", move || share.export(true))
                            .size(px(40.0))
                            .disabled(move || !ready.get()),
                    )
                    .when_some(self.download.clone(), |header, (watch, downloads)| {
                        let ready = watch.clone();
                        header.child(
                            ui::icon_button(p, DOWNLOAD, "Download this video", move || {
                                if let Some(video) = watch.video() {
                                    downloads.confirm_video(video);
                                }
                            })
                            .size(px(40.0))
                            .disabled(move || ready.video().is_none()),
                        )
                    })
                    .child(
                        ui::icon_button(p, ui::CLOSE, "Close transcript", move || {
                            search.set(false);
                            copied.set(false);
                            query.set(String::new());
                            close.close();
                        })
                        .size(px(40.0)),
                    ),
            )
            .child(when(copied, move || {
                text("Transcript copied")
                    .px(px(20.0))
                    .text_size(px(13.0))
                    .text_color(move || p.muted())
            }))
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

    pub fn language_sheet(&self) -> If {
        self.languages.sheet()
    }

    pub fn close_language_sheet(&self) -> bool {
        self.languages.close()
    }

    fn language_bar(&self) -> View {
        let p = self.palette;
        let documents = self.documents;
        let mut_error = self.error;
        view().child(self.languages.bar()).child(when(
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
                            format!("YouTube is limiting this translation. Retrying in {seconds}s…")
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
        let playback = self.clock.clone();
        let clock = self.clock.clone();
        let line_audio = self.audio.clone();
        let playing_audio = self.audio.clone();
        let can_play = playback.is_some() || line_audio.is_some();
        let query = self.query;
        let follow = self.following;
        let jump = self.jump;
        let words = self.words.clone();
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
        let position = computed(move || {
            let state = documents.get()?;
            let data = &state.document(track)?.data;
            if let Some(clock) = &clock {
                clock.position(&state.video_id)
            } else {
                playing_audio
                    .as_ref()?
                    .position(&state.video_id, &data.track_id)
            }
        });
        let active = computed(move || {
            let state = documents.get()?;
            let data = &state.document(track)?.data;
            let position = position.get()?;
            let index = data
                .segments
                .partition_point(|cue| cue.start <= position)
                .checked_sub(1)?;
            let cue = data.segments.get(index)?;
            let end = if cue.duration > 0.0 {
                cue.start + cue.duration
            } else {
                data.segments
                    .get(index + 1)
                    .map_or(f64::INFINITY, |next| next.start)
            };
            (position < end).then_some(index)
        });
        let list = element_ref();
        effect(move || {
            let _ = jump.get();
            if follow.get()
                && query.with(|query| query.trim().is_empty())
                && let Some(active) = active.get()
                && let Some(row) =
                    indices.with(|rows| rows.iter().position(|index| *index == active))
            {
                list.scroll_to_index(row as u32, percentage(0.0), px(0.0), true);
            }
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
                        let playback = playback.clone();
                        let line_audio = line_audio.clone();
                        let words = words.clone();
                        let word_line = move || {
                            let index = indices.with(|rows| rows.get(row.index()).copied())?;
                            let document = documents.get()?;
                            let data = &document.document(track)?.data;
                            let caption = data.segments.get(index)?;
                            Some(CaptionLine {
                                video: data.video_id.clone(),
                                track: data.track_id.clone(),
                                index,
                                start: caption.start,
                                text: caption.text.clone(),
                            })
                        };
                        let word_panel = words.clone();
                        let selecting = move || {
                            word_line()
                                .is_some_and(|line| word_panel.source().as_ref() == Some(&line))
                        };
                        let play_at = move |seconds| {
                            follow.set(true);
                            jump.update(|revision| *revision += 1);
                            if let Some(playback) = &playback {
                                playback.seek(seconds);
                            } else if let Some((audio, line)) = line_audio.as_ref().zip(word_line())
                            {
                                audio.play(line);
                            }
                        };
                        let word_seek = can_play.then(|| play_at.clone());
                        let selected = move || {
                            active.get().is_some_and(|active| {
                                indices.with(|rows| rows.get(row.index()).copied()) == Some(active)
                            })
                        };
                        let word_position = move || {
                            if !selected() {
                                return None;
                            }
                            position.get()
                        };
                        view()
                            .mx(px(20.0))
                            .px(px(8.0))
                            .py(px(7.0))
                            .gap(px(4.0))
                            .rounded(px(6.0))
                            .border_l(px(3.0))
                            .border_color(move || {
                                if selected() {
                                    p.ink()
                                } else {
                                    rgba(0x00000000).into()
                                }
                            })
                            .bg(move || {
                                if selected() {
                                    p.raised()
                                } else {
                                    p.background()
                                }
                            })
                            .when(can_play, |row_view| {
                                row_view.role(AccessibilityRole::Button).on_press(move || {
                                    if !selecting()
                                        && let Some(line) = segment(row.index())
                                    {
                                        play_at(line.start);
                                    }
                                })
                            })
                            .child(
                                text(move || {
                                    segment(row.index())
                                        .map_or_else(String::new, |line| timestamp(line.start))
                                })
                                .text_size(px(10.0))
                                .text_color(move || p.muted()),
                            )
                            .child(words.line(word_line, word_position, word_seek, move || {
                                follow.set(false)
                            }))
                    },
                )
                .element_ref(&list)
                .on_scroll_drag_start(move |_: &nsh_core::Point<Points>| follow.set(false))
                .flex_1(),
            )
    }

    fn export(&self, system_share: bool) {
        let Some(state) = self.documents.get_untracked() else {
            return;
        };
        let reader = self.reader.clone();
        let error = self.error;
        let copied = self.copied;
        spawn(async move {
            match reader.export(&state).await {
                Ok(content) if system_share => {
                    if let Err(reason) = share(ShareRequest::new().text(content)).await {
                        error.set(Some(reason.to_string()));
                    }
                }
                Ok(content) => {
                    set_clipboard(content);
                    copied.set(true);
                }
                Err(reason) => error.set(Some(reason.to_string())),
            }
        });
    }

    fn details(&self) {
        let Some(state) = self.documents.get_untracked() else {
            return;
        };
        let Some(document) = state.document(0) else {
            return;
        };
        let language = document.data.language.clone();
        let reader = self.reader.clone();
        spawn(async move {
            let (title, message) = match reader.details(&state).await {
                Ok(message) => (format!("{language} captions"), message),
                Err(_) => (
                    "Caption details unavailable".into(),
                    "Refresh the transcript and try again.".into(),
                ),
            };
            let _ = dialog(Dialog {
                id: 0,
                title: Some(title),
                message: Some(message),
                buttons: vec![DialogButton {
                    text: "OK".into(),
                    style: DialogButtonStyle::Default,
                }],
                cancelable: true,
                sheet: false,
            })
            .await;
        });
    }
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
