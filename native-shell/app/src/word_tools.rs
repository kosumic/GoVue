use std::time::Duration;

use govue_shell_runtime::Core;
use nsh_core::{
    AccessibilityActionDef, PointerEvents, PressEvent,
    element::{ElementRef, NativeEvent},
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::caption_audio::{CaptionAudio, Phase};
use crate::ui::{self, Palette, press};
use crate::word_assistance::WordHelp;

const BOOKMARK: &str = "M6 4h12v16l-6-4-6 4zm6 3v5m-2.5-2.5h5";
const SPEAKER: &str = "M3 9h4l5-4v14l-5-4H3zm13-1a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14";
const RETRY: &str = "M20 4v7h-7m7-1a8 8 0 1 0 .5 5";

#[derive(Clone, PartialEq)]
pub struct CaptionLine {
    pub video: String,
    pub track: String,
    pub index: usize,
    pub start: f64,
    pub text: String,
}

impl CaptionLine {
    fn command(&self, op: &str) -> Value {
        json!({"op":op,"videoId":self.video,"trackId":self.track,"segmentIndex":self.index})
    }
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Token {
    text: String,
    word: bool,
    start: Option<f64>,
    end: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    reading: Option<String>,
    speaker_change: Option<bool>,
}

#[derive(Deserialize)]
struct CaptionTokens {
    text: String,
    tokens: Vec<Token>,
}

#[derive(Clone, PartialEq)]
struct Seed {
    line: CaptionLine,
    index: usize,
    word: String,
    anchor: Point<Points>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Range {
    word: String,
    token_index: usize,
    end_token_index: usize,
    selection_start: u32,
    selection_end: u32,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TranslationPlan {
    input: String,
    source_lang_code: String,
    target_lang_code: String,
    target_language: String,
    same_language: bool,
}

#[derive(Clone, PartialEq)]
struct Translation {
    plan: TranslationPlan,
    output: Option<Result<String, String>>,
}

#[derive(Clone, PartialEq)]
struct TranslationRequest {
    id: String,
    index: usize,
}

#[derive(Clone)]
pub struct WordTools {
    core: Core,
    p: Palette,
    owner: Owner,
    audio: CaptionAudio,
    help: WordHelp,
    seed: Signal<Option<Seed>>,
    initial: Signal<Option<Range>>,
    range: Signal<Option<Range>>,
    pending: Signal<bool>,
    error: Signal<Option<String>>,
    translation_error: Signal<Option<String>>,
    feedback: Signal<Option<String>>,
    translations: Signal<Vec<Translation>>,
    loading: Signal<bool>,
    audio_kind: Signal<Option<String>>,
    saving: Signal<bool>,
    revision: Signal<u64>,
    translation_revision: Signal<u64>,
    native: ElementRef,
    request: Signal<Option<TranslationRequest>>,
}

impl WordTools {
    pub fn new(core: Core, p: Palette, audio: CaptionAudio, help: WordHelp) -> Self {
        Self {
            core,
            p,
            owner: Owner::current(),
            audio,
            help,
            seed: signal(None),
            initial: signal(None),
            range: signal(None),
            pending: signal(false),
            error: signal(None),
            translation_error: signal(None),
            feedback: signal(None),
            translations: signal(Vec::new()),
            loading: signal(false),
            audio_kind: signal(None),
            saving: signal(false),
            revision: signal(0),
            translation_revision: signal(0),
            native: element_ref(),
            request: signal(None),
        }
    }

    pub fn source(&self) -> Option<CaptionLine> {
        self.seed
            .with(|seed| seed.as_ref().map(|seed| seed.line.clone()))
    }

    pub fn close(&self) -> bool {
        if !self.seed.with_untracked(Option::is_some) {
            return false;
        }
        self.revision.update(|version| *version += 1);
        self.seed.set(None);
        self.initial.set(None);
        self.range.set(None);
        self.cancel_translation();
        self.audio.stop();
        self.help.close();
        true
    }

    pub fn back(&self) -> bool {
        self.help.back() || self.close()
    }

    pub fn provider_picker(&self) -> If {
        self.help.picker()
    }

    fn open(&self, seed: Seed) {
        self.close();
        self.audio.stop();
        self.feedback.set(None);
        self.saving.set(false);
        self.seed.set(Some(seed));
        self.help.refresh();
        self.update_selection(None);
    }

    fn update_selection(&self, offsets: Option<(u32, u32)>) {
        let Some(seed) = self.seed.get_untracked() else {
            return;
        };
        if !self.pending.get_untracked()
            && offsets.is_some_and(|offsets| {
                self.range.with_untracked(|range| {
                    range.as_ref().is_some_and(|range| {
                        offsets == (range.selection_start, range.selection_end)
                    })
                })
            })
        {
            return;
        }
        self.revision.update(|version| *version += 1);
        let version = self.revision.get_untracked();
        self.cancel_translation();
        self.pending.set(true);
        self.range.set(None);
        self.saving.set(false);
        self.error.set(None);
        self.feedback.set(None);
        self.audio_kind.set(None);
        self.audio.stop();
        self.help.clear();
        if offsets.is_some_and(|(start, end)| start == end) {
            self.error.set(Some("Select at least one word.".into()));
            return;
        }
        let mut command = seed.line.command("vocabulary_selection");
        command["tokenIndex"] = json!(seed.index);
        if let Some((start, end)) = offsets {
            command["selectionStart"] = json!(start);
            command["selectionEnd"] = json!(end);
        }
        let panel = self.clone();
        // The held text span is replaced by the native selection view.
        self.run(async move {
            if offsets.is_some() {
                timer(Duration::from_millis(60)).await;
            }
            if panel.revision.get_untracked() != version {
                return;
            }
            let result = panel.core.execute::<Range>(command).await;
            if panel.revision.get_untracked() != version {
                return;
            }
            match result {
                Ok(range) => {
                    if offsets.is_none() {
                        panel.initial.set(Some(range.clone()));
                    }
                    panel.range.set(Some(range));
                    panel.pending.set(false);
                    panel.translate();
                    panel.find_audio(version);
                }
                Err(error) => panel.error.set(Some(error.to_string())),
            }
        });
    }

    fn run(&self, future: impl Future<Output = ()> + 'static) {
        self.owner.run(|| spawn(future));
    }

    fn selected_command(&self, op: &str) -> Option<Value> {
        if self.pending.get_untracked() {
            return None;
        }
        let seed = self.seed.get_untracked()?;
        let range = self.range.get_untracked()?;
        let mut command = seed.line.command(op);
        command["word"] = json!(range.word);
        command["tokenIndex"] = json!(range.token_index);
        command["endTokenIndex"] = json!(range.end_token_index);
        Some(command)
    }

    fn find_audio(&self, version: u64) {
        let Some(command) = self.selected_command("vocabulary_word_audio") else {
            return;
        };
        let panel = self.clone();
        self.run(async move {
            let timing = panel
                .core
                .execute::<Value>(command)
                .await
                .unwrap_or(Value::Null);
            if panel.revision.get_untracked() == version {
                panel
                    .audio_kind
                    .set(timing["kind"].as_str().map(str::to_owned));
            }
        });
    }

    fn save(&self) {
        let Some(command) = self.selected_command("vocabulary_save") else {
            return;
        };
        let panel = self.clone();
        let version = self.revision.get_untracked();
        self.saving.set(true);
        self.run(async move {
            let result = panel.core.execute::<Value>(command).await;
            if panel.revision.get_untracked() == version {
                panel.saving.set(false);
                panel.feedback.set(Some(match result {
                    Ok(value) => {
                        let word = value["entry"]["word"].as_str().unwrap_or_default();
                        if value["saved"] == true {
                            format!("Saved “{word}”")
                        } else {
                            format!("“{word}” is already saved")
                        }
                    }
                    Err(error) => error.to_string(),
                }));
            }
        });
    }

    fn save_span(&self, caption: &CaptionLine, index: usize, word: &str) {
        let mut command = caption.command("vocabulary_save");
        command["tokenIndex"] = json!(index);
        command["word"] = json!(word);
        let core = self.core.clone();
        self.run(async move {
            let notice = match core.execute::<Value>(command).await {
                Ok(value) if value["saved"] == true => "Word saved.".into(),
                Ok(_) => "This word is already saved.".into(),
                Err(error) => error.to_string(),
            };
            nsh_core::element::announce_for_accessibility(notice);
        });
    }

    fn accessible_action(
        &self,
        action: &str,
        caption: &CaptionLine,
        words: &[Token],
        seek: Option<&dyn Fn(f64)>,
        select: &dyn Fn(),
    ) {
        let Some((name, index)) = action.split_once('-') else {
            return;
        };
        let Ok(index) = index.parse::<usize>() else {
            return;
        };
        let Some(token) = words.get(index).filter(|token| token.word) else {
            return;
        };
        match name {
            "translate" => {
                let window = nsh_core::element::window();
                select();
                self.open(Seed {
                    line: caption.clone(),
                    index,
                    word: token.text.clone(),
                    anchor: Point {
                        x: px(window.width.0 / 2.0),
                        y: px(window.height.0 / 2.0),
                    },
                });
            }
            "save" => self.save_span(caption, index, &token.text),
            "play" => {
                if let Some((seek, start)) = seek.zip(token.start) {
                    seek(start);
                }
            }
            _ => {}
        }
    }

    fn play(&self) {
        self.help.stop_speech();
        if self.audio.phase.get_untracked() != Phase::Idle {
            self.audio.stop();
            return;
        }
        let Some(mut command) = self.selected_command("ai_word_audio") else {
            return;
        };
        command["cueId"] = json!("word");
        let panel = self.clone();
        let version = self.revision.get_untracked();
        self.run(async move {
            if let Err(error) = panel.audio.play(command, || {}).await
                && panel.revision.get_untracked() == version
            {
                panel.feedback.set(Some(error.to_string()));
            }
        });
    }

    fn cancel_translation(&self) {
        self.translation_revision.update(|revision| *revision += 1);
        if let Some(active) = self.request.get_untracked() {
            self.request.set(None);
            self.native.command("cancel", json!(active.id));
        }
        self.translations.set(Vec::new());
        self.translation_error.set(None);
        self.loading.set(false);
    }

    fn translate(&self) {
        let Some(command) = self.selected_command("vocabulary_translation") else {
            return;
        };
        self.cancel_translation();
        self.loading.set(true);
        let version = self.translation_revision.get_untracked();
        let panel = self.clone();
        self.run(async move {
            let plans = panel.core.execute::<Vec<TranslationPlan>>(command).await;
            if panel.translation_revision.get_untracked() != version {
                return;
            }
            panel.loading.set(false);
            match plans {
                Ok(plans) => {
                    panel.translations.set(
                        plans
                            .into_iter()
                            .map(|plan| Translation {
                                output: plan.same_language.then(|| Ok(plan.input.clone())),
                                plan,
                            })
                            .collect(),
                    );
                    panel.translate_next();
                }
                Err(error) => panel.translation_error.set(Some(error.to_string())),
            }
        });
    }

    fn translate_next(&self) {
        let plans = self.translations.get_untracked();
        let Some((index, item)) = plans
            .iter()
            .enumerate()
            .find(|(_, item)| item.output.is_none())
        else {
            return;
        };
        let id = format!("{}-{index}", self.translation_revision.get_untracked());
        self.request.set(Some(TranslationRequest {
            id: id.clone(),
            index,
        }));
        self.native.command(
            "translate",
            json!({
                "request":id,
                "input":item.plan.input,
                "source":item.plan.source_lang_code,
                "target":item.plan.target_lang_code,
            }),
        );
        let panel = self.clone();
        self.run(async move {
            timer(Duration::from_secs(90)).await;
            if panel
                .request
                .with_untracked(|active| active.as_ref().is_some_and(|active| active.id == id))
            {
                panel.native.command("cancel", json!(id));
                panel.translation_finished(
                    &id,
                    Err("Translation timed out. Please try again.".into()),
                );
            }
        });
    }

    fn translation_finished(&self, id: &str, output: Result<String, String>) {
        let Some(active) = self
            .request
            .get_untracked()
            .filter(|active| active.id == id)
        else {
            return;
        };
        self.request.set(None);
        self.translations.update(|items| {
            if let Some(item) = items.get_mut(active.index) {
                item.output = Some(output);
            }
        });
        self.translate_next();
    }

    pub fn native(&self) -> NativeView {
        let panel = self.clone();
        native("govue/word-translation")
            .element_ref(&self.native)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() != "translation" {
                    return;
                }
                let Some(id) = event.data["request"].as_str() else {
                    return;
                };
                let output = event.data["text"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty() && text.len() <= 20_000)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        event.data["error"]
                            .as_str()
                            .unwrap_or("No translation was returned.")
                            .to_owned()
                    });
                panel.translation_finished(id, output);
            })
    }

    pub fn line(
        &self,
        source: impl Fn() -> Option<CaptionLine> + Clone + 'static,
        position: impl Fn() -> Option<f64> + Clone + 'static,
        seek: Option<impl Fn(f64) + Clone + 'static>,
        select: impl Fn() + Clone + 'static,
    ) -> View {
        let p = self.p;
        let parsed = signal(None::<(CaptionLine, Vec<Token>)>);
        let height = signal(24.0_f32);
        let revision = signal(0_u64);
        let error = signal(None::<String>);
        let line = computed(source);
        effect({
            let core = self.core.clone();
            move || {
                let request = revision.get();
                let Some(caption) = line.get() else { return };
                let core = core.clone();
                spawn(async move {
                    let result = core
                        .execute::<CaptionTokens>(caption.command("vocabulary_caption"))
                        .await;
                    if revision.get_untracked() == request
                        && line.with_untracked(|current| current.as_ref() == Some(&caption))
                    {
                        match result {
                            Ok(words) if words.text == caption.text => {
                                parsed.set(Some((caption, words.tokens)));
                                error.set(None);
                            }
                            Ok(_) => error.set(Some("This caption changed. Refresh the transcript to prepare its words.".into())),
                            Err(reason) => error.set(Some(reason.to_string())),
                        }
                    }
                });
            }
        });
        let seed = self.seed;
        let selecting = computed(move || {
            let line = line.get()?;
            seed.with(|seed| seed.as_ref().filter(|seed| seed.line == line).cloned())
        });
        let selector = self.clone();
        let translate = self.clone();
        view()
            .child(when(
                move || selecting.with(Option::is_none),
                move || {
                    let seek = seek.clone();
                    let select = select.clone();
                    let position = position.clone();
                    let translate = translate.clone();
                    dynamic(move || {
                        let Some(caption) = line.get() else {
                            return text("");
                        };
                        let tokens = parsed.get().filter(|(source, _)| *source == caption);
                        let base = text("")
                            .text_size(px(15.0))
                            .line_height(px(23.0))
                            .text_color(move || p.ink())
                            .on_layout(move |bounds: &Bounds<Points>| {
                                let measured = bounds.size.height.0;
                                if measured.is_finite() && measured > 0.0 {
                                    height.set(measured.ceil());
                                }
                            });
                        if let Some((_, tokens)) = tokens {
                            let label = tokens
                                .iter()
                                .map(|token| {
                                    let reading = token
                                        .reading
                                        .as_ref()
                                        .map(|reading| format!(" ({reading})"))
                                        .unwrap_or_default();
                                    format!(
                                        "{}{}{reading}",
                                        if token.speaker_change == Some(true) {
                                            " Possible speaker change. "
                                        } else {
                                            ""
                                        },
                                        token.text
                                    )
                                })
                                .collect::<String>();
                        let timed_seek = seek.is_some();
                        let actions = word_actions(&tokens, timed_seek);
                        let accessible = translate.clone();
                        let words = tokens.clone();
                        let accessible_caption = caption.clone();
                        let accessible_seek = seek.clone();
                        let accessible_select = select.clone();
                        base.label(label)
                            .hint("Hold a word to translate it, or use the translate and save word actions.")
                            .accessibility_actions(actions)
                            .on_accessibility_action(move |action: &str| {
                                accessible.accessible_action(action, &accessible_caption, &words,
                                    accessible_seek.as_ref().map(|seek| seek as &dyn Fn(f64)), &accessible_select);
                            })
                                .children(caption_spans(
                                    p,
                                    translate.clone(),
                                    caption,
                                    tokens,
                                    position.clone(),
                                    seek.clone(),
                                    select.clone(),
                                ))
                        } else {
                            base.child(text(caption.text)).on_long_press(move || {
                                let message = error.get_untracked().unwrap_or_else(|| {
                                    "Words are still loading. Please try again.".into()
                                });
                                revision.update(|value| *value += 1);
                                spawn(crate::downloads::alert(message));
                            })
                        }
                    })
                },
            ))
            .child(when(
                move || selecting.with(Option::is_some),
                move || {
                    view()
                        .child(when(
                            move || selector.initial.with(Option::is_none),
                            move || {
                                text(move || {
                                    line.with(|line| {
                                        line.as_ref()
                                            .map(|line| line.text.clone())
                                            .unwrap_or_default()
                                    })
                                })
                                .text_size(px(15.0))
                                .line_height(px(23.0))
                                .text_color(move || p.ink())
                            },
                        ))
                        .child(when(move || selector.initial.with(Option::is_some), {
                            let selector = selector.clone();
                            move || selector.selection_view(line, height)
                        }))
                },
            ))
    }

    fn selection_view(&self, line: Memo<Option<CaptionLine>>, height: Signal<f32>) -> NativeView {
        let panel = self.clone();
        let initial = self.initial;
        let p = self.p;
        native("govue/caption-words")
            .h(move || px(height.get()))
            .prop("selection", move || {
                initial
                    .get()
                    .zip(line.get())
                    .map_or(Value::Null, |(range, line)| {
                        json!({
                            "text": line.text,
                            "selectionStart": range.selection_start,
                            "selectionEnd": range.selection_end,
                            "dark": p.dark.get(),
                        })
                    })
            })
            .on_native_event(move |event: &NativeEvent| match event.name.as_ref() {
                "size" => {
                    if let Some(size) = event
                        .data
                        .as_f64()
                        .filter(|size| (1.0..=100_000.0).contains(size))
                    {
                        height.set(size.ceil() as f32);
                    }
                }
                "selection" => {
                    let start = offset(&event.data, "selectionStart");
                    let end = offset(&event.data, "selectionEnd");
                    if let Some((start, end)) = start.zip(end).filter(|(start, end)| end >= start) {
                        panel.update_selection(Some((start, end)));
                    }
                }
                _ => {}
            })
    }

    pub fn popup(&self) -> If {
        let panel = self.clone();
        let seed = self.seed;
        when(move || seed.with(Option::is_some), move || panel.card())
    }

    fn card(&self) -> View {
        let p = self.p;
        let seed = self.seed;
        let range = self.range;
        let open = self.clone();
        let save = self.clone();
        let play = self.clone();
        let retry = self.clone();
        let selected = self.clone();
        let pending = self.pending;
        let loading = self.loading;
        let error = self.error;
        let translation_error = self.translation_error;
        let feedback = self.feedback;
        let phase = self.audio.phase;
        let audio_error = self.audio.error;
        let audio_kind = self.audio_kind;
        let saving = self.saving;
        let translations = self.translations;
        let phrase = computed(move || {
            range.with(|range| {
                range
                    .as_ref()
                    .is_some_and(|range| range.end_token_index != range.token_index)
            })
        });
        let measured = signal(210.0_f32);
        let layout = computed(move || {
            let window = nsh_core::element::window();
            let w = (window.width.0 - 32.0).clamp(1.0, 420.0);
            let top = 12.0;
            let bottom = (window.height.0 - 12.0).max(top + 1.0);
            let anchor = seed
                .with(|seed| seed.as_ref().map(|seed| seed.anchor))
                .unwrap_or(Point {
                    x: px(w / 2.0),
                    y: px(bottom / 2.0),
                });
            let available = (bottom - anchor.y.0 - 24.0).max(anchor.y.0 - top - 24.0);
            let limit = (window.height.0 * 0.7)
                .min(bottom - top)
                .min(available.max(120.0));
            let h = measured.get().min(limit);
            let left = (anchor.x.0 - w / 2.0)
                .max(16.0)
                .min((window.width.0 - w - 16.0).max(16.0));
            let y = if anchor.y.0 + 24.0 + h <= bottom {
                anchor.y.0 + 24.0
            } else {
                anchor.y.0 - h - 24.0
            };
            (left, y.clamp(top, (bottom - h).max(top)), w, limit)
        });
        view().absolute().inset(px(0.0)).z_index(12).pointer_events(PointerEvents::BoxNone)
            .child(view().absolute()
                .left(move || px(layout.get().0)).top(move || px(layout.get().1)).w(move || px(layout.get().2))
                .max_h(move || px(layout.get().3))
                .rounded(px(20.0)).border(px(0.5)).border_color(move || p.muted()).bg(move || p.background())
                .overflow_hidden()
                .on_layout(move |bounds: &Bounds<Points>| measured.set(bounds.size.height.0))
                .child(view().flex_row().items_center().justify_between().pt(px(8.0)).pl(px(20.0)).pr(px(8.0))
                    .child(text(move || if phrase.get() { "Translate phrase" } else { "Translate word" })
                        .text_size(px(19.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
                    .child(ui::icon_button(p, ui::CLOSE, "Close word translation", move || { open.close(); })))
                .child(scroll().flex_shrink(1.0).max_h(move || px((layout.get().3 - 52.0).max(0.0)))
                    .child(view().px(px(20.0)).pb(px(12.0)).gap(px(14.0))
                        .child(view().flex_row().items_center().justify_between().gap(px(8.0))
                            .child(text(move || range.get().map(|range| range.word).or_else(|| seed.get().map(|seed| seed.word)).unwrap_or_default())
                                .flex_shrink(1.0).selectable(true).text_size(px(28.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
                            .child(view().flex_row().items_center()
                                .child(icon_action(p, ui::COPY, move || if phrase.get() { "Copy phrase" } else { "Copy word" }, pending, move || {
                                    if let Some(range) = range.get_untracked() {
                                        set_clipboard(range.word);
                                    }
                                }))
                                .child(when(move || audio_kind.with(Option::is_some) && !pending.get(), move || {
                                    let play = play.clone();
                                    dynamic(move || {
                                        let play = play.clone();
                                        let playing = phase.get() != Phase::Idle;
                                        let caption = audio_kind.with(|kind| kind.as_deref() == Some("caption"));
                                        icon_action(p, if playing { ui::STOP } else { SPEAKER }, move || {
                                            match (playing, caption, phrase.get()) {
                                                (true, true, _) => "Stop caption audio",
                                                (true, false, _) => "Stop word audio",
                                                (false, true, _) => "Play caption",
                                                (false, false, true) => "Play phrase",
                                                _ => "Play word",
                                            }
                                        }, false, move || play.play())
                                    })
                                }))
                                .child(icon_action(p, BOOKMARK, move || if phrase.get() { "Save phrase" } else { "Save word" },
                                    move || pending.get() || saving.get(), move || save.save()))))
                        .child(help(p, || "Drag either handle to change the selection.".into()))
                        .child(when(move || pending.get() && error.with(Option::is_none), move || help(p, || "Updating selection…".into())))
                        .child(when(move || loading.get() && !pending.get(), move || help(p, || "Preparing word translations…".into())))
                        .child(each(move || translations.get(), |entry| entry.plan.target_lang_code.clone(), move |entry, _| {
                            view().gap(px(8.0)).pb(px(8.0)).border_b(px(0.5)).border_color(move || p.raised())
                                .child(help(p, move || entry.with(|entry| entry.plan.target_language.clone())))
                                .child(when(move || entry.with(|entry| matches!(entry.output, Some(Ok(_)))), move || {
                                    text(move || entry.with(|entry| entry.output.as_ref().and_then(|result| result.as_ref().ok()).cloned().unwrap_or_default()))
                                        .selectable(true).text_size(px(24.0)).line_height(px(34.0)).text_color(move || p.ink())
                                }))
                                .child(when(move || entry.with(|entry| entry.plan.same_language), move || {
                                    help(p, move || entry.with(|entry| format!("Already in {}.", entry.plan.target_language)))
                                }))
                                .child(when(move || entry.with(|entry| entry.output.is_none()), move || help(p, || "Translating…".into())))
                                .child(when(move || entry.with(|entry| matches!(entry.output, Some(Err(_)))), move || {
                                    help(p, move || entry.with(|entry| entry.output.as_ref().and_then(|result| result.as_ref().err()).cloned().unwrap_or_default()))
                                        .text_color(rgb(0xd32f2f))
                                }))
                        }))
                        .child(when(move || translations.with(|items| items.iter().any(|item| item.output.is_none())), move || {
                            help(p, || "The first translation may download language models.".into())
                        }))
                        .child(when(move || translation_error.with(Option::is_some) || translations.with(|items| items.iter().any(|item| matches!(item.output, Some(Err(_))))), move || {
                            let retry = retry.clone();
                            ui::icon_button(p, RETRY, "Retry word translation", move || retry.translate())
                        }))
                        .child(when(move || error.get().or_else(|| translation_error.get()).or_else(|| audio_error.get()).is_some(), move || {
                            help(p, move || error.get().or_else(|| translation_error.get()).or_else(|| audio_error.get()).unwrap_or_default())
                                .text_color(rgb(0xd32f2f))
                        }))
                        .child(when(move || feedback.with(Option::is_some), move || {
                            help(p, move || feedback.get().unwrap_or_default())
                        }))
                        .child(self.help.contents(pending, move || {
                            selected.selected_command("ai_word_assistance")
                        })))))
    }
}

fn word_actions(tokens: &[Token], timed_seek: bool) -> Vec<AccessibilityActionDef> {
    let mut actions = Vec::new();
    for (index, token) in tokens.iter().enumerate().filter(|(_, token)| token.word) {
        actions.push(AccessibilityActionDef {
            name: format!("translate-{index}").into(),
            label: Some(format!("Translate {}", token.text).into()),
        });
        actions.push(AccessibilityActionDef {
            name: format!("save-{index}").into(),
            label: Some(format!("Save {}", token.text).into()),
        });
        if timed_seek && token.start.is_some() {
            actions.push(AccessibilityActionDef {
                name: format!("play-{index}").into(),
                label: Some(format!("Play from {}", token.text).into()),
            });
        }
    }
    actions
}

fn caption_spans(
    p: Palette,
    panel: WordTools,
    caption: CaptionLine,
    tokens: Vec<Token>,
    position: impl Fn() -> Option<f64> + Clone + 'static,
    seek: Option<impl Fn(f64) + Clone + 'static>,
    select: impl Fn() + Clone + 'static,
) -> impl Iterator<Item = Text> {
    tokens
        .into_iter()
        .enumerate()
        .flat_map(move |(index, token)| {
            let separator = token.speaker_change.unwrap_or(false).then(|| {
                text(if index == 0 {
                    "───\n"
                } else {
                    "\n───\n"
                })
                .text_size(px(12.0))
                .text_color(move || p.muted())
            });
            let span = text(token.text.clone())
                .font_weight(if token.bold == Some(true) {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .when(token.italic == Some(true), |span| span.italic())
                .when(token.underline == Some(true), |span| span.underline());
            if !token.word {
                return separator.into_iter().chain(std::iter::once(span));
            }
            let panel = panel.clone();
            let caption = caption.clone();
            let select = select.clone();
            let position = position.clone();
            let seek_at = token.start.unwrap_or(caption.start);
            let active = computed(move || {
                token.start.zip(position()).is_some_and(|(start, now)| {
                    now >= start && token.end.is_none_or(|end| now < end)
                })
            });
            let word = token.text;
            let span = span
                .bg(move || {
                    if active.get() {
                        p.ink()
                    } else {
                        rgba(0x00000000).into()
                    }
                })
                .text_color(move || {
                    if active.get() {
                        p.background()
                    } else {
                        p.ink()
                    }
                })
                .when_some(token.reading, |span, reading| {
                    span.child(
                        text(format!("（{reading}）"))
                            .text_size(px(11.0))
                            .font_weight(FontWeight::NORMAL),
                    )
                })
                .on_long_press(move |event: &PressEvent| {
                    select();
                    panel.open(Seed {
                        line: caption.clone(),
                        index,
                        word: word.clone(),
                        anchor: event.page_position,
                    });
                })
                .when_some(seek.clone(), |span, seek| {
                    span.on_press(move || seek(seek_at))
                });
            separator.into_iter().chain(std::iter::once(span))
        })
}

fn offset(data: &Value, key: &str) -> Option<u32> {
    u32::try_from(data[key].as_u64()?).ok()
}

fn icon_action(
    p: Palette,
    path: &'static str,
    label: impl Fn() -> &'static str + 'static,
    disabled: impl IntoReactive<bool>,
    action: impl Fn() + 'static,
) -> View {
    press()
        .size(px(48.0))
        .items_center()
        .justify_center()
        .label(label)
        .disabled(disabled)
        .on_press(action)
        .child(ui::icon(path, 22.0, move || p.ink()))
}

fn help(p: Palette, content: impl Fn() -> String + 'static) -> Text {
    text(content)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}
