use std::time::Duration;

use govue_shell_runtime::{Core, CoreError};
use nsh_core::{
    PointerEvents,
    element::{ElementRef, NativeEvent},
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    chat::Chat,
    ui::{self, Palette, press},
};

const MIC: &str = "M9 5a3 3 0 0 1 6 0v7a3 3 0 1 1-6 0zM5 11v1a7 7 0 1 0 14 0v-1M12 19v3";
const ARC: &str = "M12 3a9 9 0 1 1-9 9";
const NO_SPEECH: &str = "No speech detected. Tap the microphone to try again.";

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Idle,
    Preparing,
    Listening,
    Finishing,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dictation {
    phase: Phase,
    interim_text: String,
    draft: Option<String>,
    no_speech: Option<bool>,
}

#[derive(Deserialize)]
struct Event {
    id: u64,
    kind: String,
    text: Option<String>,
    #[serde(rename = "final")]
    final_result: Option<bool>,
    message: Option<String>,
    settings: Option<bool>,
}

#[derive(Clone)]
pub struct ChatVoice {
    core: Core,
    platform: ElementRef,
    draft: Signal<String>,
    revision: Signal<u64>,
    owner: Signal<Option<u64>>,
    phase: Signal<Phase>,
    preview: Signal<String>,
    error: Signal<Option<(String, bool)>>,
}

impl ChatVoice {
    pub fn new(core: Core, draft: Signal<String>) -> Self {
        Self {
            core,
            platform: element_ref(),
            draft,
            revision: signal(0),
            owner: signal(None),
            phase: signal(Phase::Idle),
            preview: signal(String::new()),
            error: signal(None),
        }
    }

    pub fn active(&self) -> bool {
        self.owner.with(Option::is_some)
    }

    pub fn clear(&self) {
        self.cancel();
        self.error.set(None);
    }

    pub fn cancel(&self) {
        if let Some(id) = self.owner.get_untracked() {
            self.platform.command("cancel", json!({"id":id}));
            let _ = self.command(id, "cancel", json!({}));
            self.end(id, false);
        }
    }

    fn command(&self, id: u64, op: &str, mut values: Value) -> Result<Dictation, CoreError> {
        values["op"] = json!(op);
        values["sessionId"] = json!(format!("native-voice-{id}"));
        let reply = self.core.dictation::<Dictation>(values)?;
        self.phase.set(reply.phase);
        self.preview.set(reply.interim_text.clone());
        if let Some(draft) = &reply.draft {
            self.draft.set(draft.clone());
        }
        Ok(reply)
    }

    fn end(&self, id: u64, report_empty: bool) {
        if self.owner.get_untracked() == Some(id) {
            let result = self.command(id, "end", json!({}));
            self.owner.set(None);
            self.phase.set(Phase::Idle);
            self.preview.set(String::new());
            if report_empty && result.is_ok_and(|result| result.no_speech == Some(true)) {
                self.error.set(Some((NO_SPEECH.into(), false)));
            }
        }
    }

    fn fail(&self, message: String, needs_settings: bool) {
        self.cancel();
        self.error.set(Some((message, needs_settings)));
    }

    fn start(&self, chat: &Chat) {
        self.error.set(None);
        let id = self.revision.get_untracked() + 1;
        self.revision.set(id);
        let draft = self.draft.get_untracked();
        if let Err(error) = self.command(id, "begin", json!({"draft":draft})) {
            self.fail(error.to_string(), false);
            return;
        }
        self.owner.set(Some(id));
        let active = chat.current();
        let transcript = active
            .as_ref()
            .and_then(|thread| thread.transcript.as_ref())
            .map(|transcript| json!({"videoId":transcript.video_id,"trackId":transcript.track_id}));
        let chat_id = active.map(|thread| thread.id.clone());
        let voice = self.clone();
        spawn(async move {
            let fallback = || {
                voice
                    .core
                    .dictation(json!({"op":"hints","draft":draft}))
                    .unwrap_or_default()
            };
            let request = voice.core.ai::<Vec<String>>(json!({"op":"ai_chat_speech_hints","draft":draft,"chatId":chat_id,"transcript":transcript}));
            let hints = tokio::select! {
                result = request => result.unwrap_or_else(|_| fallback()),
                () = timer(Duration::from_millis(1500)) => fallback(),
            };
            if voice.owner.get_untracked() == Some(id) {
                voice
                    .platform
                    .command("start", json!({"id":id,"hints":hints}));
            }
        });
    }

    fn timeout(&self, id: u64, phase: Phase, message: &'static str) {
        let voice = self.clone();
        spawn(async move {
            timer(Duration::from_secs(10)).await;
            if voice.owner.get_untracked() == Some(id) && voice.phase.get_untracked() == phase {
                voice.fail(message.into(), false);
            }
        });
    }

    fn finish(&self, id: u64) {
        match self.phase.get_untracked() {
            Phase::Preparing => self.cancel(),
            Phase::Listening => {
                if self.command(id, "finish", json!({})).is_ok() {
                    self.platform.command("finish", json!({"id":id}));
                    self.timeout(
                        id,
                        Phase::Finishing,
                        "Speech recognition did not finish. Tap the microphone to try again.",
                    );
                }
            }
            Phase::Idle | Phase::Finishing => {}
        }
    }

    fn receive(&self, event: Event) {
        if self.owner.get_untracked() != Some(event.id) {
            return;
        }
        let id = event.id;
        match event.kind.as_str() {
            "starting" => self.timeout(
                id,
                Phase::Preparing,
                "Speech recognition did not start. Tap the microphone to try again.",
            ),
            "listening" => {
                let _ = self.command(id, "listening", json!({}));
            }
            "result" => {
                if self.phase.get_untracked() == Phase::Preparing {
                    let _ = self.command(id, "listening", json!({}));
                }
                if let Some((text, complete)) = event.text.zip(event.final_result)
                    && let Err(error) = self.command(
                        id,
                        "result",
                        json!({"text":text,"isFinal":complete,"draft":self.draft.get_untracked()}),
                    )
                {
                    self.fail(error.to_string(), false);
                }
            }
            "end" => self.end(id, true),
            "error" => self.fail(
                event
                    .message
                    .unwrap_or_else(|| "Could not start voice input.".into()),
                event.settings == Some(true),
            ),
            _ => {}
        }
    }

    pub fn native_view(&self) -> NativeView {
        let voice = self.clone();
        nsh_core::element::native("govue/chat-speech")
            .element_ref(&self.platform)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "voice"
                    && let Ok(event) = Event::deserialize(&event.data)
                {
                    voice.receive(event);
                }
            })
    }

    pub fn button(&self, p: Palette, chat: Chat, input: ElementRef) -> View {
        let phase = self.phase;
        let owner = self.owner;
        let voice = self.clone();
        let enabled = chat.clone();
        press()
            .size(px(42.0))
            .rounded(px(21.0))
            .items_center()
            .justify_center()
            .label(move || {
                if owner.with(Option::is_some) {
                    "Stop voice input"
                } else {
                    "Start voice input"
                }
            })
            .disabled(move || phase.get() == Phase::Finishing || enabled.loading())
            .on_press(move || {
                input.blur();
                if let Some(id) = owner.get_untracked() {
                    voice.finish(id);
                } else {
                    voice.start(&chat);
                }
            })
            .child(dynamic(move || {
                ui::icon(
                    match phase.get() {
                        Phase::Idle => MIC,
                        Phase::Preparing | Phase::Finishing => ARC,
                        Phase::Listening => ui::STOP,
                    },
                    22.0,
                    move || p.ink(),
                )
            }))
    }

    pub fn preview(&self, p: Palette) -> If {
        let preview = self.preview;
        when(
            move || preview.with(|text| !text.is_empty()),
            move || {
                text(move || preview.get())
                    .label("Speech preview")
                    .number_of_lines(3)
                    .px(px(18.0))
                    .text_size(px(13.0))
                    .text_color(move || p.muted())
            },
        )
    }

    pub fn status(&self, p: Palette) -> If {
        let phase = self.phase;
        when(
            move || phase.get() != Phase::Idle,
            move || {
                text(move || match phase.get() {
                    Phase::Preparing => "Starting voice input…",
                    Phase::Listening => "Listening… Tap the microphone to finish.",
                    Phase::Finishing => "Finishing voice input…",
                    Phase::Idle => "",
                })
                .px(px(18.0))
                .pt(px(4.0))
                .text_size(px(11.0))
                .text_color(move || p.muted())
            },
        )
    }

    pub fn error(&self, p: Palette) -> If {
        let error = self.error;
        let platform = self.platform;
        when(
            move || error.with(Option::is_some),
            move || {
                view()
                    .px(px(16.0))
                    .gap(px(3.0))
                    .child(
                        text(move || error.get().map_or_else(String::new, |(error, _)| error))
                            .text_size(px(13.0))
                            .text_color(rgb(0xff0000)),
                    )
                    .child(when(
                        move || error.get().is_some_and(|(_, settings)| settings),
                        move || {
                            press()
                                .h(px(44.0))
                                .self_start()
                                .justify_center()
                                .label("Open Settings")
                                .on_press(move || platform.command("settings", Value::Null))
                                .child(
                                    text("Open Settings")
                                        .text_size(px(13.0))
                                        .text_color(move || p.ink()),
                                )
                        },
                    ))
            },
        )
    }
}
