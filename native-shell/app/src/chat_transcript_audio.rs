use govue_shell_runtime::TranscriptReader;
use nsh_core::{platform::Lifecycle, prelude::*};
use serde_json::json;

use crate::{
    caption_audio::{CaptionAudio, Phase},
    chat::Chat,
    chat_read_aloud::ReadAloud,
    ui::{self, Palette},
    word_tools::CaptionLine,
};

#[derive(Clone, PartialEq)]
struct Selection {
    chat: String,
    line: CaptionLine,
}

#[derive(Clone)]
pub struct TranscriptAudio {
    chat: Chat,
    audio: CaptionAudio,
    speech: ReadAloud,
    selected: Signal<Option<Selection>>,
    owner: Owner,
}

impl TranscriptAudio {
    pub fn new(
        chat: Chat,
        audio: CaptionAudio,
        speech: ReadAloud,
        reader: TranscriptReader,
    ) -> Self {
        let playback = Self {
            chat,
            audio,
            speech,
            selected: signal(None),
            owner: Owner::current(),
        };
        effect({
            let playback = playback.clone();
            let current_chat = computed({
                let chat = playback.chat.clone();
                move || chat.current().as_ref().map(|chat| chat.id.clone())
            });
            move || {
                let same = playback.selected.with(|selected| {
                    selected.as_ref().is_some_and(|selected| {
                        current_chat.with(|current| current.as_ref() == Some(&selected.chat))
                    })
                });
                if !reader.visible() || !same || lifecycle() != Lifecycle::Active {
                    playback.stop();
                }
            }
        });
        playback
    }

    pub fn play(&self, line: CaptionLine) {
        let Some(chat) = self.chat.current() else {
            return;
        };
        let command = json!({
            "op":"ai_chat_audio","chatId":chat.id,"videoId":line.video,"trackId":line.track,
            "cueId":format!("cue-{}",line.index),
        });
        self.selected.set(Some(Selection {
            chat: chat.id.clone(),
            line,
        }));
        let audio = self.audio.clone();
        let speech = self.speech.clone();
        self.owner.run(|| {
            spawn(async move {
                let _ = audio.play(command, move || speech.stop()).await;
            })
        });
    }

    pub fn position(&self, video: &str, track: &str) -> Option<f64> {
        let elapsed = self.audio.transcript_elapsed()?;
        self.selected.with(|selection| {
            let line = &selection.as_ref()?.line;
            (line.video == video && line.track == track).then_some(line.start + elapsed)
        })
    }

    fn stop(&self) {
        self.selected.set(None);
        if self.audio.is_transcript() {
            self.audio.stop();
        }
    }

    pub fn bar(&self, p: Palette) -> View {
        let audio = self.audio.clone();
        let phase = self.audio.phase;
        let error = self.audio.error;
        let active = self.selected;
        let visible = computed(move || active.with(Option::is_some) && audio.is_transcript());
        let stop = self.clone();
        view()
            .child(when(
                move || visible.get() && phase.get() != Phase::Idle,
                move || {
                    let stop = stop.clone();
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(20.0))
                        .child(
                            text(move || {
                                if phase.get() == Phase::Loading {
                                    "Loading line audio…"
                                } else {
                                    "Playing line audio"
                                }
                            })
                            .text_size(px(13.0))
                            .text_color(move || p.muted()),
                        )
                        .child(
                            ui::icon_button(p, ui::STOP, "Stop line audio", move || stop.stop())
                                .size(px(40.0)),
                        )
                },
            ))
            .child(when(
                move || visible.get() && error.with(Option::is_some),
                move || {
                    text(move || error.get().unwrap_or_default())
                        .px(px(20.0))
                        .text_size(px(13.0))
                        .text_color(rgb(0xd32f2f))
                },
            ))
    }
}
