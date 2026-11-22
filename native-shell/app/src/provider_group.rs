use nsh_core::{KeyboardType, element::input, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chat::Chat;
use crate::provider_model::{EFFORTS, order, toggle};
use crate::provider_view::{action, array, entry, field, field_control, help, section, string};
use crate::ui::{self, Palette, press};

const PLUS: &str = "M12 5v14m-7-7h14";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    provider_id: String,
    model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    strategy: String,
    fallback_on_any_error: bool,
    reasoning: String,
    members: Vec<Member>,
    context_limit: Option<u64>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            strategy: "fallback".into(),
            fallback_on_any_error: false,
            reasoning: String::new(),
            members: Vec::new(),
            context_limit: None,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct GroupDraft {
    pub group: Group,
    context: String,
}

impl From<Group> for GroupDraft {
    fn from(group: Group) -> Self {
        Self {
            context: group
                .context_limit
                .map(|n| n.to_string())
                .unwrap_or_default(),
            group,
        }
    }
}

impl GroupDraft {
    pub fn request(mut self) -> Result<Value, &'static str> {
        self.group.context_limit = if self.context.trim().is_empty() {
            None
        } else {
            Some(
                self.context
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value >= 256)
                    .ok_or("Enter a context limit of at least 256 tokens, or leave it blank.")?,
            )
        };
        Ok(json!({"op":"ai_save_group","group":self.group}))
    }
}

pub fn saved(config: &Value) -> Vec<Group> {
    array(config, "groups")
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect()
}

pub fn list(
    p: Palette,
    chat: Chat,
    busy: Signal<bool>,
    open: impl Fn(Group) + Clone + 'static,
    reorder: impl Fn(String, isize) + Clone + 'static,
) -> View {
    let create = open.clone();
    view()
        .gap(px(14.0))
        .child(section(p, "Model groups"))
        .child(help(p, || {
            "Group models across connections for fallback or round robin routing.".into()
        }))
        .child(each(
            move || {
                let groups = saved(&chat.config());
                let count = groups.len();
                groups
                    .into_iter()
                    .enumerate()
                    .map(|(index, group)| (group, index > 0, index + 1 < count))
                    .collect::<Vec<_>>()
            },
            |(group, _, _)| group.id.clone(),
            move |item, _| {
                let open = open.clone();
                let reorder = reorder.clone();
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        entry(
                            p,
                            ui::NEXT,
                            move || item.with(|(group, _, _)| group.name.clone()),
                            move || {
                                item.with(|(group, _, _)| {
                                    let strategy = if group.strategy == "loadBalance" {
                                        "Round robin"
                                    } else {
                                        "Fallback"
                                    };
                                    format!("{} models · {strategy}", group.members.len())
                                })
                            },
                            move || open(item.with_untracked(|(group, _, _)| group.clone())),
                        )
                        .flex_1()
                        .disabled(busy),
                    )
                    .child(dynamic(move || {
                        let (group, up, down) = item.get();
                        let reorder = reorder.clone();
                        order(p, up, down, busy, move |delta| {
                            reorder(group.id.clone(), delta)
                        })
                    }))
            },
        ))
        .child(action(p, "Create model group", move || create(Group::default())).disabled(busy))
}

#[derive(Clone, PartialEq)]
struct Candidate {
    id: Member,
    provider: String,
    name: String,
}

pub fn editor(
    p: Palette,
    chat: Chat,
    draft: Signal<Option<GroupDraft>>,
    busy: Signal<bool>,
    close: impl Fn() + 'static,
    save: impl Fn() + 'static,
    remove: impl Fn() + Clone + 'static,
) -> View {
    let query = signal(String::new());
    let config = computed(move || array(&chat.config(), "providers"));
    let get = move |key: fn(&Group) -> &str| {
        draft.with(|draft| {
            draft
                .as_ref()
                .map(|draft| key(&draft.group).to_owned())
                .unwrap_or_default()
        })
    };
    let editing = computed(move || !get(|group| &group.id).is_empty());
    let valid = move || {
        draft.with(|draft| {
            draft.as_ref().is_some_and(|draft| {
                !draft.group.name.trim().is_empty() && !draft.group.members.is_empty()
            })
        })
    };
    let members = computed(move || {
        draft.with(|draft| {
            draft
                .as_ref()
                .map(|draft| draft.group.members.clone())
                .unwrap_or_default()
        })
    });
    view().gap(px(14.0))
        .child(view().flex_row().items_center().gap(px(8.0))
            .child(ui::icon_button(p, ui::BACK, "Back in provider settings", close))
            .child(text(move || if editing.get() { "Edit model group" } else { "New model group" })
                .text_size(px(19.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink())))
        .child(field(p, "Group name", "", false, false,
            move || get(|group| &group.name),
            move |name| edit(draft, |draft| draft.group.name = name.into())))
        .child(section(p, "Routing strategy"))
        .child(view().flex_row().gap(px(8.0))
            .child(choice(p, "Ordered fallback", move || get(|group| &group.strategy) == "fallback",
                move || edit(draft, |draft| draft.group.strategy = "fallback".into())))
            .child(choice(p, "Round robin", move || get(|group| &group.strategy) == "loadBalance",
                move || edit(draft, |draft| draft.group.strategy = "loadBalance".into()))))
        .child(help(p, || "Fallback tries the next model after a rate limit or server error. Round robin rotates the first model for each turn. An answer already streaming stays with that model.".into()))
        .child(toggle(p, "Fallback on any error",
            move || draft.with(|draft| draft.as_ref().is_some_and(|draft| draft.group.fallback_on_any_error)),
            busy, move |value| edit(draft, |draft| draft.group.fallback_on_any_error = value)))
        .child(field_control(p, "Group context limit (tokens)", input()
            .label("Group context limit (tokens)").placeholder("Use each model’s context window")
            .keyboard_type(KeyboardType::Number)
            .value(move || draft.with(|draft| draft.as_ref().map(|draft| draft.context.clone()).unwrap_or_default()))
            .on_change(move |value: &str| edit(draft, |draft| draft.context = value.into())), false))
        .child(section(p, "Default reasoning effort"))
        .child(view().flex_row().flex_wrap().gap(px(8.0)).children(
            std::iter::once("").chain(EFFORTS).map(|effort| choice(
                p, if effort.is_empty() { "Use model setting" } else { effort },
                move || get(|group| &group.reasoning) == effort,
                move || edit(draft, |draft| draft.group.reasoning = effort.into()),
            )),
        ))
        .child(text(move || format!("Members · {}", members.with(Vec::len)))
            .text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
        .child(each(
            move || {
                let members = members.get();
                let count = members.len();
                members.into_iter().enumerate().map(|(index, member)| (member, index > 0, index + 1 < count)).collect::<Vec<_>>()
            },
            |(member, _, _)| (member.provider_id.clone(), member.model.clone()),
            move |item, _| view().flex_row().gap(px(8.0)).items_center()
                .child(view().flex_1().min_w(px(0.0))
                    .child(text(move || item.with(|(member, _, _)| member.model.clone()))
                        .truncate().text_size(px(14.0)).text_color(move || p.ink()))
                    .child(help(p, move || {
                        config.with(|providers| item.with(|(member, _, _)| {
                            providers.iter().find(|provider| provider["id"] == member.provider_id)
                                .map_or_else(|| "Removed connection".into(), |provider| string(provider, "label"))
                        }))
                    })))
                .child(dynamic(move || {
                    let (member, up, down) = item.get();
                    order(p, up, down, busy, move |delta| edit(draft, |draft| {
                        let members = &mut draft.group.members;
                        if let Some(index) = members.iter().position(|value| value == &member)
                            && let Some(target) = index.checked_add_signed(delta).filter(|target| *target < members.len()) {
                            members.swap(index, target);
                        }
                    }))
                }))
                .child(action(p, "Remove", move || {
                    let member = item.with_untracked(|(member, _, _)| member.clone());
                    edit(draft, |draft| draft.group.members.retain(|value| value != &member));
                }).disabled(busy)),
        ))
        .child(field(p, "Find a saved model", "", false, false, move || query.get(), move |value| query.set(value.into())))
        .child(each(
            move || {
                let query = query.get().to_lowercase();
                let added = members.get();
                config.with(|providers| providers.iter().flat_map(|provider| {
                    let id = string(provider, "id");
                    let label = string(provider, "label");
                    array(provider, "models").into_iter().filter_map(|model| {
                        let member = Member { provider_id: id.clone(), model: string(&model, "id") };
                        let name = string(&model, "name");
                        (model["hidden"] != true && !added.contains(&member)
                            && format!("{name} {} {label}", member.model).to_lowercase().contains(&query))
                            .then(|| Candidate { id: member, provider: label.clone(), name })
                    }).take(30).collect::<Vec<_>>()
                }).collect::<Vec<_>>())
            },
            |item| (item.id.provider_id.clone(), item.id.model.clone()),
            move |item, _| entry(p, PLUS,
                move || item.with(|item| item.name.clone()),
                move || item.with(|item| item.provider.clone()),
                move || {
                    let member = item.with_untracked(|item| item.id.clone());
                    edit(draft, |draft| {
                        if !draft.group.members.contains(&member) {
                            draft.group.members.push(member);
                        }
                    });
                }).disabled(busy)
                .label(move || item.with(|item| format!("Add {} from {} to group", item.name, item.provider))),
        ))
        .child(action(p, "Save group", save).disabled(move || busy.get() || !valid()))
        .child(when(editing, move || action(p, "Delete group", remove.clone()).disabled(busy)))
}

fn choice(
    p: Palette,
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    run: impl Fn() + 'static,
) -> View {
    press()
        .min_h(px(40.0))
        .px(px(12.0))
        .rounded(px(9.0))
        .justify_center()
        .bg(move || if active() { p.raised() } else { p.background() })
        .label(label)
        .on_press(run)
        .child(text(label).text_size(px(12.0)).text_color(move || p.ink()))
}

fn edit(draft: Signal<Option<GroupDraft>>, update: impl FnOnce(&mut GroupDraft)) {
    draft.update(|draft| {
        if let Some(draft) = draft {
            update(draft);
        }
    });
}
