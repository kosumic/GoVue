use nsh_core::{PointerEvents, prelude::*};

use crate::{
    chat::{Chat, ChatBlock, Message},
    chat_markdown::chat_markdown,
    ui::{self, Palette, press},
};

const BRAIN: &str = "M12 4c-4-5-8 0-7 3-4 2-3 6 0 7-1 5 3 7 7 4zm0 0c4-5 8 0 7 3 4 2 3 6 0 7 1 5-3 7-7 4M5 7l3 2m-3 5 3-2m11-5-3 2m3 5-3-2";
const CHECK: &str = "m4 12 5 5L20 6";
const CIRCLE: &str = "M12 3a9 9 0 1 0 .01 0";
const TOOL: &str = "M5 7h14l2 8-1 3-4-1H8l-4 1-1-3zm3 3v4m-2-2h4m5 0h1m2-2h1";
const WARNING: &str = "M12 3 2 20h20zm0 6v4m0 3h.01";
const RETRY: &str = "M20 11a8 8 0 1 0-2.5 6.5M20 4v7h-7";
const BRANCH: &str = "M6 6v12m0-5h5a6 6 0 0 0 6-6M6 2a2 2 0 1 1 0 4 2 2 0 0 1 0-4zm0 16a2 2 0 1 1 0 4 2 2 0 0 1 0-4zm11-15a2 2 0 1 1 0 4 2 2 0 0 1 0-4z";

pub type ToolSelection = Signal<Option<(String, String)>>;

pub fn reply(
    p: Palette,
    message: ReadSignal<Message>,
    chat: Chat,
    selected: ToolSelection,
) -> View {
    let busy =
        computed(move || chat.busy() && message.with(|message| message.status == "streaming"));
    let blocks = computed(move || message.with(|message| message.blocks.clone()));
    view()
        .child(each(
            blocks,
            |block| block.id.clone(),
            move |block, _| {
                let kind = block.with_untracked(|block| block.kind.clone());
                let active = computed(move || {
                    busy.get()
                        && block
                            .with(|block| matches!(block.status.as_str(), "running" | "preparing"))
                });
                match kind.as_str() {
                    "text" => view().child(chat_markdown(
                        p,
                        move || block.with(|block| block.content.clone().unwrap_or_default()),
                        move || active.get(),
                    )),
                    "reasoning" => reasoning(
                        p,
                        move || block.with(|block| block.content.clone().unwrap_or_default()),
                        active,
                    ),
                    "plan" => plan(p, block, message, active),
                    "tool" => tool(p, block, message, active, selected),
                    _ => view(),
                }
            },
        ))
        .child(when(
            move || blocks.with(Vec::is_empty),
            move || {
                let thinking = computed(move || {
                    busy.get() && message.with(|message| message.content.is_empty())
                });
                view()
                    .child(when(
                        move || message.with(|message| !message.reasoning.is_empty()),
                        move || {
                            reasoning(
                                p,
                                move || message.with(|message| message.reasoning.clone()),
                                thinking,
                            )
                        },
                    ))
                    .child(when(
                        move || message.with(|message| !message.content.is_empty()),
                        move || {
                            chat_markdown(
                                p,
                                move || message.with(|message| message.content.clone()),
                                move || busy.get(),
                            )
                        },
                    ))
            },
        ))
        .child(when(
            move || {
                busy.get()
                    && message.with(|message| {
                        if message.blocks.is_empty() {
                            message.content.is_empty() && message.reasoning.is_empty()
                        } else {
                            !message.blocks.iter().any(|block| {
                                block.kind != "plan"
                                    && matches!(block.status.as_str(), "running" | "preparing")
                            })
                        }
                    })
            },
            move || muted(p, "Thinking…"),
        ))
        .child(when(
            move || message.with(|message| message.status == "stopped" && message.error.is_none()),
            move || muted(p, "Response stopped"),
        ))
}

fn reasoning(
    p: Palette,
    content: impl Fn() -> String + Clone + 'static,
    active: Memo<bool>,
) -> View {
    let open = signal(false);
    effect(move || {
        if !active.get() {
            open.set(false);
        }
    });
    view()
        .mb(px(8.0))
        .child(
            press()
                .min_h(px(44.0))
                .self_start()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .label(move || {
                    if active.get() {
                        "Thinking"
                    } else {
                        "Reasoning"
                    }
                })
                .accessibility_state(move || AccessibilityState {
                    expanded: Some(open.get()),
                    ..Default::default()
                })
                .on_press(move || open.update(|open| *open = !*open))
                .child(ui::icon(BRAIN, 16.0, move || p.muted()))
                .child(muted(p, move || {
                    if active.get() {
                        "Thinking…"
                    } else {
                        "Reasoning"
                    }
                }))
                .child(ui::icon(ui::DOWN, 14.0, move || p.muted())),
        )
        .child(when(open, move || {
            scroll()
                .max_h(px(240.0))
                .pl(px(12.0))
                .border_l(px(2.0))
                .border_color(move || p.muted())
                .child(chat_markdown(p, content.clone(), move || active.get()))
        }))
}

fn plan(
    p: Palette,
    block: ReadSignal<ChatBlock>,
    message: ReadSignal<Message>,
    active: Memo<bool>,
) -> View {
    let steps = computed(move || block.with(|block| block.steps.clone()));
    view()
        .my(px(10.0))
        .pl(px(12.0))
        .gap(px(9.0))
        .border_l(px(2.0))
        .border_color(move || p.muted())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(move || {
                        block.with(|block| match block.status.as_str() {
                            "completed" => "Plan complete",
                            "failed" => "Plan interrupted",
                            _ if message.with(|message| message.status == "failed") => {
                                "Plan interrupted"
                            }
                            _ if !active.get() => "Plan unfinished",
                            _ => "Plan",
                        })
                    })
                    .role(AccessibilityRole::Header)
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || p.ink()),
                )
                .child(muted(p, move || {
                    steps.with(|steps| {
                        format!(
                            "{}/{}",
                            steps
                                .iter()
                                .filter(|step| step.status == "completed")
                                .count(),
                            steps.len()
                        )
                    })
                })),
        )
        .child(when(
            move || block.with(|block| block.explanation.is_some()),
            move || {
                muted(p, move || {
                    block.with(|block| block.explanation.clone().unwrap_or_default())
                })
            },
        ))
        .child(each(
            steps,
            |step| step.step.clone(),
            move |step, _| {
                let done = computed(move || step.with(|step| step.status == "completed"));
                view()
                    .flex_row()
                    .items_start()
                    .gap(px(9.0))
                    .label(move || {
                        step.with(|step| {
                            let state = match step.status.as_str() {
                                "completed" => "completed",
                                "in_progress" if active.get() => "in progress",
                                "in_progress" => "unfinished",
                                _ => "pending",
                            };
                            format!("{}: {state}", step.step)
                        })
                    })
                    .child(dynamic(move || {
                        ui::icon(if done.get() { CHECK } else { CIRCLE }, 15.0, move || {
                            p.muted()
                        })
                    }))
                    .child(
                        text(move || step.with(|step| step.step.clone()))
                            .flex_1()
                            .text_size(px(13.0))
                            .line_height(px(20.0))
                            .text_color(move || if done.get() { p.muted() } else { p.ink() }),
                    )
            },
        ))
}

fn tool(
    p: Palette,
    block: ReadSignal<ChatBlock>,
    message: ReadSignal<Message>,
    active: Memo<bool>,
    selected: ToolSelection,
) -> View {
    view().child(when(
        move || {
            block.with(|block| {
                block.name.as_deref() != Some("update_plan") || block.status == "failed"
            })
        },
        move || {
            let label = computed(move || block.with(|block| tool_label(block, active.get())));
            press()
                .my(px(6.0))
                .min_h(px(44.0))
                .self_start()
                .rounded(px(22.0))
                .bg(move || p.raised())
                .px(px(12.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .label(move || format!("{}. Show details", label.get()))
                .on_press(move || {
                    selected.set(Some((
                        message.with(|message| message.id.clone()),
                        block.with(|block| block.id.clone()),
                    )))
                })
                .child(dynamic(move || {
                    let icon = block.with(|block| match block.status.as_str() {
                        "failed" => WARNING,
                        "completed" => CHECK,
                        "stopped" => ui::STOP,
                        _ => TOOL,
                    });
                    ui::icon(icon, 16.0, move || p.muted())
                }))
                .child(muted(p, move || label.get()).truncate())
                .child(when(
                    move || block.with(|block| block.duration_ms.is_some_and(|ms| ms > 0)),
                    move || {
                        muted(p, move || {
                            block.with(|block| {
                                format!("{:.1}s", block.duration_ms.unwrap_or(0) as f64 / 1000.0)
                            })
                        })
                    },
                ))
                .child(ui::icon(ui::NEXT, 14.0, move || p.muted()))
        },
    ))
}

fn tool_label(block: &ChatBlock, active: bool) -> &'static str {
    let name = block.name.as_deref().unwrap_or_default();
    match (block.status.as_str(), name, active) {
        ("preparing", "create_interactive_game", true) => "Generating exercise…",
        ("running", "create_interactive_game", true) => "Validating exercise…",
        (_, "list_interactive_games", true) => "Finding saved games…",
        (_, "read_interactive_game", true) => "Reading saved game…",
        (_, _, true) => "Running tool…",
        ("completed", "create_interactive_game", false) => "Exercise ready",
        ("completed", "list_interactive_games", false) => "Saved games found",
        ("completed", "read_interactive_game", false) => "Saved game read",
        ("completed", _, false) => "Tool completed",
        ("failed", "create_interactive_game", false) => "Exercise creation failed",
        ("failed", _, false) => "Tool failed",
        (_, "create_interactive_game", false) => "Exercise creation stopped",
        _ => "Tool stopped",
    }
}

pub fn tool_sheet(p: Palette, chat: Chat, selection: ToolSelection) -> impl IntoElement {
    let selected = computed(move || {
        let (message, block) = selection.get()?;
        chat.current()?
            .messages
            .iter()
            .find(|entry| entry.id == message)?
            .blocks
            .iter()
            .find(|entry| entry.id == block)
            .cloned()
    });
    when(
        move || selected.with(Option::is_some),
        move || {
            view()
                .absolute()
                .inset(px(0.0))
                .pointer_events(PointerEvents::BoxNone)
                .child(
                    press()
                        .absolute()
                        .inset(px(0.0))
                        .bg(rgba(0x00000066))
                        .label("Close tool activity")
                        .on_press(move || selection.set(None)),
                )
                .child(
                    view()
                        .absolute()
                        .left(px(0.0))
                        .right(px(0.0))
                        .bottom(px(0.0))
                        .rounded(px(16.0))
                        .bg(move || p.background())
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .px(px(12.0))
                                .child(
                                    text("Tool activity")
                                        .flex_1()
                                        .text_color(move || p.ink())
                                        .text_size(px(17.0))
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(ui::icon_button(
                                    p,
                                    ui::CLOSE,
                                    "Close tool activity",
                                    move || selection.set(None),
                                )),
                        )
                        .child(scroll().max_h(percentage(0.6)).child(dynamic(move || {
                            let Some(block) = selected.get() else {
                                return view();
                            };
                            let active = matches!(block.status.as_str(), "running" | "preparing");
                            view()
                                .px(px(20.0))
                                .pb(px(20.0))
                                .gap(px(12.0))
                                .child(
                                    text(tool_label(&block, active))
                                        .text_size(px(18.0))
                                        .text_color(move || p.ink())
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .children(block.title.map(|title| {
                                    text(title)
                                        .selectable(true)
                                        .text_size(px(15.0))
                                        .text_color(move || p.ink())
                                }))
                                .child(detail(
                                    p,
                                    "Tool",
                                    block.name.unwrap_or_else(|| "Receiving tool call".into()),
                                ))
                                .child(detail(
                                    p,
                                    "Status",
                                    match block.status.as_str() {
                                        "preparing" => "Receiving tool call",
                                        "running" => "Running",
                                        "completed" => "Completed",
                                        "failed" => "Failed",
                                        _ => "Stopped",
                                    }
                                    .into(),
                                ))
                                .children(block.duration_ms.filter(|ms| *ms > 0).map(|ms| {
                                    detail(p, "Duration", format!("{:.1}s", ms as f64 / 1000.0))
                                }))
                                .children(block.error.map(|error| {
                                    text(error)
                                        .selectable(true)
                                        .role(AccessibilityRole::Alert)
                                        .text_color(rgb(0xd53535))
                                }))
                        }))),
                )
        },
    )
}

pub fn footer(
    p: Palette,
    chat: Chat,
    message: ReadSignal<Message>,
    reading: &crate::chat_read_aloud::ReadAloud,
    allowed: Memo<bool>,
) -> View {
    let open = signal(false);
    let copying = signal(false);
    let retry = chat.clone();
    let branch = chat.clone();
    let read = reading.clone();
    let disabled = computed(move || {
        chat.busy()
            || chat.loading()
            || chat.sending()
            || message.with(|message| message.status == "streaming")
    });
    view()
        .mt(px(4.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .child(
                    press()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(44.0))
                        .items_center()
                        .flex_row()
                        .gap(px(6.0))
                        .label("Response details")
                        .on_press(move || open.update(|open| *open = !*open))
                        .accessibility_state(move || AccessibilityState {
                            expanded: Some(open.get()),
                            ..Default::default()
                        })
                        .child(
                            text(move || message.with(summary))
                                .flex_1()
                                .text_size(px(11.0))
                                .line_height(px(17.0))
                                .text_color(move || p.muted())
                                .number_of_lines(2),
                        )
                        .child(dynamic(move || {
                            ui::icon(
                                if open.get() { ui::DOWN } else { ui::NEXT },
                                13.0,
                                move || p.muted(),
                            )
                        })),
                )
                .child(when(
                    move || message.with(|message| !message.content.is_empty()),
                    move || read.button(p, message, allowed),
                ))
                .child(when(
                    move || message.with(|message| !message.content.is_empty()),
                    move || {
                        let label = move || {
                            if copying.get() {
                                "Response copied"
                            } else {
                                "Copy response"
                            }
                        };
                        icon_action(p, label, ui::COPY, move || {
                            copy(message.with(|message| message.content.clone()), copying)
                        })
                    },
                ))
                .child(
                    icon_action(
                        p,
                        move || {
                            message.with(|message| {
                                if matches!(message.status.as_str(), "failed" | "stopped") {
                                    "Try again"
                                } else {
                                    "Regenerate response"
                                }
                            })
                        },
                        RETRY,
                        move || retry.retry(message.with(|message| message.id.clone())),
                    )
                    .disabled(move || disabled.get()),
                )
                .child(
                    icon_action(p, "Branch in new chat", BRANCH, move || {
                        branch.branch(message.with(|message| message.id.clone()))
                    })
                    .disabled(move || disabled.get()),
                ),
        )
        .child(when(open, move || dynamic(move || usage(p, message.get()))))
}

fn summary(message: &Message) -> String {
    let mut summary = Vec::new();
    let model = model(message);
    if !model.is_empty() {
        summary.push(model.to_owned());
    }
    let input = message.usage["inputTokens"].as_u64();
    let output = message.usage["outputTokens"].as_u64();
    summary.push(
        if let Some(total) = message.usage_details["totalTokens"].as_u64() {
            format!("{} tokens", tokens(Some(total)))
        } else if input.is_some() || output.is_some() {
            format!("{} in / {} out", tokens(input), tokens(output))
        } else if message.status == "streaming" {
            "Responding…".into()
        } else {
            "Usage not reported".into()
        },
    );
    if let Some(ms) = message.usage["streamMs"].as_u64().filter(|ms| *ms > 0) {
        summary.push(format!("Stream duration: {:.1}s", ms as f64 / 1000.0));
    }
    summary.join(" · ")
}

fn usage(p: Palette, message: Message) -> View {
    let fields = [
        ("inputTokens", "Input (including cache)"),
        ("outputTokens", "Output"),
        ("uncachedInputTokens", "Uncached input"),
        ("cacheReadTokens", "Cache read"),
        ("cacheCreationTokens", "Cache write"),
        ("reasoningTokens", "Reasoning (included in output)"),
    ];
    let counts = fields.into_iter().filter_map(|(key, label)| {
        message.usage[key]
            .as_u64()
            .map(|count| detail(p, label, count.to_string()))
    });
    let cache = match message.usage_details["cacheStatus"].as_str() {
        Some("hit") => "Cache hit",
        Some("miss") => "Cache miss",
        _ => "Cache unknown",
    };
    view()
        .gap(px(8.0))
        .border_l(px(1.0))
        .border_color(move || p.muted())
        .pl(px(12.0))
        .py(px(8.0))
        .children((!model(&message).is_empty()).then(|| detail(p, "Model", model(&message).into())))
        .children(
            (!message.provider_label.is_empty())
                .then(|| detail(p, "Provider", message.provider_label.clone())),
        )
        .child(detail(
            p,
            "Status",
            match message.status.as_str() {
                "streaming" => "Responding",
                "completed" => "Completed",
                "failed" => "Failed",
                _ => "Stopped",
            }
            .into(),
        ))
        .children(counts.collect::<Vec<_>>())
        .children(
            message.usage_details["totalTokens"]
                .as_u64()
                .map(|total| detail(p, "Total tokens", total.to_string())),
        )
        .child(detail(p, "Cache", cache.into()))
        .children(
            message.usage["streamMs"]
                .as_u64()
                .filter(|ms| *ms > 0)
                .map(|ms| detail(p, "Stream duration", format!("{:.1}s", ms as f64 / 1000.0))),
        )
        .children(
            message.usage_details["outputTokensPerSecond"]
                .as_f64()
                .map(|rate| detail(p, "Output speed", format!("{rate:.1} tok/s"))),
        )
        .children(
            (message.usage["inputTokens"].is_null() && message.usage["outputTokens"].is_null())
                .then(|| {
                    muted(
                        p,
                        if message.status == "streaming" {
                            "Token counts appear when reported by the provider."
                        } else {
                            "The provider did not report token counts for this response."
                        },
                    )
                }),
        )
}

fn model(message: &Message) -> &str {
    if message.model_name.is_empty() {
        &message.model
    } else {
        &message.model_name
    }
}

fn tokens(value: Option<u64>) -> String {
    match value {
        Some(n) if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        Some(n) if n >= 1_000 => format!("{:.1}K", n as f64 / 1_000.0),
        Some(n) => n.to_string(),
        None => "—".into(),
    }
}

fn detail(p: Palette, name: &'static str, value: String) -> View {
    view()
        .flex_row()
        .gap(px(12.0))
        .child(
            text(name)
                .flex_1()
                .text_color(move || p.muted())
                .text_size(px(11.0))
                .line_height(px(18.0)),
        )
        .child(
            text(value)
                .flex_1()
                .selectable(true)
                .text_color(move || p.ink())
                .text_size(px(11.0))
                .line_height(px(18.0)),
        )
}

fn muted(p: Palette, value: impl IntoReactive<SharedString>) -> Text {
    text(value)
        .text_color(move || p.muted())
        .text_size(px(13.0))
        .line_height(px(20.0))
}

fn icon_action(
    p: Palette,
    label: impl IntoReactive<SharedString>,
    icon: &'static str,
    callback: impl Fn() + 'static,
) -> View {
    press()
        .size(px(44.0))
        .items_center()
        .justify_center()
        .label(label)
        .on_press(callback)
        .child(ui::icon(icon, 16.0, move || p.muted()))
}

fn copy(content: String, copied: Signal<bool>) {
    set_clipboard(content);
    copied.set(true);
    spawn(async move {
        timer(std::time::Duration::from_millis(1300)).await;
        copied.set(false);
    });
}
