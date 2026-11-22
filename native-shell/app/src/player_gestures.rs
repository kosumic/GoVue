use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use govue_shell_runtime::{Core, EmbeddedVideoFrame, WatchPlayback};
use nsh_core::{
    Haptic, HapticImpact, PointerEvents, PressEvent, TouchEvent, TouchPhase,
    element::{ClaimRequest, TouchClaim, haptic},
    prelude::*,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    player_window::PlayerWindow, reader_view::timestamp, settings_view::Preferences, ui::press,
};

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Seek,
    Volume,
    Brightness,
    Fullscreen,
    Toggle,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Self::Seek => "Seek",
            Self::Volume => "Volume",
            Self::Brightness => "Brightness",
            Self::Fullscreen => "Fullscreen",
            Self::Toggle => "Tap",
        }
    }

    fn wire(self) -> &'static str {
        match self {
            Self::Seek => "seek",
            Self::Volume => "volume",
            Self::Brightness => "brightness",
            Self::Fullscreen => "fullscreen",
            Self::Toggle => "toggle",
        }
    }
}

#[derive(Deserialize)]
struct Adjustment {
    kind: Kind,
    target: Option<f64>,
    accumulated: Option<i64>,
    enabled: Option<bool>,
    commit: Option<bool>,
}

struct Drag {
    generation: u64,
    id: u32,
    x: f32,
    y: f32,
    local_x: f32,
    position: f64,
    mode: Option<Kind>,
    held_rate: Option<f64>,
    moved: bool,
    levels: Option<(f64, f64)>,
}

#[derive(Clone, PartialEq)]
enum Feedback {
    Speed,
    Seek { target: f64, delta: f64 },
    Double(i64),
    Level { volume: bool, percent: f32 },
}

#[derive(Clone)]
pub struct PlayerGestures {
    core: Core,
    watch: WatchPlayback,
    settings: Preferences,
    drag: Rc<RefCell<Option<Drag>>>,
    pinch: Signal<Option<(f32, f32)>>,
    pub zoom: Signal<f32>,
    feedback: Signal<Option<Feedback>>,
    hold_revision: Signal<u64>,
    tap_revision: Signal<u64>,
    last_tap: Signal<Option<Instant>>,
    blocked_at: Signal<Option<Instant>>,
    last_seek: Signal<Option<(u64, Instant, f64)>>,
    started: Instant,
}

#[derive(Clone)]
pub struct GestureActions {
    pub show: Rc<dyn Fn()>,
    pub toggle_controls: Rc<dyn Fn()>,
    pub toggle_playback: Rc<dyn Fn()>,
    pub back: Rc<dyn Fn()>,
}

impl PlayerGestures {
    pub fn new(core: Core, watch: WatchPlayback, settings: Preferences) -> Self {
        Self {
            core,
            watch,
            settings,
            drag: Rc::new(RefCell::new(None)),
            pinch: signal(None),
            zoom: signal(1.0),
            feedback: signal(None),
            hold_revision: signal(0),
            tap_revision: signal(0),
            last_tap: signal(None),
            blocked_at: signal(None),
            last_seek: signal(None),
            started: Instant::now(),
        }
    }

    pub fn busy(&self) -> bool {
        self.feedback.with(Option::is_some)
    }

    pub fn cancel(&self) {
        self.reset_tap();
        self.blocked_at.set(None);
        self.last_seek.set(None);
        self.pinch.set(None);
        self.finish_drag();
    }

    fn configured(&self, name: &str, fullscreen: bool) -> bool {
        let layout = if fullscreen { "Fullscreen" } else { "Windowed" };
        self.settings.read(&format!("gesture{name}{layout}")) != false
    }

    fn reset_tap(&self) {
        self.tap_revision.update(|rev| *rev += 1);
        self.last_tap.set(None);
    }

    fn finish_drag(&self) {
        self.hold_revision.update(|rev| *rev += 1);
        if let Some(gesture) = self.drag.borrow_mut().take()
            && let Some(rate) = gesture.held_rate
        {
            self.watch.player().rate(gesture.generation, rate);
        }
        self.feedback.set(None);
    }

    fn touch(
        &self,
        event: &TouchEvent,
        bounds: EmbeddedVideoFrame,
        window: &PlayerWindow,
        actions: &GestureActions,
    ) {
        if self.pinch(event, window.presented.get_untracked()) {
            return;
        }
        if event.phase == TouchPhase::Start {
            if event.points.len() != 1 {
                self.finish_drag();
                self.reset_tap();
                self.blocked_at.set(Some(Instant::now()));
                return;
            }
            if let (Some(point), Some(video), Some(status)) = (
                event.points.first(),
                self.watch.video(),
                self.watch.status(),
            ) {
                window.refresh_levels();
                *self.drag.borrow_mut() = Some(Drag {
                    generation: video.generation,
                    id: point.id,
                    x: point.page.x.0,
                    y: point.page.y.0,
                    local_x: point.location.x.0,
                    position: status.position,
                    mode: None,
                    held_rate: None,
                    moved: false,
                    levels: None,
                });
                if self.configured("LongPress", window.presented.get_untracked()) {
                    let player = self.clone();
                    let revision = advance(self.hold_revision);
                    spawn(async move {
                        timer(Duration::from_millis(500)).await;
                        if player.hold_revision.get_untracked() == revision {
                            player.hold();
                        }
                    });
                }
            }
            return;
        }
        if event.phase == TouchPhase::Cancel {
            self.finish_drag();
            self.reset_tap();
            self.blocked_at.set(Some(Instant::now()));
            return;
        }
        let commit = event.phase == TouchPhase::End;
        let mut active = self.drag.borrow_mut();
        if let Some(gesture) = active.as_mut()
            && let Some(point) = event.points.iter().find(|point| point.id == gesture.id)
            && self
                .watch
                .video()
                .is_some_and(|video| video.generation == gesture.generation)
        {
            let dx = point.page.x.0 - gesture.x;
            let dy = point.page.y.0 - gesture.y;
            if gesture.moved || dx.abs().max(dy.abs()) > 12.0 {
                if !gesture.moved {
                    self.reset_tap();
                    self.hold_revision.update(|rev| *rev += 1);
                    if let Some(rate) = gesture.held_rate.take() {
                        self.watch.player().rate(gesture.generation, rate);
                    }
                    (actions.show)();
                    gesture.moved = true;
                }
                self.blocked_at.set(Some(Instant::now()));
                self.adjust(gesture, bounds, (dx, dy), commit, window, actions);
            }
        }
        if commit {
            let gesture = active.take();
            drop(active);
            self.hold_revision.update(|rev| *rev += 1);
            if let Some(rate) =
                gesture.and_then(|gesture| gesture.held_rate.map(|rate| (gesture.generation, rate)))
            {
                self.watch.player().rate(rate.0, rate.1);
                self.blocked_at.set(Some(Instant::now()));
            }
            if self
                .feedback
                .with_untracked(|feedback| !matches!(feedback, Some(Feedback::Double(_))))
            {
                self.feedback.set(None);
            }
        }
    }

    fn pinch(&self, event: &TouchEvent, fullscreen: bool) -> bool {
        if event.phase == TouchPhase::Cancel {
            return self
                .pinch
                .get_untracked()
                .inspect(|_| self.pinch.set(None))
                .is_some();
        }
        if self.pinch.with_untracked(Option::is_some) && event.phase == TouchPhase::End {
            if event.points.iter().all(|point| point.changed) {
                self.pinch.set(None);
            }
            self.blocked_at.set(Some(Instant::now()));
            return true;
        }
        if fullscreen && let [a, b, ..] = event.points.as_slice() {
            let distance = (a.page.x.0 - b.page.x.0).hypot(a.page.y.0 - b.page.y.0);
            if distance > 0.0 {
                if let Some((initial, zoom)) = self.pinch.get_untracked() {
                    self.zoom.set((zoom * distance / initial).clamp(1.0, 5.0));
                } else {
                    self.finish_drag();
                    self.reset_tap();
                    self.pinch.set(Some((distance, self.zoom.get_untracked())));
                }
                self.blocked_at.set(Some(Instant::now()));
                return true;
            }
        }
        self.pinch.with_untracked(Option::is_some)
    }

    fn hold(&self) {
        if let Some(gesture) = self.drag.borrow_mut().as_mut()
            && self
                .watch
                .video()
                .is_some_and(|video| video.generation == gesture.generation)
            && let Some(status) = self.watch.status()
        {
            gesture.held_rate = Some(status.rate);
            self.watch.player().rate(gesture.generation, 2.0);
            self.reset_tap();
            self.feedback.set(Some(Feedback::Speed));
            self.blocked_at.set(Some(Instant::now()));
            match self.settings.read("hapticStrength").as_u64().unwrap_or(50) {
                0 => {}
                1..=33 => haptic(Haptic::Impact(HapticImpact::Light)),
                34..=66 => haptic(Haptic::Impact(HapticImpact::Medium)),
                _ => haptic(Haptic::Impact(HapticImpact::Heavy)),
            }
        }
    }

    fn adjust(
        &self,
        gesture: &mut Drag,
        bounds: EmbeddedVideoFrame,
        delta: (f32, f32),
        commit: bool,
        window: &PlayerWindow,
        actions: &GestureActions,
    ) {
        let (volume, brightness) = gesture.levels.unwrap_or_else(|| window.levels());
        let duration = self
            .watch
            .status()
            .and_then(|status| status.duration)
            .unwrap_or_default();
        let result = self.core.state::<Adjustment>(json!({"op":"player_gesture", "width":bounds.width,"height":bounds.height,
            "dx":delta.0, "dy":delta.1, "x":gesture.local_x, "mode":gesture.mode.map(Kind::wire),
            "position":gesture.position,"duration":duration,"brightness":brightness,"volume":volume}));
        let Ok(result) = result else {
            return;
        };
        gesture.mode = Some(result.kind);
        if !self.configured(result.kind.key(), window.presented.get_untracked()) {
            return;
        }
        match result.kind {
            Kind::Seek => {
                if let Some(target) = result.target {
                    if commit {
                        self.watch.seek(gesture.generation, target);
                    } else {
                        self.feedback.set(Some(Feedback::Seek {
                            target,
                            delta: target - gesture.position,
                        }));
                    }
                }
            }
            Kind::Volume | Kind::Brightness => {
                if let Some(target) = result.target {
                    gesture.levels = Some((volume, brightness));
                    let volume = result.kind == Kind::Volume;
                    window.set_level(volume, target);
                    if !commit {
                        self.feedback.set(Some(Feedback::Level {
                            volume,
                            percent: (target * 100.0).round() as f32,
                        }));
                    }
                }
            }
            Kind::Fullscreen if commit && result.commit == Some(true) => {
                let enabled = result.enabled == Some(true);
                if !enabled && self.watch.offline() {
                    (actions.back)();
                } else {
                    window.set(enabled);
                }
            }
            _ => {}
        }
    }

    fn tap(&self, x: f32, width: f32, fullscreen: bool, actions: &GestureActions) {
        let now = Instant::now();
        if self
            .blocked_at
            .get_untracked()
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(400))
        {
            return;
        }
        let Some(video) = self.watch.video() else {
            return;
        };
        if self.configured("DoubleTap", fullscreen)
            && self
                .last_tap
                .get_untracked()
                .is_some_and(|at| now.duration_since(at) < Duration::from_millis(300))
        {
            self.reset_tap();
            self.double_tap(&video.video_id, video.generation, (x, width), now, actions);
            return;
        }
        self.last_tap.set(Some(now));
        let revision = advance(self.tap_revision);
        if self.configured("Tap", fullscreen) {
            let player = self.clone();
            let toggle = actions.toggle_controls.clone();
            spawn(async move {
                timer(Duration::from_millis(300)).await;
                if player.tap_revision.get_untracked() == revision
                    && player
                        .watch
                        .video()
                        .is_some_and(|current| current.generation == video.generation)
                {
                    toggle();
                }
            });
        }
    }

    fn double_tap(
        &self,
        video: &str,
        generation: u64,
        (x, width): (f32, f32),
        now: Instant,
        actions: &GestureActions,
    ) {
        let Some(status) = self.watch.status() else {
            return;
        };
        let position = self
            .last_seek
            .get_untracked()
            .filter(|(current, at, _)| {
                *current == generation && now.duration_since(*at) < Duration::from_millis(600)
            })
            .map_or(status.position, |(_, _, position)| position);
        let Ok(result) = self.core.state::<Adjustment>(json!({"op":"player_double_tap", "videoId":video,
            "owner":format!("native-{generation}"),"width":width,"x":x.clamp(0.0,width),
            "position":position,"duration":status.duration.unwrap_or_default(),"at":self.started.elapsed().as_millis() as u64})) else { return; };
        if result.kind == Kind::Toggle {
            (actions.toggle_playback)();
        } else if let Some((target, seconds)) = result.target.zip(result.accumulated) {
            self.last_seek.set(Some((generation, now, target)));
            self.watch.seek(generation, target);
            self.feedback.set(Some(Feedback::Double(seconds)));
            let player = self.clone();
            spawn(async move {
                timer(Duration::from_millis(650)).await;
                if player
                    .last_seek
                    .get_untracked()
                    .is_some_and(|(_, at, _)| at == now)
                    && matches!(player.feedback.get_untracked(), Some(Feedback::Double(_)))
                {
                    player.feedback.set(None);
                }
            });
        }
    }

    pub fn view(
        &self,
        bounds: Memo<EmbeddedVideoFrame>,
        window: PlayerWindow,
        actions: GestureActions,
    ) -> View {
        let touch = self.clone();
        let tap = self.clone();
        let tap_window = window.clone();
        let tap_actions = actions.clone();
        let feedback = self.feedback;
        let zoom = self.zoom;
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(PointerEvents::BoxNone)
            .child(
                view()
                    .absolute()
                    .inset(px(0.0))
                    .on_touch(move |event: &TouchEvent| {
                        touch.touch(event, bounds.get_untracked(), &window, &actions)
                    })
                    .claim_touches(TouchClaim::on_move().ask(|request: &ClaimRequest| {
                        let delta = request.event.gesture.delta;
                        if request.event.gesture.active_touches > 1
                            || delta.x.0.abs().max(delta.y.0.abs()) > 12.0
                        {
                            request.claim();
                        }
                    }))
                    .child(
                        press()
                            .absolute()
                            .inset(px(0.0))
                            .label("Show player controls")
                            .on_press(move |event: &PressEvent| {
                                tap.tap(
                                    event.position.x.0,
                                    bounds.get_untracked().width,
                                    tap_window.presented.get_untracked(),
                                    &tap_actions,
                                )
                            }),
                    ),
            )
            .child(dynamic(move || match feedback.get()? {
                Feedback::Speed => Some(
                    view()
                        .absolute()
                        .top(px(16.0))
                        .self_center()
                        .pointer_events(PointerEvents::None)
                        .rounded(px(32.0))
                        .bg(rgba(0x000000b8))
                        .px(px(16.0))
                        .py(px(8.0))
                        .child(label("2×  ››", 14.0)),
                ),
                Feedback::Seek { target, delta } => Some(
                    view()
                        .absolute()
                        .top(percentage(0.22))
                        .self_center()
                        .items_center()
                        .pointer_events(PointerEvents::None)
                        .rounded(px(16.0))
                        .border(px(0.5))
                        .border_color(rgba(0xffffff24))
                        .bg(rgba(0x00000066))
                        .px(px(14.0))
                        .py(px(5.0))
                        .child(label(timestamp(target), 18.0))
                        .child(label(format!("{delta:+.0}s"), 13.0)),
                ),
                Feedback::Double(seconds) => {
                    let bubble = view()
                        .absolute()
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(percentage(0.42))
                        .items_center()
                        .justify_center()
                        .rounded(px(100.0))
                        .bg(rgba(0xffffff24))
                        .pointer_events(PointerEvents::None)
                        .child(label(
                            if seconds > 0 {
                                "›››"
                            } else {
                                "‹‹‹"
                            },
                            22.0,
                        ))
                        .child(label(format!("{} seconds", seconds.abs()), 14.0));
                    Some(if seconds > 0 {
                        bubble.right(px(0.0))
                    } else {
                        bubble.left(px(0.0))
                    })
                }
                Feedback::Level { volume, percent } => {
                    let bubble = view()
                        .absolute()
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .justify_center()
                        .pointer_events(PointerEvents::None)
                        .child(
                            view()
                                .items_center()
                                .gap(px(8.0))
                                .rounded(px(16.0))
                                .border(px(0.5))
                                .border_color(rgba(0xffffff24))
                                .bg(rgba(0x00000066))
                                .p(px(6.0))
                                .child(label(if volume { "♪" } else { "☼" }, 18.0))
                                .child(
                                    view()
                                        .w(px(4.0))
                                        .h(px((bounds.get().height * 0.26).clamp(48.0, 112.0)))
                                        .rounded(px(2.0))
                                        .bg(rgba(0xffffff33))
                                        .justify_end()
                                        .overflow_hidden()
                                        .child(
                                            view()
                                                .w_full()
                                                .h(percentage(percent / 100.0))
                                                .bg(rgb(0xffffff)),
                                        ),
                                )
                                .child(label(format!("{percent:.0}%"), 13.0)),
                        );
                    let side = (bounds.get().width * 0.12).max(24.0);
                    Some(if volume {
                        bubble.left(px(side))
                    } else {
                        bubble.right(px(side))
                    })
                }
            }))
            .child(when(
                move || zoom.get() > 1.0,
                move || {
                    press()
                        .absolute()
                        .right(px(16.0))
                        .bottom(px(80.0))
                        .rounded(px(18.0))
                        .bg(rgba(0x000000bb))
                        .p(px(10.0))
                        .label("Reset zoom")
                        .on_press(move || zoom.set(1.0))
                        .child(
                            text(move || format!("Reset zoom · {:.1}×", zoom.get()))
                                .text_size(px(12.0))
                                .text_color(rgb(0xffffff)),
                        )
                },
            ))
    }
}

fn label(content: impl Into<String>, size: f32) -> Text {
    text(content.into())
        .text_size(px(size))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0xffffff))
}

fn advance(revision: Signal<u64>) -> u64 {
    let next = revision.get_untracked() + 1;
    revision.set(next);
    next
}
