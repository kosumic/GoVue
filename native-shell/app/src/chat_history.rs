use std::rc::Rc;

use nsh_core::{
    KeyboardDismissMode, KeyboardTaps,
    element::{ElementRef, input},
    prelude::*,
};
use serde_json::{Value, json};

use crate::chat::{Chat, SavedChat};
use crate::ui::{self, Palette, press};

const ARCHIVE: &str = "M3 4h18v4H3zm2 4v12h14V8m-9 4h4";
const PIN: &str = "m16 3 5 5-3 1-3 3v4l-2 2-7-7 2-2h4l3-3zM9 15l-6 6";
const TRASH: &str = "M3 6h18M5 6l1 14h12l1-14M9 6V3h6v3m-5 4v6m4-6v6";
const SETTINGS: &str = "M4 6h16M4 12h16M4 18h16M8 4v4m8 2v4m-6 2v4";
const GAUGE: &str = "M4 19a9 9 0 1 1 16 0zM12 15l4-4";
const PRACTICE: &str = "m12 3 9 5-9 5-9-5 9-5zM3 12l9 5 9-5M3 16l9 5 9-5";

pub struct HistoryNavigation {
    pub practice: Rc<dyn Fn()>,
    pub providers: Rc<dyn Fn()>,
    pub usage: Rc<dyn Fn()>,
}

#[derive(Clone, PartialEq)]
enum Action {
    Options(SavedChat),
    Rename(SavedChat),
    Delete(SavedChat),
}

#[derive(Clone)]
pub struct ChatHistory {
    chat: Chat,
    query: Signal<String>,
    archived: Signal<bool>,
    search: ElementRef,
    action: Signal<Option<Action>>,
    busy: Signal<bool>,
    name: Signal<String>,
    error: Signal<Option<String>>,
}

impl ChatHistory {
    pub fn new(chat: Chat) -> Self {
        Self {
            chat,
            query: signal(String::new()),
            archived: signal(false),
            search: element_ref(),
            action: signal(None),
            busy: signal(false),
            name: signal(String::new()),
            error: signal(None),
        }
    }

    pub fn open(&self) {
        self.action.set(None);
        self.filter(String::new(), false);
    }

    pub fn back(&self) -> bool {
        if !self.action.with_untracked(Option::is_some) {
            return false;
        }
        if !self.busy.get_untracked() {
            self.action.set(None);
        }
        true
    }

    fn filter(&self, query: String, archived: bool) {
        let query: String = query.chars().take(200).collect();
        self.query.set(query.clone());
        self.archived.set(archived);
        self.chat.find_chats(query, archived);
    }

    fn manage(&self, entry: SavedChat) {
        self.search.blur();
        self.name.set(entry.title.clone());
        self.error.set(None);
        self.action.set(Some(Action::Options(entry)));
    }

    fn apply(&self, id: String, update: Option<Value>) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let history = self.clone();
        spawn(async move {
            let result = match update {
                Some(update) => history.chat.update_saved(id, update).await,
                None => history.chat.delete_saved(id).await,
            };
            history.busy.set(false);
            match result {
                Ok(()) => history.action.set(None),
                Err(error) => history.error.set(Some(error.to_string())),
            }
        });
    }

    pub fn view(
        &self,
        p: Palette,
        close: impl Fn() + Clone + 'static,
        open: impl Fn(String, Option<String>) + Clone + 'static,
        new: impl Fn() + 'static,
        navigation: HistoryNavigation,
    ) -> View {
        let list = self.chat.clone();
        let status = self.chat.clone();
        let empty = self.chat.clone();
        let error = self.chat.history_error;
        let query = self.query;
        let archived = self.archived;
        let update_search = self.clone();
        let clear = self.clone();
        let archive = self.clone();
        let manage = self.clone();
        let search = self.search;
        let action = self.action;
        let HistoryNavigation {
            practice,
            providers,
            usage,
        } = navigation;
        let manage_sheet = self.clone();
        let backdrop = close.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000077))
                    .label("Close conversations")
                    .on_press(backdrop),
            )
            .child(
                view()
                    .h_full()
                    .w(percentage(0.86))
                    .max_w(px(360.0))
                    .px(px(14.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .pt(px(5.0))
                            .child(
                                text("GoVue")
                                    .px(px(10.0))
                                    .text_size(px(20.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(p, ui::CLOSE, "Close conversations", close)),
                    )
                    .child(
                        row(p, ui::EDIT, || "New chat".into())
                            .h(px(50.0))
                            .mt(px(14.0))
                            .mb(px(12.0))
                            .rounded(px(14.0))
                            .bg(move || p.raised())
                            .on_press(new),
                    )
                    .child(row(p, PRACTICE, || "Practice".into()).on_press(move || {
                        search.blur();
                        practice();
                    }))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .h(px(48.0))
                            .gap(px(8.0))
                            .pl(px(12.0))
                            .pr(px(2.0))
                            .mb(px(18.0))
                            .mt(px(4.0))
                            .rounded(px(12.0))
                            .bg(move || p.raised())
                            .child(ui::icon(ui::SEARCH, 18.0, move || p.muted()))
                            .child(
                                input()
                                    .element_ref(&search)
                                    .label("Search conversations")
                                    .placeholder("Search conversations")
                                    .h(px(48.0))
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .value(move || query.get())
                                    .text_size(px(14.0))
                                    .text_color(move || p.ink())
                                    .placeholder_color(move || p.muted())
                                    .on_change(move |value: &str| {
                                        update_search.filter(value.into(), archived.get_untracked());
                                    })
                                    .on_submit(move |_: &str| search.blur()),
                            )
                            .child(when(
                                move || !query.with(String::is_empty),
                                move || {
                                    let clear = clear.clone();
                                    ui::icon_button(p, ui::CLOSE, "Clear conversation search", move || {
                                        clear.filter(String::new(), archived.get_untracked());
                                    })
                                },
                            )),
                    )
                    .child(
                        text(move || {
                            if error.with(Option::is_some) {
                                "Search unavailable".into()
                            } else if !query.with(|query| query.trim().is_empty()) {
                                if status.history_pending.get() {
                                    "Searching…".into()
                                } else {
                                    let count = status.history().len();
                                    format!("{count} conversation{}", if count == 1 { "" } else { "s" })
                                }
                            } else if archived.get() {
                                "Archived conversations".into()
                            } else {
                                "Conversations".into()
                            }
                        })
                        .text_size(px(12.0))
                        .text_color(move || p.muted())
                        .px(px(12.0))
                        .mb(px(8.0)),
                    )
                    .child(
                        scroll()
                            .flex_1()
                            .keyboard_taps(KeyboardTaps::Handled)
                            .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
                            .child(
                                view()
                                    .pb(px(16.0))
                                    .child(each(
                                        move || list.history(),
                                        |chat| chat.id.clone(),
                                        move |entry, _| {
                                            let manage = manage.clone();
                                            let selected = manage.chat.clone();
                                            let open = open.clone();
                                            press()
                                                .min_h(px(54.0))
                                                .px(px(12.0))
                                                .py(px(10.0))
                                                .gap(px(5.0))
                                                .rounded(px(12.0))
                                                .label(move || entry.with(|entry| {
                                                    let pinned = if entry.pinned { ", pinned" } else { "" };
                                                    format!("Open chat {}{pinned}", entry.title)
                                                }))
                                                .bg(move || {
                                                    if selected.current().is_some_and(|chat| entry.with(|entry| entry.id == chat.id)) {
                                                        p.raised()
                                                    } else {
                                                        p.background()
                                                    }
                                                })
                                                .on_long_press(move || manage.manage(entry.with_untracked(Clone::clone)))
                                                .on_press(move || {
                                                    search.blur();
                                                    let entry = entry.with_untracked(Clone::clone);
                                                    let search = entry.search_match.and_then(|hit| hit.message_id)
                                                        .map(|_| query.get_untracked().trim().into());
                                                    open(entry.id, search);
                                                })
                                                .child(
                                                    view().flex_row().items_center().gap(px(7.0))
                                                        .child(when(
                                                            move || entry.with(|entry| entry.pinned),
                                                            move || ui::icon(PIN, 13.0, move || p.muted()),
                                                        ))
                                                        .child(
                                                            text(move || entry.with(|entry| entry.title.clone()))
                                                                .flex_1().truncate().text_size(px(14.0)).text_color(move || p.ink()),
                                                        ),
                                                )
                                                .child(
                                                    text(move || entry.with(|entry| {
                                                        entry.search_match.as_ref().map_or_else(
                                                            || entry.model.clone(),
                                                            |hit| format!("{}{}{}{}{}",
                                                                if hit.leading { "…" } else { "" },
                                                                hit.before, hit.matched, hit.after,
                                                                if hit.trailing { "…" } else { "" },
                                                            ),
                                                        )
                                                    }))
                                                    .truncate().text_size(px(11.0)).text_color(move || p.muted()),
                                                )
                                        },
                                    ))
                                    .child(when(
                                        move || !empty.history_pending.get() && empty.history().is_empty() && !error.with(Option::is_some),
                                        move || help(p, move || {
                                            if !query.with(|query| query.trim().is_empty()) {
                                                "No conversations found. Try another word or phrase.".into()
                                            } else if archived.get() {
                                                "No archived conversations.".into()
                                            } else {
                                                "Your conversations will appear here.".into()
                                            }
                                        }),
                                    ))
                                    .child(when(
                                        move || error.with(Option::is_some),
                                        move || error_text(move || error.get().unwrap_or_default()),
                                    )),
                            ),
                    )
                    .child(
                        view()
                            .border_t(px(0.5))
                            .border_color(move || p.raised())
                            .py(px(10.0))
                            .child(row(p, ARCHIVE, move || {
                                if archived.get() { "Back to conversations".into() } else { "Archived chats".into() }
                            }).on_press(move || {
                                search.blur();
                                archive.filter(query.get_untracked(), !archived.get_untracked());
                            }))
                            .child(row(p, SETTINGS, || "Models & providers".into()).on_press(move || {
                                search.blur();
                                providers();
                            }))
                            .child(row(p, GAUGE, || "Token usage".into()).on_press(move || {
                                search.blur();
                                usage();
                            })),
                    ),
            )
            .child(when(
                move || action.with(Option::is_some),
                move || manage_sheet.dialog(p),
            ))
    }

    fn dialog(&self, p: Palette) -> View {
        let action = self.action;
        let busy = self.busy;
        let error = self.error;
        let panel = self.clone();
        let close = self.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000080))
                    .label("Close chat options")
                    .on_press(move || {
                        close.back();
                    }),
            )
            .child(dynamic(move || {
                let Some(current) = action.get() else {
                    return view();
                };
                let title = match current {
                    Action::Options(_) => "Chat options",
                    Action::Rename(_) => "Rename chat",
                    Action::Delete(_) => "Delete chat?",
                };
                let options = matches!(current, Action::Options(_));
                let surface = view()
                    .self_center()
                    .w_full()
                    .max_w(if options { px(680.0) } else { px(340.0) })
                    .rounded(px(18.0))
                    .bg(move || p.background())
                    .px(px(20.0))
                    .pt(px(16.0))
                    .pb(px(18.0))
                    .gap(px(8.0))
                    .child(
                        text(title)
                            .text_size(px(19.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    );
                let contents = match current {
                    Action::Options(chat) => {
                        let pin = panel.clone();
                        let archive = panel.clone();
                        let pin_id = chat.id.clone();
                        let archive_id = chat.id.clone();
                        let rename = chat.clone();
                        view()
                            .child(help(p, {
                                let title = chat.title.clone();
                                move || title.clone()
                            }))
                            .child(
                                row(p, PIN, move || {
                                    if chat.pinned {
                                        "Unpin".into()
                                    } else {
                                        "Pin".into()
                                    }
                                })
                                .disabled(busy)
                                .on_press(move || {
                                    pin.apply(pin_id.clone(), Some(json!({"pinned":!chat.pinned})))
                                }),
                            )
                            .child(
                                row(p, ARCHIVE, move || {
                                    if chat.archived {
                                        "Unarchive".into()
                                    } else {
                                        "Archive".into()
                                    }
                                })
                                .disabled(busy)
                                .on_press(move || {
                                    archive.apply(
                                        archive_id.clone(),
                                        Some(json!({"archived":!chat.archived})),
                                    )
                                }),
                            )
                            .child(
                                row(p, ui::EDIT, || "Rename".into())
                                    .disabled(busy)
                                    .on_press(move || {
                                        action.set(Some(Action::Rename(rename.clone())))
                                    }),
                            )
                            .child(
                                row(p, TRASH, || "Delete".into()).disabled(busy).on_press(
                                    move || action.set(Some(Action::Delete(chat.clone()))),
                                ),
                            )
                    }
                    Action::Rename(chat) => {
                        let name = panel.name;
                        let save_id = chat.id;
                        let save = {
                            let panel = panel.clone();
                            move || {
                                let title = name.get_untracked();
                                if !title.trim().is_empty() {
                                    panel.apply(save_id.clone(), Some(json!({"title":title})));
                                }
                            }
                        };
                        view()
                            .gap(px(12.0))
                            .child(
                                input()
                                    .label("Chat name")
                                    .placeholder("Chat name")
                                    .value(move || name.get())
                                    .h(px(52.0))
                                    .px(px(12.0))
                                    .rounded(px(12.0))
                                    .bg(move || p.raised())
                                    .text_size(px(16.0))
                                    .text_color(move || p.ink())
                                    .on_change(move |value: &str| {
                                        name.set(value.chars().take(200).collect())
                                    })
                                    .on_submit({
                                        let save = save.clone();
                                        move |_: &str| save()
                                    }),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .justify_end()
                                    .gap(px(10.0))
                                    .child(button(p, "Cancel", busy, move || action.set(None)))
                                    .child(button(
                                        p,
                                        "Save",
                                        move || {
                                            busy.get() || name.with(|name| name.trim().is_empty())
                                        },
                                        save,
                                    )),
                            )
                    }
                    Action::Delete(chat) => {
                        let delete = panel.clone();
                        view()
                            .gap(px(12.0))
                            .child(help(p, move || chat.title.clone()))
                            .child(
                                view()
                                    .flex_row()
                                    .justify_end()
                                    .gap(px(10.0))
                                    .child(button(p, "Keep", busy, move || action.set(None)))
                                    .child(button(p, "Delete", busy, move || {
                                        delete.apply(chat.id.clone(), None)
                                    })),
                            )
                    }
                };
                let sheet = surface.child(contents).child(when(
                    move || error.with(Option::is_some),
                    move || error_text(move || error.get().unwrap_or_default()),
                ));
                let placement = view()
                    .absolute()
                    .inset(px(0.0))
                    .pointer_events(PointerEvents::BoxNone);
                if options {
                    placement.justify_end().child(sheet)
                } else {
                    placement.justify_center().child(sheet)
                }
            }))
    }
}

fn row(p: Palette, icon: &'static str, label: impl Fn() -> String + Clone + 'static) -> View {
    press()
        .min_h(px(48.0))
        .flex_row()
        .items_center()
        .px(px(12.0))
        .gap(px(12.0))
        .label(label.clone())
        .child(ui::icon(icon, 20.0, move || p.ink()))
        .child(text(label).text_size(px(14.0)).text_color(move || p.ink()))
}

fn button(
    p: Palette,
    label: &'static str,
    disabled: impl IntoReactive<bool>,
    run: impl Fn() + 'static,
) -> View {
    press()
        .h(px(48.0))
        .min_w(px(80.0))
        .px(px(18.0))
        .rounded(px(24.0))
        .items_center()
        .justify_center()
        .bg(move || p.raised())
        .label(label)
        .disabled(disabled)
        .on_press(run)
        .child(
            text(label)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
}

fn help(p: Palette, label: impl Fn() -> String + 'static) -> Text {
    text(label)
        .px(px(12.0))
        .py(px(10.0))
        .text_size(px(13.0))
        .text_color(move || p.muted())
}

fn error_text(label: impl Fn() -> String + 'static) -> Text {
    text(label)
        .px(px(12.0))
        .py(px(10.0))
        .text_size(px(13.0))
        .text_color(rgb(0xd32f2f))
}
