use nsh_core::{
    KeyboardTaps,
    element::{Input, input},
    prelude::*,
};
use serde_json::{Value, json};

use crate::chat::Chat;
use crate::provider_group::{self, Group, GroupDraft};
use crate::provider_model::{self, ModelDraft};
use crate::ui::{self, Palette, press};

const PLUS: &str = "M12 5v14m-7-7h14";
const PROTOCOLS: [(&str, &str); 5] = [
    ("", "Provider default"),
    ("chat", "Chat Completions"),
    ("responses", "Responses"),
    ("anthropic", "Anthropic Messages"),
    ("gemini", "Gemini"),
];

#[derive(Clone)]
pub struct ProviderView {
    chat: Chat,
    palette: Palette,
    owner: Owner,
    draft: Signal<Option<Value>>,
    key: Signal<String>,
    model: Signal<Option<ModelDraft>>,
    group: Signal<Option<GroupDraft>>,
    model_query: Signal<String>,
    remove_model: Signal<bool>,
    remove_group: Signal<bool>,
    instruction: Signal<Option<String>>,
    advanced: Signal<bool>,
    busy: Signal<bool>,
    revision: Signal<u64>,
    error: Signal<Option<String>>,
}

impl ProviderView {
    pub fn new(chat: Chat, palette: Palette) -> Self {
        Self {
            chat,
            palette,
            owner: Owner::current(),
            draft: signal(None),
            key: signal(String::new()),
            model: signal(None),
            group: signal(None),
            model_query: signal(String::new()),
            remove_model: signal(false),
            remove_group: signal(false),
            instruction: signal(None),
            advanced: signal(false),
            busy: signal(false),
            revision: signal(0),
            error: signal(None),
        }
    }

    pub fn back(&self) -> bool {
        if self.remove_group.get_untracked() {
            self.remove_group.set(false);
            return true;
        }
        if self.group.with_untracked(Option::is_some) {
            self.revision.update(|revision| *revision += 1);
            self.group.set(None);
            self.error.set(None);
            return true;
        }
        if self.remove_model.get_untracked() {
            self.remove_model.set(false);
            return true;
        }
        if self.model.get_untracked().is_some() {
            self.revision.update(|revision| *revision += 1);
            self.model.set(None);
            self.error.set(None);
            return true;
        }
        if self.draft.get_untracked().is_some() {
            self.revision.update(|revision| *revision += 1);
            self.draft.set(None);
            self.key.set(String::new());
            self.error.set(None);
            return true;
        }
        false
    }

    pub fn view(&self) -> View {
        let p = self.palette;
        let listing = self.clone();
        let form = self.clone();
        let model_form = self.clone();
        let draft = self.draft;
        let model = self.model;
        let group = self.group;
        let error = self.error;
        let busy = self.busy;
        let remove = self.remove_model;
        let removal = self.clone();
        let remove_group = self.remove_group;
        let group_removal = self.clone();
        let group_form = self.clone();
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(when(
                move || draft.with(Option::is_none) && group.with(Option::is_none),
                move || page(p, listing.list(), busy, error),
            ))
            .child(when(
                move || draft.with(Option::is_some) && model.with(Option::is_none),
                move || page(p, form.connection(), busy, error),
            ))
            .child(when(
                move || model.with(Option::is_some),
                move || page(p, model_form.model(), busy, error),
            ))
            .child(when(
                move || group.with(Option::is_some),
                move || page(p, group_form.group(), busy, error),
            ))
            .child(when(remove, move || {
                let removal = removal.clone();
                let detail = model.with_untracked(|model| {
                    model
                        .as_ref()
                        .map(|model| string(&model.model, "name"))
                        .unwrap_or_default()
                });
                removal_dialog(
                    p,
                    "Remove model?",
                    detail,
                    "Remove",
                    move || remove.set(false),
                    move || {
                        remove.set(false);
                        removal.delete_model();
                    },
                )
            }))
            .child(when(remove_group, move || {
                let removal = group_removal.clone();
                let name = group.with_untracked(|group| {
                    group
                        .as_ref()
                        .map(|group| group.group.name.clone())
                        .unwrap_or_default()
                });
                removal_dialog(
                    p,
                    "Delete model group?",
                    name,
                    "Delete",
                    move || remove_group.set(false),
                    move || {
                        remove_group.set(false);
                        removal.delete_group();
                    },
                )
            }))
    }

    fn list(&self) -> View {
        let p = self.palette;
        let instructions = self.instruction;
        let original = self.chat.clone();
        let saved = self.clone();
        let connections = self.chat.clone();
        let existing = self.clone();
        let move_provider = self.clone();
        let choices = self.chat.clone();
        let edit = self.clone();
        let advanced = self.advanced;
        let edit_group = self.clone();
        let reorder_group = self.clone();
        let busy = self.busy;
        view().gap(px(14.0))
            .child(section(p, "AGENT.md"))
            .child(help(p, || "Add instructions for every AI chat and video summary, such as your preferred language, learning level, or response style. Saved changes apply to your next message. Clear and save to remove them.".into()))
            .child(field(p, "AGENT.md instructions", "", false, true,
                move || instructions.get().unwrap_or_else(|| string(&original.config(), "agentMd")),
                move |value| instructions.set(Some(value.into()))))
            .child(action(p, "Save instructions", move || {
                if let Some(value) = instructions.get() {
                    saved.mutate(json!({"op":"ai_settings","agentMd":value}), move |_|instructions.set(None));
                }
            }))
            .child(section(p, "Connections"))
            .child(each(move || {
                let providers = array(&connections.config(), "providers");
                let count = providers.len();
                providers.into_iter().enumerate().map(|(index, provider)| (provider, index > 0, index + 1 < count)).collect::<Vec<_>>()
            },
                |(provider, _, _)| string(provider, "id"),
                move |row, _| {
                    let pane = existing.clone();
                    let move_provider = move_provider.clone();
                    view().flex_row().items_center().gap(px(8.0))
                    .child(entry(p, ui::NEXT, move || row.with(|(p, _, _)|string(p,"label")), move || row.with(|(p, _, _)|{
                        if p["enabled"] == false { "Disabled".into() }
                        else if p["needsReauth"] == true { "Sign-in expired".into() }
                        else if p["connected"] == true { format!("{} models",array(p,"models").iter().filter(|model| model["hidden"] != true).count()) }
                        else { "Connect to get started".into() }
                    }), move || pane.edit(row.with_untracked(|(provider, _, _)|provider.clone())))
                        .flex_1().disabled(busy)
                        .label(move || row.with(|(provider, _, _)| format!("Edit provider {}", string(provider,"label")))))
                    .child(dynamic(move || {
                        let (provider, up, down) = row.get();
                        let move_provider = move_provider.clone();
                        let id = string(&provider, "id");
                        provider_model::order(p, up, down, busy, move |delta| move_provider.reorder_section("providers", &id, delta))
                    }))
                }))
            .child(section(p, "Add a connection"))
            .child(each(move || array(&choices.config(), "choices").into_iter()
                .filter(|choice| choice["auth"].as_array().is_some_and(|auth|auth.iter().any(|mode|mode == "key"))
                    && (advanced.get() || choice["advanced"] != true)).collect::<Vec<_>>(),
                |choice| string(choice, "kind"),
                move |choice, _| {
                    let pane = edit.clone();
                    entry(p, PLUS, move || choice.with(|choice|string(choice,"name")),
                        move || choice.with(|choice|string(choice,"description")),
                        move || pane.edit(choice.with(|choice|json!({"id":"","kind":choice["kind"],
                            "label":choice["name"],"auth":"key","baseUrl":"","model":"","models":[],
                            "enabled":true,"protocol":"","azure":false,"appendV1Suffix":false,"customUserAgent":""}))))
                }))
            .child(action(p, "Advanced connections", move || advanced.update(|open|*open = !*open)))
            .child(provider_group::list(p, self.chat.clone(), busy,
                move |group| edit_group.edit_group(group),
                move |id, delta| reorder_group.reorder_section("groups", &id, delta)))
    }

    fn edit(&self, provider: Value) {
        self.revision.update(|revision| *revision += 1);
        self.key.set(String::new());
        self.error.set(None);
        self.model.set(None);
        self.model_query.set(String::new());
        self.draft.set(Some(provider));
    }

    fn connection(&self) -> View {
        let p = self.palette;
        let draft = self.draft;
        let key = self.key;
        let submit = self.clone();
        let models = self.clone();
        let back = self.clone();
        let saved =
            move || draft.with(|draft| draft.as_ref().is_some_and(|p| !string(p, "id").is_empty()));
        let enabled =
            move || draft.with(|draft| draft.as_ref().is_some_and(|p| p["enabled"] != false));
        let protocol = move || {
            draft.with(|draft| {
                draft
                    .as_ref()
                    .map_or_else(String::new, |p| string(p, "protocol"))
            })
        };
        let options = PROTOCOLS.iter().map(|(id, label)| {
            press()
                .min_h(px(40.0))
                .px(px(12.0))
                .rounded(px(9.0))
                .justify_center()
                .label(*label)
                .bg(move || {
                    if protocol() == *id {
                        p.raised()
                    } else {
                        p.background()
                    }
                })
                .on_press(move || change(draft, "protocol", json!(id)))
                .child(text(*label).text_size(px(12.0)).text_color(move || p.ink()))
        });
        view().gap(px(14.0))
            .child(view().flex_row().items_center().gap(px(8.0))
                .child(ui::icon_button(p, ui::BACK, "Back in provider settings", move || { back.back(); }))
                .child(text(move || draft.with(|draft| draft.as_ref().map_or_else(String::new, |draft| {
                    if string(draft,"id").is_empty() { "New connection".into() } else { string(draft,"label") }
                }))).text_size(px(19.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink())))
            .child(draft_field(p, draft, "Connection name", "label", ""))
            .child(action(p, "Enabled", move || change(draft,"enabled",json!(!enabled()))))
            .child(field(p,"API key", "Leave blank to keep saved key", true, false,
                move ||key.get(), move |value|key.set(value.into())))
            .child(draft_field(p,draft,"API base URL","baseUrl","Use provider endpoint"))
            .child(help(p, ||"HTTPS is required except on localhost. The saved API key will be used with this endpoint.".into()))
            .child(section(p,"API protocol"))
            .child(view().flex_row().flex_wrap().gap(px(8.0)).children(options))
            .child(draft_field(p,draft,"User-Agent","customUserAgent","Use provider default"))
            .child(action(p,"Save connection",move ||submit.save()))
            .child(when(saved, move || models.saved_models()))
    }

    fn saved_models(&self) -> View {
        let p = self.palette;
        let draft = self.draft;
        let query = self.model_query;
        let busy = self.busy;
        let add = self.clone();
        let refresh = self.clone();
        let edit = self.clone();
        let reorder = self.clone();
        view()
            .gap(px(14.0))
            .child(section(p, "Saved models"))
            .child(
                view()
                    .flex_row()
                    .gap(px(8.0))
                    .child(
                        action(p, "Refresh model list", move || refresh.refresh()).disabled(busy),
                    )
                    .child(action(p, "Add model ID", move || add.edit_model(None)).disabled(busy)),
            )
            .child(field(
                p,
                "Search saved models",
                "",
                false,
                false,
                move || query.get(),
                move |value| query.set(value.into()),
            ))
            .child(each(
                move || {
                    let query = query.get().to_lowercase();
                    let models = draft.with(|draft| {
                        draft
                            .as_ref()
                            .map_or_else(Vec::new, |draft| array(draft, "models"))
                    });
                    let count = models.len();
                    models
                        .into_iter()
                        .enumerate()
                        .filter(|(_, model)| {
                            format!("{} {}", string(model, "id"), string(model, "name"))
                                .to_lowercase()
                                .contains(&query)
                        })
                        .take(80)
                        .map(|(index, model)| (model, index > 0, index + 1 < count))
                        .collect::<Vec<_>>()
                },
                |(model, _, _)| string(model, "id"),
                move |row, _| {
                    let edit = edit.clone();
                    let reorder = reorder.clone();
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            press()
                                .flex_1()
                                .min_h(px(48.0))
                                .justify_center()
                                .disabled(busy)
                                .label(move || {
                                    row.with(|(model, _, _)| {
                                        format!("Configure {}", string(model, "name"))
                                    })
                                })
                                .on_press(move || {
                                    edit.edit_model(Some(
                                        row.with_untracked(|(model, _, _)| model.clone()),
                                    ))
                                })
                                .child(
                                    text(move || row.with(|(model, _, _)| string(model, "name")))
                                        .text_size(px(14.0))
                                        .text_color(move || p.ink()),
                                )
                                .child(help(p, move || {
                                    row.with(|(model, _, _)| {
                                        let id = string(model, "id");
                                        let selected = draft.with(|draft| {
                                            draft.as_ref().is_some_and(|draft| draft["model"] == id)
                                        });
                                        format!(
                                            "{id}{}{}",
                                            if model["hidden"] == true {
                                                " · Hidden"
                                            } else {
                                                ""
                                            },
                                            if selected { " · Selected" } else { "" }
                                        )
                                    })
                                })),
                        )
                        .child(dynamic(move || {
                            let (model, up, down) = row.get();
                            let reorder = reorder.clone();
                            let id = string(&model, "id");
                            provider_model::order(p, up, down, busy, move |delta| {
                                reorder.reorder_model(&id, delta)
                            })
                        }))
                        .child(ui::icon(ui::NEXT, 17.0, move || p.muted()))
                },
            ))
    }

    fn model(&self) -> View {
        let p = self.palette;
        let save = self.clone();
        let back = self.clone();
        let remove = self.remove_model;
        view()
            .gap(px(10.0))
            .child(ui::icon_button(
                p,
                ui::BACK,
                "Back in provider settings",
                move || {
                    back.back();
                },
            ))
            .child(provider_model::editor(
                p,
                self.model,
                self.busy,
                move || save.save_model(),
                move || remove.set(true),
            ))
    }

    fn edit_group(&self, group: Group) {
        self.revision.update(|revision| *revision += 1);
        self.error.set(None);
        self.group.set(Some(group.into()));
    }

    fn group(&self) -> View {
        let close = self.clone();
        let save = self.clone();
        let remove = self.remove_group;
        provider_group::editor(
            self.palette,
            self.chat.clone(),
            self.group,
            self.busy,
            move || {
                close.back();
            },
            move || save.save_group(),
            move || remove.set(true),
        )
    }

    fn save_group(&self) {
        let Some(draft) = self.group.get_untracked() else {
            return;
        };
        match draft.request() {
            Ok(request) => {
                let group = self.group;
                self.mutate(request, move |_| group.set(None));
            }
            Err(error) => self.error.set(Some(error.into())),
        }
    }

    fn delete_group(&self) {
        if let Some(draft) = self.group.get_untracked() {
            let group = self.group;
            self.mutate(
                json!({"op":"ai_remove_group","id":draft.group.id}),
                move |_| group.set(None),
            );
        }
    }

    fn reorder_section(&self, kind: &'static str, id: &str, delta: isize) {
        let mut ids = array(&self.chat.config(), kind)
            .iter()
            .map(|item| string(item, "id"))
            .collect::<Vec<_>>();
        if move_id(&mut ids, id, delta) {
            self.mutate(json!({"op":"ai_reorder","kind":kind,"ids":ids}), |_| {});
        }
    }

    fn provider_id(&self) -> String {
        self.draft.with_untracked(|draft| {
            draft
                .as_ref()
                .map(|draft| string(draft, "id"))
                .unwrap_or_default()
        })
    }

    fn edit_model(&self, model: Option<Value>) {
        if self.busy.get_untracked() {
            return;
        }
        self.error.set(None);
        let Some(model) = model else {
            self.model
                .set(Some(ModelDraft::new(json!({"id":""}), Value::Null, false)));
            return;
        };
        let id = self.provider_id();
        let pane = self.clone();
        let revision = self.revision.get_untracked();
        self.busy.set(true);
        self.owner.run(|| {
            spawn(async move {
                let result = pane
                    .chat
                    .core()
                    .ai(json!({"op":"ai_model_options","id":id,"model":model["id"]}))
                    .await;
                pane.busy.set(false);
                if pane.revision.get_untracked() == revision {
                    match result {
                        Ok(options) => pane.model.set(Some(ModelDraft::new(model, options, true))),
                        Err(error) => pane.error.set(Some(error.to_string())),
                    }
                }
            });
        });
    }

    fn save(&self) {
        let Some(draft) = self.draft.get_untracked() else {
            return;
        };
        let id = string(&draft, "id");
        let mut request = json!({"op":"ai_save","provider":draft});
        let key = self.key.get_untracked();
        if !key.is_empty() {
            request["key"] = json!(key);
        }
        let pane = self.clone();
        self.mutate(request, move |config| {
            if let Some(saved) = saved_provider(&config, &id) {
                pane.edit(saved);
            }
        });
    }

    fn save_model(&self) {
        let Some(model) = self.model.get_untracked() else {
            return;
        };
        match model.request(self.provider_id()) {
            Ok(request) => self.update_models(request),
            Err(error) => self.error.set(Some(error.into())),
        }
    }

    fn delete_model(&self) {
        if let Some(model) = self.model.get_untracked() {
            self.update_models(
                json!({"op":"ai_remove_model","id":self.provider_id(),"model":model.model["id"]}),
            );
        }
    }

    fn reorder_model(&self, model_id: &str, delta: isize) {
        let mut ids = self.draft.with_untracked(|draft| {
            draft
                .as_ref()
                .map(|draft| {
                    array(draft, "models")
                        .iter()
                        .map(|model| string(model, "id"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
        if move_id(&mut ids, model_id, delta) {
            self.update_models(json!({"op":"ai_reorder","id":self.provider_id(),"ids":ids}));
        }
    }

    fn update_models(&self, request: Value) {
        let id = self.provider_id();
        let update = self.clone();
        self.mutate(request, move |config| {
            if let Some(saved) = saved_provider(&config, &id) {
                change(update.draft, "models", saved["models"].clone());
                change(update.draft, "model", saved["model"].clone());
                update.model.set(None);
            }
        });
    }

    fn refresh(&self) {
        let id = self
            .draft
            .with_untracked(|draft| draft.as_ref().map_or_else(String::new, |p| string(p, "id")));
        if id.is_empty() || self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let pane = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = pane.chat.refresh_models(&id).await;
                pane.busy.set(false);
                match result {
                    Ok(())
                        if pane.draft.with_untracked(|draft| {
                            draft.as_ref().is_some_and(|p| p["id"] == id)
                        }) =>
                    {
                        if let Some(saved) = saved_provider(&pane.chat.config(), &id) {
                            pane.edit(saved);
                        }
                    }
                    Err(error) => pane.error.set(Some(error.to_string())),
                    _ => {}
                }
            })
        });
    }

    fn mutate(&self, request: Value, complete: impl FnOnce(Value) + 'static) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let pane = self.clone();
        let revision = self.revision.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                let result = pane.chat.update_config(request).await;
                pane.busy.set(false);
                if pane.revision.get_untracked() != revision {
                    return;
                }
                match result {
                    Ok(config) => complete(config),
                    Err(error) => pane.error.set(Some(error.to_string())),
                }
            })
        });
    }
}

fn saved_provider(config: &Value, id: &str) -> Option<Value> {
    let providers = config["providers"].as_array()?;
    if id.is_empty() {
        providers.last().cloned()
    } else {
        providers
            .iter()
            .find(|provider| provider["id"] == id)
            .cloned()
    }
}

fn move_id(ids: &mut [String], id: &str, delta: isize) -> bool {
    let Some(index) = ids.iter().position(|other| other == id) else {
        return false;
    };
    let Some(other) = index
        .checked_add_signed(delta)
        .filter(|other| *other < ids.len())
    else {
        return false;
    };
    ids.swap(index, other);
    true
}

pub(super) fn array(value: &Value, key: &str) -> Vec<Value> {
    value[key].as_array().cloned().unwrap_or_default()
}

pub(super) fn string(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn change(draft: Signal<Option<Value>>, key: &'static str, value: Value) {
    draft.update(|draft| {
        if let Some(draft) = draft {
            draft[key] = value;
        }
    });
}

fn draft_field(
    p: Palette,
    draft: Signal<Option<Value>>,
    label: &'static str,
    key: &'static str,
    placeholder: &'static str,
) -> View {
    field(
        p,
        label,
        placeholder,
        false,
        false,
        move || {
            draft.with(|current| {
                current
                    .as_ref()
                    .map_or_else(String::new, |draft| string(draft, key))
            })
        },
        move |value| change(draft, key, json!(value)),
    )
}

pub(super) fn field(
    p: Palette,
    label: &'static str,
    placeholder: &'static str,
    masked: bool,
    multiline: bool,
    value: impl Fn() -> String + 'static,
    change: impl Fn(&str) + 'static,
) -> View {
    field_control(
        p,
        label,
        input()
            .label(label)
            .placeholder(placeholder)
            .value(value)
            .on_change(change)
            .masked(masked)
            .multiline(multiline),
        multiline,
    )
}

pub(super) fn field_control(
    p: Palette,
    label: &'static str,
    input: Input,
    multiline: bool,
) -> View {
    view()
        .gap(px(7.0))
        .child(
            text(label)
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || p.ink()),
        )
        .child(
            input
                .min_h(px(if multiline { 100.0 } else { 44.0 }))
                .bg(move || p.background())
                .rounded(px(10.0))
                .border(px(0.5))
                .border_color(move || p.muted())
                .px(px(12.0))
                .text_size(px(14.0))
                .text_color(move || p.ink())
                .placeholder_color(move || p.muted()),
        )
}

pub(super) fn section(p: Palette, value: &'static str) -> Text {
    text(value)
        .text_size(px(17.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(move || p.ink())
}

pub(super) fn help(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(12.0))
        .line_height(px(19.0))
        .text_color(move || p.muted())
}

pub(super) fn action(p: Palette, label: &'static str, action: impl Fn() + 'static) -> View {
    press()
        .min_h(px(44.0))
        .px(px(14.0))
        .rounded(px(10.0))
        .items_center()
        .justify_center()
        .label(label)
        .on_press(action)
        .bg(move || p.raised())
        .child(
            text(label)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
}

fn page(p: Palette, content: View, busy: Signal<bool>, error: Signal<Option<String>>) -> Scroll {
    scroll()
        .flex_1()
        .keyboard_taps(KeyboardTaps::Handled)
        .child(
            view()
                .w_full()
                .max_w(px(720.0))
                .self_center()
                .p(px(20.0))
                .pb(px(48.0))
                .gap(px(14.0))
                .child(content)
                .child(when(busy, move || {
                    help(p, || "Saving or contacting provider…".into())
                }))
                .child(when(
                    move || error.with(Option::is_some),
                    move || {
                        text(move || error.get().unwrap_or_default())
                            .text_size(px(13.0))
                            .text_color(rgb(0xd32f2f))
                    },
                )),
        )
}

pub(super) fn removal_dialog(
    p: Palette,
    title: &'static str,
    detail: String,
    label: &'static str,
    close: impl Fn() + Clone + 'static,
    remove: impl Fn() + 'static,
) -> View {
    view()
        .absolute()
        .inset(px(0.0))
        .z_index(2)
        .justify_center()
        .child(
            press()
                .absolute()
                .inset(px(0.0))
                .bg(rgba(0x00000080))
                .label("Close confirmation")
                .on_press(close.clone()),
        )
        .child(
            view()
                .w_full()
                .max_w(px(340.0))
                .self_center()
                .gap(px(16.0))
                .p(px(20.0))
                .rounded(px(18.0))
                .bg(move || p.background())
                .child(section(p, title))
                .child(help(p, move || detail.clone()))
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap(px(10.0))
                        .child(action(p, "Keep", close))
                        .child(action(p, label, remove)),
                ),
        )
}

pub(super) fn entry(
    p: Palette,
    path: &'static str,
    title: impl Fn() -> String + 'static,
    detail: impl Fn() -> String + 'static,
    action: impl Fn() + 'static,
) -> View {
    press()
        .min_h(px(48.0))
        .py(px(8.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .on_press(action)
        .child(ui::icon(path, 18.0, move || p.ink()))
        .child(
            view()
                .flex_1()
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || p.ink()),
                )
                .child(help(p, detail)),
        )
}
