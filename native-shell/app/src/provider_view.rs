use nsh_core::{
    KeyboardTaps,
    element::{Input, clipboard_string, input, share, share_available},
    prelude::*,
    share::ShareRequest,
};
use serde_json::{Value, json};

use crate::chat::Chat;
use crate::provider_group::{self, Group, GroupDraft};
use crate::provider_jobs::{Job, ProviderJobs};
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
    transfer: Signal<Option<String>>,
    model_query: Signal<String>,
    remove_model: Signal<bool>,
    remove_group: Signal<bool>,
    remove_provider: Signal<bool>,
    copilot_consent: Signal<bool>,
    instruction: Signal<Option<String>>,
    advanced: Signal<bool>,
    busy: Signal<bool>,
    revision: Signal<u64>,
    error: Signal<Option<String>>,
    jobs: ProviderJobs,
    qr: crate::provider_qr::ProviderQr,
}

impl ProviderView {
    pub fn new(chat: Chat, palette: Palette) -> Self {
        Self {
            jobs: ProviderJobs::new(chat.core()),
            qr: crate::provider_qr::ProviderQr::new(chat.clone(), palette),
            chat,
            palette,
            owner: Owner::current(),
            draft: signal(None),
            key: signal(String::new()),
            model: signal(None),
            group: signal(None),
            transfer: signal(None),
            model_query: signal(String::new()),
            remove_model: signal(false),
            remove_group: signal(false),
            remove_provider: signal(false),
            copilot_consent: signal(false),
            instruction: signal(None),
            advanced: signal(false),
            busy: signal(false),
            revision: signal(0),
            error: signal(None),
        }
    }

    pub fn back(&self) -> bool {
        if self.qr.back() {
            return true;
        }
        if self.jobs.cancel_login() || self.busy.get_untracked() {
            return true;
        }
        if self.copilot_consent.get_untracked() {
            self.copilot_consent.set(false);
            return true;
        }
        if self.remove_provider.get_untracked() {
            self.remove_provider.set(false);
            return true;
        }
        if self.transfer.with_untracked(Option::is_some) {
            self.revision.update(|revision| *revision += 1);
            self.transfer.set(None);
            self.error.set(None);
            return true;
        }
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
            self.jobs.clear();
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
        let transfer = self.transfer;
        let import = self.clone();
        let error = self.error;
        let busy = self.busy;
        let remove = self.remove_model;
        let removal = self.clone();
        let remove_group = self.remove_group;
        let group_removal = self.clone();
        let group_form = self.clone();
        let remove_provider = self.remove_provider;
        let provider_removal = self.clone();
        let consent = self.copilot_consent;
        let login = self.clone();
        view()
            .flex_1()
            .min_h(px(0.0))
            .child(when(
                move || draft.with(Option::is_none) && group.with(Option::is_none) && transfer.with(Option::is_none),
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
            .child(when(move || transfer.with(Option::is_some), move || page(p, import.import_view(), busy, error)))
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
            .child(when(remove_provider, move || {
                let pane = provider_removal.clone();
                let name = draft.with_untracked(|draft| draft.as_ref().map(|draft| string(draft, "label")).unwrap_or_default());
                removal_dialog(p, "Remove provider?", format!("Remove {name}, its saved models and device credentials?"), "Remove",
                    move || remove_provider.set(false),
                    move || {
                        remove_provider.set(false);
                        pane.run(Job::Remove, pane.provider_id(), json!({}));
                    })
            }))
            .child(when(consent, move || {
                let login = login.clone();
                removal_dialog(p, "Unofficial Copilot connection",
                    "This uses the VS Code Copilot client identity. GitHub may restrict accounts for unsupported clients. Continue only if you accept that risk.".into(),
                    "Continue", move || consent.set(false), move || {
                        consent.set(false);
                        login.save_then(|pane, id| pane.run(Job::Login, id, json!({"consent":true})));
                    })
            }))
    }

    pub fn qr_view(&self) -> If {
        let imported = self.clone();
        self.qr.view(move |provider| {
            imported.transfer.set(None);
            imported.edit(provider);
        })
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
        let transfer = self.transfer;
        let export = self.clone();
        let import_error = self.error;
        let scan = self.qr.clone();
        let scan_button = self.qr.clone();
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
            .child(action(p, "Scan provider QR code", move || scan.show_scanner()).disabled(busy))
            .child(each(move || array(&choices.config(), "choices").into_iter()
                .filter(|choice| advanced.get() || choice["advanced"] != true).collect::<Vec<_>>(),
                |choice| string(choice, "kind"),
                move |choice, _| {
                    let pane = edit.clone();
                    entry(p, PLUS, move || choice.with(|choice|string(choice,"name")),
                        move || choice.with(|choice|string(choice,"description")),
                        move || pane.edit(choice.with(|choice|json!({"id":"","kind":choice["kind"],
                            "label":choice["name"],"auth":choice["auth"][0],"baseUrl":"","model":"","models":[],
                            "enabled":true,"protocol":"","azure":false,"appendV1Suffix":false,"customUserAgent":""}))))
                }))
            .child(action(p, "Advanced connections", move || advanced.update(|open|*open = !*open)))
            .child(provider_group::list(p, self.chat.clone(), busy,
                move |group| edit_group.edit_group(group),
                move |id, delta| reorder_group.reorder_section("groups", &id, delta)))
            .child(section(p, "Configuration"))
            .child(help(p, || "Exports contain connections, models and groups. API keys, account tokens, custom headers and extra request parameters stay on this device.".into()))
            .child(view().flex_row().gap(px(8.0))
                .child(action(p, "Import", move || {
                    import_error.set(None);
                    transfer.set(Some(String::new()));
                }).disabled(busy))
                .child(action(p, "Export", move || export.export_configuration()).disabled(busy))
                .child(action(p, "Scan QR", move || scan_button.show_scanner()).disabled(busy)))
    }

    fn edit(&self, provider: Value) {
        self.revision.update(|revision| *revision += 1);
        self.key.set(String::new());
        self.error.set(None);
        self.model.set(None);
        self.model_query.set(String::new());
        self.jobs.clear();
        self.draft.set(Some(provider));
    }

    fn import_view(&self) -> View {
        let p = self.palette;
        let transfer = self.transfer;
        let busy = self.busy;
        let pane = self.clone();
        let close = self.clone();
        view().gap(px(14.0))
            .child(view().flex_row().items_center().gap(px(8.0))
                .child(ui::icon_button(p, ui::BACK, "Back in provider settings", move || { close.back(); }))
                .child(section(p, "Import providers")))
            .child(help(p, || "Import a GoVue provider configuration. It adds separate connections and model groups. Reconnect imported accounts and keys on this device.".into()))
            .child(action(p, "Paste configuration", move || {
                spawn(async move {
                    let value = clipboard_string().await;
                    if transfer.with_untracked(Option::is_some) {
                        transfer.set(Some(value));
                    }
                });
            }).disabled(busy))
            .child(field(p, "Provider configuration JSON", "", false, true,
                move || transfer.get().unwrap_or_default(),
                move |value| transfer.set(Some(value.into()))))
            .child(action(p, "Import configuration", move || {
                let value = transfer.get_untracked().unwrap_or_default();
                match serde_json::from_str::<Value>(&value) {
                    Ok(value) => pane.mutate(json!({"op":"ai_import","data":value}), move |_| transfer.set(None)),
                    Err(_) => pane.error.set(Some("Enter valid provider configuration JSON.".into())),
                }
            }).disabled(move || busy.get() || transfer.with(|value| value.as_ref().is_none_or(|value| value.trim().is_empty()))))
    }

    fn export_configuration(&self) {
        if self.busy.get_untracked() {
            return;
        }
        let busy = self.busy;
        busy.set(true);
        let error = self.error;
        error.set(None);
        let core = self.chat.core();
        self.owner.run(|| {
            spawn(async move {
                let result = if share_available().await {
                    match core.ai::<Value>(json!({"op":"ai_export_file"})).await {
                        Ok(file) => {
                            let path = file["uri"]
                                .as_str()
                                .and_then(|value| url::Url::parse(value).ok())
                                .and_then(|url| url.to_file_path().ok());
                            match path {
                                Some(path) => share(
                                    ShareRequest::new().title("Export AI providers").file(path),
                                )
                                .await
                                .map(|_| ())
                                .map_err(|error| error.message.to_string()),
                                None => Err("Could not prepare the provider configuration.".into()),
                            }
                        }
                        Err(error) => Err(error.to_string()),
                    }
                } else {
                    match core.ai::<Value>(json!({"op":"ai_export"})).await {
                        Ok(data) => {
                            set_clipboard(
                                serde_json::to_string_pretty(&data)
                                    .unwrap_or_else(|_| data.to_string()),
                            );
                            crate::downloads::alert(
                                "Provider and model settings were copied without credentials."
                                    .into(),
                            )
                            .await;
                            Ok(())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                };
                busy.set(false);
                if let Err(message) = result {
                    error.set(Some(message));
                }
            });
        });
    }

    fn connection(&self) -> View {
        let p = self.palette;
        let draft = self.draft;
        let submit = self.clone();
        let models = self.clone();
        let share_qr = self.clone();
        let back = self.clone();
        let oauth_fields = self.clone();
        let key_fields = self.clone();
        let sign_in = self.jobs.active;
        let cancel = self.jobs.clone();
        let notice = self.jobs.notice;
        let reply = self.jobs.reply;
        let busy = self.busy;
        let config = self.chat.clone();
        let choice = computed(move || {
            let kind = draft.with(|draft| {
                draft
                    .as_ref()
                    .map(|draft| string(draft, "kind"))
                    .unwrap_or_default()
            });
            array(&config.config(), "choices")
                .into_iter()
                .find(|choice| choice["kind"] == kind)
                .unwrap_or(Value::Null)
        });
        let saved =
            move || draft.with(|draft| draft.as_ref().is_some_and(|p| !string(p, "id").is_empty()));
        let enabled =
            move || draft.with(|draft| draft.as_ref().is_some_and(|p| p["enabled"] != false));
        let oauth = computed(move || {
            draft.with(|draft| draft.as_ref().is_some_and(|p| p["auth"] == "oauth"))
        });
        let multiple_auth = move || {
            choice.with(|choice| {
                choice["auth"]
                    .as_array()
                    .is_some_and(|choices| choices.len() > 1)
            })
        };
        let auth_changed = computed({
            let chat = self.chat.clone();
            move || {
                draft.with(|draft| {
                    draft.as_ref().is_some_and(|draft| {
                        let id = string(draft, "id");
                        !id.is_empty()
                            && saved_provider(&chat.config(), &id)
                                .is_some_and(|provider| provider["auth"] != draft["auth"])
                    })
                })
            }
        });
        let preview = signal(Value::Null);
        effect({
            let core = self.chat.core();
            move || {
                let provider = draft.get();
                let core = core.clone();
                spawn(async move {
                    timer(std::time::Duration::from_millis(160)).await;
                    if draft.get_untracked() != provider {
                        return;
                    }
                    let fields = provider.as_ref().map(connection_fields);
                    let result = core
                        .ai::<Value>(json!({"op":"ai_connection_preview","provider":fields}))
                        .await;
                    if draft.get_untracked() == provider {
                        preview.set(result.unwrap_or(Value::Null));
                    }
                });
            }
        });
        view().gap(px(14.0))
            .child(view().flex_row().items_center().gap(px(8.0))
                .child(ui::icon_button(p, ui::BACK, "Back in provider settings", move || { back.back(); }))
                .child(text(move || draft.with(|draft| draft.as_ref().map_or_else(String::new, |draft| {
                    if string(draft,"id").is_empty() { "New connection".into() } else { string(draft,"label") }
                }))).text_size(px(19.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink())))
            .child(help(p, move || choice.with(|choice| string(choice,"description"))))
            .child(draft_field(p, draft, "Connection name", "label", ""))
            .child(provider_model::toggle(p, "Enabled", enabled, busy, move |value| change(draft, "enabled", json!(value))))
            .child(when(multiple_auth, move || view().gap(px(8.0))
                .child(section(p, "Authentication"))
                .child(view().flex_row().gap(px(8.0))
                    .child(choice_button(p, "API key", move || !oauth.get(), move || change(draft, "auth", json!("key"))))
                    .child(choice_button(p, "Account sign-in", move || oauth.get(), move || change(draft, "auth", json!("oauth")))))))
            .child(when(auth_changed, move || help(p, || "Saving this authentication change signs out the previous credentials. Your models, groups and chats stay linked to this connection.".into())))
            .child(when(oauth, move || oauth_fields.account_fields(auth_changed)))
            .child(when(move || !oauth.get(), move || key_fields.key_fields(preview, auth_changed)))
            .child(draft_field(p,draft,"User-Agent","customUserAgent","Use provider default"))
            .child(help(p, || "Applies to model discovery and AI replies. A model-specific User-Agent takes precedence for that model.".into()))
            .child(when(move || preview.with(|preview| preview["effectiveBaseUrl"].is_string()), move || {
                view().gap(px(4.0)).child(section(p, "Effective API base URL"))
                    .child(help(p, move || preview.with(|preview| string(preview,"effectiveBaseUrl"))).selectable(true))
            }))
            .child(when(move || preview.with(|preview| preview["error"].is_string()), move || {
                help(p, move || preview.with(|preview| string(preview,"error"))).text_color(rgb(0xd32f2f))
            }))
            .child(when(saved, move || {
                let share_qr = share_qr.clone();
                action(p, "Share provider QR code", move || {
                    share_qr.save_then(|pane, id| {
                        if let Some(provider) = pane.draft.get_untracked() {
                            pane.qr.show_share(id, string(&provider, "label"), array(&provider, "models").len() > 1);
                        }
                    });
                }).disabled(busy)
            }))
            .child(when(move || notice.with(Option::is_some), move || {
                help(p, move || notice.get().unwrap_or_default()).selectable(true)
            }))
            .child(when(move || sign_in.get() == Some(Job::Login), move || {
                let cancel = cancel.clone();
                action(p, "Cancel sign-in", move || { cancel.cancel_login(); })
            }))
            .child(action(p,"Save connection",move ||submit.save_then(|_, _| {})).disabled(busy))
            .child(when(saved, move || models.saved_models()))
            .child(when(move || !reply.with(String::is_empty), move || {
                crate::chat_markdown::chat_markdown(p, move || reply.get(), move || sign_in.get() == Some(Job::Test))
            }))
    }

    fn key_fields(&self, preview: Signal<Value>, auth_changed: Memo<bool>) -> View {
        let p = self.palette;
        let draft = self.draft;
        let key = self.key;
        let busy = self.busy;
        let connected = move || {
            !auth_changed.get()
                && draft.with(|draft| {
                    draft
                        .as_ref()
                        .is_some_and(|draft| draft["connected"] == true)
                })
        };
        let azure =
            move || draft.with(|draft| draft.as_ref().is_some_and(|draft| draft["azure"] == true));
        let suffix = move || {
            draft.with(|draft| {
                draft
                    .as_ref()
                    .is_some_and(|draft| draft["appendV1Suffix"] == true)
            })
        };
        let protocol = move || {
            draft.with(|draft| {
                draft
                    .as_ref()
                    .map(|p| string(p, "protocol"))
                    .unwrap_or_default()
            })
        };
        view().gap(px(14.0))
            .child(when(connected, move || field(p, "Replace API key (optional)", "Leave blank to keep saved key", true, false,
                move || key.get(), move |value| key.set(value.into()))))
            .child(when(move || !connected(), move || field(p, "API key", "Enter your API key", true, false,
                move || key.get(), move |value| key.set(value.into()))))
            .child(draft_field(p, draft, "API base URL", "baseUrl", "Use provider endpoint"))
            .child(help(p, || "HTTPS is required except on localhost. The saved API key will be used with this endpoint.".into()))
            .child(section(p, "API protocol"))
            .child(view().flex_row().flex_wrap().gap(px(8.0)).children(PROTOCOLS.map(|(id, label)| {
                choice_button(p, label, move || protocol() == id, move || {
                    change(draft, "protocol", json!(id));
                    if id == "gemini" {
                        change(draft, "appendV1Suffix", json!(false));
                    }
                })
            })))
            .child(provider_model::toggle(p, "Azure OpenAI deployment", azure, busy, move |value| {
                change(draft, "azure", json!(value));
                if value {
                    change(draft, "appendV1Suffix", json!(false));
                }
            }))
            .child(when(azure, move || help(p, || "Use the Azure resource endpoint with its api-version query. Model IDs are deployment names.".into())))
            .child(when(move || suffix() || preview.with(|value| value["canAppendV1"] == true), move || {
                provider_model::toggle(p, "Append /v1 automatically", suffix, busy, move |value| change(draft, "appendV1Suffix", json!(value)))
            }))
    }

    fn account_fields(&self, auth_changed: Memo<bool>) -> View {
        let p = self.palette;
        let draft = self.draft;
        let busy = self.busy;
        let consent = self.copilot_consent;
        let login = self.clone();
        let disconnect = self.clone();
        let connected = move || {
            !auth_changed.get()
                && draft.with(|draft| {
                    draft
                        .as_ref()
                        .is_some_and(|draft| draft["connected"] == true)
                })
        };
        let plan = move || {
            draft.with(|draft| {
                draft
                    .as_ref()
                    .is_some_and(|draft| draft["kind"] == "chatgpt")
            })
        };
        let sign_in = move || {
            if draft.with_untracked(|draft| {
                draft
                    .as_ref()
                    .is_some_and(|draft| draft["kind"] == "copilot")
            }) {
                consent.set(true);
            } else {
                login.save_then(|pane, id| pane.run(Job::Login, id, json!({})));
            }
        };
        view().gap(px(12.0))
            .child(when(connected, move || help(p, move || draft.with(|draft| draft.as_ref().map(|draft| string(draft, "account")).unwrap_or_default()))))
            .child(help(p, move || {
                if !auth_changed.get() && draft.with(|draft| draft.as_ref().is_some_and(|draft| draft["needsReauth"] == true)) {
                    "Your account needs to sign in again.".into()
                } else if connected() {
                    "Account connected".into()
                } else {
                    "Sign in to connect this account.".into()
                }
            }))
            .child(dynamic(move || {
                let label = if connected() { "Reconnect account".into() } else { draft.with(|draft| draft.as_ref()
                    .map(|draft| format!("Continue with {}", string(draft, "label"))).unwrap_or_default()) };
                let sign_in = sign_in.clone();
                press().min_h(px(44.0)).rounded(px(10.0)).items_center().justify_center()
                    .bg(move || p.raised()).label(label.clone()).disabled(busy).on_press(sign_in)
                    .child(text(label).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
            }))
            .child(when(connected, move || {
                let disconnect = disconnect.clone();
                action(p, "Disconnect account", move || disconnect.run(Job::Disconnect, disconnect.provider_id(), json!({}))).disabled(busy)
            }))
            .child(when(plan, move || view().gap(px(10.0))
                .child(help(p, || "Available models depend on ChatGPT plan sharing eligibility. You can also add a separate ChatGPT / Codex connection.".into()))
                .child(action(p, "Manage ChatGPT usage", move || { spawn(async { open_url("https://chatgpt.com/settings/usage").await; }); }))))
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
        let test = self.clone();
        let remove = self.remove_provider;
        let connected = move || {
            draft.with(|draft| {
                draft
                    .as_ref()
                    .is_some_and(|draft| draft["connected"] == true && draft["enabled"] != false)
            })
        };
        view()
            .gap(px(14.0))
            .child(section(p, "Saved models"))
            .child(
                view()
                    .flex_row()
                    .gap(px(8.0))
                    .child(
                        action(p, "Refresh model list", move || refresh.save_then(|pane, id| pane.run(Job::Models, id, json!({}))))
                            .disabled(move || busy.get() || !connected()),
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
            .child(action(p, "Test selected model", move || {
                test.save_then(|pane, id| pane.run(Job::Test, id, json!({"prompt":"Reply with exactly: Connection successful.","includeTranscript":false})));
            }).disabled(move || busy.get() || !connected() || draft.with(|draft| draft.as_ref().is_none_or(|draft| string(draft, "model").is_empty()))))
            .child(action(p, "Remove connection", move || remove.set(true)).disabled(busy))
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

    fn save_then(&self, complete: impl FnOnce(&Self, String) + 'static) {
        let Some(draft) = self.draft.get_untracked() else {
            return;
        };
        let id = string(&draft, "id");
        let mut request = json!({"op":"ai_save","provider":connection_fields(&draft)});
        let key = self.key.get_untracked();
        if request["provider"]["auth"] != "oauth" && !key.is_empty() {
            request["key"] = json!(key);
        }
        let pane = self.clone();
        self.mutate(request, move |config| {
            if let Some(saved) = saved_provider(&config, &id) {
                let id = string(&saved, "id");
                pane.edit(saved);
                complete(&pane, id);
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

    fn run(&self, job: Job, id: String, extra: Value) {
        if id.is_empty() || self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let pane = self.clone();
        let revision = pane.revision.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                let result = match pane.jobs.run(job, &id, extra).await {
                    Ok(Some(_)) => pane
                        .chat
                        .update_config(json!({"op":"ai_config"}))
                        .await
                        .map(Some),
                    Ok(None) => Ok(None),
                    Err(error) => Err(error),
                };
                pane.busy.set(false);
                if pane.revision.get_untracked() != revision {
                    return;
                }
                match result {
                    Ok(Some(config)) => {
                        if job == Job::Remove {
                            pane.key.set(String::new());
                            pane.draft.set(None);
                        } else if let Some(saved) = saved_provider(&config, &id) {
                            let connected = saved["connected"] == true;
                            pane.draft.set(Some(saved));
                            if job == Job::Disconnect {
                                pane.key.set(String::new());
                            }
                            if job == Job::Login && connected {
                                pane.run(Job::Models, id, json!({}));
                            }
                        }
                    }
                    Err(error) => pane.error.set(Some(error.to_string())),
                    Ok(None) => {}
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

fn connection_fields(draft: &Value) -> Value {
    let keys = [
        "id",
        "label",
        "kind",
        "baseUrl",
        "model",
        "enabled",
        "auth",
        "protocol",
        "azure",
        "appendV1Suffix",
        "customUserAgent",
        "models",
    ];
    Value::Object(
        keys.into_iter()
            .filter_map(|key| draft.get(key).map(|value| (key.into(), value.clone())))
            .collect(),
    )
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

fn choice_button(
    p: Palette,
    label: &'static str,
    selected: impl Fn() -> bool + 'static,
    choose: impl Fn() + 'static,
) -> View {
    press()
        .min_h(px(40.0))
        .px(px(12.0))
        .rounded(px(9.0))
        .justify_center()
        .label(label)
        .bg(move || {
            if selected() {
                p.raised()
            } else {
                p.background()
            }
        })
        .on_press(choose)
        .child(text(label).text_size(px(12.0)).text_color(move || p.ink()))
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
