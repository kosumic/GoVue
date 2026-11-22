use std::rc::Rc;
use std::time::Duration;

use govue_shell_runtime::{
    Chapter, PlaybackEvent, PlaybackFit, PlaybackSelection, TranscriptReader, WatchPlayback,
    WatchSubtitles,
};
use nsh_core::{
    Dialog, DialogButton, DialogButtonStyle, PointerEvents, PressEvent, element::dialog, prelude::*,
};
use serde_json::{Value, json};

use crate::browser_card::{SHARE, share_video};
use crate::cast_view::{CAST, CastView};
use crate::chat::Chat;
use crate::download_library::DOWNLOAD;
use crate::downloads::Downloads;
use crate::pip::Pip;
use crate::player_gestures::{GestureActions, PlayerGestures};
use crate::player_info;
use crate::player_window::PlayerWindow;
use crate::queue::Queue;
use crate::queue_view::queue_sheet;
use crate::reader_view::timestamp;
use crate::segments_view::{SEGMENTS, category, segments_view};
use crate::ui::{self, Palette, press};
use crate::{settings_view::Preferences, subtitle_style::subtitle_style};

const CC: &str = "M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zm6 5a2 2 0 1 0 0 4m8-4a2 2 0 1 0 0 4";
const TRANSCRIPT: &str =
    "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zm0 0v6h6M8 13h8m-8 4h8";
const SPARKLES: &str =
    "m9.9 2 1.9 5.8L18 10l-6.2 2.1-1.9 5.9-2-5.9L2 10l5.9-2.2zM19 12v8m-4-4h8m-4-15v6m-3-3h6";
const PAUSE: &str = "M8 4h2v16H8zM15 4h2v16h-2z";
const BACK: &str = "m12 19-7-7 7-7M5 12h14";
const COLLAPSE: &str = "m6 9 6 6 6-6";
const EXPAND: &str = "M8 3H5a2 2 0 0 0-2 2v3m13-5h3a2 2 0 0 1 2 2v3m-5 13h3a2 2 0 0 0 2-2v-3M8 21H5a2 2 0 0 1-2-2v-3";
const SHRINK: &str = "M8 3v3a2 2 0 0 1-2 2H3m13-5v3a2 2 0 0 0 2 2h3m-5 13v-3a2 2 0 0 1 2-2h3M8 21v-3a2 2 0 0 0-2-2H3";
const CHECK: &str = "m4 12 5 5L20 6";
const PREVIOUS: &str = "M19 20 9 12l10-8zM5 4v16";
const NEXT: &str = "m5 4 10 8-10 8zM19 4v16";
const SETTINGS: &str = "M12 8a4 4 0 1 0 0 8 4 4 0 1 0 0-8zM9 3h6l1 3 3-1 2 4-2 3 2 3-2 4-3-1-1 3H9l-1-3-3 1-2-4 2-3-2-3 2-4 3 1z";
const QUEUE: &str = "M3 6h18M3 12h18M3 18h10m4-2 4 2-4 2z";
const SPEED: &str = "M3 18a9 9 0 1 1 18 0H3zm9-2 4-5m-9 3h1m4-5v1";
const AUDIO: &str = "M3 9h4l5-4v14l-5-4H3zm13-1a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14";
const PIP: &str = "M3 4h18v16H3zM12 11h7v6h-7z";
const LOCK: &str = "M5 11h14v10H5zM8 11V7a4 4 0 1 1 8 0v4m-4 4v2";
const UNLOCK: &str = "M5 11h14v10H5zM8 11V7a4 4 0 0 1 7.5-2m-3.5 10v2";
const ADD: &str = "M12 5v14m-7-7h14";
const LOOP: &str = "m17 1 4 4-4 4m4-4H7a4 4 0 0 0-4 4v2m4 12-4-4 4-4m-4 4h14a4 4 0 0 0 4-4v-2";

type OpenVideo = Rc<dyn Fn(String)>;

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Subtitles,
    More,
    Speed,
    Resize,
    Quality,
    Audio,
    Queue,
    Style,
    Segments,
    Info,
}

#[derive(Clone)]
pub struct PlayerControls {
    watch: WatchPlayback,
    subs: WatchSubtitles,
    reader: TranscriptReader,
    chat: Chat,
    palette: Palette,
    queue: Queue,
    settings: Preferences,
    visible: Signal<bool>,
    locked: Signal<bool>,
    menu: Signal<Option<Menu>>,
    chapter_preview: Signal<Option<Chapter>>,
    activity: Signal<u64>,
    ended: Signal<Option<u64>>,
    speeds: Signal<Vec<f64>>,
    gestures: PlayerGestures,
    pub cast: CastView,
}

impl PlayerControls {
    pub fn new(
        watch: WatchPlayback,
        subs: WatchSubtitles,
        reader: TranscriptReader,
        chat: Chat,
        palette: Palette,
        queue: Queue,
        settings: Preferences,
    ) -> Self {
        let controller = Self {
            gestures: PlayerGestures::new(settings.core(), watch.clone(), settings.clone()),
            cast: CastView::new(settings.core(), watch.clone()),
            watch,
            subs,
            reader,
            chat,
            palette,
            queue,
            settings,
            visible: signal(true),
            locked: signal(false),
            menu: signal(None),
            chapter_preview: signal(None),
            activity: signal(0),
            ended: signal(None),
            speeds: signal(Vec::new()),
        };
        spawn({
            let watch = controller.watch.clone();
            let speeds = controller.speeds;
            async move {
                if let Ok(values) = watch.speeds().await {
                    speeds.set(values);
                }
            }
        });
        let playing = computed({
            let watch = controller.watch.clone();
            move || watch.status().is_some_and(|status| status.playing)
        });
        effect({
            let controller = controller.clone();
            move || {
                let revision = controller.activity.get();
                if controller.visible.get()
                    && controller.menu.with(Option::is_none)
                    && playing.get()
                    && !controller.gestures.busy()
                {
                    let controller = controller.clone();
                    spawn(async move {
                        timer(Duration::from_secs(3)).await;
                        if controller.activity.get_untracked() == revision
                            && !controller.gestures.busy()
                            && controller.menu.with_untracked(Option::is_none)
                            && playing.get_untracked()
                        {
                            controller.visible.set(false);
                        }
                    });
                }
            }
        });
        let video = computed({
            let watch = controller.watch.clone();
            move || watch.video().map(|video| (video.video_id, video.offline))
        });
        effect({
            let controller = controller.clone();
            move || {
                let _ = video.get();
                controller.menu.set(None);
                controller.chapter_preview.set(None);
                controller.locked.set(false);
                controller.gestures.zoom.set(1.0);
                controller.show();
            }
        });
        controller
    }

    pub fn visible(&self) -> bool {
        self.visible.get() && !self.locked.get()
    }

    pub fn zoom(&self) -> Signal<f32> {
        self.gestures.zoom
    }

    pub fn menu_open(&self) -> bool {
        self.menu.with(Option::is_some)
    }

    pub fn error_view(&self) -> If {
        let watch = self.watch.clone();
        let controls = self.clone();
        let p = self.palette;
        when(
            move || watch.video().is_none() && watch.error().is_some(),
            move || {
                let error = controls.watch.clone();
                let retry = controls.watch.clone();
                let close = controls.watch.clone();
                let data = controls.reader.clone();
                let transcript = controls.reader.clone();
                view()
                    .absolute()
                    .top(px(60.0))
                    .left(px(12.0))
                    .right(px(12.0))
                    .p(px(16.0))
                    .bg(move || p.background())
                    .child(
                        text(move || error.error().unwrap_or_default())
                            .text_size(px(13.0))
                            .text_color(move || p.ink()),
                    )
                    .child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .child(error_button(p, "Retry", move || retry.start()))
                            .child(error_button(p, "Web player", move || close.close()))
                            .child(when(
                                move || {
                                    data.snapshot()
                                        .is_some_and(|state| state.document(0).is_some())
                                },
                                move || {
                                    let transcript = transcript.clone();
                                    error_button(p, "Read transcript", move || transcript.toggle())
                                },
                            )),
                    )
            },
        )
    }

    pub fn close_menu(&self) -> bool {
        if self.cast.close() {
            return true;
        }
        if self.chapter_preview.with_untracked(Option::is_some) {
            self.chapter_preview.set(None);
            return true;
        }
        let was_open = self.menu.with_untracked(Option::is_some);
        if was_open {
            self.menu.set(None);
            return true;
        }
        if self.locked.get_untracked() {
            self.locked.set(false);
            self.show();
            return true;
        }
        false
    }

    fn show(&self) {
        self.visible.set(true);
        self.activity.update(|value| *value += 1);
    }

    pub fn playback(&self, event: &PlaybackEvent, open: impl FnOnce(String) + 'static) {
        let PlaybackEvent::Status { generation, status } = event else {
            return;
        };
        if !self
            .watch
            .video()
            .is_some_and(|video| video.generation == *generation && !video.offline)
        {
            return;
        }
        if status.state == "ended" {
            if self.ended.get_untracked() != Some(*generation) {
                self.ended.set(Some(*generation));
                self.queue.advance("ended", open);
            }
        } else if self.ended.with_untracked(Option::is_some) {
            self.ended.set(None);
        }
    }

    pub fn view(
        &self,
        viewport: impl Fn() -> (f32, bool) + 'static,
        back: impl Fn() + 'static,
        open: impl Fn(String) + 'static,
        window: PlayerWindow,
        pip: Pip,
        downloads: Downloads,
    ) -> View {
        let watch = self.watch.clone();
        let viewport = computed(viewport);
        let bounds = computed(move || watch.frame(viewport.get().0));
        let panel = self.clone();
        let visible = self.visible;
        let locked = self.locked;
        let back = Rc::new(back);
        let open: OpenVideo = Rc::new(open);
        let navigate = open.clone();
        let menu = self.menu;
        let popup = self.clone();
        let more = self.clone();
        let download_menu = downloads.clone();
        let options = self.clone();
        let style = self.clone();
        let segments = self.watch.clone();
        let preview = self.chapter_preview;
        let queue = self.queue.clone();
        let palette = self.palette;
        let in_pip = pip.mode;
        let fullscreen = window.presented;
        let diagnostic_watch = self.watch.clone();
        let diagnostic_sample =
            player_info::monitor(self.settings.core(), self.watch.clone(), bounds, fullscreen);
        let scope = computed({
            let watch = self.watch.clone();
            move || {
                (bounds.get().visible
                    && !in_pip.get()
                    && !viewport.get().1
                    && !locked.get()
                    && menu.with(Option::is_none))
                .then(|| watch.video().map(|video| video.generation))
                .flatten()
            }
        });
        effect({
            let gesture = self.gestures.clone();
            move || {
                scope.get();
                gesture.cancel();
            }
        });
        effect({
            let panel = self.clone();
            move || {
                fullscreen.get();
                viewport.get();
                panel.locked.set(false);
                panel.show();
            }
        });
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(PointerEvents::BoxNone)
            .child(when(
                move || bounds.get().visible && !in_pip.get() && !viewport.get().1,
                move || {
                    let panel = panel.clone();
                    let back = back.clone();
                    let open = navigate.clone();
                    let window = window.clone();
                    let gestures = panel.gestures.clone();
                    let gesture_window = window.clone();
                    let gesture_back = back.clone();
                    let chrome = panel.clone();
                    let gesture_actions = GestureActions {
                        show: Rc::new({
                            let panel = panel.clone();
                            move || panel.show()
                        }),
                        toggle_controls: Rc::new({
                            let panel = panel.clone();
                            move || {
                                if panel.visible.get_untracked() {
                                    panel.visible.set(false);
                                } else {
                                    panel.show();
                                }
                            }
                        }),
                        toggle_playback: Rc::new({
                            let panel = panel.clone();
                            move || panel.toggle()
                        }),
                        back: gesture_back,
                    };
                    view()
                        .absolute()
                        .left(move || px(bounds.get().left))
                        .top(move || px(bounds.get().top))
                        .w(move || px(bounds.get().width))
                        .h(move || px(bounds.get().height))
                        .overflow_hidden()
                        .pointer_events(PointerEvents::BoxNone)
                        .child(when(
                            move || scope.with(Option::is_some),
                            move || {
                                gestures.view(
                                    bounds,
                                    gesture_window.clone(),
                                    gesture_actions.clone(),
                                )
                            },
                        ))
                        .child(when(
                            move || visible.get() && !locked.get(),
                            move || {
                                chrome.chrome(bounds, back.clone(), open.clone(), window.clone())
                            },
                        ))
                        .child(when(
                            move || fullscreen.get() && (locked.get() || visible.get()),
                            move || {
                                let panel = panel.clone();
                                view()
                                    .absolute()
                                    .left(px(8.0))
                                    .top(percentage(0.5))
                                    .mt(px(-24.0))
                                    .child(
                                        press()
                                            .size(px(48.0))
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(24.0))
                                            .bg(rgba(0x00000066))
                                            .label(move || {
                                                if locked.get() {
                                                    "Unlock screen"
                                                } else {
                                                    "Lock screen"
                                                }
                                            })
                                            .on_press(move || {
                                                locked.update(|value| *value = !*value);
                                                panel.show();
                                            })
                                            .child(dynamic(move || {
                                                ui::icon(
                                                    if locked.get() { LOCK } else { UNLOCK },
                                                    22.0,
                                                    || rgb(0xffffff).into(),
                                                )
                                            })),
                                    )
                            },
                        ))
                },
            ))
            .child(when(
                move || {
                    menu.get() == Some(Menu::Subtitles) && bounds.get().visible && !in_pip.get()
                },
                move || popup.subtitle_menu(bounds),
            ))
            .child(when(
                move || menu.get() == Some(Menu::More) && bounds.get().visible && !in_pip.get(),
                move || more.more_menu(bounds, pip.clone(), download_menu.clone()),
            ))
            .child(when(
                move || {
                    matches!(
                        menu.get(),
                        Some(Menu::Speed | Menu::Resize | Menu::Quality | Menu::Audio)
                    ) && bounds.get().visible
                        && !in_pip.get()
                },
                move || options.preference_menu(bounds),
            ))
            .child(when(
                move || menu.get() == Some(Menu::Queue) && bounds.get().visible && !in_pip.get(),
                move || {
                    queue_sheet(
                        palette,
                        queue.clone(),
                        open.clone(),
                        downloads.clone(),
                        move || menu.set(None),
                    )
                },
            ))
            .child(when(
                move || menu.get() == Some(Menu::Style) && bounds.get().visible && !in_pip.get(),
                move || {
                    subtitle_style(
                        palette,
                        style.settings.clone(),
                        style.watch.video().map(|video| video.video_id),
                        || nsh_core::element::window().height.0,
                        move || menu.set(None),
                    )
                },
            ))
            .child(when(
                move || menu.get() == Some(Menu::Segments) && bounds.get().visible && !in_pip.get(),
                move || {
                    segments_view(palette, segments.clone(), preview, move || {
                        preview.set(None);
                        menu.set(None);
                    })
                },
            ))
            .child(when(
                move || menu.get() == Some(Menu::Info) && bounds.get().visible && !in_pip.get(),
                move || {
                    player_info::dialog(
                        palette,
                        diagnostic_watch.clone(),
                        diagnostic_sample,
                        move || menu.set(None),
                    )
                },
            ))
    }

    fn chrome(
        &self,
        bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>,
        back: Rc<impl Fn() + 'static>,
        open: OpenVideo,
        window: PlayerWindow,
    ) -> View {
        let menu = self.menu;
        let play = self.clone();
        let state = self.watch.clone();
        let paused = computed(move || !state.status().is_some_and(|status| status.playing));
        let clock = self.watch.clone();
        let live = computed({
            let watch = self.watch.clone();
            move || watch.live()
        });
        let live_clock = self.watch.clone();
        let seek_live = self.clone();
        let wide = computed(move || bounds.get().width >= 440.0);
        let title = self.watch.clone();
        let author = self.watch.clone();
        let chapters = self.watch.clone();
        let has_chapters = computed(move || {
            chapters
                .video()
                .is_some_and(|video| !video.chapters.is_empty())
        });
        let loop_queue = self.queue.clone();
        let loop_mode = self.queue.signal();
        let captions = self.subs.clone();
        let transcript = self.reader.clone();
        let chat = self.chat.clone();
        let video = self.watch.clone();
        let fullscreen = window.presented;
        let close_window = window.clone();
        let offline = computed({
            let watch = self.watch.clone();
            move || watch.offline()
        });
        let fullscreen_back = back.clone();
        let cast = self.cast.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(PointerEvents::BoxNone)
            .child(
                view()
                    .absolute()
                    .top(px(0.0))
                    .left(px(2.0))
                    .right(px(4.0))
                    .h(px(48.0))
                    .flex_row()
                    .items_center()
                    .pointer_events(PointerEvents::BoxNone)
                    .child(
                        press()
                            .size(px(48.0))
                            .items_center()
                            .justify_center()
                            .label("Navigate back")
                            .on_press(move || {
                                if offline.get_untracked() || !close_window.close() {
                                    back();
                                }
                            })
                            .child(when(fullscreen, || {
                                ui::icon(COLLAPSE, 24.0, || rgb(0xffffff).into())
                            }))
                            .child(when(
                                move || !fullscreen.get(),
                                || ui::icon(BACK, 24.0, || rgb(0xffffff).into()),
                            )),
                    )
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(when(fullscreen, move || {
                                let title = title.clone();
                                let author = author.clone();
                                view()
                                    .min_w(px(0.0))
                                    .child(
                                        text(move || {
                                            title.video().map_or_else(
                                                || "Loading video…".into(),
                                                |video| video.title,
                                            )
                                        })
                                        .truncate()
                                        .text_size(px(16.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(0xffffff)),
                                    )
                                    .child(
                                        text(move || {
                                            author
                                                .video()
                                                .map(|video| video.author)
                                                .unwrap_or_default()
                                        })
                                        .truncate()
                                        .text_size(px(12.0))
                                        .text_color(rgba(0xffffffbb)),
                                    )
                            })),
                    )
                    .child(when(wide, move || {
                        let queue = loop_queue.clone();
                        view()
                            .flex_row()
                            .items_center()
                            .child(player_button(
                                QUEUE,
                                "Queue",
                                44.0,
                                || rgb(0xffffff).into(),
                                move || menu.set(Some(Menu::Queue)),
                            ))
                            .child(when(has_chapters, move || {
                                player_button(
                                    SEGMENTS,
                                    "Chapters",
                                    44.0,
                                    || rgb(0xffffff).into(),
                                    move || menu.set(Some(Menu::Segments)),
                                )
                            }))
                            .child(when(
                                move || !has_chapters.get(),
                                move || {
                                    player_button(
                                        SEGMENTS,
                                        "Segments",
                                        44.0,
                                        || rgb(0xffffff).into(),
                                        move || menu.set(Some(Menu::Segments)),
                                    )
                                },
                            ))
                            .child(dynamic(move || {
                                let queue = queue.clone();
                                let (title, active) = loop_mode.with(|state| {
                                    (loop_title(&state.loop_mode), state.loop_mode != "next")
                                });
                                player_button(
                                    LOOP,
                                    title,
                                    44.0,
                                    move || rgb(if active { 0xff0000 } else { 0xffffff }).into(),
                                    move || queue.update("loop", json!({})),
                                )
                            }))
                    }))
                    .child(
                        player_button(
                            CAST,
                            "Cast",
                            48.0,
                            || rgb(0xffffff).into(),
                            move || cast.show(),
                        )
                        .disabled(offline)
                        .opacity(move || if offline.get() { 0.45_f32 } else { 1.0 }),
                    )
                    .child(
                        player_button(
                            CC,
                            "Subtitles",
                            48.0,
                            move || {
                                rgb(if captions.selected() {
                                    0xffffff
                                } else {
                                    0xe6e6e6
                                })
                                .into()
                            },
                            move || menu.set(Some(Menu::Subtitles)),
                        )
                        .on_long_press(move || menu.set(Some(Menu::Style))),
                    )
                    .child(player_button(
                        SETTINGS,
                        "More options",
                        48.0,
                        || rgb(0xffffff).into(),
                        move || menu.set(Some(Menu::More)),
                    )),
            )
            .child(
                view()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .top(percentage(0.5))
                    .mt(px(-32.0))
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(16.0))
                    .pointer_events(PointerEvents::BoxNone)
                    .child(self.skip_button(
                        "Previous",
                        PREVIOUS,
                        "previous",
                        open.clone(),
                        |state| state.has_previous,
                    ))
                    .child(
                        press()
                            .size(px(64.0))
                            .rounded(px(32.0))
                            .bg(rgba(0x00000066))
                            .items_center()
                            .justify_center()
                            .label(move || if paused.get() { "Play" } else { "Pause" })
                            .on_press(move || play.toggle())
                            .child(when(paused, || {
                                ui::icon(ui::PLAY, 38.0, || rgb(0xffffff).into())
                            }))
                            .child(when(
                                move || !paused.get(),
                                || ui::icon(PAUSE, 31.0, || rgb(0xffffff).into()),
                            )),
                    )
                    .child(self.skip_button("Next", NEXT, "next", open, |state| state.has_next)),
            )
            .child(
                view()
                    .absolute()
                    .bottom(px(0.0))
                    .left(px(12.0))
                    .right(px(12.0))
                    .pointer_events(PointerEvents::BoxNone)
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .h(px(44.0))
                            .pointer_events(PointerEvents::BoxNone)
                            .child(when(
                                move || !live.get(),
                                move || {
                                    let clock = clock.clone();
                                    text(move || {
                                        clock.status().map_or_else(
                                            || "0:00 / 0:00".into(),
                                            |status| {
                                                format!(
                                                    "{} / {}",
                                                    timestamp(status.position),
                                                    timestamp(status.duration.unwrap_or(0.0))
                                                )
                                            },
                                        )
                                    })
                                    .text_size(px(14.0))
                                    .text_color(rgb(0xffffff))
                                },
                            ))
                            .child(when(live, move || {
                                let disabled = live_clock.clone();
                                let clock = live_clock.clone();
                                let seek_live = seek_live.clone();
                                press()
                                    .h(px(44.0))
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.0))
                                    .label("Seek to live")
                                    .disabled(move || {
                                        !disabled
                                            .status()
                                            .is_some_and(|status| status.live_offset.is_some())
                                    })
                                    .on_press(move || seek_live.seek_live())
                                    .child(
                                        text(move || {
                                            format!(
                                                "{} /",
                                                timestamp(
                                                    clock
                                                        .status()
                                                        .map_or(0.0, |status| status.position)
                                                )
                                            )
                                        })
                                        .text_size(px(14.0))
                                        .text_color(rgb(0xffffff)),
                                    )
                                    .child(
                                        text("LIVE")
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(0xff0000)),
                                    )
                            }))
                            .child(when(has_chapters, move || {
                                player_button(
                                    SEGMENTS,
                                    "Chapters",
                                    36.0,
                                    || rgb(0xffffff).into(),
                                    move || menu.set(Some(Menu::Segments)),
                                )
                            }))
                            .child(view().flex_1())
                            .child(player_button(
                                TRANSCRIPT,
                                "Show transcript",
                                44.0,
                                {
                                    let reader = transcript.clone();
                                    move || {
                                        rgb(if reader.visible() { 0xff0000 } else { 0xffffff })
                                            .into()
                                    }
                                },
                                move || transcript.toggle(),
                            ))
                            .child(player_button(
                                SPARKLES,
                                "Open AI chat",
                                44.0,
                                || rgb(0xffffff).into(),
                                move || {
                                    if let Some(video) = video.video() {
                                        chat.show(Some(video.video_id));
                                    }
                                },
                            ))
                            .child(when(wide, move || {
                                player_button(
                                    SPEED,
                                    "Playback speed",
                                    44.0,
                                    || rgb(0xffffff).into(),
                                    move || menu.set(Some(Menu::Speed)),
                                )
                            }))
                            .child(
                                press()
                                    .size(px(44.0))
                                    .items_center()
                                    .justify_center()
                                    .label(move || {
                                        if fullscreen.get() {
                                            "Exit fullscreen"
                                        } else {
                                            "Fullscreen"
                                        }
                                    })
                                    .on_press(move || {
                                        if offline.get_untracked() {
                                            fullscreen_back();
                                        } else {
                                            window.toggle();
                                        }
                                    })
                                    .child(when(fullscreen, || {
                                        ui::icon(SHRINK, 24.0, || rgb(0xffffff).into())
                                    }))
                                    .child(when(
                                        move || !fullscreen.get(),
                                        || ui::icon(EXPAND, 24.0, || rgb(0xffffff).into()),
                                    )),
                            ),
                    )
                    .child(self.seek_bar(bounds)),
            )
    }

    fn skip_button(
        &self,
        label: &'static str,
        icon: &'static str,
        direction: &'static str,
        open: OpenVideo,
        available: impl Fn(&crate::queue::QueueState) -> bool + 'static,
    ) -> View {
        let queue = self.queue.clone();
        let watch = self.watch.clone();
        let enabled = computed(move || !watch.offline() && queue.signal().with(&available));
        let queue = self.queue.clone();
        press()
            .size(px(48.0))
            .rounded(px(24.0))
            .bg(rgba(0x00000066))
            .justify_center()
            .items_center()
            .label(label)
            .opacity(move || if enabled.get() { 1.0 } else { 0.38_f32 })
            .accessibility_state(move || AccessibilityState {
                disabled: !enabled.get(),
                ..Default::default()
            })
            .on_press(move || {
                if enabled.get_untracked() {
                    let open = open.clone();
                    queue.advance(direction, move |url| open(url));
                }
            })
            .child(ui::icon(icon, 24.0, || rgb(0xffffff).into()))
    }

    fn more_menu(
        &self,
        bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>,
        pip: Pip,
        downloads: Downloads,
    ) -> View {
        let menu = self.menu;
        let watch = self.watch.clone();
        let sharing = self.watch.clone();
        let adding = self.watch.clone();
        let queue = self.queue.clone();
        let loop_state = queue.signal();
        let loop_queue = queue.clone();
        let compact = move || bounds.get().width < 440.0;
        let menu_height = move || {
            let height = nsh_core::element::window().height.0;
            let content = if compact() { 14.0 } else { 10.0 } * 48.0;
            px((height - bounds.get().top - 60.0)
                .max(112.0)
                .min(height * 0.7)
                .min(content))
        };
        let cast = self.cast.clone();
        let offline = computed({
            let watch = self.watch.clone();
            move || watch.offline()
        });
        let has_chapters = computed({
            let watch = self.watch.clone();
            move || {
                watch
                    .video()
                    .is_some_and(|video| !video.chapters.is_empty())
            }
        });
        let available = computed({
            let pip = pip.clone();
            move || pip.available()
        });
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(1)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .label("Close options")
                    .on_press(move || menu.set(None)),
            )
            .child(
                view()
                    .absolute()
                    .right(px(8.0))
                    .top(move || px(bounds.get().top + 46.0))
                    .w(px(194.0))
                    .max_w(percentage(0.94))
                    .h(menu_height)
                    .rounded(px(12.0))
                    .bg(rgba(0x262626fa))
                    .overflow_hidden()
                    .child(scroll().flex_1().min_h(px(0.0)).child(view()
                    .child(player_menu_row("Quality", SETTINGS, move || {
                        menu.set(Some(Menu::Quality))
                    }).disabled(offline))
                    .child(when(compact, move || player_menu_row("Playback speed", SPEED, move || {
                        menu.set(Some(Menu::Speed))
                    })))
                    .child(player_menu_row("Resize", EXPAND, move || {
                        menu.set(Some(Menu::Resize))
                    }))
                    .child(
                        player_menu_row("Picture-in-picture", PIP, move || {
                            if available.get_untracked() {
                                menu.set(None);
                                pip.enter();
                            }
                        })
                        .opacity(move || if available.get() { 1.0 } else { 0.38_f32 })
                        .accessibility_state(move || AccessibilityState {
                            disabled: !available.get(),
                            ..Default::default()
                        }),
                    )
                    .child(player_menu_row("Audio track", AUDIO, move || {
                        menu.set(Some(Menu::Audio))
                    }))
                    .child(player_menu_row("Cast", CAST, move || {
                        menu.set(None);
                        cast.show();
                    }).disabled(offline).opacity(move || if offline.get() { 0.45_f32 } else { 1.0 }))
                    .child(player_menu_row("Subtitle style", CC, move || {
                        menu.set(Some(Menu::Style))
                    }))
                    .child(player_menu_row("Info", player_info::INFORMATION, move || menu.set(Some(Menu::Info))))
                    .child(when(compact, move || player_menu_row("Queue", QUEUE, move || {
                        menu.set(Some(Menu::Queue))
                    })))
                    .child(when(move || compact() && has_chapters.get(), move || {
                        player_menu_row("Chapters", SEGMENTS, move || menu.set(Some(Menu::Segments)))
                    }))
                    .child(when(move || compact() && !has_chapters.get(), move || {
                        player_menu_row("Segments", SEGMENTS, move || menu.set(Some(Menu::Segments)))
                    }))
                    .child(when(compact, move || {
                        let loop_queue = loop_queue.clone();
                        dynamic(move || {
                            let loop_queue = loop_queue.clone();
                            let title = loop_state.with(|state| loop_title(&state.loop_mode));
                            player_menu_row(title, LOOP, move || loop_queue.update("loop", json!({})))
                        })
                    }))
                    .child(player_menu_row("Download", DOWNLOAD, move || {
                        if let Some(video) = watch.video() {
                            menu.set(None);
                            downloads.confirm("", json!([{"videoId":video.video_id,"title":video.title,"author":video.author}]));
                        }
                    }))
                    .child(player_menu_row("Add to queue", ADD, move || {
                        if let Some(video) = adding.video() {
                            queue.add(json!({"videoId":video.video_id,"title":video.title,"author":video.author}), || {});
                        }
                    }))
                    .child(player_menu_row("Share", SHARE, move || {
                        if let Some(video) = sharing.video() {
                            share_video(format!("https://www.youtube.com/watch?v={}", video.video_id));
                        }
                    }))))
            )
    }

    fn seek_live(&self) {
        let (Some(video), Some(status)) = (self.watch.video(), self.watch.status()) else {
            return;
        };
        let Some(offset) = status.live_offset else {
            return;
        };
        let core = self.settings.core();
        let watch = self.watch.clone();
        spawn(async move {
            if let Ok(advance) = core
                .execute::<f64>(json!({"op":"player_live_seek","offset":offset}))
                .await
                && let Some(current) = watch.status()
            {
                watch.seek(video.generation, current.position + advance);
            }
        });
    }

    fn preference_menu(&self, bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>) -> View {
        let p = self.palette;
        let menu = self.menu;
        let (title, entries) = match menu.get_untracked() {
            Some(Menu::Quality) => ("Quality", self.quality_choices()),
            Some(Menu::Audio) => ("Audio track", self.audio_choices()),
            Some(Menu::Speed) => {
                let watch = self.watch.clone();
                let write = self.settings.clone();
                (
                    "Playback speed",
                    view().child(each(
                        self.speeds,
                        |speed| speed.to_bits(),
                        move |speed, _| {
                            let watch = watch.clone();
                            let write = write.clone();
                            let speed = speed.get_untracked();
                            let label = if speed == 1.0 {
                                "1× (Normal)".into()
                            } else {
                                format!("{speed}×")
                            };
                            let selected = computed({
                                let watch = watch.clone();
                                move || watch.status().is_some_and(|status| status.rate == speed)
                            });
                            player_option(p, label, selected, move || {
                                watch.rate(speed);
                                write.update(json!({"playbackSpeed":speed}));
                                menu.set(None);
                            })
                        },
                    )),
                )
            }
            _ => (
                "Resize",
                view().children(
                    [
                        ("fit", "Fit"),
                        ("fill", "Fill"),
                        ("zoom", "Zoom"),
                        ("width", "Fixed width"),
                        ("height", "Fixed height"),
                    ]
                    .map(|(mode, label)| {
                        let fit = PlaybackFit::from_setting(mode);
                        let watch = self.watch.clone();
                        let write = self.settings.clone();
                        let selected = computed({
                            let watch = watch.clone();
                            move || watch.fit() == fit
                        });
                        player_option(p, label.into(), selected, move || {
                            watch.resize(fit);
                            write.update(json!({"resizeMode":mode}));
                            menu.set(None);
                        })
                    }),
                ),
            ),
        };
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(1)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .label("Close options")
                    .on_press(move || menu.set(None)),
            )
            .child(
                view()
                    .absolute()
                    .right(px(8.0))
                    .top(move || px(bounds.get().top + 46.0))
                    .w(px(255.0))
                    .max_w(percentage(0.94))
                    .max_h(percentage(0.8))
                    .rounded(px(12.0))
                    .bg(move || p.raised())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .pl(px(13.0))
                            .child(
                                text(title)
                                    .flex_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(15.0))
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(p, ui::CLOSE, "Close options", move || {
                                menu.set(None)
                            })),
                    )
                    .child(scroll().max_h(px(300.0)).child(entries)),
            )
    }

    fn quality_choices(&self) -> View {
        let watch = self.watch.clone();
        let controls = self.clone();
        view()
            .child(self.quality_choice("Auto".into(), 0, 0))
            .child(each(
                move || {
                    watch
                        .video()
                        .map_or_else(Vec::new, |video| video.qualities.to_vec())
                },
                |quality| (quality.height, quality.fps),
                move |quality, _| {
                    let quality = quality.get_untracked();
                    controls.quality_choice(quality.label, quality.height, quality.fps.unwrap_or(0))
                },
            ))
    }

    fn quality_choice(&self, label: String, height: u32, fps: u32) -> View {
        let watch = self.watch.clone();
        let selected = computed(move || {
            watch.video().is_some_and(|video| {
                video.quality_height == height && (fps == 0 || video.quality_fps == fps)
            })
        });
        let controls = self.clone();
        player_option(self.palette, label, selected, move || {
            controls.switch_stream(
                PlaybackSelection::Quality { height, fps },
                json!({"qualityHeight":height,"qualityFps":fps}),
            );
        })
    }

    fn audio_choices(&self) -> View {
        let watch = self.watch.clone();
        let controls = self.clone();
        view()
            .child(self.audio_choice("Default".into(), String::new()))
            .child(each(
                move || {
                    watch
                        .video()
                        .map_or_else(Vec::new, |video| video.audio_tracks.to_vec())
                },
                |audio| audio.key.clone(),
                move |audio, _| {
                    let audio = audio.get_untracked();
                    controls.audio_choice(audio.label, audio.key)
                },
            ))
    }

    fn audio_choice(&self, label: String, key: String) -> View {
        let watch = self.watch.clone();
        let current = key.clone();
        let selected = computed(move || {
            watch
                .video()
                .is_some_and(|video| video.audio_key == current)
        });
        let controls = self.clone();
        player_option(self.palette, label, selected, move || {
            controls.switch_stream(
                PlaybackSelection::Audio(key.clone()),
                json!({"audioKey":key}),
            );
        })
    }

    fn switch_stream(&self, selection: PlaybackSelection, settings: Value) {
        self.menu.set(None);
        let write = self.settings.clone();
        let accepted = move |result: Result<(), String>| match result {
            Ok(()) => write.update(settings),
            Err(message) => {
                spawn(async move {
                    dialog(Dialog {
                        id: 0,
                        title: Some("Could not change playback".into()),
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
        };
        if self.watch.remote() {
            self.cast.replace_stream(&selection, accepted);
        } else {
            self.watch.select_stream(selection, accepted);
        }
    }

    fn seek_bar(&self, bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>) -> View {
        let player = self.watch.clone();
        let state = self.watch.clone();
        let marks = self.watch.clone();
        let duration = computed({
            let watch = self.watch.clone();
            move || watch.status().and_then(|status| status.duration)
        });
        press()
            .label("Seek")
            .h(px(8.0))
            .justify_center()
            .on_press(move |event: &PressEvent| {
                let width = (bounds.get_untracked().width - 24.0).max(1.0);
                let fraction = event.position.x.0.clamp(0.0, width) / width;
                if let Some(video) = player.video()
                    && let Some(duration) = player.status().and_then(|status| status.duration)
                {
                    player.seek(video.generation, duration * f64::from(fraction));
                }
            })
            .child(
                view()
                    .h(px(3.0))
                    .w_full()
                    .bg(rgba(0xffffff66))
                    .rounded(px(1.0))
                    .overflow_hidden()
                    .child(
                        view()
                            .absolute()
                            .left(px(0.0))
                            .top(px(0.0))
                            .h_full()
                            .bg(rgba(0xffffff55))
                            .w({
                                let state = state.clone();
                                move || {
                                    percentage(
                                        duration
                                            .get()
                                            .filter(|n| *n > 0.0)
                                            .and_then(|total| {
                                                state.status().map(|status| {
                                                    (status.buffered_position / total)
                                                        .clamp(0.0, 1.0)
                                                        as f32
                                                })
                                            })
                                            .unwrap_or(0.0),
                                    )
                                }
                            }),
                    )
                    .child(view().h_full().bg(rgb(0xff0000)).w(move || {
                        percentage(state.status().map_or(0.0, |status| {
                            let duration = status.duration.unwrap_or(0.0);
                            if duration > 0.0 {
                                (status.position / duration).clamp(0.0, 1.0) as f32
                            } else {
                                0.0
                            }
                        }))
                    }))
                    .child(dynamic(move || {
                        let duration = duration.get().filter(|n| *n > 0.0)?;
                        let video = marks.video()?;
                        let starts = video
                            .chapters
                            .iter()
                            .filter(|chapter| chapter.start > 0.0 && chapter.start < duration);
                        let segments = if video.offline {
                            Vec::new()
                        } else {
                            marks.sponsor_segments(&video.video_id)
                        };
                        Some(
                            view()
                                .absolute()
                                .inset(px(0.0))
                                .pointer_events(PointerEvents::None)
                                .children(
                                    segments
                                        .into_iter()
                                        .filter(|segment| {
                                            segment.start < duration && segment.end >= segment.start
                                        })
                                        .map(|segment| {
                                            view()
                                                .absolute()
                                                .top(px(0.0))
                                                .h_full()
                                                .left(percentage((segment.start / duration) as f32))
                                                .w(percentage(
                                                    ((segment.end.min(duration) - segment.start)
                                                        / duration)
                                                        as f32,
                                                ))
                                                .bg(rgb(category(&segment.category).1))
                                        })
                                        .collect::<Vec<_>>(),
                                )
                                .children(
                                    starts
                                        .map(|chapter| {
                                            view()
                                                .absolute()
                                                .top(px(0.0))
                                                .h_full()
                                                .left(percentage((chapter.start / duration) as f32))
                                                .w(px(1.5))
                                                .bg(rgba(0x000000bb))
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        )
                    })),
            )
    }

    pub fn toggle(&self) {
        if let Some(video) = self.watch.video() {
            let player = self.watch.player();
            let status = self.watch.status();
            if status.as_ref().is_some_and(|status| status.playing) {
                player.pause(video.generation);
            } else {
                if status
                    .as_ref()
                    .is_some_and(|status| status.state == "ended")
                {
                    player.seek(video.generation, 0.0);
                }
                player.play(video.generation);
            }
            self.show();
        }
    }

    pub fn open_queue(&self) {
        self.menu.set(Some(Menu::Queue));
    }

    fn subtitle_menu(&self, bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>) -> View {
        let p = self.palette;
        let menu = self.menu;
        let player = self.watch.clone();
        let selected = self.subs.clone();
        let loading = self.subs.clone();
        let errors = self.subs.clone();
        let settings = self.settings.clone();
        let tracks = computed(move || {
            player
                .video()
                .map(|video| video.subtitles.to_vec())
                .unwrap_or_default()
        });
        view()
            .absolute()
            .inset(px(0.0))
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .label("Close subtitles")
                    .on_press(move || menu.set(None)),
            )
            .child(
                view()
                    .absolute()
                    .right(px(8.0))
                    .top(move || px(bounds.get().top + 46.0))
                    .w(px(255.0))
                    .max_w(percentage(0.94))
                    .max_h(px(300.0))
                    .rounded(px(12.0))
                    .bg(move || p.raised())
                    .child(
                        text("Subtitles")
                            .px(px(13.0))
                            .pt(px(10.0))
                            .pb(px(5.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(15.0))
                            .text_color(move || p.ink()),
                    )
                    .child(when(
                        move || tracks.with(Vec::is_empty),
                        move || {
                            text("No subtitles are available for this video.")
                                .px(px(13.0))
                                .py(px(9.0))
                                .text_size(px(13.0))
                                .text_color(move || p.muted())
                        },
                    ))
                    .child(scroll().max_h(px(220.0)).child(view().child(each(
                        tracks,
                        |track| track.id.clone(),
                        move |track, _| {
                            let selected = selected.clone();
                            let checked = computed({
                                let selected = selected.clone();
                                move || selected.key() == track.with(|track| track.id.clone())
                            });
                            let settings = settings.clone();
                            press()
                                .px(px(13.0))
                                .min_h(px(46.0))
                                .flex_row()
                                .items_center()
                                .on_press(move || {
                                    let key = if checked.get() {
                                        String::new()
                                    } else {
                                        track.with(|track| track.id.clone())
                                    };
                                    settings.update(json!({"subtitleKey":key}));
                                    menu.set(None);
                                })
                                .child(
                                    text(move || {
                                        track.with(|track| {
                                            if track.automatic
                                                && !track
                                                    .label
                                                    .to_ascii_lowercase()
                                                    .contains("auto-generated")
                                            {
                                                format!("{} (auto-generated)", track.label)
                                            } else {
                                                track.label.clone()
                                            }
                                        })
                                    })
                                    .flex_1()
                                    .text_size(px(14.0))
                                    .text_color(move || p.ink()),
                                )
                                .child(when(checked, move || {
                                    ui::icon(CHECK, 20.0, move || p.ink())
                                }))
                        },
                    ))))
                    .child(
                        press()
                            .min_h(px(46.0))
                            .px(px(13.0))
                            .justify_center()
                            .on_press(move || menu.set(Some(Menu::Style)))
                            .child(
                                text("Subtitle style")
                                    .text_size(px(14.0))
                                    .text_color(move || p.ink()),
                            ),
                    )
                    .child(when(
                        move || loading.loading(),
                        move || {
                            text("Loading…")
                                .px(px(13.0))
                                .py(px(8.0))
                                .text_size(px(13.0))
                                .text_color(move || p.muted())
                        },
                    ))
                    .child(
                        text(move || errors.error().unwrap_or_default())
                            .px(px(13.0))
                            .text_size(px(13.0))
                            .text_color(move || p.muted()),
                    ),
            )
    }
}

fn error_button(p: Palette, title: &'static str, action: impl Fn() + 'static) -> View {
    press()
        .label(title)
        .p(px(12.0))
        .on_press(action)
        .child(text(title).text_size(px(13.0)).text_color(move || p.ink()))
}

fn player_menu_row(title: &'static str, path: &'static str, action: impl Fn() + 'static) -> View {
    press()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(13.0))
        .min_h(px(48.0))
        .label(title)
        .on_press(action)
        .child(ui::icon(path, 24.0, || rgb(0xffffff).into()))
        .child(text(title).text_size(px(15.0)).text_color(rgb(0xffffff)))
}

fn loop_title(mode: &str) -> &'static str {
    match mode {
        "repeat" => "Loop one",
        "pause" => "Pause at end",
        "shuffle" => "Shuffle",
        _ => "Play next",
    }
}

fn player_option(
    p: Palette,
    title: String,
    selected: Memo<bool>,
    action: impl Fn() + 'static,
) -> View {
    press()
        .flex_row()
        .items_center()
        .px(px(13.0))
        .min_h(px(46.0))
        .label(title.clone())
        .on_press(action)
        .accessibility_state(move || AccessibilityState {
            selected: selected.get(),
            ..Default::default()
        })
        .child(
            text(title)
                .flex_1()
                .text_size(px(14.0))
                .text_color(move || p.ink()),
        )
        .child(when(selected, move || {
            ui::icon(CHECK, 20.0, move || p.ink())
        }))
}

fn player_button(
    path: &'static str,
    name: &'static str,
    size: f32,
    color: impl Fn() -> Hsla + 'static,
    action: impl Fn() + 'static,
) -> View {
    press()
        .size(px(size))
        .label(name)
        .items_center()
        .justify_center()
        .on_press(action)
        .child(ui::icon(path, 24.0, color))
}
