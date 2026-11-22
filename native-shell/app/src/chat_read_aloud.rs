use govue_shell_runtime::{Core, WatchPlayback};
use nsh_core::{
    PointerEvents,
    element::{ElementRef, NativeEvent},
    prelude::*,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    chat::Message,
    ui::{self, Palette, press},
};

const SPEAKER: &str = "M3 9h4l5-4v14l-5-4H3zm13-1a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14";

#[derive(Deserialize)]
struct Script {
    chunks: Vec<String>,
}

#[derive(Deserialize)]
struct Finished {
    id: u64,
    error: Option<String>,
}

#[derive(Clone)]
pub struct ReadAloud {
    core: Core,
    watch: WatchPlayback,
    platform: ElementRef,
    revision: Signal<u64>,
    playing: Signal<Option<(String, u64)>>,
    error: Signal<Option<String>>,
    example_error: Signal<Option<String>>,
}

impl ReadAloud {
    pub fn new(core: Core, watch: WatchPlayback) -> Self {
        Self {
            core,
            watch,
            platform: element_ref(),
            revision: signal(0),
            playing: signal(None),
            error: signal(None),
            example_error: signal(None),
        }
    }

    pub fn active(&self) -> Option<String> {
        self.playing
            .with(|playing| playing.as_ref().map(|(message, _)| message.clone()))
    }

    pub fn clear(&self) {
        self.stop();
        self.error.set(None);
    }

    pub fn stop(&self) {
        if let Some((_, id)) = self.playing.get_untracked() {
            self.playing.set(None);
            self.platform.command("stop", json!({"id":id}));
        }
    }

    pub fn stop_example(&self) {
        if self
            .active()
            .is_some_and(|id| id.starts_with("word-example:"))
        {
            self.stop();
        }
        self.example_error.set(None);
    }

    pub fn example_error(&self) -> Option<String> {
        self.example_error.get()
    }

    pub fn speak_example(&self, key: &str, text: String, language: String) {
        let key = format!("word-example:{key}");
        let same = self.active().as_ref() == Some(&key);
        self.stop();
        self.example_error.set(None);
        if same || lifecycle() != nsh_core::platform::Lifecycle::Active {
            return;
        }
        let id = self.revision.get_untracked() + 1;
        self.revision.set(id);
        self.playing.set(Some((key, id)));
        self.pause_video();
        self.platform
            .command("read", json!({"id":id,"chunks":[text],"language":language}));
    }

    fn toggle(&self, message: Message) {
        let same = self
            .playing
            .with_untracked(|playing| playing.as_ref().is_some_and(|(id, _)| *id == message.id));
        self.clear();
        if same || lifecycle() != nsh_core::platform::Lifecycle::Active {
            return;
        }
        let id = self.revision.get_untracked() + 1;
        self.revision.set(id);
        self.playing.set(Some((message.id, id)));
        self.pause_video();
        let speech = self.clone();
        spawn(async move {
            let result = speech
                .core
                .execute::<Script>(
                    json!({"op":"response_speech","text":message.content,"maxLength":4000}),
                )
                .await;
            if !speech.owns(id) {
                return;
            }
            match result {
                Ok(script) => speech
                    .platform
                    .command("read", json!({"id":id,"chunks":script.chunks})),
                Err(error) => {
                    speech.playing.set(None);
                    speech.error.set(Some(error.to_string()));
                }
            }
        });
    }

    fn pause_video(&self) {
        if let Some(video) = self.watch.video()
            && self.watch.status().is_some_and(|status| status.playing)
        {
            self.watch.player().pause(video.generation);
        }
    }

    fn owns(&self, id: u64) -> bool {
        self.playing
            .with_untracked(|playing| playing.as_ref().is_some_and(|(_, request)| *request == id))
    }

    pub fn button(&self, p: Palette, message: ReadSignal<Message>, allowed: Memo<bool>) -> View {
        let reading = computed({
            let speech = self.clone();
            move || {
                speech
                    .active()
                    .is_some_and(|id| message.with(|message| message.id == id))
            }
        });
        let speech = self.clone();
        let disabled = computed(move || {
            !allowed.get() || message.with(|message| message.status == "streaming")
        });
        press()
            .size(px(38.0))
            .items_center()
            .justify_center()
            .label(move || {
                if reading.get() {
                    "Stop reading response"
                } else {
                    "Read response aloud"
                }
            })
            .disabled(disabled)
            .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0 })
            .accessibility_state(move || AccessibilityState {
                selected: reading.get(),
                disabled: disabled.get(),
                ..Default::default()
            })
            .on_press(move || speech.toggle(message.get_untracked()))
            .child(dynamic(move || {
                ui::icon(
                    if reading.get() { ui::STOP } else { SPEAKER },
                    17.0,
                    move || p.muted(),
                )
            }))
    }

    pub fn notice(&self, p: Palette) -> If {
        let error = self.error;
        when(
            move || error.with(Option::is_some),
            move || {
                text(move || error.get().unwrap_or_default())
                    .px(px(20.0))
                    .pb(px(8.0))
                    .text_size(px(13.0))
                    .text_color(move || p.ink())
            },
        )
    }

    pub fn native(&self) -> NativeView {
        let speech = self.clone();
        nsh_core::element::native("govue/chat-read-aloud")
            .element_ref(&self.platform)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "readAloud"
                    && let Ok(event) = Finished::deserialize(&event.data)
                    && speech.owns(event.id)
                {
                    let example = speech
                        .active()
                        .is_some_and(|id| id.starts_with("word-example:"));
                    speech.playing.set(None);
                    if example {
                        speech.example_error.set(event.error);
                    } else {
                        speech.error.set(event.error);
                    }
                }
            })
    }
}
