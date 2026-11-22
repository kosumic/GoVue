use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    time::Duration,
};

use govue_shell_runtime::{CoreError, WatchPlayback};
use nsh_core::{PointerEvents, element::ElementRef, platform::Lifecycle, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::caption_audio::{CaptionAudio, Phase};
use crate::chat::Chat;
use crate::ui::{self, Palette, press};

const RELOAD: &str = "M20 4v7h-7m7-1a8 8 0 1 0 .5 5";
const EXPAND: &str = "M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5";
const COLLAPSE: &str = "M3 8h5V3m8 0v5h5M8 21v-5H3m18 0h-5v5";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LearningGame {
    pub id: String,
    pub definition: Value,
    pub state: Value,
    pub cues: Vec<Value>,
    pub revision: u64,
    pub source: Option<crate::practice_view::Source>,
}

impl LearningGame {
    pub fn title(&self) -> &str {
        self.definition["title"]
            .as_str()
            .unwrap_or("Interactive learning game")
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Target {
    chat: Option<(String, String)>,
    game: String,
}

impl Target {
    fn command(&self, op: &str) -> Value {
        if let Some((chat, message)) = &self.chat {
            json!({"op":format!("ai_game_{op}"), "chatId":chat, "messageId":message, "gameId":self.game})
        } else {
            json!({"op":format!("ai_practice_{op}"), "gameId":self.game})
        }
    }
}

#[derive(Clone)]
pub struct Games {
    chat: Chat,
    active: Signal<Option<Target>>,
    expanded: Signal<bool>,
    reload: Signal<u64>,
    practice: Signal<Option<LearningGame>>,
    pub practice_busy: Signal<bool>,
    pub audio: CaptionAudio,
    before_audio: Rc<dyn Fn()>,
}

impl Games {
    pub fn new(chat: Chat, watch: WatchPlayback, before_audio: impl Fn() + 'static) -> Self {
        let games = Self {
            audio: CaptionAudio::new(chat.core(), watch),
            chat,
            active: signal(None),
            expanded: signal(false),
            reload: signal(0),
            practice: signal(None),
            practice_busy: signal(false),
            before_audio: Rc::new(before_audio),
        };
        let lifecycle_games = games.clone();
        effect(move || {
            if !lifecycle_games.chat.visible() {
                lifecycle_games.close();
            } else if lifecycle() != Lifecycle::Active {
                lifecycle_games.expanded.set(false);
                lifecycle_games.audio.stop();
            }
        });
        games
    }

    pub fn back(&self) -> bool {
        if self.expanded.get_untracked() {
            self.expanded.set(false);
            return true;
        }
        false
    }

    pub fn close(&self) {
        self.expanded.set(false);
        self.active.set(None);
        self.audio.stop();
    }

    fn open(&self, target: Target) {
        self.audio.stop();
        self.active.set(Some(target));
        self.expanded.set(false);
    }

    fn latest(&self, target: &Target) -> Option<LearningGame> {
        let Some((chat_id, message_id)) = &target.chat else {
            return self.practice.get().filter(|game| game.id == target.game);
        };
        self.chat
            .current()
            .filter(|chat| chat.id == *chat_id)?
            .messages
            .iter()
            .find(|message| message.id == *message_id)?
            .games
            .iter()
            .find(|game| game.id == target.game)
            .cloned()
    }

    pub fn practice_game(&self) -> Option<LearningGame> {
        self.practice.get()
    }

    pub fn set_practice_game(&self, game: Option<LearningGame>) {
        if let Some(game) = &game {
            self.chat.merge_game(None, game);
        }
        self.practice.set(game);
    }

    pub fn practice_surface(&self, p: Palette, game: String) -> View {
        self.surface(p, Target { chat: None, game }, true)
    }

    pub fn card(&self, p: Palette, game: ReadSignal<LearningGame>, message: String) -> View {
        let Some(thread) = self.chat.current() else {
            return view();
        };
        let target = Target {
            chat: Some((thread.id.clone(), message)),
            game: game.with_untracked(|game| game.id.clone()),
        };
        let current = self.active;
        let expanded = self.expanded;
        let is_active = computed({
            let target = target.clone();
            move || {
                current.with(|active| active.as_ref() == Some(&target))
                    && !expanded.get()
                    && lifecycle() == Lifecycle::Active
            }
        });
        let select = self.clone();
        let surface = self.clone();
        let title = move || game.with(|game| game.title().to_owned());
        let target_for_surface = target.clone();
        let enabled = computed({
            let chat = self.chat.clone();
            move || chat.visible() && !chat.loading()
        });
        view()
            .my(px(10.0))
            .rounded(px(16.0))
            .border(px(0.5))
            .border_color(move || p.raised())
            .overflow_hidden()
            .bg(move || p.background())
            .child(self.header(p, target.clone(), title, is_active, false))
            .child(when(is_active, move || {
                surface.reloading(p, target_for_surface.clone(), false)
            }))
            .child(when(
                move || !is_active.get(),
                move || {
                    let target = target.clone();
                    let select = select.clone();
                    let label = move || {
                        if game.with(|game| game.revision > 0) {
                            "Continue game"
                        } else {
                            "Play game"
                        }
                    };
                    press()
                        .p(px(24.0))
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .gap(px(10.0))
                        .bg(move || p.raised())
                        .disabled(move || !enabled.get())
                        .label(label)
                        .on_press(move || select.open(target.clone()))
                        .child(ui::icon(ui::PLAY, 18.0, move || p.ink()))
                        .child(
                            text(label)
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                        )
                },
            ))
    }

    pub fn fullscreen(&self, p: Palette) -> impl IntoElement {
        let expanded = self.expanded;
        let current = self.active;
        let panel = self.clone();
        when(
            move || expanded.get() && current.with(Option::is_some),
            move || {
                let Some(target) = current.get_untracked() else {
                    return view();
                };
                let game = panel.clone();
                let name = target.clone();
                view()
                    .absolute()
                    .inset(px(0.0))
                    .z_index(8)
                    .bg(move || p.background())
                    .child(panel.header(
                        p,
                        target.clone(),
                        move || {
                            game.latest(&name)
                                .map(|game| game.title().to_owned())
                                .unwrap_or_default()
                        },
                        computed(|| true),
                        true,
                    ))
                    .child(panel.reloading(p, target, true))
            },
        )
    }

    fn header(
        &self,
        p: Palette,
        target: Target,
        title: impl Fn() -> String + 'static,
        playing: Memo<bool>,
        full: bool,
    ) -> View {
        let copied = signal(false);
        let error = signal(None::<String>);
        let core = self.chat.core();
        let reload = self.reload;
        let expand = self.expanded;
        let dismiss = self.clone();
        view()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .min_h(px(50.0))
                    .pl(px(16.0))
                    .pr(px(4.0))
                    .child(
                        text(title)
                            .flex_1()
                            .py(px(12.0))
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    )
                    .child(dynamic(move || {
                        let core = core.clone();
                        let target = target.clone();
                        ui::icon_button(
                            p,
                            if copied.get() {
                                "m4 12 5 5L20 6"
                            } else {
                                ui::COPY
                            },
                            if copied.get() {
                                "Game copied"
                            } else {
                                "Copy game"
                            },
                            move || {
                                let core = core.clone();
                                let command = target.command("export");
                                error.set(None);
                                spawn(async move {
                                    match core.ai::<Value>(command).await {
                                        Ok(result) => {
                                            if let Some(text) = result["text"].as_str() {
                                                set_clipboard(text);
                                                copied.set(true);
                                                timer(Duration::from_secs(2)).await;
                                                copied.set(false);
                                            }
                                        }
                                        Err(reason) => error.set(Some(reason.to_string())),
                                    }
                                });
                            },
                        )
                    }))
                    .child(when(playing, move || {
                        let dismiss = dismiss.clone();
                        view()
                            .flex_row()
                            .items_center()
                            .child(ui::icon_button(p, RELOAD, "Reload game", move || {
                                reload.update(|revision| *revision += 1)
                            }))
                            .child(ui::icon_button(
                                p,
                                if full { COLLAPSE } else { EXPAND },
                                if full { "Collapse game" } else { "Expand game" },
                                move || expand.set(!full),
                            ))
                            .child(ui::icon_button(p, ui::CLOSE, "Close game", move || {
                                dismiss.close()
                            }))
                    })),
            )
            .child(when(
                move || error.with(Option::is_some),
                move || help(p, move || error.get().unwrap_or_default()),
            ))
    }

    fn reloading(&self, p: Palette, target: Target, fullscreen: bool) -> View {
        let reload = self.reload;
        let controller = self.clone();
        let surface = view().min_h(px(0.0)).child(each(
            move || vec![reload.get()],
            |revision| *revision,
            move |_, _| controller.surface(p, target.clone(), fullscreen),
        ));
        if fullscreen {
            surface.flex_1()
        } else {
            surface
        }
    }

    fn surface(&self, p: Palette, target: Target, full: bool) -> View {
        let core = self.chat.core();
        let Some(game) = self.latest(&target) else {
            return view().child(help(p, || "This game is no longer available.".into()));
        };
        let Ok(session) = core.state::<String>(json!({"op":"ai_game_session"})) else {
            return view().child(help(p, || "Could not open this game.".into()));
        };
        let html = document(p, &game, &session);
        let renderer = element_ref();
        let ready = signal(false);
        let failed = signal(false);
        let error = signal(None::<String>);
        let audio = self.audio.clone();
        let stop = audio.clone();
        let phase = audio.phase;
        let audio_error = audio.error;
        let alive = Rc::new(Cell::new(true));
        cleanup({
            let alive = alive.clone();
            let audio = self.audio.clone();
            move || {
                alive.set(false);
                audio.stop();
            }
        });
        spawn(async move {
            timer(Duration::from_secs(10)).await;
            if !ready.get_untracked() {
                failed.set(true);
            }
        });
        let pending = Rc::new(RefCell::new(HashSet::new()));
        effect({
            let controller = self.clone();
            let target = target.clone();
            move || {
                if ready.get()
                    && !failed.get()
                    && let Some(game) = controller.latest(&target)
                {
                    reply(renderer, "state", Some(game.state), None);
                }
            }
        });
        let controller = self.clone();
        let scaffold = view().min_h(px(0.0))
            .child(view().flex_1().min_h(px(0.0))
                .child(when(move || !failed.get(), move || {
                    let controller = controller.clone();
                    let target = target.clone();
                    let pending = pending.clone();
                    let session = session.clone();
                    let alive = alive.clone();
                    native("govue/game").element_ref(&renderer).w_full().h_full()
                        .prop("document", json!(html))
                        .on_native_event(move |event: &NativeEvent| {
                            if event.name.as_ref() == "failed" {
                                failed.set(true);
                                return;
                            }
                            let Some(raw) = event.data.as_str().filter(|raw| raw.len() <= 10_000) else { return };
                            let Ok(packet) = serde_json::from_str::<Value>(raw) else { return };
                            if packet["session"] != session
                                || packet["request"]["v"] != 1 {
                                return;
                            }
                            let request = &packet["request"];
                            match request["method"].as_str() {
                                Some("ready") => { ready.set(true); return; }
                                Some("error") => {
                                    error.set(Some("The generated interface encountered an error. You can reopen the game or ask the assistant to fix it.".into()));
                                    return;
                                }
                                Some("action" | "audio") if ready.get_untracked() => {}
                                _ => return,
                            }
                            let Some(id) = request["id"].as_str().filter(|id| {
                                !id.is_empty() && id.len() <= 8 && id.bytes().all(|byte| byte.is_ascii_digit())
                            }) else { return };
                            if pending.borrow().len() >= 8 || !pending.borrow_mut().insert(id.to_owned()) {
                                return;
                            }
                            let controller = controller.clone();
                            let target = target.clone();
                            let pending = pending.clone();
                            let alive = alive.clone();
                            let (id, request, event_id) = (id.to_owned(), request.clone(), format!("{session}-{id}"));
                            error.set(None);
                            spawn(async move {
                                let result = controller.request(&target, request, event_id).await;
                                pending.borrow_mut().remove(&id);
                                if !alive.get() || failed.get_untracked() {
                                    return;
                                }
                                match result {
                                    Ok(state) => reply(renderer, &id, state, None),
                                    Err(reason) => {
                                        let reason = reason.to_string();
                                        error.set(Some(reason.clone()));
                                        reply(renderer, &id, None, Some(reason));
                                    }
                                }
                            });
                        })
                }))
                .child(when(move || !ready.get() && !failed.get(), move || {
                    view().absolute().inset(px(0.0)).items_center().justify_center().bg(move || p.background())
                        .pointer_events(PointerEvents::None)
                        .child(help(p, || "Opening game…".into()))
                }))
                .child(when(failed, move || {
                    view().flex_1().justify_center().px(px(24.0)).child(help(p, || "This interface could not be loaded. Reopen the game to try again, or ask the assistant to create a replacement.".into()))
                })))
            .child(when(move || phase.get() != Phase::Idle, move || {
                let stop = stop.clone();
                view().h(px(44.0)).bg(move || p.raised()).flex_row().items_center().justify_center()
                    .child(when(move || phase.get() == Phase::Loading, move || help(p, || "Preparing audio…".into())))
                    .child(ui::icon_button(p, ui::STOP, "Stop game audio", move || stop.stop()))
            }))
            .child(when(move || error.with(Option::is_some) || audio_error.with(Option::is_some), move || {
                help(p, move || error.get().or_else(|| audio_error.get()).unwrap_or_default()).px(px(12.0)).pb(px(8.0))
            }));
        if full {
            scaffold.flex_1()
        } else {
            scaffold.h(px(400.0))
        }
    }

    async fn request(
        &self,
        target: &Target,
        request: Value,
        event_id: String,
    ) -> Result<Option<Value>, CoreError> {
        let audio = request["method"] == "audio";
        let mut command = target.command("action");
        command["eventId"] = json!(event_id);
        let game = self
            .latest(target)
            .ok_or_else(|| CoreError::Request("This game is no longer available.".into()))?;
        command["revision"] = json!(game.revision);
        if audio {
            let cue = request["cueId"]
                .as_str()
                .filter(|cue| game.cues.iter().any(|entry| entry["id"] == *cue))
                .ok_or_else(|| CoreError::Request("This audio is not part of the game.".into()))?;
            command["action"] = json!("__play_audio");
            command["input"] = json!({"cueId":cue});
        } else {
            let action = request["action"]
                .as_str()
                .filter(|action| *action != "__play_audio" && request["input"].is_object())
                .ok_or_else(|| CoreError::Request("Invalid game action.".into()))?;
            command["action"] = json!(action);
            command["input"] = request["input"].clone();
        }
        if target.chat.is_none() && self.practice_busy.get_untracked() {
            return Err(CoreError::Request("The game is busy. Please wait.".into()));
        }
        let result = if let Some((chat, message)) = &target.chat {
            self.chat.apply_game(command, Some((chat, message))).await?
        } else {
            self.practice_busy.set(true);
            let result = self.chat.apply_game(command, None).await;
            self.practice_busy.set(false);
            let game = result?;
            if self.practice.with_untracked(|current| {
                current.as_ref().is_some_and(|current| {
                    current.id == game.id && current.revision <= game.revision
                })
            }) {
                self.practice.set(Some(game.clone()));
            }
            game
        };
        if audio {
            if self
                .active
                .with_untracked(|active| active.as_ref() == Some(target))
                || target.chat.is_none()
                    && self.practice.with_untracked(|game| {
                        game.as_ref().is_some_and(|game| game.id == target.game)
                    })
            {
                let mut command = target.command("audio");
                command["cueId"] = request["cueId"].clone();
                let before = self.before_audio.clone();
                self.audio.play(command, move || before()).await?;
            }
            Ok(None)
        } else {
            Ok(Some(result.state))
        }
    }
}

fn reply(renderer: ElementRef, id: &str, state: Option<Value>, error: Option<String>) {
    renderer.command(
        "reply",
        Value::String(json!({"id":id,"state":state,"error":error}).to_string()),
    );
}

fn help(p: Palette, label: impl Fn() -> String + 'static) -> Text {
    text(label)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}

fn script_json(value: &Value) -> String {
    value
        .to_string()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn document(p: Palette, game: &LearningGame, session: &str) -> String {
    let dark = p.dark.get_untracked();
    let (background, ink, muted, accent, raised) = if dark {
        ("#0f0f0f", "#ffffff", "#aaaaaa", "#ffffff", "#272727")
    } else {
        ("#ffffff", "#0f0f0f", "#606060", "#0f0f0f", "#f2f2f2")
    };
    let mode = if dark { "dark" } else { "light" };
    let theme = format!(
        ":root{{color-scheme:{mode};--background:{background};--ink:{ink};--muted:{muted};--accent:{accent};--raised:{raised}}}"
    );
    let spec = json!({
        "html":game.definition["html"],
        "css":game.definition["css"],
        "script":game.definition["script"],
        "state":game.state,
        "cues":game.cues,
    });
    let frame = include_str!("game_frame.html")
        .replace("__GOVUE_THEME__", &theme)
        .replace("__GOVUE_SPEC__", &script_json(&spec));
    include_str!("game_shell.html")
        .replace("__GOVUE_BACKGROUND__", background)
        .replace("__GOVUE_SESSION__", &script_json(&json!(session)))
        .replace("__GOVUE_FRAME__", &script_json(&json!(frame)))
}
