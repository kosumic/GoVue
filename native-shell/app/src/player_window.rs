use govue_shell_runtime::{Core, EmbeddedVideoFrame};
use nsh_core::{PointerEvents, element::NativeEvent, platform::Lifecycle, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Deserialize)]
struct Surface {
    fullscreen: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Orientation {
    degrees: i32,
    system_auto_rotate: bool,
}

#[derive(Clone)]
pub struct PlayerWindow {
    actions: mpsc::UnboundedSender<Value>,
    fullscreen: Signal<bool>,
    pub presented: Memo<bool>,
    observe: Memo<bool>,
}

impl PlayerWindow {
    pub fn new(
        core: Core,
        playing_video: impl Fn() -> bool + 'static,
        can_present: impl Fn() -> bool + 'static,
    ) -> Self {
        let fullscreen = signal(false);
        let (actions, mut pending) = mpsc::unbounded_channel();
        let _ = actions.send(json!({"op":"player_surface_reset"}));
        spawn(async move {
            while let Some(command) = pending.recv().await {
                if let Ok(surface) = core.execute::<Surface>(command).await {
                    fullscreen.set(surface.fullscreen);
                }
            }
        });
        let visible = computed(can_present);
        Self {
            actions,
            fullscreen,
            presented: computed(move || visible.get() && fullscreen.get()),
            observe: computed(move || {
                lifecycle() == Lifecycle::Active
                    && visible.get()
                    && (playing_video() || fullscreen.get())
            }),
        }
    }

    pub fn toggle(&self) {
        self.set(!self.fullscreen.get_untracked());
    }

    pub fn frame(&self, width: f32, height: f32, transcript: bool) -> Option<EmbeddedVideoFrame> {
        if !self.presented.get() {
            return None;
        }
        let beside = transcript && width > height;
        Some(EmbeddedVideoFrame {
            left: 0.0,
            top: 0.0,
            width: if beside {
                width - (width * 0.4).min(400.0)
            } else {
                width
            }
            .max(1.0),
            height: if transcript && !beside {
                (height * 0.45).min(width * 9.0 / 16.0)
            } else {
                height
            }
            .max(1.0),
            visible: true,
        })
    }

    pub fn close(&self) -> bool {
        if self.fullscreen.get_untracked() {
            self.set(false);
            return true;
        }
        false
    }

    pub fn set(&self, value: bool) {
        self.update(json!({"op":"player_fullscreen","enabled":value}));
    }

    fn update(&self, value: Value) {
        let _ = self.actions.send(value);
    }

    pub fn view(&self) -> NativeView {
        let window = self.clone();
        let presented = self.presented;
        let observe = self.observe;
        nsh_core::element::native("govue/player-window")
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .prop("fullscreen", move || json!(presented.get()))
            .prop("observe", move || json!(observe.get()))
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "orientation"
                    && window.observe.get_untracked()
                    && let Ok(sample) = Orientation::deserialize(&event.data)
                {
                    window.update(json!({"op":"player_orientation","visible":true,
                        "degrees":sample.degrees,"systemAutoRotate":sample.system_auto_rotate}));
                }
            })
    }
}
