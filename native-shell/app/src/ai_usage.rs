use govue_shell_runtime::Core;
use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ui::{self, Palette, press};

const REFRESH: &str = "M20 7v5h-5M4 17v-5h5m-4 1a7 7 0 0 1 12-6l3 4m-16 2 3 4a7 7 0 0 0 12-6";

#[derive(Clone, Deserialize, PartialEq)]
struct Report {
    title: Option<String>,
    context: Option<Value>,
    stats: Value,
    providers: Vec<Provider>,
    chats: Vec<UsageChat>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Provider {
    label: String,
    models: Vec<Model>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Model {
    model_name: String,
    estimated: bool,
    removed: bool,
    stats: Value,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct UsageChat {
    id: String,
    title: String,
    model: String,
    provider_label: String,
}

#[derive(Clone)]
pub struct UsageView {
    core: Core,
    selected: Signal<Option<String>>,
}

impl UsageView {
    pub fn new(core: Core) -> Self {
        Self {
            core,
            selected: signal(None),
        }
    }

    pub fn open(&self, chat_id: Option<String>) {
        self.selected.set(chat_id);
    }

    pub fn back(&self) -> bool {
        let selected = self.selected.with_untracked(Option::is_some);
        if selected {
            self.selected.set(None);
        }
        selected
    }

    pub fn view(&self, p: Palette, close: impl Fn() + Clone + 'static) -> View {
        let selected = self.selected;
        let core = self.core.clone();
        let refresh = signal(0);
        let report = signal(None::<Report>);
        let error = signal(None::<String>);
        effect(move || {
            let chat = selected.get();
            let revision = refresh.get();
            error.set(None);
            report.set(None);
            let core = core.clone();
            spawn(async move {
                let mut request = json!({"op":"ai_usage"});
                if let Some(chat) = &chat {
                    request["chatId"] = json!(chat);
                }
                let result = core.ai(request).await;
                if selected.get_untracked() == chat && refresh.get_untracked() == revision {
                    match result {
                        Ok(data) => report.set(Some(data)),
                        Err(problem) => error.set(Some(problem.to_string())),
                    }
                }
            });
        });
        let backdrop = close.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .justify_end()
            .z_index(4)
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000080))
                    .label("Close token usage")
                    .on_press(backdrop),
            )
            .child(
                view()
                    .w_full()
                    .max_w(px(680.0))
                    .self_center()
                    .max_h(percentage(0.65))
                    .rounded(px(18.0))
                    .bg(move || p.background())
                    .overflow_hidden()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .h(px(48.0))
                            .px(px(6.0))
                            .child(when(
                                move || selected.with(Option::is_some),
                                move || {
                                    ui::icon_button(p, ui::BACK, "All saved chats", move || {
                                        selected.set(None)
                                    })
                                },
                            ))
                            .child(
                                text("Token usage")
                                    .flex_1()
                                    .px(px(10.0))
                                    .text_size(px(19.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(p, ui::CLOSE, "Close token usage", close)),
                    )
                    .child(
                        scroll().flex_shrink(1.0).child(
                            view()
                                .px(px(20.0))
                                .pb(px(12.0))
                                .gap(px(16.0))
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            text(move || {
                                                report
                                                    .with(|report| {
                                                        report
                                                            .as_ref()
                                                            .and_then(|report| report.title.clone())
                                                    })
                                                    .unwrap_or_else(|| {
                                                        if selected.with(Option::is_some) {
                                                            "Conversation".into()
                                                        } else {
                                                            "All saved chats".into()
                                                        }
                                                    })
                                            })
                                            .flex_1()
                                            .text_size(px(16.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(move || p.ink()),
                                        )
                                        .child(ui::icon_button(
                                            p,
                                            REFRESH,
                                            "Refresh token usage",
                                            move || refresh.update(|revision| *revision += 1),
                                        )),
                                )
                                .child(when(
                                    move || error.with(Option::is_some),
                                    move || {
                                        view()
                                            .gap(px(8.0))
                                            .child(
                                                text(move || error.get().unwrap_or_default())
                                                    .text_size(px(14.0))
                                                    .text_color(rgb(0xe63939)),
                                            )
                                            .child(
                                                press()
                                                    .h(px(44.0))
                                                    .label("Try again")
                                                    .on_press(move || {
                                                        refresh.update(|revision| *revision += 1)
                                                    })
                                                    .child(help(p, "Try again".into())),
                                            )
                                    },
                                ))
                                .child(when(
                                    move || {
                                        report.with(Option::is_none) && error.with(Option::is_none)
                                    },
                                    move || {
                                        view().p(px(32.0)).items_center().child(ui::icon(
                                            "M12 3a9 9 0 1 1-9 9",
                                            22.0,
                                            move || p.ink(),
                                        ))
                                    },
                                ))
                                .child(dynamic(move || {
                                    report.get().map(|report| report_view(p, report, selected))
                                })),
                        ),
                    ),
            )
    }
}

fn report_view(p: Palette, report: Report, selected: Signal<Option<String>>) -> View {
    let stats = &report.stats;
    let missing = stats["unreportedReplies"].as_u64().unwrap_or_default();
    view().gap(px(16.0))
        .children(report.context.filter(Value::is_object).map(|context| {
            section(p, "Context at last recorded reply")
                .child(help(p, format!("{} · {}", field(&context, "providerLabel"), field(&context, "model"))))
                .child(stat(p, "Context used", number(&context["used"], "")))
                .child(stat(p, "Context window", number(&context["window"], "")))
                .child(stat(p, "Maximum output", number(&context["maxOutputTokens"], "")))
                .child(stat(p, "Reasoning effort", super::chat_view::effort_name(context["reasoningEffort"].as_str())))
        }))
        .child(section(p, if selected.with_untracked(Option::is_some) { "Conversation totals" } else { "Total usage" }).child(counts(p, stats, false)))
        .children((stats["replyCount"] == 0).then(|| help(p, "No provider-reported token usage has been recorded for these saved chats yet.".into())))
        .child(help(p, "Provider-reported counts from saved replies. Cache tokens are included in input once. Deleted chats are excluded.".into()))
        .children((missing > 0).then(|| help(p, format!("{missing} {} no reported token counts.", if missing == 1 { "reply has" } else { "replies have" }))))
        .child(help(p, "Cache hit rate uses only replies with reported input and cache counts.".into()))
        .children((stats["legacyUsage"] == true).then(|| help(p, "Some replies may not include cache details, completion dates, or output speed.".into())))
        .children(report.providers.into_iter().map(|provider| section(p, &provider.label).children(provider.models.into_iter().map(|model| model_row(p, model)).collect::<Vec<_>>())).collect::<Vec<_>>())
        .children((selected.with_untracked(Option::is_none) && !report.chats.is_empty()).then(|| {
            section(p, "By conversation").children(report.chats.into_iter().map(|chat| {
                let name = if chat.title.is_empty() { "Untitled chat".into() } else { chat.title };
                entry(p, name, format!("{} · {}", chat.provider_label, chat.model), move || selected.set(Some(chat.id.clone())))
            }).collect::<Vec<_>>())
        }))
}

fn counts(p: Palette, usage: &Value, expanded: bool) -> View {
    let labels = [
        ("Input (including cache)", "inputTokens", "", true),
        ("Output", "outputTokens", "", true),
        (
            "Reasoning (included in output)",
            "reasoningTokens",
            "",
            usage["reasoningTokens"].as_u64().unwrap_or_default() > 0,
        ),
        (
            "Cache read",
            "cacheReadTokens",
            "",
            usage["cacheReadTokens"].as_u64().unwrap_or_default() > 0,
        ),
        (
            "Cache write",
            "cacheCreationTokens",
            "",
            usage["cacheCreationTokens"].as_u64().unwrap_or_default() > 0,
        ),
        (
            "Cache hit rate",
            "cacheHitRate",
            "%",
            usage["cacheHitRate"].is_number(),
        ),
        ("Output speed", "outputTokensPerSecond", " tok/s", true),
        ("Recorded replies", "replyCount", "", true),
        (
            "Uncached input",
            "uncachedInputTokens",
            "",
            expanded && usage["legacyUsage"] != true,
        ),
        ("Average tokens per chat", "averagePerChat", "", expanded),
        ("Average tokens per day", "averagePerDay", "", expanded),
        ("Chats", "chatCount", "", expanded),
        ("Active days (UTC)", "activeDays", "", expanded),
    ];
    view().children(
        labels
            .into_iter()
            .filter(|(_, _, _, show)| *show)
            .map(|(label, key, suffix, _)| stat(p, label, number(&usage[key], suffix)))
            .collect::<Vec<_>>(),
    )
}

fn model_row(p: Palette, model: Model) -> View {
    let expanded = signal(false);
    let stats = model.stats;
    let title = model.model_name;
    let label = format!("Usage for {title}");
    let summary = format!(
        "{} in / {} out",
        number(&stats["inputTokens"], ""),
        number(&stats["outputTokens"], "")
    );
    view()
        .child(
            press()
                .label(label)
                .min_h(px(56.0))
                .py(px(12.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .on_press(move || expanded.update(|expanded| *expanded = !*expanded))
                .accessibility_state(move || AccessibilityState {
                    expanded: Some(expanded.get()),
                    ..Default::default()
                })
                .child(
                    view()
                        .flex_1()
                        .gap(px(4.0))
                        .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
                        .children(
                            model
                                .estimated
                                .then(|| help(p, "Legacy model attribution".into())),
                        )
                        .children(
                            model
                                .removed
                                .then(|| help(p, "Not in current configuration".into())),
                        ),
                )
                .child(help(p, summary))
                .child(dynamic(move || {
                    ui::icon(
                        if expanded.get() { ui::DOWN } else { ui::NEXT },
                        20.0,
                        move || p.muted(),
                    )
                })),
        )
        .child(when(expanded, move || {
            counts(p, &stats, true).pl(px(16.0)).pb(px(12.0))
        }))
}

fn section(p: Palette, name: &str) -> View {
    view().gap(px(2.0)).child(
        help(p, name.to_owned())
            .font_weight(FontWeight::SEMIBOLD)
            .my(px(8.0)),
    )
}

fn stat(p: Palette, name: &str, value: String) -> View {
    view()
        .min_h(px(42.0))
        .py(px(8.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .border_b(px(0.5))
        .border_color(move || p.raised())
        .child(
            text(name.to_owned())
                .flex_1()
                .text_size(px(14.0))
                .text_color(move || p.ink()),
        )
        .child(
            text(value)
                .text_size(px(14.0))
                .text_color(move || p.muted()),
        )
}

fn entry(p: Palette, title: String, description: String, click: impl Fn() + 'static) -> View {
    press()
        .label(title.clone())
        .on_press(click)
        .flex_row()
        .items_center()
        .min_h(px(48.0))
        .gap(px(8.0))
        .child(
            view()
                .flex_1()
                .gap(px(3.0))
                .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
                .child(help(p, description)),
        )
        .child(ui::icon(ui::NEXT, 20.0, move || p.muted()))
}

fn help(p: Palette, value: String) -> Text {
    text(value)
        .text_size(px(12.0))
        .line_height(px(18.0))
        .text_color(move || p.muted())
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap_or_default()
}

fn number(value: &Value, suffix: &str) -> String {
    value.as_f64().map_or_else(
        || "—".into(),
        |value| {
            if !suffix.is_empty() {
                return format!("{value:.1}{suffix}");
            }
            if value >= 1_000_000.0 {
                format!("{:.1}M", value / 1_000_000.0)
            } else if value >= 1_000.0 {
                format!("{:.1}K", value / 1_000.0)
            } else {
                format!("{value:.0}")
            }
        },
    )
}
