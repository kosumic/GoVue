use std::time::Duration;

use govue_shell_runtime::Core;
use nsh_core::prelude::*;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    caption_audio::CaptionAudio,
    chat::Chat,
    chat_read_aloud::ReadAloud,
    chat_view::{Model, model_choices, provider_models},
    provider_view::ProviderView,
    ui::{self, Palette, press},
};

const SPARKLES: &str = "M9 2 11 7 16 9 11 11 9 16 7 11 2 9 7 7zm10 11v8m-4-4h8";
const BOOK: &str = "M12 6C9 3 5 3 2 4v15c3-1 7-1 10 2 3-3 7-3 10-2V4c-3-1-7-1-10 2v15";
const SPEAKER: &str = "M3 9h4l5-4v14l-5-4H3zm13-1a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14";

#[derive(Clone, Copy, PartialEq)]
enum Picker {
    Models,
    Providers,
}

#[derive(Clone)]
pub struct WordHelp {
    chat: Chat,
    providers: ProviderView,
    speech: ReadAloud,
    audio: CaptionAudio,
    p: Palette,
    example: Assistance<Example>,
    grammar: Assistance<Grammar>,
    active: Memo<Option<Model>>,
    picker: Signal<Option<Picker>>,
    saving: Signal<bool>,
    error: Signal<Option<String>>,
    owner: Owner,
}

impl WordHelp {
    pub fn new(
        chat: Chat,
        providers: ProviderView,
        speech: ReadAloud,
        audio: CaptionAudio,
        p: Palette,
    ) -> Self {
        let active = computed({
            let chat = chat.clone();
            move || {
                let config = chat.config();
                let id = config["activeId"].as_str()?;
                let saved_model = config["providers"]
                    .as_array()?
                    .iter()
                    .find(|provider| provider["id"] == id)
                    .and_then(|provider| provider["model"].as_str());
                provider_models(&config).into_iter().find(|model| {
                    model.provider == id && (model.group || saved_model == Some(&model.id))
                })
            }
        });
        let panel = Self {
            example: Assistance::new(chat.core(), "example"),
            grammar: Assistance::new(chat.core(), "grammar"),
            chat,
            providers,
            speech,
            audio,
            p,
            active,
            picker: signal(None),
            saving: signal(false),
            error: signal(None),
            owner: Owner::current(),
        };
        effect({
            let example = panel.example.clone();
            let grammar = panel.grammar.clone();
            let speech = panel.speech.clone();
            move || {
                active.get();
                example.clear();
                grammar.clear();
                speech.stop_example();
            }
        });
        panel
    }

    pub fn refresh(&self) {
        if self.chat.config().is_null() {
            self.chat.read_config();
        }
    }

    pub fn clear(&self) {
        self.example.clear();
        self.grammar.clear();
        self.speech.stop_example();
    }

    pub fn close(&self) {
        self.picker.set(None);
        self.error.set(None);
        self.clear();
    }

    pub fn stop_speech(&self) {
        self.speech.stop_example();
    }

    pub fn back(&self) -> bool {
        match self.picker.get_untracked() {
            None => false,
            Some(Picker::Models) => {
                self.picker.set(None);
                self.error.set(None);
                true
            }
            Some(Picker::Providers) => {
                if !self.providers.back() {
                    self.picker.set(Some(Picker::Models));
                }
                true
            }
        }
    }

    pub fn contents(
        &self,
        pending: Signal<bool>,
        selected: impl Fn() -> Option<Value> + Clone + 'static,
    ) -> View {
        let p = self.p;
        let active = self.active;
        let available = computed(move || {
            !pending.get()
                && active.with(|model| model.as_ref().is_some_and(|model| model.connected))
        });
        let example = self.example.clone();
        let grammar = self.grammar.clone();
        let example_result = example.result;
        let grammar_result = grammar.result;
        let read = self.clone();
        let speech = self.speech.clone();
        let picker = self.picker;
        view().gap(px(14.0))
            .child(task_button(p, SPARKLES, "Generate example", "Generating example…", available,
                example.busy, {
                    let selected = selected.clone();
                    move || if let Some(command) = selected() { example.generate(command); }
                }))
            .child(error_text(self.example.error))
            .child(when(move || example_result.with(Option::is_some), move || read.example_view()))
            .child(dynamic(move || match speech.example_error() {
                Some(error) => failure(error),
                None => text(""),
            }))
            .child(task_button(p, BOOK, "Explain grammar", "Explaining grammar…", available,
                grammar.busy, move || if let Some(command) = selected() { grammar.generate(command); }))
            .child(error_text(self.grammar.error))
            .child(when(move || grammar_result.with(Option::is_some), move || {
                view().gap(px(8.0))
                    .child(label(p, move || grammar_result.with(|result| result.as_ref()
                        .map(|result| format!("Grammar · {}", result.language)).unwrap_or_default())))
                    .child(text(move || grammar_result.with(|result| result.as_ref()
                        .map(|result| format!("Base form: {}", result.base_form)).unwrap_or_default()))
                        .selectable(true).font_weight(FontWeight::SEMIBOLD).text_size(px(15.0)).text_color(move || p.ink()))
                    .child(answer(p, move || grammar_result.with(|result| result.as_ref()
                        .map(|result| result.explanation.clone()).unwrap_or_default())))
            }))
            .child(press().label("Choose AI provider and model").min_h(px(64.0))
                .rounded(px(10.0)).border(px(0.5)).border_color(move || p.raised())
                .px(px(12.0)).py(px(8.0)).flex_row().items_center().justify_between()
                .on_press(move || picker.set(Some(Picker::Models)))
                .child(view().gap(px(4.0))
                    .child(label(p, || "AI provider · examples & grammar".into()))
                    .child(text(move || active.with(|model| model.as_ref().map_or_else(
                        || "Choose a provider and model".into(),
                        |model| if model.group { model.label.clone() } else { model.provider_label.clone() },
                    ))).font_weight(FontWeight::SEMIBOLD).text_size(px(14.0)).text_color(move || p.ink()))
                    .child(when(move || active.with(|model| model.as_ref().is_some_and(|model| !model.group)),
                        move || label(p, move || active.with(|model| model.as_ref().map_or_else(String::new, |model| model.label.clone()))))))
                .child(ui::icon(ui::DOWN, 18.0, move || p.ink())))
            .child(when(move || !pending.get() && !available.get(), move || {
                label(p, || "Choose a connected provider and model to generate examples and grammar explanations.".into())
            }))
    }

    fn example_view(&self) -> View {
        let example = self.example.result;
        let speech = self.clone();
        view()
            .gap(px(8.0))
            .child(self.spoken_line("example".into(), move || {
                example.with(|example| {
                    example.as_ref().map(|example| Spoken {
                        title: format!("Example · {}", example.source_language),
                        language: example.source_language.clone(),
                        code: example.source_lang_code.clone(),
                        text: example.sentence.clone(),
                    })
                })
            }))
            .child(each(
                move || {
                    example.with(|example| {
                        example
                            .as_ref()
                            .map(|example| example.translations.clone())
                            .unwrap_or_default()
                    })
                },
                |translation| translation.language_code.clone(),
                move |translation, _| {
                    let id =
                        translation.with_untracked(|translation| translation.language_code.clone());
                    speech.spoken_line(id, move || {
                        Some(translation.with(|translation| Spoken {
                            title: translation.language.clone(),
                            language: translation.language.clone(),
                            code: translation.language_code.clone(),
                            text: translation.text.clone(),
                        }))
                    })
                },
            ))
    }

    fn spoken_line(
        &self,
        key: String,
        read: impl Fn() -> Option<Spoken> + Clone + 'static,
    ) -> View {
        let p = self.p;
        let speech = self.speech.clone();
        let audio = self.audio.clone();
        let active = computed({
            let speech = speech.clone();
            let request = format!("word-example:{key}");
            move || speech.active().as_ref() == Some(&request)
        });
        view()
            .gap(px(4.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(label(p, {
                        let read = read.clone();
                        move || read().map(|line| line.title).unwrap_or_default()
                    }))
                    .child(
                        press()
                            .size(px(40.0))
                            .items_center()
                            .justify_center()
                            .label({
                                let read = read.clone();
                                move || {
                                    if active.get() {
                                        "Stop example speech".into()
                                    } else {
                                        format!(
                                            "Speak example in {}",
                                            read().map(|line| line.language).unwrap_or_default()
                                        )
                                    }
                                }
                            })
                            .on_press({
                                let read = read.clone();
                                move || {
                                    if let Some(line) = read() {
                                        audio.stop();
                                        speech.speak_example(&key, line.text, line.code);
                                    }
                                }
                            })
                            .child(dynamic(move || {
                                ui::icon(
                                    if active.get() { ui::STOP } else { SPEAKER },
                                    22.0,
                                    move || p.ink(),
                                )
                            })),
                    ),
            )
            .child(answer(p, move || {
                read().map(|line| line.text).unwrap_or_default()
            }))
    }

    pub fn picker(&self) -> If {
        let panel = self.clone();
        let picker = self.picker;
        when(
            move || picker.with(Option::is_some),
            move || panel.picker_page(),
        )
    }

    fn picker_page(&self) -> View {
        let p = self.p;
        let choose = self.clone();
        let close = self.clone();
        let back = self.clone();
        let provider_view = self.providers.clone();
        let chat = self.chat.clone();
        let picker = self.picker;
        let saving = self.saving;
        let providers = move || picker.get() == Some(Picker::Providers);
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(13)
            .bg(move || p.background())
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .min_h(px(56.0))
                    .pl(px(16.0))
                    .pr(px(8.0))
                    .child(when(providers, move || {
                        let back = back.clone();
                        ui::icon_button(p, ui::BACK, "Back to AI models", move || {
                            back.back();
                        })
                    }))
                    .child(
                        text(move || {
                            if providers() {
                                "AI providers"
                            } else {
                                "Choose AI provider and model"
                            }
                        })
                        .flex_1()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || p.ink()),
                    )
                    .child(ui::icon_button(
                        p,
                        ui::CLOSE,
                        "Close AI provider picker",
                        move || {
                            close.picker.set(None);
                            close.error.set(None);
                        },
                    )),
            )
            .child(error_text(self.error))
            .child(when(saving, move || {
                label(p, || "Selecting AI provider…".into())
            }))
            .child(when(
                move || !providers(),
                move || {
                    let choose = choose.clone();
                    model_choices(
                        p,
                        chat.clone(),
                        move |model| choose.choose(model),
                        move || picker.set(Some(Picker::Providers)),
                    )
                },
            ))
            .child(when(providers, move || provider_view.view()))
    }

    fn choose(&self, model: Model) {
        if self.saving.get_untracked() {
            return;
        }
        if !model.connected {
            self.picker.set(Some(Picker::Providers));
            return;
        }
        self.saving.set(true);
        self.error.set(None);
        let panel = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let mut command = json!({"op":"ai_select","id":model.provider});
                if !model.group {
                    command["model"] = json!(model.id);
                }
                let result = panel.chat.update_config(command).await;
                panel.saving.set(false);
                match result {
                    Ok(_) => panel.picker.set(None),
                    Err(error) => panel.error.set(Some(error.to_string())),
                }
            });
        });
    }
}

struct Spoken {
    title: String,
    language: String,
    code: String,
    text: String,
}

fn task_button(
    p: Palette,
    icon: &'static str,
    title: &'static str,
    loading: &'static str,
    available: Memo<bool>,
    busy: Signal<bool>,
    on_press: impl Fn() + 'static,
) -> View {
    press()
        .label(title)
        .min_h(px(48.0))
        .rounded(px(10.0))
        .bg(move || p.raised())
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .disabled(move || !available.get() || busy.get())
        .opacity(move || if available.get() { 1.0_f32 } else { 0.45_f32 })
        .on_press(on_press)
        .child(ui::icon(icon, 19.0, move || p.ink()))
        .child(
            text(move || if busy.get() { loading } else { title })
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
}

fn label(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(12.0))
        .line_height(px(18.0))
        .text_color(move || p.muted())
}

fn answer(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .selectable(true)
        .text_size(px(15.0))
        .line_height(px(23.0))
        .text_color(move || p.ink())
}

fn failure(error: impl IntoReactive<SharedString>) -> Text {
    text(error)
        .text_size(px(14.0))
        .line_height(px(21.0))
        .text_color(rgb(0xe53935))
}

fn error_text(error: Signal<Option<String>>) -> If {
    when(
        move || error.with(Option::is_some),
        move || failure(move || error.get().unwrap_or_default()),
    )
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Example {
    pub sentence: String,
    pub source_language: String,
    pub source_lang_code: String,
    pub translations: Vec<ExampleTranslation>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExampleTranslation {
    pub language_code: String,
    pub language: String,
    pub text: String,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Grammar {
    pub base_form: String,
    pub explanation: String,
    pub language: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    task_id: String,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum Task<T> {
    Running,
    Completed { result: T },
    Failed { error: String },
    Cancelled,
}

#[derive(Clone)]
pub struct Assistance<T: 'static> {
    core: Core,
    kind: &'static str,
    owner: Owner,
    revision: Signal<u64>,
    task: Signal<Option<String>>,
    pub result: Signal<Option<T>>,
    pub error: Signal<Option<String>>,
    pub busy: Signal<bool>,
}

impl<T: Clone + PartialEq + DeserializeOwned + Send + 'static> Assistance<T> {
    pub fn new(core: Core, kind: &'static str) -> Self {
        Self {
            core,
            kind,
            owner: Owner::current(),
            revision: signal(0),
            task: signal(None),
            result: signal(None),
            error: signal(None),
            busy: signal(false),
        }
    }

    pub fn clear(&self) {
        self.revision.update(|revision| *revision += 1);
        self.busy.set(false);
        self.result.set(None);
        self.error.set(None);
        if let Some(task) = self.task.get_untracked() {
            self.task.set(None);
            let core = self.core.clone();
            self.owner.run(|| spawn(release(core, task, true)));
        }
    }

    pub fn generate(&self, mut request: Value) {
        self.clear();
        request["kind"] = json!(self.kind);
        let revision = self.revision.get_untracked();
        self.busy.set(true);
        let assistance = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = assistance.run(request, revision).await;
                if assistance.revision.get_untracked() == revision {
                    assistance.task.set(None);
                    assistance.busy.set(false);
                    match result {
                        Ok(value) => assistance.result.set(value),
                        Err(error) => assistance.error.set(Some(error)),
                    }
                }
            })
        });
    }

    async fn run(&self, request: Value, revision: u64) -> Result<Option<T>, String> {
        let Started { task_id } = self
            .core
            .ai(request)
            .await
            .map_err(|error| error.to_string())?;
        if self.revision.get_untracked() != revision {
            release(self.core.clone(), task_id, true).await;
            return Ok(None);
        }
        self.task.set(Some(task_id.clone()));
        let result = self.poll(&task_id, revision).await;
        if self.revision.get_untracked() == revision {
            self.task.set(None);
            release(self.core.clone(), task_id, result.is_err()).await;
        }
        result
    }

    async fn poll(&self, task: &str, revision: u64) -> Result<Option<T>, String> {
        while self.revision.get_untracked() == revision {
            let state = self
                .core
                .ai(json!({"op":"ai_poll","taskId":task}))
                .await
                .map_err(|error| error.to_string())?;
            match state {
                Task::Running => timer(Duration::from_millis(250)).await,
                Task::Completed { result } => return Ok(Some(result)),
                Task::Failed { error } => return Err(error),
                Task::Cancelled => break,
            };
        }
        Ok(None)
    }
}

async fn release(core: Core, task: String, cancel: bool) {
    if cancel {
        let _ = core
            .ai::<Value>(json!({"op":"ai_cancel","taskId":task}))
            .await;
    }
    let _ = core
        .ai::<Value>(json!({"op":"ai_forget","taskId":task}))
        .await;
}
