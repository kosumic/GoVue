use nsh_core::{element::input, prelude::*};
use serde_json::{Value, json};

use crate::chat::Chat;
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
    model: Signal<Option<String>>,
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
            instruction: signal(None),
            advanced: signal(false),
            busy: signal(false),
            revision: signal(0),
            error: signal(None),
        }
    }

    pub fn back(&self) -> bool {
        if self.model.get_untracked().is_some() {
            self.revision.update(|revision| *revision += 1);
            self.model.set(None);
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
        let error = self.error;
        let busy = self.busy;
        view().flex_1().min_h(px(0.0)).child(
            scroll().flex_1().child(
                view()
                    .w_full()
                    .max_w(px(720.0))
                    .self_center()
                    .p(px(20.0))
                    .pb(px(48.0))
                    .gap(px(14.0))
                    .child(when(
                        move || draft.with(Option::is_none),
                        move || listing.list(),
                    ))
                    .child(when(
                        move || draft.with(Option::is_some) && model.with(Option::is_none),
                        move || form.connection(),
                    ))
                    .child(when(
                        move || model.with(Option::is_some),
                        move || model_form.model(),
                    ))
                    .child(when(busy, move || {
                        help(p, || "Saving or contacting provider…".into())
                    }))
                    .child(when(
                        move || error.with(Option::is_some),
                        move || {
                            text(move || error.get().unwrap_or_default())
                                .text_size(px(13.0))
                                .text_color(rgb(0xff0000))
                        },
                    )),
            ),
        )
    }

    fn list(&self) -> View {
        let p = self.palette;
        let instructions = self.instruction;
        let original = self.chat.clone();
        let saved = self.clone();
        let connections = self.chat.clone();
        let existing = self.clone();
        let choices = self.chat.clone();
        let edit = self.clone();
        let advanced = self.advanced;
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
            .child(each(move || array(&connections.config(), "providers"),
                |provider| string(provider, "id"),
                move |row, _| {
                    let pane = existing.clone();
                    entry(p, ui::NEXT, move || row.with(|p|string(p,"label")), move || row.with(|p|{
                        if p["enabled"] == false { "Disabled".into() }
                        else if p["needsReauth"] == true { "Sign-in expired".into() }
                        else if p["connected"] == true { format!("{} models",array(p,"models").len()) }
                        else { "Connect to get started".into() }
                    }), move || pane.edit(row.get()))
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
    }

    fn edit(&self, provider: Value) {
        self.revision.update(|revision| *revision += 1);
        self.key.set(String::new());
        self.error.set(None);
        self.model.set(None);
        self.draft.set(Some(provider));
    }

    fn connection(&self) -> View {
        let p = self.palette;
        let draft = self.draft;
        let key = self.key;
        let submit = self.clone();
        let reload = self.clone();
        let model = self.model;
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
            .child(when(saved, move || {
                let reload = reload.clone();
                view().gap(px(14.0))
                    .child(section(p,"Saved models"))
                    .child(view().flex_row().gap(px(8.0))
                        .child(action(p,"Refresh model list",move ||reload.refresh()))
                        .child(action(p,"Add model ID",move ||model.set(Some(String::new())))))
                    .child(each(move || draft.with(|draft|draft.as_ref().map_or_else(Vec::new, |p|array(p,"models"))),
                        |model|string(model,"id"),
                        move |row,_| text(move ||row.with(|row|string(row,"name"))).py(px(5.0))
                            .text_size(px(14.0)).text_color(move ||p.ink())))
            }))
    }

    fn model(&self) -> View {
        let p = self.palette;
        let model = self.model;
        let save = self.clone();
        view()
            .gap(px(14.0))
            .child(field(
                p,
                "Model ID",
                "",
                false,
                false,
                move || model.get().unwrap_or_default(),
                move |id| model.set(Some(id.into())),
            ))
            .child(action(p, "Save model", move || save.save_model()))
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
        let Some(model) = self
            .model
            .get_untracked()
            .filter(|model| !model.trim().is_empty())
        else {
            return;
        };
        let id = self
            .draft
            .with_untracked(|draft| draft.as_ref().map_or_else(String::new, |p| string(p, "id")));
        let update = self.clone();
        self.mutate(
            json!({"op":"ai_save_model","id":id,"model":{"id":model.trim(),"name":model.trim()}}),
            move |config| {
                if let Some(saved) = saved_provider(&config, &id) {
                    update.edit(saved);
                }
            },
        );
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

fn array(value: &Value, key: &str) -> Vec<Value> {
    value[key].as_array().cloned().unwrap_or_default()
}

fn string(value: &Value, key: &str) -> String {
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

fn field(
    p: Palette,
    label: &'static str,
    placeholder: &'static str,
    masked: bool,
    multiline: bool,
    value: impl Fn() -> String + 'static,
    change: impl Fn(&str) + 'static,
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
            input()
                .label(label)
                .placeholder(placeholder)
                .value(value)
                .on_change(change)
                .masked(masked)
                .multiline(multiline)
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

fn section(p: Palette, value: &'static str) -> Text {
    text(value)
        .text_size(px(17.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(move || p.ink())
}

fn help(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(12.0))
        .line_height(px(19.0))
        .text_color(move || p.muted())
}

fn action(p: Palette, label: &'static str, action: impl Fn() + 'static) -> View {
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

fn entry(
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
