use std::rc::Rc;
use std::time::Duration;

use govue_shell_runtime::{PlaybackEvent, TranscriptReader, WatchPlayback, WatchSubtitles};
use nsh_core::{PointerEvents, PressEvent, prelude::*};
use serde_json::{Value, json};

use crate::chat::Chat;
use crate::queue::Queue;
use crate::queue_view::queue_sheet;
use crate::reader_view::timestamp;
use crate::ui::{self, Palette, press};

const CC: &str = "M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zm6 5a2 2 0 1 0 0 4m8-4a2 2 0 1 0 0 4";
const TRANSCRIPT: &str =
    "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zm0 0v6h6M8 13h8m-8 4h8";
const SPARKLES: &str =
    "m9.9 2 1.9 5.8L18 10l-6.2 2.1-1.9 5.9-2-5.9L2 10l5.9-2.2zM19 12v8m-4-4h8m-4-15v6m-3-3h6";
const PAUSE: &str = "M8 4h2v16H8zM15 4h2v16h-2z";
const BACK: &str = "m12 19-7-7 7-7M5 12h14";
const CHECK: &str = "m4 12 5 5L20 6";
const PREVIOUS: &str = "M19 20 9 12l10-8zM5 4v16";
const NEXT: &str = "m5 4 10 8-10 8zM19 4v16";
const SETTINGS: &str = "M12 8a4 4 0 1 0 0 8 4 4 0 1 0 0-8zM9 3h6l1 3 3-1 2 4-2 3 2 3-2 4-3-1-1 3H9l-1-3-3 1-2-4 2-3-2-3 2-4 3 1z";
const QUEUE: &str = "M3 6h18M3 12h18M3 18h10m4-2 4 2-4 2z";

type WriteSetting = Rc<dyn Fn(Value)>;
type OpenVideo = Rc<dyn Fn(String)>;

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Subtitles,
    More,
    Queue,
}

#[derive(Clone)]
pub struct PlayerControls {
    watch: WatchPlayback,
    subs: WatchSubtitles,
    reader: TranscriptReader,
    chat: Chat,
    palette: Palette,
    queue: Queue,
    settings: WriteSetting,
    visible: Signal<bool>,
    menu: Signal<Option<Menu>>,
    activity: Signal<u64>,
    ended: Signal<Option<u64>>,
}

impl PlayerControls {
    pub fn new(
        watch: WatchPlayback,
        subs: WatchSubtitles,
        reader: TranscriptReader,
        chat: Chat,
        palette: Palette,
        queue: Queue,
        settings: impl Fn(Value) + 'static,
    ) -> Self {
        let controller = Self {
            watch,
            subs,
            reader,
            chat,
            palette,
            queue,
            settings: Rc::new(settings),
            visible: signal(true),
            menu: signal(None),
            activity: signal(0),
            ended: signal(None),
        };
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
                {
                    let controller = controller.clone();
                    spawn(async move {
                        timer(Duration::from_secs(3)).await;
                        if controller.activity.get_untracked() == revision
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
            move || watch.video().map(|video| video.video_id)
        });
        effect({
            let controller = controller.clone();
            move || {
                let _ = video.get();
                controller.menu.set(None);
                controller.show();
            }
        });
        controller
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
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
        let was_open = self.menu.with_untracked(Option::is_some);
        if was_open {
            self.menu.set(None);
        }
        was_open
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
            .is_some_and(|video| video.generation == *generation)
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
        width: impl Fn() -> f32 + 'static,
        back: impl Fn() + 'static,
        open: impl Fn(String) + 'static,
    ) -> View {
        let watch = self.watch.clone();
        let bounds = computed(move || watch.frame(width()));
        let panel = self.clone();
        let visible = self.visible;
        let tap = self.clone();
        let back = Rc::new(back);
        let open: OpenVideo = Rc::new(open);
        let navigate = open.clone();
        let menu = self.menu;
        let popup = self.clone();
        let more = self.clone();
        let queue = self.queue.clone();
        let palette = self.palette;
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(PointerEvents::BoxNone)
            .child(when(
                move || bounds.get().visible,
                move || {
                    let tap = tap.clone();
                    let panel = panel.clone();
                    let back = back.clone();
                    let open = navigate.clone();
                    view()
                        .absolute()
                        .left(move || px(bounds.get().left))
                        .top(move || px(bounds.get().top))
                        .w(move || px(bounds.get().width))
                        .h(move || px(bounds.get().height))
                        .overflow_hidden()
                        .pointer_events(PointerEvents::BoxNone)
                        .child(
                            press()
                                .absolute()
                                .inset(px(0.0))
                                .label("Show player controls")
                                .on_press(move || {
                                    if tap.visible.get_untracked() {
                                        tap.visible.set(false);
                                    } else {
                                        tap.show();
                                    }
                                }),
                        )
                        .child(when(
                            move || visible.get(),
                            move || panel.chrome(bounds, back.clone(), open.clone()),
                        ))
                },
            ))
            .child(when(
                move || menu.get() == Some(Menu::Subtitles) && bounds.get().visible,
                move || popup.subtitle_menu(bounds),
            ))
            .child(when(
                move || menu.get() == Some(Menu::More) && bounds.get().visible,
                move || more.more_menu(bounds),
            ))
            .child(when(
                move || menu.get() == Some(Menu::Queue) && bounds.get().visible,
                move || queue_sheet(palette, queue.clone(), open.clone(), move || menu.set(None)),
            ))
    }

    fn chrome(
        &self,
        bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>,
        back: Rc<impl Fn() + 'static>,
        open: OpenVideo,
    ) -> View {
        let menu = self.menu;
        let play = self.clone();
        let state = self.watch.clone();
        let paused = computed(move || !state.status().is_some_and(|status| status.playing));
        let clock = self.watch.clone();
        let captions = self.subs.clone();
        let transcript = self.reader.clone();
        let chat = self.chat.clone();
        let video = self.watch.clone();
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
                    .child(player_button(
                        BACK,
                        "Navigate back",
                        48.0,
                        || rgb(0xffffff).into(),
                        move || back(),
                    ))
                    .child(view().flex_1())
                    .child(player_button(
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
                    ))
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
                            .child(
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
                                .text_color(rgb(0xffffff)),
                            )
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
                            )),
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
        let enabled = computed(move || queue.signal().with(&available));
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

    fn more_menu(&self, bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>) -> View {
        let menu = self.menu;
        view()
            .absolute()
            .inset(px(0.0))
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
                    .w(px(240.0))
                    .max_w(percentage(0.94))
                    .rounded(px(12.0))
                    .bg(rgba(0x262626fa))
                    .child(
                        press()
                            .flex_row()
                            .items_center()
                            .gap(px(12.0))
                            .px(px(13.0))
                            .min_h(px(48.0))
                            .label("Queue")
                            .on_press(move || menu.set(Some(Menu::Queue)))
                            .child(ui::icon(QUEUE, 24.0, || rgb(0xffffff).into()))
                            .child(text("Queue").text_size(px(15.0)).text_color(rgb(0xffffff))),
                    ),
            )
    }

    fn seek_bar(&self, bounds: Memo<govue_shell_runtime::EmbeddedVideoFrame>) -> View {
        let player = self.watch.clone();
        let state = self.watch.clone();
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
                    player
                        .player()
                        .seek(video.generation, duration * f64::from(fraction));
                }
            })
            .child(
                view()
                    .h(px(3.0))
                    .w_full()
                    .bg(rgba(0xffffff66))
                    .rounded(px(1.0))
                    .child(view().h_full().bg(rgb(0xff0000)).w(move || {
                        percentage(state.status().map_or(0.0, |status| {
                            let duration = status.duration.unwrap_or(0.0);
                            if duration > 0.0 {
                                (status.position / duration).clamp(0.0, 1.0) as f32
                            } else {
                                0.0
                            }
                        }))
                    })),
            )
    }

    fn toggle(&self) {
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
                                    settings(json!({"subtitleKey":key}));
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
