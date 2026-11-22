use std::rc::Rc;

use nsh_core::{element::input, prelude::*};
use serde_json::Value;

use crate::chat::{Chat, ChatTranscript};
use crate::chat_markdown::chat_markdown;
use crate::provider_view::ProviderView;
use crate::ui::{self, Palette, press};

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Thread,
    History,
    Models,
    Providers,
    Find,
}

#[derive(Clone, PartialEq)]
struct Model {
    provider: String,
    id: String,
    label: String,
    provider_label: String,
    connected: bool,
    group: bool,
}

#[derive(Clone)]
pub struct ChatView {
    chat: Chat,
    layer: Signal<Layer>,
    draft: Signal<String>,
    query: Signal<String>,
    find_query: Signal<String>,
    palette: Palette,
    providers: ProviderView,
}

impl ChatView {
    pub fn new(chat: Chat, palette: Palette) -> Self {
        Self {
            providers: ProviderView::new(chat.clone(), palette),
            chat,
            palette,
            layer: signal(Layer::Thread),
            draft: signal(String::new()),
            query: signal(String::new()),
            find_query: signal(String::new()),
        }
    }

    pub fn back(&self) -> bool {
        if !self.chat.visible() {
            return false;
        }
        match self.layer.get_untracked() {
            Layer::Thread => self.chat.close(),
            Layer::Providers => {
                if !self.providers.back() {
                    self.layer.set(Layer::Models);
                }
            }
            Layer::Find => {
                self.find_query.set(String::new());
                self.chat.find_messages(String::new(), None);
                self.layer.set(Layer::Thread);
            }
            Layer::History | Layer::Models => self.layer.set(Layer::Thread),
        }
        true
    }

    pub fn view(
        &self,
        open_video: impl Fn(String) + 'static,
        open_transcript: impl Fn(ChatTranscript) + 'static,
    ) -> If {
        let panel = self.clone();
        let visible = self.chat.clone();
        let open_video: Rc<dyn Fn(String)> = Rc::new(open_video);
        let open_transcript: Rc<dyn Fn(ChatTranscript)> = Rc::new(open_transcript);
        when(
            move || visible.visible(),
            move || {
                let theme = panel.palette;
                let layer = panel.layer;
                let thread = panel.clone();
                let models = panel.clone();
                let providers = panel.clone();
                let history = panel.clone();
                let overlay = panel.clone();
                let open_video = open_video.clone();
                let open_transcript = open_transcript.clone();
                view()
                    .absolute()
                    .inset(px(0.0))
                    .flex_col()
                    .bg(move || theme.background())
                    .child(when(
                        move || !matches!(layer.get(), Layer::Models | Layer::Providers),
                        move || thread.thread(open_video.clone(), open_transcript.clone()),
                    ))
                    .child(when(
                        move || layer.get() == Layer::Models,
                        move || models.models(),
                    ))
                    .child(when(
                        move || layer.get() == Layer::Providers,
                        move || {
                            let close = providers.clone();
                            view()
                                .flex_1()
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .px(px(6.0))
                                        .py(px(5.0))
                                        .child(ui::icon_button(
                                            theme,
                                            ui::BACK,
                                            "Back",
                                            move || {
                                                close.back();
                                            },
                                        ))
                                        .child(
                                            text("Providers")
                                                .text_size(px(19.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(move || theme.ink()),
                                        ),
                                )
                                .child(providers.providers.view())
                        },
                    ))
                    .child(when(
                        move || layer.get() == Layer::History,
                        move || history.history(),
                    ))
                    .child(when(
                        move || layer.get() == Layer::Find,
                        move || overlay.find_bar(),
                    ))
            },
        )
    }

    fn thread(
        &self,
        open_video: Rc<dyn Fn(String)>,
        open_transcript: Rc<dyn Fn(ChatTranscript)>,
    ) -> View {
        let panel = self.clone();
        let chat = self.chat.clone();
        let p = self.palette;
        let layer = self.layer;
        let list = computed(move || {
            chat.current()
                .map(|thread| thread.messages.clone())
                .unwrap_or_default()
        });
        let empty = self.clone();
        let context = self.chat.clone();
        let visible_video = computed(move || {
            context
                .current()
                .and_then(|thread| thread.video_id.clone())
                .is_some()
        });
        let video_chat = self.chat.clone();
        let find = self.chat.clone();
        let composer = self.composer(open_transcript);

        view()
            .flex_1()
            .flex_col()
            .child(self.heading())
            .child(when(
                move || visible_video.get(),
                move || {
                    let chat = video_chat.clone();
                    let callback = open_video.clone();
                    press()
                        .h(px(44.0))
                        .self_start()
                        .mx(px(20.0))
                        .mb(px(6.0))
                        .px(px(14.0))
                        .rounded(px(22.0))
                        .bg(move || p.raised())
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .label("Open video")
                        .on_press(move || {
                            if let Some(video) =
                                chat.current().and_then(|thread| thread.video_id.clone())
                            {
                                chat.close();
                                callback(video);
                            }
                        })
                        .child(ui::icon(ui::PLAY, 16.0, move || p.ink()))
                        .child(
                            text("Open video")
                                .text_size(px(13.0))
                                .text_color(move || p.ink()),
                        )
                },
            ))
            .child(
                view()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(when(
                        move || list.with(Vec::is_empty),
                        move || empty.welcome(),
                    ))
                    .child(when(
                        move || list.with(|messages| !messages.is_empty()),
                        move || {
                            let search = find.clone();
                            scroll().h_full().w_full().child(
                                view()
                                    .w_full()
                                    .max_w(px(800.0))
                                    .self_center()
                                    .px(px(20.0))
                                    .py(px(24.0))
                                    .gap(px(28.0))
                                    .child(each(
                                        list,
                                        |message| message.id.clone(),
                                        move |message, _| {
                                            let search = search.clone();
                                            let id = message
                                                .with_untracked(|message| message.id.clone());
                                            let found = computed(move || {
                                                search
                                                    .matches()
                                                    .and_then(|matches| {
                                                        matches
                                                            .current()
                                                            .map(|hit| hit.message_id == id)
                                                    })
                                                    .unwrap_or(false)
                                            });
                                            let row = element_ref();
                                            effect({
                                                move || {
                                                    if found.get() {
                                                        row.reveal(Default::default());
                                                    }
                                                }
                                            });
                                            let user = message
                                                .with_untracked(|message| message.role == "user");
                                            let value = move || {
                                                message.with(|message| {
                                                    if message.content.is_empty()
                                                        && message.status == "streaming"
                                                    {
                                                        "Thinking…".to_owned()
                                                    } else {
                                                        message.content.clone()
                                                    }
                                                })
                                            };
                                            let bubble = if user {
                                                view()
                                                    .w(percentage(0.88))
                                                    .self_end()
                                                    .rounded(px(24.0))
                                                    .bg(move || p.raised())
                                                    .px(px(18.0))
                                                    .py(px(12.0))
                                                    .child(
                                                        text(value)
                                                            .selectable(true)
                                                            .text_size(px(16.0))
                                                            .line_height(px(24.0))
                                                            .text_color(move || p.ink()),
                                                    )
                                            } else {
                                                view().child(chat_markdown(p, value, move || {
                                                    message.with(|message| {
                                                        message.status == "streaming"
                                                    })
                                                }))
                                            };
                                            view()
                                                .element_ref(&row)
                                                .w_full()
                                                .rounded(px(12.0))
                                                .bg(move || {
                                                    if found.get() {
                                                        p.raised()
                                                    } else {
                                                        rgba(0x00000000).into()
                                                    }
                                                })
                                                .child(bubble)
                                                .child(when(
                                                    move || {
                                                        message
                                                            .with(|message| message.error.is_some())
                                                    },
                                                    move || {
                                                        text(move || {
                                                            message.with(|message| {
                                                                message
                                                                    .error
                                                                    .clone()
                                                                    .unwrap_or_default()
                                                            })
                                                        })
                                                        .text_size(px(13.0))
                                                        .text_color(rgb(0xff0000))
                                                    },
                                                ))
                                                .child(view().self_end().child(ui::icon_button(
                                                    p,
                                                    ui::COPY,
                                                    "Copy message",
                                                    move || {
                                                        set_clipboard(message.with(|message| {
                                                            message.content.clone()
                                                        }));
                                                    },
                                                )))
                                        },
                                    )),
                            )
                        },
                    )),
            )
            .child(when(move || panel.chat.error().is_some(), {
                let chat = self.chat.clone();
                move || {
                    let chat = chat.clone();
                    text(move || chat.error().unwrap_or_default())
                        .px(px(20.0))
                        .pb(px(8.0))
                        .text_size(px(13.0))
                        .text_color(rgb(0xff0000))
                }
            }))
            .child(when(
                move || layer.get() != Layer::Find,
                move || composer.clone(),
            ))
    }

    fn heading(&self) -> View {
        let p = self.palette;
        let layer = self.layer;
        let chat = self.chat.clone();
        let name = self.chat.clone();
        let model = self.chat.clone();
        let close = self.clone();
        let draft = self.draft;
        view()
            .flex_row()
            .items_center()
            .px(px(6.0))
            .py(px(5.0))
            .child(ui::icon_button(p, ui::MENU, "Conversations", move || {
                layer.set(Layer::History)
            }))
            .child(
                press()
                    .flex_1()
                    .min_w(px(0.0))
                    .px(px(5.0))
                    .py(px(4.0))
                    .gap(px(2.0))
                    .label("Choose AI model")
                    .on_press(move || layer.set(Layer::Models))
                    .child(
                        text(move || {
                            name.current()
                                .map_or_else(|| "GoVue".into(), |chat| chat.title.clone())
                        })
                        .text_size(px(17.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || p.ink())
                        .truncate(),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                text(move || {
                                    model
                                        .current()
                                        .filter(|c| !c.model.is_empty())
                                        .map(|c| c.model.clone())
                                        .or_else(|| active_model(&model.config()))
                                        .map(|name| model_name(&name))
                                        .unwrap_or_else(|| "Choose a model".into())
                                })
                                .text_size(px(11.0))
                                .text_color(move || p.muted())
                                .truncate(),
                            )
                            .child(ui::icon(ui::DOWN, 12.0, move || p.muted())),
                    ),
            )
            .child(ui::icon_button(
                p,
                ui::SEARCH,
                "Find in conversation",
                move || layer.set(Layer::Find),
            ))
            .child(ui::icon_button(p, ui::EDIT, "New chat", move || {
                draft.set(String::new());
                chat.new_chat();
            }))
            .child(ui::icon_button(p, ui::CLOSE, "Close chat", move || {
                close.back();
            }))
    }

    fn welcome(&self) -> View {
        let p = self.palette;
        let layer = self.layer;
        let draft = self.draft;
        let chat = self.chat.clone();
        let ready = computed(move || active_model(&chat.config()).is_some());
        view()
            .h_full()
            .w_full()
            .justify_center()
            .items_center()
            .gap(px(16.0))
            .px(px(20.0))
            .py(px(28.0))
            .child(
                text("How can I help?")
                    .text_size(px(28.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || p.ink()),
            )
            .child(
                text(move || {
                    if ready.get() {
                        "Ask a question or explore a video."
                    } else {
                        "Connect a model to start a conversation."
                    }
                })
                .text_size(px(14.0))
                .line_height(px(22.0))
                .max_w(px(260.0))
                .text_color(move || p.muted())
                .text_center(),
            )
            .child(when(
                move || ready.get(),
                move || {
                    pill(p, "Explain a phrase", move || {
                        draft.set("Explain this phrase: ".into())
                    })
                },
            ))
            .child(when(
                move || !ready.get(),
                move || pill(p, "Choose a model", move || layer.set(Layer::Models)),
            ))
    }

    fn composer(&self, open_transcript: Rc<dyn Fn(ChatTranscript)>) -> View {
        let p = self.palette;
        let draft = self.draft;
        let busy = self.chat.clone();
        let send = self.chat.clone();
        let context = self.chat.clone();
        let loaded = computed(move || {
            context
                .current()
                .and_then(|thread| thread.transcript.clone())
        });
        let loading = self.chat.clone();
        view()
            .self_center()
            .w_full()
            .max_w(px(800.0))
            .px(px(12.0))
            .pt(px(8.0))
            .pb(px(10.0))
            .gap(px(8.0))
            .child(when(
                move || loaded.with(Option::is_some),
                move || {
                    let open = open_transcript.clone();
                    press()
                        .h(px(56.0))
                        .px(px(14.0))
                        .rounded(px(14.0))
                        .bg(move || p.raised())
                        .flex_row()
                        .items_center()
                        .gap(px(9.0))
                        .label("Open transcript")
                        .on_press(move || {
                            if let Some(transcript) = loaded.get() {
                                open(transcript);
                            }
                        })
                        .child(ui::icon(ui::CAPTIONS, 17.0, move || p.ink()))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(2.0))
                                .child(
                                    text(move || {
                                        loaded.get().map_or_else(String::new, |data| data.title)
                                    })
                                    .truncate()
                                    .text_size(px(13.0))
                                    .text_color(move || p.ink()),
                                )
                                .child(
                                    text(move || {
                                        loaded.get().map_or_else(String::new, |data| {
                                            format!("{} transcript · AI context", data.language)
                                        })
                                    })
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(move || p.muted()),
                                ),
                        )
                        .child(ui::icon(ui::NEXT, 16.0, move || p.muted()))
                },
            ))
            .child(
                view()
                    .rounded(px(28.0))
                    .pb(px(7.0))
                    .bg(move || p.raised())
                    .child(
                        input()
                            .label("Message GoVue")
                            .value(move || draft.get())
                            .placeholder(move || {
                                if loaded.with(Option::is_some) {
                                    "Ask about this video…"
                                } else {
                                    "Ask anything…"
                                }
                            })
                            .placeholder_color(move || p.muted())
                            .multiline(true)
                            .on_change(move |value: &str| {
                                draft.set(value.chars().take(8000).collect())
                            })
                            .h(px(56.0))
                            .pt(px(18.0))
                            .pb(px(10.0))
                            .px(px(18.0))
                            .text_size(px(16.0))
                            .line_height(px(24.0))
                            .text_color(move || p.ink())
                            .readonly(move || loading.loading()),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(10.0))
                            .px(px(7.0))
                            .child(when(
                                {
                                    let busy = busy.clone();
                                    move || busy.busy()
                                },
                                move || {
                                    let busy = busy.clone();
                                    ui::icon_button(p, ui::STOP, "Stop response", move || {
                                        busy.stop()
                                    })
                                },
                            ))
                            .child(
                                press()
                                    .size(px(42.0))
                                    .rounded(px(21.0))
                                    .items_center()
                                    .justify_center()
                                    .bg(move || p.ink())
                                    .opacity(move || {
                                        if draft.with(|value| value.trim().is_empty()) {
                                            0.45
                                        } else {
                                            1.0_f32
                                        }
                                    })
                                    .label("Send message")
                                    .on_press(move || {
                                        if !send.sending() && send.send(draft.get()) {
                                            draft.set(String::new());
                                        }
                                    })
                                    .child(ui::icon(ui::UP, 22.0, move || p.background())),
                            ),
                    ),
            )
    }

    fn models(&self) -> View {
        let p = self.palette;
        let layer = self.layer;
        let chat = self.chat.clone();
        let read = self.chat.clone();
        let query = signal(String::new());
        let models = computed(move || {
            let query = query.get().trim().to_lowercase();
            provider_models(&read.config())
                .into_iter()
                .filter(|model| {
                    format!("{} {} {}", model.label, model.id, model.provider_label)
                        .to_lowercase()
                        .contains(&query)
                })
                .collect::<Vec<_>>()
        });
        view()
            .flex_1()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .px(px(6.0))
                    .py(px(5.0))
                    .child(ui::icon_button(p, ui::BACK, "Back to chat", move || {
                        layer.set(Layer::Thread)
                    }))
                    .child(
                        text("Choose a model")
                            .text_size(px(19.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                    ),
            )
            .child(input().label("Search AI models").placeholder("Search models or connections…")
                .value(move ||query.get()).on_change(move |value: &str|query.set(value.into()))
                .mx(px(20.0)).my(px(8.0)).h(px(43.0)).px(px(12.0)).rounded(px(12.0)).border(px(0.5))
                .border_color(move ||p.muted()).text_size(px(14.0))
                .text_color(move ||p.ink()).placeholder_color(move ||p.muted()))
            .child(scroll().flex_1().child(view().px(px(20.0))
                .child(text("Choose a saved model or group.").text_size(px(12.0)).text_color(move ||p.muted()))
                .child(when(move || models.with(Vec::is_empty), move || {
                    view().py(px(32.0)).gap(px(8.0))
                        .child(text(move || if query.with(String::is_empty) {"Connect your first provider"} else {"No matching visible models"})
                            .text_size(px(15.0)).text_color(move ||p.ink()))
                        .child(text("Add a connection and refresh its models, or save a model ID.")
                            .text_size(px(12.0)).text_color(move ||p.muted()))
                }))
                .child(each(
                models,
                |row| (row.provider.clone(), row.id.clone()),
                move |row, _| {
                    let chat = chat.clone();
                    let model = row.get_untracked();
                    press()
                        .px(px(12.0))
                        .min_h(px(54.0))
                        .justify_center()
                        .gap(px(2.0))
                        .on_press(move || {
                            if model.connected {
                                chat.choose_provider(model.provider.clone(), (!model.group).then(||model.id.clone()));
                                layer.set(Layer::Thread);
                            } else {
                                layer.set(Layer::Providers);
                            }
                        })
                        .child(
                            text(move || row.with(|row| row.label.clone()))
                                .text_size(px(15.0))
                                .text_color(move || p.ink()),
                        )
                        .child(
                            text(move || row.with(|row| row.provider_label.clone()))
                                .text_size(px(11.0))
                                .text_color(move || p.muted()),
                        )
                },
            ))))
            .child(press().h(px(54.0)).px(px(20.0)).justify_center()
                .label("Manage providers and models").on_press(move ||layer.set(Layer::Providers))
                .child(text("Manage providers and models").text_size(px(14.0)).text_color(move ||p.ink())))
    }

    fn history(&self) -> View {
        let p = self.palette;
        let layer = self.layer;
        let draft = self.draft;
        let query = self.query;
        let list = self.chat.clone();
        let select = self.chat.clone();
        let search = self.chat.clone();
        let new = self.chat.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .flex_row()
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000066))
                    .label("Close conversations")
                    .on_press(move || layer.set(Layer::Thread)),
            )
            .child(
                view()
                    .h_full()
                    .w(percentage(0.86))
                    .max_w(px(360.0))
                    .px(px(12.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                text("GoVue")
                                    .text_size(px(20.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(
                                p,
                                ui::CLOSE,
                                "Close conversations",
                                move || layer.set(Layer::Thread),
                            )),
                    )
                    .child(pill(p, "New chat", move || {
                        draft.set(String::new());
                        new.new_chat();
                        layer.set(Layer::Thread);
                    }))
                    .child(
                        input()
                            .label("Search conversations")
                            .placeholder("Search conversations")
                            .h(px(42.0))
                            .value(move || query.get())
                            .text_size(px(14.0))
                            .text_color(move || p.ink())
                            .placeholder_color(move || p.muted())
                            .on_change(move |value: &str| {
                                query.set(value.into());
                                search.find_chats(value.into());
                            }),
                    )
                    .child(
                        text("Conversations")
                            .text_size(px(12.0))
                            .text_color(move || p.muted())
                            .px(px(10.0))
                            .py(px(8.0)),
                    )
                    .child(scroll().flex_1().child(view().child(each(
                        move || list.history(),
                        |chat| chat.id.clone(),
                        move |entry, _| {
                            let chat = select.clone();
                            press()
                                .min_h(px(54.0))
                                .px(px(10.0))
                                .gap(px(3.0))
                                .justify_center()
                                .on_press(move || {
                                    draft.set(String::new());
                                    chat.open_saved(entry.with(|entry| entry.id.clone()));
                                    layer.set(Layer::Thread);
                                })
                                .child(
                                    text(move || entry.with(|entry| entry.title.clone()))
                                        .truncate()
                                        .text_size(px(14.0))
                                        .text_color(move || p.ink()),
                                )
                                .child(
                                    text(move || entry.with(|entry| entry.model.clone()))
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(move || p.muted()),
                                )
                        },
                    )))),
            )
    }

    fn find_bar(&self) -> View {
        let p = self.palette;
        let query = self.find_query;
        let change = self.chat.clone();
        let previous = self.chat.clone();
        let next = self.chat.clone();
        let items = self.chat.clone();
        let close = self.clone();
        view()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .right(px(0.0))
            .flex_row()
            .items_center()
            .bg(move || p.background())
            .child(
                input()
                    .label("Find in conversation")
                    .placeholder("Find in conversation")
                    .value(move || query.get())
                    .h(px(44.0))
                    .flex_1()
                    .text_size(px(15.0))
                    .text_color(move || p.ink())
                    .placeholder_color(move || p.muted())
                    .on_change(move |value: &str| {
                        query.set(value.into());
                        change.find_messages(value.into(), None);
                    }),
            )
            .child(
                text(move || {
                    items.matches().map_or_else(String::new, |matches| {
                        format!(
                            "{} / {}",
                            matches.selected.map_or(0, |index| index + 1),
                            matches.matches.len()
                        )
                    })
                })
                .text_size(px(12.0))
                .text_color(move || p.muted()),
            )
            .child(ui::icon_button(p, ui::BACK, "Previous match", move || {
                previous.find_messages(query.get(), Some("previous"))
            }))
            .child(ui::icon_button(p, ui::NEXT, "Next match", move || {
                next.find_messages(query.get(), Some("next"))
            }))
            .child(ui::icon_button(p, ui::CLOSE, "Close search", move || {
                close.back();
            }))
    }
}

fn pill(p: Palette, label: &'static str, action: impl Fn() + 'static) -> View {
    press()
        .min_h(px(44.0))
        .px(px(18.0))
        .rounded(px(14.0))
        .items_center()
        .justify_center()
        .label(label)
        .bg(move || p.raised())
        .on_press(action)
        .child(text(label).text_size(px(14.0)).text_color(move || p.ink()))
}

fn active_model(config: &Value) -> Option<String> {
    let active = config["activeId"].as_str()?;
    config["providers"]
        .as_array()?
        .iter()
        .find(|provider| provider["id"] == active && provider["connected"] == true)?["model"]
        .as_str()
        .map(str::to_owned)
}

fn model_name(name: &str) -> String {
    name.strip_prefix("gpt-")
        .map_or_else(|| name.to_owned(), |rest| format!("GPT-{rest}"))
}

fn provider_models(config: &Value) -> Vec<Model> {
    let groups = config["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| {
            let name = group["name"].as_str()?;
            Some(Model {
                provider: group["id"].as_str()?.into(),
                id: name.into(),
                label: name.into(),
                provider_label: format!(
                    "{} models · {}",
                    group["members"].as_array().map_or(0, Vec::len),
                    if group["strategy"] == "loadBalance" {
                        "Round robin"
                    } else {
                        "Fallback"
                    }
                ),
                group: true,
                connected: group["connected"] == true,
            })
        });
    let models = config["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|provider| {
            if provider["enabled"] == false {
                return None;
            }
            let id = provider["id"].as_str()?;
            let label = provider["label"].as_str()?;
            let connected = provider["connected"] == true && provider["needsReauth"] != true;
            Some(
                provider["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(move |model| {
                        let model_id = model["id"].as_str()?;
                        (model["hidden"] != true).then(|| Model {
                            provider: id.into(),
                            id: model_id.into(),
                            label: model_name(
                                model["name"]
                                    .as_str()
                                    .filter(|name| !name.is_empty())
                                    .unwrap_or(model_id),
                            ),
                            provider_label: if connected {
                                label.into()
                            } else {
                                format!("{label} · Reconnect")
                            },
                            connected,
                            group: false,
                        })
                    }),
            )
        })
        .flatten();
    groups.chain(models).collect()
}
