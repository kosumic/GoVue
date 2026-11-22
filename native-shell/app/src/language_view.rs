use std::time::Duration;

use govue_shell_runtime::Core;
use nsh_core::{Checked, KeyboardDismissMode, KeyboardTaps, element::input, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ui::{self, Palette, press};

const PLUS: &str = "M12 5v14M5 12h14";
const DELETE: &str = "M3 6h18m-2 0-1.5 15h-11L5 6m4 0V3h6v3M10 10v7m4-7v7";
const CHECK: &str = "m4 12 5 5L20 6";

const LEVELS: [(&str, &str, &str); 7] = [
    (
        "A1",
        "Beginner",
        "Use basic phrases, introduce yourself, and ask simple questions with help.",
    ),
    (
        "A2",
        "Elementary",
        "Handle routine exchanges about shopping, family, work, and familiar needs.",
    ),
    (
        "B1",
        "Intermediate",
        "Follow familiar topics, manage most travel situations, and explain experiences and plans.",
    ),
    (
        "B2",
        "Upper intermediate",
        "Understand complex topics, converse comfortably, and explain viewpoints in detail.",
    ),
    (
        "C1",
        "Advanced",
        "Understand implied meaning and use the language fluently for work, study, and social life.",
    ),
    (
        "C2",
        "Proficient",
        "Understand almost everything you hear or read and express subtle meanings precisely.",
    ),
    (
        "native",
        "Native",
        "A language you grew up speaking. This is separate from the CEFR scale.",
    ),
];

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Known,
    Learning,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::Known => "Languages I know",
            Self::Learning => "Languages I want to learn",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Known => {
                "Choose your current level in each language, including your native language."
            }
            Self::Learning => {
                "Choose the level you want to reach. A language can also be in your known list at its current level."
            }
        }
    }

    fn level_label(self) -> &'static str {
        if self == Self::Known {
            "Current"
        } else {
            "Target"
        }
    }
    fn entries(self, data: &mut Settings) -> &mut Vec<Entry> {
        match self {
            Self::Known => &mut data.known,
            Self::Learning => &mut data.learning,
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
struct Entry {
    language: String,
    level: String,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
struct Settings {
    known: Vec<Entry>,
    learning: Vec<Entry>,
}

#[derive(Clone, PartialEq)]
struct Editor {
    group: Group,
    index: Option<usize>,
    value: Entry,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Language {
    code: String,
    name: String,
    native_name: String,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Section {
    title: String,
    data: Vec<Language>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Choices {
    selected: Option<Language>,
    sections: Vec<Section>,
}

#[derive(Clone)]
pub struct Languages {
    core: Core,
    p: Palette,
    settings: Signal<Option<Settings>>,
    editor: Signal<Option<Editor>>,
    picker: Signal<bool>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    saved: Signal<bool>,
}

impl Languages {
    pub fn new(core: Core, p: Palette) -> Self {
        let panel = Self {
            core,
            p,
            settings: signal(None),
            editor: signal(None),
            picker: signal(false),
            busy: signal(false),
            error: signal(None),
            saved: signal(false),
        };
        panel.load(None);
        panel
    }

    fn load(&self, request: Option<Settings>) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        self.saved.set(false);
        let panel = self.clone();
        spawn(async move {
            let saving = request.is_some();
            let mut command = json!({"op":"language_settings"});
            if let Some(settings) = request {
                command["settings"] = json!(settings);
            }
            let result = panel.core.execute::<Settings>(command).await;
            panel.busy.set(false);
            match result {
                Ok(settings) => {
                    panel.settings.set(Some(settings));
                    if saving {
                        panel.editor.set(None);
                        panel.saved.set(true);
                    }
                }
                Err(error) => panel.error.set(Some(error.to_string())),
            }
        });
    }

    fn edit(&self, editor: Editor) {
        self.error.set(None);
        self.saved.set(false);
        self.editor.set(Some(editor));
    }

    pub fn view(&self) -> View {
        let p = self.p;
        let state = self.settings;
        let panel = self.clone();
        let retry = self.clone();
        let busy = self.busy;
        let error = self.error;
        let saved = self.saved;
        view().px(px(16.0)).pb(px(24.0)).gap(px(12.0))
            .child(text("Your language levels").text_size(px(20.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
            .child(help(p, "Use the CEFR scale (Common European Framework of Reference): A1 is beginner and C2 is proficient. Choose a level to see what it means."))
            .child(when(move || state.with(Option::is_some), move || {
                view().gap(px(20.0)).child(panel.group(Group::Known)).child(panel.group(Group::Learning))
            }))
            .child(when(move || busy.get() && state.with(Option::is_none), move || help(p, "Loading language settings…")))
            .child(when(move || error.with(Option::is_some), move || {
                text(move || error.get().unwrap_or_default()).text_color(rgb(0xd53333)).text_size(px(13.0)).role(AccessibilityRole::Alert)
            }))
            .child(when(move || !busy.get() && state.with(Option::is_none), move || {
                let retry = retry.clone();
                action(p, "Retry loading languages", None, move || retry.load(None))
            }))
            .child(when(move || saved.get(), move || help(p, "Languages saved on this device.")))
            .child(self.language_picker())
    }

    fn group(&self, group: Group) -> View {
        let p = self.p;
        let state = self.settings;
        let editor = self.editor;
        let busy = self.busy;
        let editing = self.clone();
        let adding = self.clone();
        let rows = self.clone();
        let entries = computed(move || {
            state
                .get()
                .map(|mut settings| std::mem::take(group.entries(&mut settings)))
                .unwrap_or_default()
        });
        let open = computed(move || {
            editor.with(|editor| editor.as_ref().is_some_and(|edit| edit.group == group))
        });
        view()
            .gap(px(8.0))
            .child(
                text(group.title())
                    .role(AccessibilityRole::Header)
                    .text_size(px(17.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || p.ink()),
            )
            .child(help(p, group.help()))
            .child(when(
                move || entries.with(Vec::is_empty),
                move || help(p, "No languages added yet.").py(px(8.0)),
            ))
            .child(each(
                entries,
                |entry| entry.language.clone(),
                move |entry, index| {
                    let edit = rows.clone();
                    let remove = rows.clone();
                    let name = entry.with_untracked(|entry| entry.language.clone());
                    let value = move || entry.with(|entry| level_name(&entry.level));
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .py(px(4.0))
                        .border_b(px(0.5))
                        .border_color(move || p.muted())
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(4.0))
                                .child(
                                    text(name.clone())
                                        .text_size(px(15.0))
                                        .text_color(move || p.ink()),
                                )
                                .child(help(p, move || {
                                    format!("{}: {}", group.level_label(), value())
                                })),
                        )
                        .child(
                            icon_action(
                                p,
                                ui::EDIT,
                                format!(
                                    "Edit {name} {} level",
                                    group.level_label().to_ascii_lowercase()
                                ),
                                move || {
                                    edit.edit(Editor {
                                        group,
                                        index: Some(index.get()),
                                        value: entry.get(),
                                    });
                                },
                            )
                            .disabled(move || busy.get() || editor.with(Option::is_some)),
                        )
                        .child(
                            icon_action(
                                p,
                                DELETE,
                                format!(
                                    "Remove {name} from {}",
                                    group.title().to_ascii_lowercase()
                                ),
                                move || {
                                    if let Some(mut data) = state.get_untracked() {
                                        group
                                            .entries(&mut data)
                                            .retain(|entry| entry.language != name);
                                        remove.load(Some(data));
                                    }
                                },
                            )
                            .disabled(move || busy.get() || editor.with(Option::is_some)),
                        )
                },
            ))
            .child(when(open, move || editing.editor(group)))
            .child(when(
                move || !open.get(),
                move || {
                    let add = adding.clone();
                    action(p, "Add language", Some(PLUS), move || {
                        add.edit(Editor {
                            group,
                            index: None,
                            value: Entry {
                                language: String::new(),
                                level: "A1".into(),
                            },
                        });
                    })
                    .label(if group == Group::Known {
                        "Add a language I know"
                    } else {
                        "Add a language I want to learn"
                    })
                    .disabled(move || busy.get() || editor.with(Option::is_some))
                },
            ))
    }

    fn editor(&self, group: Group) -> View {
        let p = self.p;
        let edit = self.editor;
        let picker = self.picker;
        let busy = self.busy;
        let state = self.settings;
        let panel = self.clone();
        let level = computed(move || {
            edit.with(|value| {
                value
                    .as_ref()
                    .map_or_else(String::new, |editor| editor.value.level.clone())
            })
        });
        view()
            .gap(px(12.0))
            .py(px(12.0))
            .child(
                text(move || {
                    if edit
                        .with(|value| value.as_ref().is_some_and(|editor| editor.index.is_some()))
                    {
                        "Edit language"
                    } else {
                        "Add a language"
                    }
                })
                .text_size(px(15.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || p.ink()),
            )
            .child(
                press()
                    .h(px(52.0))
                    .rounded(px(10.0))
                    .bg(move || p.raised())
                    .px(px(14.0))
                    .items_center()
                    .flex_row()
                    .gap(px(12.0))
                    .label(if group == Group::Known {
                        "Language I know"
                    } else {
                        "Language I want to learn"
                    })
                    .disabled(move || busy.get())
                    .on_press(move || picker.set(true))
                    .child(
                        text(move || {
                            edit.with(|editor| {
                                editor
                                    .as_ref()
                                    .map(|editor| editor.value.language.as_str())
                                    .filter(|name| !name.is_empty())
                                    .unwrap_or("Choose a language")
                                    .to_owned()
                            })
                        })
                        .flex_1()
                        .text_size(px(16.0))
                        .text_color(move || p.ink()),
                    )
                    .child(ui::icon(ui::DOWN, 20.0, move || p.muted())),
            )
            .child(
                text(format!("{} level", group.level_label()))
                    .text_size(px(15.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || p.ink()),
            )
            .child(
                view().flex_row().flex_wrap().gap(px(8.0)).children(
                    LEVELS
                        .into_iter()
                        .filter(|(id, _, _)| group == Group::Known || *id != "native")
                        .map(|(id, title, _)| level_choice(p, edit, level, busy, id, title))
                        .collect::<Vec<_>>(),
                ),
            )
            .child(
                text(move || level_name(&level.get()))
                    .text_size(px(15.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || p.ink()),
            )
            .child(help(p, move || {
                LEVELS
                    .iter()
                    .find(|(id, _, _)| level.with(|level| level == id))
                    .map_or("", |(_, _, help)| *help)
            }))
            .child(
                view()
                    .flex_row()
                    .gap(px(8.0))
                    .child(
                        action(p, "Save language", Some(CHECK), move || {
                            let (Some(mut settings), Some(editor)) =
                                (state.get_untracked(), edit.get_untracked())
                            else {
                                return;
                            };
                            let entries = editor.group.entries(&mut settings);
                            match editor.index {
                                Some(index) if index < entries.len() => {
                                    entries[index] = editor.value
                                }
                                None => entries.push(editor.value),
                                _ => return,
                            }
                            panel.load(Some(settings));
                        })
                        .disabled(move || {
                            busy.get()
                                || edit.with(|editor| {
                                    editor
                                        .as_ref()
                                        .is_none_or(|editor| editor.value.language.is_empty())
                                })
                        }),
                    )
                    .child(
                        action(p, "Cancel", None, {
                            let error = self.error;
                            move || {
                                edit.set(None);
                                error.set(None);
                            }
                        })
                        .disabled(move || busy.get()),
                    ),
            )
    }

    fn language_picker(&self) -> If {
        let panel = self.clone();
        let picker = self.picker;
        when(picker, move || {
            nsh_base::dialog()
                .presentation(nsh_core::ModalPresentation::PageSheet)
                .label("Choose a language")
                .on_cancel(move || picker.set(false))
                .child(panel.picker_sheet().p_safe_area())
        })
    }

    fn picker_sheet(&self) -> View {
        let p = self.p;
        let picker = self.picker;
        let editor = self.editor;
        let state = self.settings;
        let core = self.core.clone();
        let query = signal(String::new());
        let choices = signal(None::<Choices>);
        let error = signal(None::<String>);
        let revision = signal(0_u32);
        let request_id = std::cell::Cell::new(0_u64);
        let request_id = std::rc::Rc::new(request_id);
        effect(move || {
            let text = query.get();
            let _ = revision.get();
            let (Some(value), Some(mut settings)) = (editor.get(), state.get()) else {
                return;
            };
            let excluded = value
                .group
                .entries(&mut settings)
                .iter()
                .enumerate()
                .filter(|(index, _)| value.index != Some(*index))
                .map(|(_, entry)| entry.language.clone())
                .collect::<Vec<_>>();
            let request = request_id.get() + 1;
            request_id.set(request);
            let current = request_id.clone();
            let core = core.clone();
            error.set(None);
            spawn(async move {
                timer(Duration::from_millis(80)).await;
                if current.get() != request {
                    return;
                }
                let result = core.execute::<Choices>(json!({"op":"language_choices","query":text,"excluded":excluded,"selected":value.value.language})).await;
                if current.get() == request {
                    match result {
                        Ok(result) => choices.set(Some(result)),
                        Err(message) => error.set(Some(message.to_string())),
                    }
                }
            });
        });
        let search = element_ref();
        let close = move || picker.set(false);
        view()
            .flex_1()
            .bg(move || p.background())
            .child(
                view()
                    .flex_row()
                    .px(px(18.0))
                    .py(px(8.0))
                    .items_center()
                    .child(
                        text("Choose a language")
                            .flex_1()
                            .text_size(px(22.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    )
                    .child(ui::icon_button(
                        p,
                        ui::CLOSE,
                        "Close language picker",
                        close,
                    )),
            )
            .child(
                view()
                    .m(px(20.0))
                    .rounded(px(10.0))
                    .bg(move || p.raised())
                    .flex_row()
                    .items_center()
                    .child(
                        input()
                            .element_ref(&search)
                            .label("Search languages")
                            .placeholder("Search languages (optional)")
                            .value(move || query.get())
                            .on_change(move |text: &str| {
                                query.set(text.chars().take(200).collect())
                            })
                            .on_submit(move |_: &str| search.blur())
                            .flex_1()
                            .min_h(px(50.0))
                            .p(px(14.0))
                            .text_size(px(16.0))
                            .text_color(move || p.ink()),
                    )
                    .child(when(
                        move || !query.with(String::is_empty),
                        move || {
                            ui::icon_button(p, ui::CLOSE, "Clear language search", move || {
                                query.set(String::new())
                            })
                        },
                    )),
            )
            .child(when(
                move || error.with(Option::is_some),
                move || {
                    view()
                        .p(px(20.0))
                        .gap(px(8.0))
                        .child(help(p, move || error.get().unwrap_or_default()))
                        .child(action(p, "Retry loading languages", None, move || {
                            revision.update(|value| *value += 1)
                        }))
                },
            ))
            .child(when(
                move || choices.with(Option::is_none) && error.with(Option::is_none),
                move || help(p, "Loading languages…").p(px(20.0)),
            ))
            .child(when(
                move || error.with(Option::is_none),
                move || picker_list(p, choices, editor, picker),
            ))
    }
}

fn picker_list(
    p: Palette,
    choices: Signal<Option<Choices>>,
    editor: Signal<Option<Editor>>,
    picker: Signal<bool>,
) -> Scroll {
    let sections = computed(move || {
        choices.with(|value| {
            value
                .as_ref()
                .map(|choices| choices.sections.clone())
                .unwrap_or_default()
        })
    });
    scroll()
        .flex_1()
        .keyboard_taps(KeyboardTaps::Handled)
        .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
        .child(
            view()
                .px(px(20.0))
                .pb(px(32.0))
                .child(when(
                    move || choices.with(Option::is_some) && sections.with(Vec::is_empty),
                    move || help(p, "No matching languages.").py(px(24.0)),
                ))
                .child(each(
                    sections,
                    |section| section.title.clone(),
                    move |section, _| {
                        view()
                            .child(
                                help(p, move || section.with(|section| section.title.clone()))
                                    .py(px(10.0))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(each(
                                move || section.with(|section| section.data.clone()),
                                |language| language.code.clone(),
                                move |language, _| {
                                    language_row(p, language, choices, move |name| {
                                        editor.update(|value| {
                                            if let Some(value) = value {
                                                value.value.language = name;
                                            }
                                        });
                                        picker.set(false);
                                    })
                                },
                            ))
                    },
                )),
        )
}

fn level_name(level: &str) -> String {
    if level == "native" {
        return "Native".into();
    }
    LEVELS
        .iter()
        .find(|(id, _, _)| *id == level)
        .map_or_else(|| level.into(), |(id, title, _)| format!("{id} · {title}"))
}

fn level_choice(
    p: Palette,
    editor: Signal<Option<Editor>>,
    level: Memo<String>,
    busy: Signal<bool>,
    id: &'static str,
    title: &'static str,
) -> View {
    let checked = move || level.with(|level| level == id);
    press()
        .min_w(px(48.0))
        .min_h(px(48.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .border(px(1.0))
        .border_color(move || p.muted())
        .items_center()
        .justify_center()
        .bg(move || {
            if checked() {
                p.raised()
            } else {
                p.background()
            }
        })
        .role(AccessibilityRole::Radio)
        .label(if id == "native" {
            title.into()
        } else {
            format!("{id}, {title}")
        })
        .accessibility_state(move || AccessibilityState {
            checked: Some(if checked() {
                Checked::True
            } else {
                Checked::False
            }),
            ..Default::default()
        })
        .disabled(move || busy.get())
        .on_press(move || {
            editor.update(|editor| {
                if let Some(editor) = editor {
                    editor.value.level = id.into();
                }
            })
        })
        .child(
            text(if id == "native" { title } else { id })
                .text_size(px(14.0))
                .text_color(move || p.ink()),
        )
}

fn language_row(
    p: Palette,
    language: ReadSignal<Language>,
    choices: Signal<Option<Choices>>,
    choose: impl Fn(String) + 'static,
) -> View {
    let selected = move || {
        choices.with(|choices| {
            choices
                .as_ref()
                .and_then(|choices| choices.selected.as_ref())
                .is_some_and(|selected| language.with(|language| language.code == selected.code))
        })
    };
    press()
        .min_h(px(56.0))
        .py(px(14.0))
        .gap(px(16.0))
        .items_center()
        .flex_row()
        .border_b(px(0.5))
        .border_color(move || p.muted())
        .role(AccessibilityRole::Radio)
        .label(move || language.with(|language| format!("Choose {}", language.name)))
        .accessibility_state(move || AccessibilityState {
            checked: Some(if selected() {
                Checked::True
            } else {
                Checked::False
            }),
            ..Default::default()
        })
        .on_press(move || choose(language.with(|language| language.name.clone())))
        .child(
            view()
                .gap(px(4.0))
                .flex_1()
                .child(
                    text(move || language.with(|language| language.name.clone()))
                        .text_size(px(16.0))
                        .text_color(move || p.ink()),
                )
                .child(when(
                    move || language.with(|language| !language.native_name.is_empty()),
                    move || {
                        help(p, move || {
                            language.with(|language| language.native_name.clone())
                        })
                    },
                )),
        )
        .child(when(selected, move || {
            ui::icon(CHECK, 21.0, move || p.ink())
        }))
}

fn help(p: Palette, label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}

fn action(
    p: Palette,
    title: &'static str,
    icon: Option<&'static str>,
    press_action: impl Fn() + 'static,
) -> View {
    press()
        .min_h(px(48.0))
        .rounded(px(10.0))
        .p(px(12.0))
        .items_center()
        .justify_center()
        .flex_row()
        .gap(px(8.0))
        .bg(move || p.raised())
        .label(title)
        .on_press(press_action)
        .children(icon.map(|icon| ui::icon(icon, 18.0, move || p.ink())))
        .child(
            text(title)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
}

fn icon_action(p: Palette, icon: &'static str, label: String, action: impl Fn() + 'static) -> View {
    press()
        .size(px(44.0))
        .items_center()
        .justify_center()
        .label(label)
        .on_press(action)
        .child(ui::icon(icon, 20.0, move || p.ink()))
}
