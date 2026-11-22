use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use govue_shell_runtime::{Core, EmbeddedVideoFrame, WatchPlayback};
use nsh_core::{
    PointerEvents, TouchEvent, TouchPhase,
    element::{ClaimRequest, TouchClaim},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::watch;

use crate::{
    browser::Browser,
    player_view::PlayerControls,
    queue::Queue,
    ui::{self, press},
};

const QUEUE: &str = "M3 6h18M3 12h13m-13 6h9m5-4 4 3-4 3z";
const EXPAND: &str =
    "M8 3H5a2 2 0 0 0-2 2v3m13-5h3a2 2 0 0 1 2 2v3m0 8v3a2 2 0 0 1-2 2h-3M3 16v3a2 2 0 0 0 2 2h3";
const BACK: &str = "M18 5 7 12l11 7zM5 5v14";
const NEXT: &str = "m6 5 11 7-11 7zM19 5v14";
const REPLAY: &str = "M3 12a9 9 0 1 1 9 9m-9-9V5m0 7h7";
const PAUSE: &str = "M9 5H6v14h3zm9 0h-3v14h3z";

#[derive(Clone, Copy, PartialEq)]
struct WindowSize {
    width: f32,
    height: f32,
    inset: f32,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
struct Placement {
    x: f32,
    y: f32,
    width: f32,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
struct Layout {
    #[serde(flatten)]
    frame: Placement,
    height: f32,
    right: f32,
    bottom: f32,
}

#[derive(Clone, Copy)]
struct Request {
    revision: u64,
    size: WindowSize,
    mode: &'static str,
    placement: Option<Placement>,
    pinch: bool,
}

struct Gesture {
    base: Placement,
    x: f32,
    y: f32,
    pinch: Option<f32>,
    placement: Option<Placement>,
}

#[derive(Clone)]
pub struct MiniPlayer {
    active: Memo<bool>,
    size: Memo<WindowSize>,
    layout: Signal<Option<Layout>>,
    revision: Signal<u64>,
    controls: Signal<bool>,
    gesture: Rc<RefCell<Option<Gesture>>>,
    moved_at: Rc<Cell<Option<Instant>>>,
    outgoing: watch::Sender<Option<Request>>,
}

impl MiniPlayer {
    pub fn new(core: Core, active: Memo<bool>, width: Signal<f32>, height: Signal<f32>) -> Self {
        let (outgoing, mut pending) = watch::channel(None::<Request>);
        let size = computed(move || WindowSize {
            width: width.get(),
            height: height.get(),
            inset: safe_area().bottom.0,
        });
        let mini = Self {
            active,
            size,
            outgoing,
            revision: signal(0),
            controls: signal(true),
            layout: signal(None),
            gesture: Rc::new(RefCell::new(None)),
            moved_at: Rc::new(Cell::new(None)),
        };
        let layout = mini.layout;
        let revision = mini.revision;
        spawn(async move {
            while pending.changed().await.is_ok() {
                let Some(request) = *pending.borrow_and_update() else {
                    continue;
                };
                let result = core.execute::<Layout>(json!({
                    "op":"player_mini","width":request.size.width,"height":request.size.height,
                    "bottomInset":request.size.inset,"mode":request.mode,"frame":request.placement,
                    "pinch":request.pinch,
                })).await;
                if active.get_untracked()
                    && revision.get_untracked() == request.revision
                    && let Ok(result) = result
                {
                    layout.set(Some(result));
                }
            }
        });
        effect({
            let mini = mini.clone();
            move || {
                let size = size.get();
                let active = active.get();
                mini.revision.update(|version| *version += 1);
                mini.gesture.borrow_mut().take();
                mini.moved_at.set(None);
                mini.controls.set(true);
                mini.layout.set(None);
                if active && size.width >= 32.0 && size.height >= 32.0 {
                    mini.send("settle", None, false);
                }
            }
        });
        mini
    }

    pub fn frame(&self) -> Option<EmbeddedVideoFrame> {
        if !self.active.get() {
            return None;
        }
        let size = self.size.get();
        let layout = self.layout.get().unwrap_or_else(|| {
            let width = 190.0_f32.min((size.width - 24.0).max(1.0));
            Layout {
                frame: Placement {
                    x: 0.0,
                    y: 0.0,
                    width,
                },
                height: (width * 9.0 / 16.0).floor().max(1.0),
                right: 12.0,
                bottom: 12.0 + (56.0 - size.inset).max(0.0),
            }
        });
        Some(EmbeddedVideoFrame {
            left: (size.width - layout.right - layout.frame.width + layout.frame.x).max(0.0),
            top: (size.height - layout.bottom - layout.height + layout.frame.y).max(0.0),
            width: layout.frame.width,
            height: layout.height,
            visible: size.width >= 32.0 && size.height >= 32.0,
        })
    }

    fn send(&self, mode: &'static str, placement: Option<Placement>, pinch: bool) {
        let request = Request {
            revision: self.revision.get_untracked(),
            size: self.size.get_untracked(),
            mode,
            placement,
            pinch,
        };
        self.outgoing.send_replace(Some(request));
    }

    fn touch(&self, event: &TouchEvent) {
        if !self.active.get_untracked() {
            return;
        }
        let mut gesture = self.gesture.borrow_mut();
        match event.phase {
            TouchPhase::Start => {
                if let Some(first) = event.points.first()
                    && let Some(layout) = self.layout.get_untracked()
                    && (gesture.is_none() || event.points.len() > 1)
                {
                    self.revision.update(|version| *version += 1);
                    *gesture = Some(Gesture {
                        base: layout.frame,
                        x: first.page.x.0,
                        y: first.page.y.0,
                        pinch: gap(event),
                        placement: None,
                    });
                }
            }
            TouchPhase::Move => {
                if let Some(gesture) = gesture.as_mut()
                    && let Some(first) = event.points.first()
                {
                    let mut placement = gesture.base;
                    if let Some(initial) = gesture.pinch {
                        let Some(distance) = gap(event) else {
                            return;
                        };
                        placement.width *= distance / initial;
                    } else {
                        let dx = first.page.x.0 - gesture.x;
                        let dy = first.page.y.0 - gesture.y;
                        if gesture.placement.is_none() && dx.hypot(dy) <= 8.0 {
                            return;
                        }
                        placement.x += dx;
                        placement.y += dy;
                    }
                    gesture.placement = Some(placement);
                    self.moved_at.set(Some(Instant::now()));
                    self.send("drag", Some(placement), gesture.pinch.is_some());
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                if event.phase == TouchPhase::End && event.points.iter().any(|point| !point.changed)
                {
                    return;
                }
                if let Some(gesture) = gesture.take()
                    && let Some(placement) = gesture.placement
                {
                    self.moved_at.set(Some(Instant::now()));
                    self.send(
                        if event.phase == TouchPhase::End {
                            "release"
                        } else {
                            "settle"
                        },
                        Some(placement),
                        gesture.pinch.is_some(),
                    );
                }
            }
        }
    }

    pub fn view(
        &self,
        player: WatchPlayback,
        browser: Browser,
        queue: Queue,
        actions: PlayerControls,
    ) -> If {
        let mini = self.clone();
        let frame = computed({
            let mini = mini.clone();
            move || mini.frame()
        });
        when(
            move || frame.with(|frame| frame.is_some_and(|frame| frame.visible)),
            move || {
                let touch = mini.clone();
                let moved = mini.moved_at.clone();
                let controls = mini.controls;
                let restore = browser.clone();
                let close = browser.clone();
                let previous = browser.clone();
                let next = browser.clone();
                let prior = queue.clone();
                let upcoming = queue.clone();
                let state = queue.signal();
                let shown = player.clone();
                let usage = actions.clone();
                let playback = actions.clone();
                view()
                    .absolute()
                    .z_index(2)
                    .rounded(px(16.0))
                    .overflow_hidden()
                    .left(move || px(frame.get().map_or(0.0, |frame| frame.left)))
                    .top(move || px(frame.get().map_or(0.0, |frame| frame.top)))
                    .w(move || px(frame.get().map_or(1.0, |frame| frame.width)))
                    .h(move || px(frame.get().map_or(1.0, |frame| frame.height)))
                    .on_touch(move |event: &TouchEvent| touch.touch(event))
                    .claim_touches(TouchClaim::on_move().ask(|request: &ClaimRequest| {
                        if request.event.gesture.active_touches > 1
                            || request
                                .event
                                .gesture
                                .delta
                                .x
                                .0
                                .hypot(request.event.gesture.delta.y.0)
                                > 8.0
                        {
                            request.claim();
                        }
                    }))
                    .child(
                        press()
                            .absolute()
                            .inset(px(0.0))
                            .label("Show mini-player controls")
                            .on_press(move || {
                                if moved
                                    .get()
                                    .is_none_or(|at| at.elapsed() >= Duration::from_millis(350))
                                {
                                    controls.update(|visible| *visible = !*visible);
                                }
                            }),
                    )
                    .child(when(
                        move || controls.get(),
                        move || {
                            let restore = restore.clone();
                            let close = close.clone();
                            let previous = previous.clone();
                            let next = next.clone();
                            let prior = prior.clone();
                            let upcoming = upcoming.clone();
                            let shown = shown.clone();
                            let usage = usage.clone();
                            let playback = playback.clone();
                            view()
                                .absolute()
                                .inset(px(0.0))
                                .bg(rgba(0x00000066))
                                .pointer_events(PointerEvents::BoxNone)
                                .child(
                                    view()
                                        .absolute()
                                        .left(px(0.0))
                                        .right(px(0.0))
                                        .top(px(0.0))
                                        .flex_row()
                                        .pointer_events(PointerEvents::BoxNone)
                                        .child(
                                            button(QUEUE, "Queue", 36.0)
                                                .on_press(move || usage.open_queue()),
                                        )
                                        .child(view().flex_1())
                                        .child(
                                            button(ui::CLOSE, "Close mini-player", 36.0)
                                                .on_press(move || close.dismiss_mini()),
                                        ),
                                )
                                .child(
                                    view()
                                        .absolute()
                                        .inset(px(0.0))
                                        .items_center()
                                        .justify_center()
                                        .pointer_events(PointerEvents::BoxNone)
                                        .child(
                                            button(EXPAND, "Return to video", 40.0)
                                                .on_press(move || restore.restore_mini()),
                                        ),
                                )
                                .child(
                                    view()
                                        .absolute()
                                        .left(px(0.0))
                                        .right(px(0.0))
                                        .bottom(px(4.0))
                                        .flex_row()
                                        .justify_center()
                                        .items_center()
                                        .gap(px(8.0))
                                        .pointer_events(PointerEvents::BoxNone)
                                        .child(
                                            button(BACK, "Previous", 30.0)
                                                .disabled(move || {
                                                    !state.with(|state| state.has_previous)
                                                })
                                                .on_press(move || {
                                                    let previous = previous.clone();
                                                    prior.advance("previous", move |url| {
                                                        previous.open(url)
                                                    });
                                                }),
                                        )
                                        .child(dynamic(move || {
                                            let playback = playback.clone();
                                            let status = shown.status();
                                            let playing = status
                                                .as_ref()
                                                .is_some_and(|status| status.playing);
                                            let ended = status
                                                .is_some_and(|status| status.state == "ended");
                                            let (icon, label) = if playing {
                                                (PAUSE, "Pause")
                                            } else if ended {
                                                (REPLAY, "Replay")
                                            } else {
                                                (ui::PLAY, "Play")
                                            };
                                            button(icon, label, 34.0)
                                                .on_press(move || playback.toggle())
                                        }))
                                        .child(
                                            button(NEXT, "Next", 30.0)
                                                .disabled(move || {
                                                    !state.with(|state| state.has_next)
                                                })
                                                .on_press(move || {
                                                    let next = next.clone();
                                                    upcoming
                                                        .advance("next", move |url| next.open(url));
                                                }),
                                        ),
                                )
                        },
                    ))
            },
        )
    }
}

fn gap(event: &TouchEvent) -> Option<f32> {
    let [first, second, ..] = &event.points[..] else {
        return None;
    };
    let distance = (first.page.x.0 - second.page.x.0).hypot(first.page.y.0 - second.page.y.0);
    (distance > 1.0).then_some(distance)
}

fn button(icon: &'static str, label: &'static str, size: f32) -> View {
    press()
        .size(px(size))
        .items_center()
        .justify_center()
        .label(label)
        .child(ui::icon(icon, 20.0, || rgb(0xffffff).into()))
}
