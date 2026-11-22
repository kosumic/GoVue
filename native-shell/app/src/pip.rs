use std::rc::Rc;

use govue_shell_runtime::{Core, EmbeddedVideoFrame, WatchPlayback};
use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::queue::Queue;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct State {
    video_id: String,
    available: bool,
    auto_enter: bool,
    audio_only: bool,
    actions: Vec<Value>,
    perform: String,
}

#[derive(Clone, PartialEq)]
struct Owner {
    id: String,
    generation: u64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Action {
    video_id: String,
    generation: u64,
    action: String,
    handoff: bool,
}

struct Request {
    owner: Option<Owner>,
    action: String,
    handoff: bool,
}

#[derive(Clone)]
pub struct Pip {
    watch: WatchPlayback,
    element: ElementRef,
    state: Signal<State>,
    scope: Memo<Option<Owner>>,
    supported: Signal<bool>,
    pub mode: Signal<bool>,
    error: Signal<Option<String>>,
    tx: mpsc::UnboundedSender<Request>,
}

impl Pip {
    pub fn new(
        core: Core,
        watch: WatchPlayback,
        queue: Queue,
        settings: Signal<Option<Map<String, Value>>>,
        open: impl Fn(String) + 'static,
    ) -> Self {
        let scope = computed({
            let watch = watch.clone();
            move || {
                watch.video().map(|video| Owner {
                    id: video.video_id,
                    generation: video.generation,
                })
            }
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let pip = Self {
            watch,
            element: element_ref(),
            state: signal(State::default()),
            scope,
            supported: signal(false),
            mode: signal(false),
            error: signal(None),
            tx,
        };
        let watched = pip.watch.clone();
        let playing = computed(move || watched.status().is_some_and(|status| status.playing));
        let queue_snapshot = queue.signal();
        let allowed = computed({
            let supported = pip.supported;
            move || {
                supported.get()
                    && settings.with(|prefs| {
                        prefs
                            .as_ref()
                            .is_some_and(|prefs| prefs.get("autoPip") == Some(&Value::Bool(true)))
                    })
            }
        });
        let queue_changes =
            computed(move || queue_snapshot.with(|state| (state.has_next, state.has_previous)));
        let mode = pip.mode;
        let background = computed(move || {
            settings.with(|settings| {
                settings.as_ref().is_some_and(|settings| {
                    settings.get("backgroundPlayback") == Some(&Value::Bool(true))
                })
            })
        });
        effect({
            let pip = pip.clone();
            move || {
                let _ = (
                    scope.get(),
                    playing.get(),
                    queue_changes.get(),
                    mode.get(),
                    background.get(),
                );
                if allowed.get() {
                    pip.send("snapshot".into(), false);
                } else {
                    pip.state.set(State::default());
                }
            }
        });
        spawn(pip.clone().run(core, queue, Rc::new(open), rx));
        pip
    }

    pub fn available(&self) -> bool {
        self.supported.get()
            && self.scope.with(Option::is_some)
            && self.state.with(|state| state.available)
    }

    pub fn enter(&self) {
        if self.available()
            && let Some(video) = self.scope.get_untracked()
        {
            self.error.set(None);
            self.element.command("enter", json!(video.generation));
        }
    }

    pub fn notice(&self) -> If {
        let mode = self.mode;
        let error = self.error;
        when(
            move || !mode.get() && error.with(Option::is_some),
            move || {
                view()
                    .absolute()
                    .bottom(px(20.0))
                    .left(px(14.0))
                    .right(px(14.0))
                    .rounded(px(10.0))
                    .bg(rgba(0x272727f5))
                    .px(px(12.0))
                    .py(px(8.0))
                    .pointer_events(PointerEvents::None)
                    .child(
                        text(move || error.get().unwrap_or_default())
                            .text_size(px(13.0))
                            .text_color(rgb(0xffffff)),
                    )
            },
        )
    }

    fn send(&self, action: String, handoff: bool) {
        let _ = self.tx.send(Request {
            owner: self.scope.get_untracked(),
            action,
            handoff,
        });
    }

    async fn run(
        self,
        core: Core,
        queue: Queue,
        open: Rc<dyn Fn(String)>,
        mut commands: mpsc::UnboundedReceiver<Request>,
    ) {
        let mut renderer = None;
        while let Some(request) = commands.recv().await {
            if request.owner != self.scope.get_untracked() {
                continue;
            }
            let playing = self.watch.status();
            let action = if request.action == "audio_complete" {
                "snapshot"
            } else {
                &request.action
            };
            let id = request.owner.as_ref().map_or("", |owner| owner.id.as_str());
            let result = core
                .execute::<State>(json!({
                    "op":"player_pip","videoId":id,"action":action,"active":request.owner.is_some(),
                    "pip":self.mode.get_untracked(),"handoff":request.handoff,
                    "playing":playing.as_ref().is_some_and(|status|status.playing),
                    "ended":playing.is_some_and(|status|status.state == "ended"),
                }))
                .await;
            let Ok(state) = result else {
                continue;
            };
            if request.owner != self.scope.get_untracked() || state.video_id != id {
                continue;
            }
            if let Some(owner) = request.owner {
                if renderer != Some((owner.generation, state.audio_only)) {
                    renderer = Some((owner.generation, state.audio_only));
                    self.watch
                        .player()
                        .enable_video(owner.generation, !state.audio_only);
                }
                match state.perform.as_str() {
                    "play" => self.watch.player().play(owner.generation),
                    "pause" => self.watch.player().pause(owner.generation),
                    "replay" => {
                        self.watch.player().seek(owner.generation, 0.0);
                        self.watch.player().play(owner.generation);
                    }
                    direction @ ("next" | "previous") => {
                        let open = open.clone();
                        queue.advance(direction, move |url| open(url));
                    }
                    "backgroundAudio" => self.element.command("audio", json!(owner.generation)),
                    _ => {}
                }
            } else {
                renderer = None;
            }
            if request.action == "audio_failed" {
                self.error
                    .set(Some("Could not switch to background audio.".into()));
            }
            self.state.set(state);
        }
    }

    pub fn view(
        &self,
        frame: impl Fn() -> EmbeddedVideoFrame + 'static,
        can_present: impl Fn() -> bool + 'static,
    ) -> NativeView {
        let pip = self.clone();
        let events = self.clone();
        nsh_core::element::native("govue/player-pip")
            .absolute().size(px(1.0)).opacity(0.0).pointer_events(PointerEvents::None)
            .element_ref(&self.element)
            .prop("ready", json!(true))
            .prop("config", move || {
                let state = pip.state.get();
                let bounds = frame();
                let owner = pip.scope.get().filter(|owner|owner.id == state.video_id);
                let active = can_present() || pip.mode.get();
                json!({
                    "videoId":state.video_id,"generation":owner.as_ref().map(|owner|owner.generation),
                    "available":state.available && owner.is_some(),
                    "autoEnter":state.auto_enter && owner.is_some() && active && bounds.visible,
                    "actions":state.actions,
                    "frame":(bounds.visible && !pip.mode.get()).then_some(json!({
                        "left":bounds.left,"top":bounds.top,"width":bounds.width,"height":bounds.height
                    }))
                })
            })
            .on_native_event(move |event: &NativeEvent| match event.name.as_ref() {
                "supported" => events.supported.set(event.data == true),
                "mode" => events.mode.set(event.data == true),
                "error" => events.error.set(event.data.as_str().map(str::to_owned)),
                "action" => {
                    if let Ok(action) = Action::deserialize(&event.data)
                        && events.scope.get_untracked().is_some_and(|owner|owner.id == action.video_id && owner.generation == action.generation)
                    {
                        events.send(action.action, action.handoff);
                    }
                }
                _ => {}
            })
    }
}
