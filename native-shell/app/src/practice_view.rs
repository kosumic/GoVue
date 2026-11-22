use std::rc::Rc;

use govue_shell_runtime::Core;
use nsh_core::{
    PointerEvents,
    element::{ClaimRequest, ResponderEvent, TouchClaim, input},
    platform::Lifecycle,
    prelude::*,
};
use serde::Deserialize;
use serde_json::json;

use crate::chat_game::{Games, LearningGame};
use crate::reader_view::timestamp;
use crate::ui::{self, Palette, press};

const LIBRARY: &str = "M3 4h4v16H3zm7 0h4v16h-4zm7 0h4v16h-4z";
const UNDO: &str = "m9 10-5-5 5-5M4 5h9a7 7 0 0 1 0 14H9";
const RESTART: &str = "M20 4v7h-7m7-1a8 8 0 1 0 .5 5";
const CHECK: &str = "m4 12 5 5L20 6";
const GRIP: &str = "M5 10h14M5 14h14";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    video_id: String,
    title: String,
    start: Option<f64>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Entry {
    id: String,
    title: String,
    source: Option<Source>,
    verdict: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Session {
    revision: u64,
    started: bool,
    game: Option<LearningGame>,
    remaining: usize,
    total: usize,
    known: usize,
    forgot: usize,
    can_undo: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Library {
    games: Vec<Entry>,
    session: Session,
}

#[derive(Deserialize)]
struct SavedGame {
    game: LearningGame,
}

#[derive(Clone)]
pub struct PracticeView {
    core: Core,
    owner: Owner,
    game: Games,
    library: Signal<Option<Library>>,
    library_open: Signal<bool>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    rename: Signal<Option<String>>,
    name: Signal<String>,
    restart: Signal<u64>,
}

impl PracticeView {
    pub fn new(core: Core, game: Games) -> Self {
        Self {
            core,
            owner: Owner::current(),
            busy: game.practice_busy,
            game,
            library: signal(None),
            library_open: signal(false),
            error: signal(None),
            rename: signal(None),
            name: signal(String::new()),
            restart: signal(0),
        }
    }

    pub fn open(&self) {
        self.rename.set(None);
        self.library_open.set(false);
        self.load();
    }

    pub fn back(&self) -> bool {
        if self.rename.with_untracked(Option::is_some) && !self.busy.get_untracked() {
            self.rename.set(None);
            return true;
        }
        self.busy.get_untracked()
    }

    fn load(&self) {
        let pane = self.clone();
        self.run(move || async move {
            let library = pane.core.ai(json!({"op":"ai_practice_list"})).await?;
            pane.update(library);
            Ok(())
        });
    }

    fn update(&self, library: Library) {
        self.game.set_practice_game(library.session.game.clone());
        self.library.set(Some(library));
    }

    fn run<F>(&self, command: impl FnOnce() -> F + 'static)
    where
        F: Future<Output = Result<(), govue_shell_runtime::CoreError>> + 'static,
    {
        if self.busy.get_untracked() {
            return;
        }
        self.error.set(None);
        self.busy.set(true);
        let busy = self.busy;
        let error = self.error;
        self.owner.run(|| {
            spawn(async move {
                if let Err(reason) = command().await {
                    error.set(Some(reason.to_string()));
                }
                busy.set(false);
            });
        });
    }

    fn start(&self, filter: &'static str, game: Option<String>) {
        let pane = self.clone();
        self.run(move || async move {
            let mut request = json!({"op":"ai_practice_start","filter":filter});
            if let Some(game) = game {
                request["gameId"] = json!(game);
            }
            let session: Session = pane.core.ai(request).await?;
            pane.replace_session(session);
            pane.library_open.set(false);
            Ok(())
        });
    }

    fn replace_session(&self, session: Session) {
        self.game.set_practice_game(session.game.clone());
        self.library.update(|library| {
            if let Some(library) = library {
                library.session = session;
            }
        });
    }

    fn grade(&self, verdict: Option<&'static str>) {
        let Some(library) = self.library.get_untracked() else {
            return;
        };
        let Some(game) = library
            .session
            .game
            .map(|game| game.id)
            .or_else(|| verdict.is_none().then(String::new))
        else {
            return;
        };
        let pane = self.clone();
        self.run(move || async move {
            let event: String = pane.core.state(json!({"op":"ai_game_session"}))?;
            let revision = library.session.revision;
            let request = if let Some(verdict) = verdict {
                json!({"op":"ai_practice_review","eventId":event,"revision":revision,"gameId":game,"verdict":verdict})
            } else {
                json!({"op":"ai_practice_undo","eventId":event,"revision":revision})
            };
            let session: Session = pane.core.ai(request).await?;
            pane.replace_session(session);
            if let Some(verdict) = verdict {
                pane.library.update(|library| {
                    if let Some(entry) = library.as_mut().and_then(|library| library.games.iter_mut().find(|item| item.id == game)) {
                        entry.verdict = Some(verdict.into());
                    }
                });
            } else {
                let library = pane.core.ai(json!({"op":"ai_practice_list"})).await?;
                pane.update(library);
            }
            Ok(())
        });
    }

    fn reset(&self) {
        let Some(game) = self.game.practice_game() else {
            return;
        };
        let pane = self.clone();
        self.run(move || async move {
            let outcome: SavedGame = pane
                .core
                .ai(json!({"op":"ai_practice_reset","gameId":game.id,"revision":game.revision}))
                .await?;
            pane.game.set_practice_game(Some(outcome.game));
            pane.restart.update(|revision| *revision += 1);
            Ok(())
        });
    }

    fn rename(&self, id: String, title: String) {
        self.error.set(None);
        self.name.set(title);
        self.rename.set(Some(id));
    }

    fn save_name(&self) {
        let Some(id) = self.rename.get_untracked() else {
            return;
        };
        let title = self.name.get_untracked();
        let pane = self.clone();
        self.run(move || async move {
            let outcome: SavedGame = pane
                .core
                .ai(json!({"op":"ai_practice_rename","gameId":id,"title":title}))
                .await?;
            let game = outcome.game;
            pane.library.update(|library| {
                if let Some(entry) = library
                    .as_mut()
                    .and_then(|library| library.games.iter_mut().find(|entry| entry.id == game.id))
                {
                    entry.title = game.title().into();
                }
            });
            if pane
                .game
                .practice_game()
                .is_some_and(|current| current.id == game.id)
            {
                pane.game.set_practice_game(Some(game));
            }
            pane.rename.set(None);
            Ok(())
        });
    }

    pub fn view(
        &self,
        p: Palette,
        back: impl Fn() + 'static,
        open_video: Rc<dyn Fn(String)>,
    ) -> View {
        let library = self.library;
        let open = self.library_open;
        let rename = self.rename;
        let error = self.error;
        let loaded = computed(move || library.with(Option::is_some));
        let reviewing = computed(move || {
            !open.get()
                && library.with(|library| {
                    library.as_ref().is_some_and(|library| {
                        library.session.started
                            && (library.session.game.is_some() || library.session.can_undo)
                    })
                })
        });
        let shelf = self.clone();
        let review = self.clone();
        let name = self.clone();
        let reload = self.clone();
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(6.0))
                    .py(px(5.0))
                    .child(ui::icon_button(p, ui::BACK, "Back to chat", back))
                    .child(
                        text("Practice")
                            .text_size(px(19.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    ),
            )
            .child(when(
                move || error.with(Option::is_some) && rename.with(Option::is_none),
                move || {
                    let reload = reload.clone();
                    view()
                        .flex_row()
                        .items_center()
                        .px(px(20.0))
                        .gap(px(8.0))
                        .child(
                            help(p, move || error.get().unwrap_or_default())
                                .flex_1()
                                .text_color(rgb(0xd32f2f)),
                        )
                        .child(action(p, "Reload", move || reload.load()))
                },
            ))
            .child(when(
                move || !loaded.get(),
                move || empty(p, "Opening saved activities…", ""),
            ))
            .child(when(
                move || loaded.get() && !reviewing.get(),
                move || shelf.shelf(p),
            ))
            .child(when(reviewing, move || {
                review.review(p, open_video.clone())
            }))
            .child(when(
                move || rename.with(Option::is_some),
                move || name.rename_dialog(p),
            ))
    }

    fn shelf(&self, p: Palette) -> View {
        let library = self.library;
        let open = self.library_open;
        let busy = self.busy;
        let review = self.clone();
        let one = self.clone();
        let rename = self.clone();
        let count = computed(move || {
            library.with(|library| library.as_ref().map_or(0, |library| library.games.len()))
        });
        view().flex_1().min_h(px(0.0))
            .child(view().min_h(px(54.0)).px(px(22.0)).flex_row().items_center().justify_between()
                .child(help(p, move || format!("{} saved activit{}", count.get(), if count.get() == 1 { "y" } else { "ies" })))
                .child(when(move || library.with(|library| library.as_ref().is_some_and(|library| library.session.game.is_some())), move || {
                    action(p, "Continue", move || open.set(false)).disabled(busy)
                })))
            .child(when(move || count.get() == 0, move || empty(p, "Your practice starts in chat",
                "Ask the assistant to create a word, grammar, or sentence activity. Saved games appear here.")))
            .child(when(move || count.get() > 0, move || {
                let review = review.clone();
                let one = one.clone();
                let rename = rename.clone();
                view().flex_1().min_h(px(0.0))
                    .child(action(p, "Review all", move || review.start("all", None)).mx(px(22.0)).mb(px(18.0)).disabled(busy))
                    .child(scroll().flex_1().child(view().px(px(22.0)).pb(px(24.0)).child(each(
                        move || library.with(|library| library.as_ref().map(|library| library.games.clone()).unwrap_or_default()),
                        |entry| entry.id.clone(),
                        move |entry, _| {
                            let one = one.clone();
                            let rename = rename.clone();
                            view().min_h(px(86.0)).flex_row().items_center().border_b(px(0.5)).border_color(move || p.raised())
                                .child(press().flex_1().flex_row().items_center().gap(px(14.0)).disabled(busy)
                                    .label(move || entry.with(|entry| format!("Practice {}", entry.title)))
                                    .on_press(move || one.start("all", Some(entry.with_untracked(|entry| entry.id.clone()))))
                                    .child(view().flex_1().min_w(px(0.0)).gap(px(5.0)).py(px(14.0))
                                        .child(text(move || entry.with(|entry| entry.title.clone())).text_size(px(16.0))
                                            .font_weight(FontWeight::MEDIUM).text_color(move || p.ink()))
                                        .child(help(p, move || entry.with(|entry| {
                                            let source = entry.source.as_ref().map(|source| source.title.as_str()).unwrap_or("Learning activity");
                                            format!("{source}{}", match entry.verdict.as_deref() {
                                                Some("know") => " · Known", Some("forgot") => " · Forgotten", _ => "",
                                            })
                                        }))))
                                    .child(ui::icon(ui::NEXT, 18.0, move || p.muted())))
                                .child(ui::icon_button(p, ui::EDIT, "Rename activity", move || {
                                    let entry = entry.get_untracked();
                                    rename.rename(entry.id, entry.title);
                                }).disabled(busy))
                        },
                    ))))
            }))
    }

    fn review(&self, p: Palette, open_video: Rc<dyn Fn(String)>) -> View {
        let open = self.library_open;
        let library = self.library;
        let busy = self.busy;
        let undo = self.clone();
        let cards = self.clone();
        let repeat = self.clone();
        let game = computed(move || {
            library.with(|library| {
                library
                    .as_ref()
                    .and_then(|library| library.session.game.as_ref())
                    .map(|game| game.id.clone())
            })
        });
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(
                view()
                    .min_h(px(48.0))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .child(action(p, "Library", move || open.set(true)).disabled(busy))
                    .child(help(p, move || {
                        library.with(|library| {
                            library
                                .as_ref()
                                .map(|library| {
                                    let session = &library.session;
                                    let number = if session.game.is_some() {
                                        session.total.saturating_sub(session.remaining) + 1
                                    } else {
                                        session.total
                                    };
                                    format!("{number} / {}", session.total)
                                })
                                .unwrap_or_default()
                        })
                    }))
                    .child(
                        ui::icon_button(p, UNDO, "Undo last swipe", move || undo.grade(None))
                            .disabled(move || {
                                busy.get()
                                    || !library.with(|library| {
                                        library
                                            .as_ref()
                                            .is_some_and(|library| library.session.can_undo)
                                    })
                            }),
                    ),
            )
            .child(each(
                move || game.get().into_iter().collect::<Vec<_>>(),
                Clone::clone,
                move |game, _| cards.card(p, game.get_untracked(), open_video.clone()),
            ))
            .child(when(
                move || game.with(Option::is_none),
                move || {
                    let retry = repeat.clone();
                    let again = repeat.clone();
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap(px(18.0))
                        .px(px(28.0))
                        .child(ui::icon(CHECK, 36.0, move || p.ink()))
                        .child(
                            text("Session complete")
                                .text_size(px(22.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                        )
                        .child(help(p, move || {
                            library.with(|library| {
                                library.as_ref().map_or_else(String::new, |library| {
                                    format!(
                                        "{} known · {} forgotten",
                                        library.session.known, library.session.forgot
                                    )
                                })
                            })
                        }))
                        .child(when(
                            move || {
                                library.with(|library| {
                                    library
                                        .as_ref()
                                        .is_some_and(|library| library.session.forgot > 0)
                                })
                            },
                            move || {
                                let retry = retry.clone();
                                action(p, "Practice forgotten activities", move || {
                                    retry.start("forgot", None)
                                })
                                .disabled(busy)
                            },
                        ))
                        .child(
                            action(p, "Review all again", move || again.start("all", None))
                                .disabled(busy),
                        )
                },
            ))
    }

    fn card(&self, p: Palette, id: String, open_video: Rc<dyn Fn(String)>) -> View {
        let busy = self.busy;
        let drag = signal(0.0_f32);
        let restart = self.restart;
        let rename = self.clone();
        let reset = self.clone();
        let renderer = self.game.clone();
        let title = self.game.clone();
        let source = self.game.clone();
        let swipe = self.clone();
        let forgot = self.clone();
        let know = self.clone();
        let surface = {
            let id = id.clone();
            each(
                move || {
                    if lifecycle() == Lifecycle::Active {
                        vec![restart.get()]
                    } else {
                        Vec::new()
                    }
                },
                |revision| *revision,
                move |_, _| renderer.practice_surface(p, id.clone()),
            )
        };
        view()
            .flex_1()
            .min_h(px(0.0))
            .px(px(18.0))
            .pt(px(8.0))
            .pb(px(16.0))
            .child(
                view()
                    .absolute()
                    .top(px(16.0))
                    .left(px(28.0))
                    .right(px(28.0))
                    .bottom(px(84.0))
                    .rounded(px(24.0))
                    .bg(move || p.raised())
                    .pointer_events(PointerEvents::None),
            )
            .child(
                view()
                    .flex_1()
                    .min_h(px(0.0))
                    .mb(px(20.0))
                    .rounded(px(22.0))
                    .overflow_hidden()
                    .border(px(0.5))
                    .border_color(move || p.raised())
                    .bg(move || p.background())
                    .translate_x(move || px(drag.get()))
                    .claim_touches(TouchClaim::on_move().capture().ask(
                        move |event: &ClaimRequest| {
                            let motion = &event.event.gesture;
                            if !busy.get_untracked()
                                && motion.active_touches == 1
                                && motion.delta.x.0.abs() > 8.0
                                && motion.delta.x.0.abs() > motion.delta.y.0.abs() * 1.2
                            {
                                event.claim();
                            }
                        },
                    ))
                    .on_responder_move(move |event: &ResponderEvent| {
                        drag.set(event.gesture.delta.x.0)
                    })
                    .on_responder_terminate(move |_: &ResponderEvent| drag.set(0.0))
                    .on_responder_release(move |event: &ResponderEvent| {
                        let distance = event.gesture.delta.x.0;
                        drag.set(0.0);
                        let threshold = (nsh_core::element::window().width.0 * 0.23).min(110.0);
                        if distance.abs() >= threshold {
                            swipe.grade(Some(if distance > 0.0 { "know" } else { "forgot" }));
                        }
                    })
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .min_h(px(62.0))
                            .pl(px(16.0))
                            .pr(px(4.0))
                            .hint("Swipe card left for Forgot or right for Know")
                            .child(ui::icon(GRIP, 17.0, move || p.muted()))
                            .child(
                                text(move || {
                                    title
                                        .practice_game()
                                        .map_or_else(String::new, |game| game.title().into())
                                })
                                .flex_1()
                                .px(px(10.0))
                                .text_size(px(17.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                            )
                            .child(
                                ui::icon_button(p, ui::EDIT, "Rename activity", move || {
                                    if let Some(game) = rename.game.practice_game() {
                                        rename.rename(game.id.clone(), game.title().into());
                                    }
                                })
                                .disabled(busy),
                            )
                            .child(
                                ui::icon_button(p, RESTART, "Restart activity", move || {
                                    reset.reset()
                                })
                                .disabled(busy),
                            ),
                    )
                    .child(view().flex_1().min_h(px(0.0)).mx(px(12.0)).child(surface))
                    .child(dynamic(move || {
                        let source = source.practice_game()?.source?;
                        let navigate = open_video.clone();
                        let time = source.start.map(timestamp);
                        let title = time.as_ref().map_or_else(
                            || "Open original".into(),
                            |time| format!("Open original · {time}"),
                        );
                        Some(
                            press()
                                .min_h(px(40.0))
                                .items_center()
                                .justify_center()
                                .label(time.map_or_else(
                                    || "Open original YouTube video".into(),
                                    |time| format!("Open original YouTube video at {time}"),
                                ))
                                .on_press(move || {
                                    let mut address = format!(
                                        "https://m.youtube.com/watch?v={}",
                                        source.video_id
                                    );
                                    if let Some(start) = source.start {
                                        use std::fmt::Write;
                                        let _ = write!(address, "&t={}s", start.floor() as u64);
                                    }
                                    navigate(address);
                                })
                                .child(
                                    text(title)
                                        .text_size(px(13.0))
                                        .text_color(move || p.muted()),
                                ),
                        )
                    }))
                    .child(
                        view()
                            .min_h(px(46.0))
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(18.0))
                            .child(help(p, || "← Forgot".into()))
                            .child(help(p, || "Drag anywhere".into()))
                            .child(help(p, || "Know →".into())),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .justify_center()
                    .gap(px(18.0))
                    .child(
                        action(p, "Forgot", move || forgot.grade(Some("forgot")))
                            .label("Forgot this activity")
                            .disabled(busy),
                    )
                    .child(
                        action(p, "Know", move || know.grade(Some("know")))
                            .label("Know this activity")
                            .disabled(busy),
                    ),
            )
    }

    fn rename_dialog(&self, p: Palette) -> View {
        let name = self.name;
        let rename = self.rename;
        let busy = self.busy;
        let error = self.error;
        let save = self.clone();
        let submit = self.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .justify_center()
            .px(px(20.0))
            .z_index(4)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000073))
                    .label("Close rename activity")
                    .on_press(move || {
                        if !busy.get_untracked() {
                            rename.set(None);
                        }
                    }),
            )
            .child(
                view()
                    .w_full()
                    .max_w(px(380.0))
                    .self_center()
                    .p(px(20.0))
                    .rounded(px(18.0))
                    .bg(move || p.background())
                    .gap(px(12.0))
                    .child(
                        text("Rename activity")
                            .text_size(px(19.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    )
                    .child(
                        input()
                            .label("Activity name")
                            .value(move || name.get())
                            .auto_focus(true)
                            .readonly(busy)
                            .h(px(52.0))
                            .px(px(12.0))
                            .rounded(px(12.0))
                            .bg(move || p.raised())
                            .text_color(move || p.ink())
                            .on_change(move |value: &str| {
                                name.set(value.chars().take(120).collect())
                            })
                            .on_submit(move |_: &str| submit.save_name()),
                    )
                    .child(when(
                        move || error.with(Option::is_some),
                        move || {
                            help(p, move || error.get().unwrap_or_default())
                                .text_color(rgb(0xd32f2f))
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .justify_end()
                            .gap(px(12.0))
                            .child(action(p, "Cancel", move || rename.set(None)).disabled(busy))
                            .child(
                                action(p, "Save", move || save.save_name())
                                    .label("Save activity name")
                                    .disabled(move || {
                                        busy.get() || name.with(|value| value.trim().is_empty())
                                    }),
                            ),
                    ),
            )
    }
}

fn empty(p: Palette, title: &'static str, detail: &'static str) -> View {
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(18.0))
        .px(px(28.0))
        .child(ui::icon(LIBRARY, 36.0, move || p.muted()))
        .child(
            text(title)
                .text_size(px(20.0))
                .text_center()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
        .children((!detail.is_empty()).then(|| help(p, move || detail.into()).text_center()))
}

fn help(p: Palette, label: impl Fn() -> String + 'static) -> Text {
    text(label)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}

fn action(p: Palette, name: &'static str, run: impl Fn() + 'static) -> View {
    press()
        .min_h(px(44.0))
        .px(px(16.0))
        .rounded(px(24.0))
        .items_center()
        .justify_center()
        .bg(move || p.raised())
        .label(name)
        .on_press(run)
        .child(
            text(name)
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || p.ink()),
        )
}
