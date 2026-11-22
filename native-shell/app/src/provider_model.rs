use nsh_core::{KeyboardType, element::input, platform::SWITCH_COMPONENT, prelude::*};
use serde_json::{Value, json};

use crate::provider_view::{action, field, field_control, help, section, string};
use crate::ui::{self, Palette, press};

pub(super) const EFFORTS: [&str; 8] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Clone, PartialEq)]
pub struct ModelDraft {
    pub model: Value,
    pub existing: bool,
    limits: [String; 4],
    headers: String,
    body: String,
}

const LIMITS: [(&str, &str, &str, KeyboardType); 4] = [
    (
        "contextWindow",
        "Context window (tokens)",
        "Provider default",
        KeyboardType::Number,
    ),
    (
        "maxOutputTokens",
        "Maximum output tokens",
        "Provider default",
        KeyboardType::Number,
    ),
    (
        "temperature",
        "Temperature",
        "Provider default (0–2)",
        KeyboardType::Decimal,
    ),
    (
        "topP",
        "Top P",
        "Provider default (0–1)",
        KeyboardType::Decimal,
    ),
];

impl ModelDraft {
    pub fn new(model: Value, options: Value, existing: bool) -> Self {
        let option = |key: &str| {
            options[key]
                .as_object()
                .and_then(|value| serde_json::to_string_pretty(value).ok())
                .unwrap_or_else(|| "{}".into())
        };
        Self {
            limits: LIMITS.map(|(key, _, _, _)| {
                model[key]
                    .as_number()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            }),
            model,
            existing,
            headers: option("headers"),
            body: option("body"),
        }
    }

    pub fn request(mut self, provider: String) -> Result<Value, &'static str> {
        let id = self.model["id"].as_str().unwrap_or_default().trim();
        if id.is_empty() {
            return Err("Enter a model ID.");
        }
        self.model["id"] = json!(id);
        for ((key, _, _, kind), value) in LIMITS.into_iter().zip(self.limits) {
            let value = value.trim();
            self.model[key] = if value.is_empty() {
                Value::Null
            } else if kind == KeyboardType::Number {
                json!(
                    value
                        .parse::<u64>()
                        .map_err(|_| "Enter valid numeric model limits.")?
                )
            } else {
                json!(
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite())
                        .ok_or("Enter valid numeric model limits.")?
                )
            };
        }
        let parse = |value: &str| {
            serde_json::from_str::<serde_json::Map<String, Value>>(value)
                .map_err(|_| "Custom headers and request parameters must be JSON objects.")
        };
        Ok(
            json!({"op":"ai_save_model","id":provider,"model":self.model,
            "options":{"headers":parse(&self.headers)?,"body":parse(&self.body)?}}),
        )
    }
}

pub fn editor(
    p: Palette,
    draft: Signal<Option<ModelDraft>>,
    busy: Signal<bool>,
    save: impl Fn() + 'static,
    remove: impl Fn() + Clone + 'static,
) -> View {
    let existing = move || draft.with(|draft| draft.as_ref().is_some_and(|draft| draft.existing));
    let is_hidden = move || {
        draft.with(|draft| {
            draft
                .as_ref()
                .is_some_and(|draft| draft.model["hidden"] == true)
        })
    };
    let has_images = move || {
        draft.with(|draft| {
            draft.as_ref().is_some_and(|draft| {
                draft.model["inputModalities"]
                    .as_array()
                    .is_none_or(|inputs| {
                        inputs.is_empty() || inputs.iter().any(|value| value == "image")
                    })
            })
        })
    };
    let number_field = move |index: usize| {
        let (_, label, placeholder, keyboard) = LIMITS[index];
        field_control(
            p,
            label,
            input()
                .label(label)
                .placeholder(placeholder)
                .keyboard_type(keyboard)
                .value(move || {
                    draft.with(|draft| {
                        draft
                            .as_ref()
                            .map(|draft| draft.limits[index].clone())
                            .unwrap_or_default()
                    })
                })
                .on_change(move |value: &str| {
                    edit(draft, |draft| draft.limits[index] = value.into());
                }),
            false,
        )
    };
    view().gap(px(14.0))
        .child(section(p, "Model settings"))
        .child(field_control(p, "Model ID", input().label("Model ID").readonly(existing)
            .value(move || model_string(draft, "id"))
            .on_change(move |value: &str| edit(draft, |draft| draft.model["id"] = json!(value))), false))
        .child(field(p, "Display name", "", false, false,
            move || model_string(draft, "name"),
            move |value| edit(draft, |draft| draft.model["name"] = json!(value))))
        .child(toggle(p, "Hide from model picker", is_hidden, busy, move |checked| {
            edit(draft, |draft| draft.model["hidden"] = json!(checked));
        }))
        .child(number_field(0))
        .child(number_field(1))
        .child(section(p, "Reasoning effort"))
        .child(view().flex_row().flex_wrap().gap(px(8.0)).child(each(
            move || {
                let modes = draft.with(|draft| draft.as_ref().and_then(|draft| draft.model["reasoningEfforts"].as_array().cloned()));
                let choices = modes.filter(|modes| !modes.is_empty())
                    .map(|modes| modes.into_iter().filter_map(|mode| mode.as_str().map(str::to_owned)).collect())
                    .unwrap_or_else(|| EFFORTS.map(str::to_owned).to_vec());
                std::iter::once(String::new()).chain(choices).collect::<Vec<_>>()
            },
            Clone::clone,
            move |choice, _| {
                let value = choice.get_untracked();
                let label = if value.is_empty() { "Provider default".to_owned() } else { value.clone() };
                let selected = value.clone();
                press().min_h(px(40.0)).px(px(12.0)).rounded(px(9.0)).justify_center()
                    .label(label.clone()).disabled(busy)
                    .bg(move || if model_string(draft, "reasoning") == selected { p.raised() } else { p.background() })
                    .on_press(move || edit(draft, |draft| draft.model["reasoning"] = json!(value)))
                    .child(text(label).text_size(px(12.0)).text_color(move || p.ink()))
            },
        )))
        .child(number_field(2))
        .child(number_field(3))
        .child(toggle(p, "Image input", has_images, busy, move |checked| {
            edit(draft, |draft| {
                let mut inputs = draft.model["inputModalities"].as_array().cloned().unwrap_or_default();
                inputs.retain(|value| value != "image" && value != "text");
                inputs.insert(0, json!("text"));
                if checked {
                    inputs.push(json!("image"));
                }
                draft.model["inputModalities"] = json!(inputs);
            });
        }))
        .child(help(p, || "Enable only if this model can understand images. Undeclared capabilities use the provider's support.".into()))
        .child(help(p, move || {
            draft.with(|draft| draft.as_ref().map_or_else(String::new, |draft| {
                [("Input", "inputModalities"), ("Output", "outputModalities")].into_iter()
                    .filter_map(|(label, key)| {
                        let values = draft.model[key].as_array()?.iter().filter_map(Value::as_str).collect::<Vec<_>>();
                        (!values.is_empty()).then(|| format!("{label}: {}", values.join(", ")))
                    })
                    .collect::<Vec<_>>().join(" · ")
            }))
        }))
        .child(field(p, "Custom headers (JSON)", "", false, true,
            move || draft.with(|draft| draft.as_ref().map(|draft| draft.headers.clone()).unwrap_or_default()),
            move |value| edit(draft, |draft| draft.headers = value.into())))
        .child(field(p, "Extra request parameters (JSON)", "", false, true,
            move || draft.with(|draft| draft.as_ref().map(|draft| draft.body.clone()).unwrap_or_default()),
            move |value| edit(draft, |draft| draft.body = value.into())))
        .child(help(p, || "Headers and extra parameters use secure storage and are omitted from exports.".into()))
        .child(action(p, "Save model", save).disabled(move || busy.get() || model_string(draft, "id").trim().is_empty()))
        .child(when(existing, move || action(p, "Remove saved model", remove.clone()).disabled(busy)))
}

fn edit(draft: Signal<Option<ModelDraft>>, update: impl FnOnce(&mut ModelDraft)) {
    draft.update(|draft| {
        if let Some(draft) = draft {
            update(draft);
        }
    });
}

fn model_string(draft: Signal<Option<ModelDraft>>, key: &str) -> String {
    draft.with(|draft| {
        draft
            .as_ref()
            .map(|draft| string(&draft.model, key))
            .unwrap_or_default()
    })
}

pub fn toggle(
    p: Palette,
    label: &'static str,
    checked: impl Fn() -> bool + 'static,
    busy: Signal<bool>,
    change: impl Fn(bool) + 'static,
) -> View {
    view()
        .h(px(48.0))
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(text(label).text_size(px(14.0)).text_color(move || p.ink()))
        .child(
            native(SWITCH_COMPONENT)
                .prop("accessibilityLabel", json!(label))
                .prop("checked", move || json!(checked()))
                .prop("disabled", move || json!(busy.get()))
                .on_native_event(move |event: &NativeEvent| {
                    if &*event.name == "change"
                        && let Some(checked) = event.data.as_bool()
                    {
                        change(checked);
                    }
                }),
        )
}

pub fn order(
    p: Palette,
    up: bool,
    down: bool,
    busy: Signal<bool>,
    move_item: impl Fn(isize) + Clone + 'static,
) -> View {
    let earlier = move_item.clone();
    view()
        .flex_row()
        .items_center()
        .children(up.then(|| {
            ui::icon_button(p, "m6 15 6-6 6 6", "Move up", move || earlier(-1)).disabled(busy)
        }))
        .children(down.then(|| {
            ui::icon_button(p, ui::DOWN, "Move down", move || move_item(1)).disabled(busy)
        }))
}
