use std::rc::Rc;

use govue_shell_runtime::WatchPlayback;
use nsh_core::{
    KeyboardDismissMode, KeyboardTaps, PasteEvent,
    element::{ElementRef, input},
    platform::Lifecycle,
    prelude::*,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::attachments_view::{AttachmentsView, PAPERCLIP};
use crate::chat::{Chat, ChatTranscript, Message};
use crate::chat_activity::{self, ToolSelection};
use crate::chat_read_aloud::ReadAloud;
use crate::chat_viewport::ChatViewports;
use crate::chat_voice::ChatVoice;
use crate::provider_view::ProviderView;
use crate::ui::{self, Palette, press};

const BRAIN: &str = "M12 18V5a3 3 0 0 0-5.9-.8 4 4 0 0 0-2.9 5.6A4 4 0 0 0 5 17a3.5 3.5 0 0 0 7 1zm0 0a3.5 3.5 0 0 0 7-1 4 4 0 0 0 1.8-7.2 4 4 0 0 0-2.9-5.6A3 3 0 0 0 12 5M7 8c0 2 1 3 3 3m7-3c0 2-1 3-3 3m-9 6c0-2 1-3 3-3m11 3c0-2-1-3-3-3";

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
    editing: Signal<Option<(String, String)>>,
    reasoning_open: Signal<bool>,
    input: ElementRef,
    query: Signal<String>,
    find_query: Signal<String>,
    palette: Palette,
    providers: ProviderView,
    attachments: AttachmentsView,
    tool_selection: ToolSelection,
    voice: ChatVoice,
    reading: ReadAloud,
    viewports: ChatViewports,
}

impl ChatView {
    pub fn new(chat: Chat, palette: Palette, watch: WatchPlayback) -> Self {
        let attachments = AttachmentsView::new(chat.core(), palette);
        let draft = signal(String::new());
        let layer = signal(Layer::Thread);
        let voice = ChatVoice::new(chat.core(), draft);
        let reading = ReadAloud::new(chat.core(), watch);
        let editing = signal(None);
        let tool_selection = signal(None);
        effect({
            let attachments = attachments.clone();
            let reset = chat.clone();
            move || {
                reset.draft_revision();
                attachments.reset();
                draft.set(String::new());
                editing.set(None);
                tool_selection.set(None);
            }
        });
        let voice_context = computed({
            let chat = chat.clone();
            move || {
                chat.current()
                    .map(|thread| (thread.id.clone(), thread.transcript.clone()))
            }
        });
        effect({
            let voice = voice.clone();
            let reading = reading.clone();
            let chat = chat.clone();
            move || {
                voice_context.get();
                chat.draft_revision();
                reading.clear();
                if !chat.visible() || layer.get() != Layer::Thread {
                    voice.clear();
                } else {
                    voice.cancel();
                }
            }
        });
        effect({
            let voice = voice.clone();
            let reading = reading.clone();
            move || {
                let phase = lifecycle();
                if phase == Lifecycle::Background {
                    voice.cancel();
                }
                if phase != Lifecycle::Active {
                    reading.stop();
                }
            }
        });
        effect({
            let reading = reading.clone();
            let chat = chat.clone();
            move || {
                if let Some(id) = reading.active()
                    && !chat.current().is_some_and(|thread| {
                        thread
                            .messages
                            .iter()
                            .any(|message| message.id == id && message.status != "streaming")
                    })
                {
                    reading.stop();
                }
            }
        });
        Self {
            providers: ProviderView::new(chat.clone(), palette),
            attachments,
            tool_selection,
            chat,
            palette,
            layer,
            voice,
            reading,
            viewports: ChatViewports::default(),
            draft,
            editing,
            reasoning_open: signal(false),
            input: element_ref(),
            query: signal(String::new()),
            find_query: signal(String::new()),
        }
    }

    pub fn camera(&self) -> NativeView {
        self.attachments.camera()
    }

    pub fn speech(&self) -> NativeView {
        self.voice.native_view()
    }

    pub fn read_aloud(&self) -> NativeView {
        self.reading.native()
    }

    fn edit(&self, message: Message) {
        self.reasoning_open.set(false);
        let original = self
            .editing
            .get_untracked()
            .map_or_else(|| self.draft.get_untracked(), |(_, text)| text);
        self.editing.set(Some((message.id, original)));
        self.attachments.edit(message.attachments);
        self.draft.set(message.content);
        self.input.focus();
    }

    fn cancel_edit(&self) {
        if let Some((_, original)) = self.editing.get_untracked() {
            self.attachments.cancel_edit();
            self.draft.set(original);
            self.editing.set(None);
        }
    }

    pub fn back(&self) -> bool {
        if !self.chat.visible() {
            return false;
        }
        if self.tool_selection.with_untracked(Option::is_some) {
            self.tool_selection.set(None);
            return true;
        }
        if self.attachments.back() {
            return true;
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
                    .bottom(|| px((keyboard_height().0 - safe_area().bottom.0).max(0.0)))
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
                    .child(panel.attachments.overlays())
                    .child(chat_activity::tool_sheet(
                        theme,
                        panel.chat.clone(),
                        panel.tool_selection,
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
        let assistant = self.chat.clone();
        let reading = self.reading.clone();
        let allow_reading = computed({
            let voice = self.voice.clone();
            let chat = self.chat.clone();
            move || !voice.active() && !chat.loading()
        });
        let tool_selection = self.tool_selection;
        let viewports = self.viewports.clone();
        let finding = computed(move || layer.get() == Layer::Find);
        let sent = self.chat.sent_revision;
        let files = self.attachments.clone();
        let edit = self.clone();
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
                            let files = files.clone();
                            let assistant = assistant.clone();
                            let reading = reading.clone();
                            let edit = edit.clone();
                            let viewport = viewports.thread(
                                assistant
                                    .current()
                                    .map(|thread| thread.id.clone())
                                    .unwrap_or_default(),
                                finding,
                                sent,
                            );
                            let position = viewport.clone();
                            let scrolled = viewport.clone();
                            let begin = viewport.clone();
                            let end = viewport.clone();
                            let content = viewport.clone();
                            view()
                                .relative()
                                .h_full()
                                .w_full()
                                .child(
                                    scroll()
                                        .element_ref(&viewport.scroll)
                                        .h_full()
                                        .w_full()
                                        .keyboard_taps(KeyboardTaps::Handled)
                                        .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
                                        .on_layout(move |frame: &Bounds<Points>| {
                                            position.measured(frame, false)
                                        })
                                        .on_scroll(move |event: &nsh_core::element::ScrollEvent| {
                                            scrolled.moved(event)
                                        })
                                        .on_scroll_drag_start(move |_: &nsh_core::Point<Points>| {
                                            begin.drag(true)
                                        })
                                        .on_scroll_end(move |_: &nsh_core::Point<Points>| {
                                            end.drag(false)
                                        })
                                        .child(
                                            view()
                                                .w_full()
                                                .max_w(px(800.0))
                                                .self_center()
                                                .px(px(20.0))
                                                .py(px(24.0))
                                                .gap(px(28.0))
                                                .on_layout(move |frame: &Bounds<Points>| {
                                                    content.measured(frame, true)
                                                })
                                                .child(each(
                                                    list,
                                                    |message| message.id.clone(),
                                                    move |message, _| {
                                                        let search = search.clone();
                                                        let id =
                                                            message.with_untracked(|message| {
                                                                message.id.clone()
                                                            });
                                                        let found = computed(move || {
                                                            search
                                                                .matches()
                                                                .and_then(|matches| {
                                                                    matches.current().map(|hit| {
                                                                        hit.message_id == id
                                                                    })
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
                                                        let user =
                                                            message.with_untracked(|message| {
                                                                message.role == "user"
                                                            });
                                                        let value = move || {
                                                            message.with(|message| {
                                                                message.content.clone()
                                                            })
                                                        };
                                                        let bubble =
                                                            if user {
                                                                view()
                                                                    .flex_col()
                                                                    .w(percentage(0.88))
                                                                    .self_end()
                                                                    .gap(px(6.0))
                                                                    .child(files.strip(
                                                                        move || {
                                                                            message.with(
                                                                                |message| {
                                                                                    message
                                                                                        .attachments
                                                                                        .clone()
                                                                                },
                                                                            )
                                                                        },
                                                                        None,
                                                                    ))
                                                                    .child(when(
                                                                        move || {
                                                                            message.with(
                                                                                |message| {
                                                                                    !message
                                                                                        .content
                                                                                        .is_empty()
                                                                                },
                                                                            )
                                                                        },
                                                                        move || {
                                                                            view()
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
                                                                            .text_color(
                                                                                move || p.ink(),
                                                                            ),
                                                                    )
                                                                        },
                                                                    ))
                                                            } else {
                                                                chat_activity::reply(
                                                                    p,
                                                                    message,
                                                                    assistant.clone(),
                                                                    tool_selection,
                                                                )
                                                            };
                                                        let actions = if user {
                                                            let panel = edit.clone();
                                                            let state = edit.clone();
                                                            view()
                                                                .self_end()
                                                                .flex_row()
                                                                .child(ui::icon_button(
                                                                    p,
                                                                    ui::COPY,
                                                                    "Copy prompt",
                                                                    move || set_clipboard(value()),
                                                                ))
                                                                .child(
                                                                    ui::icon_button(
                                                                        p,
                                                                        ui::EDIT,
                                                                        "Edit message",
                                                                        move || {
                                                                            panel.edit(
                                                                                message
                                                                                    .get_untracked(
                                                                                    ),
                                                                            );
                                                                        },
                                                                    )
                                                                    .disabled(move || {
                                                                        state.chat.busy()
                                                                            || state.chat.loading()
                                                                            || state.chat.sending()
                                                                            || state
                                                                                .attachments
                                                                                .busy()
                                                                    }),
                                                                )
                                                        } else {
                                                            chat_activity::footer(
                                                                p,
                                                                assistant.clone(),
                                                                message,
                                                                &reading,
                                                                allow_reading,
                                                            )
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
                                                                    message.with(|message| {
                                                                        message.error.is_some()
                                                                    })
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
                                                            .child(actions)
                                                    },
                                                )),
                                        ),
                                )
                                .child(viewport.jump_button(p))
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
            .child(self.reading.notice(p))
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
        let draft_files = self.attachments.clone();
        let pasting = self.attachments.clone();
        let pasting_chat = self.chat.clone();
        let send_files = self.attachments.clone();
        let pick = self.attachments.clone();
        let pick_chat = self.chat.clone();
        let input_ref = self.input;
        let editing = self.editing;
        let cancel = self.clone();
        let reasoning_open = self.reasoning_open;
        let selection = computed({
            let chat = self.chat.clone();
            move || reasoning_context(&chat)
        });
        effect({
            let chat = self.chat.clone();
            move || {
                selection.get();
                chat.draft_revision();
                reasoning_open.set(false);
            }
        });
        let enabled = computed({
            let files = self.attachments.clone();
            let chat = self.chat.clone();
            let voice = self.voice.clone();
            move || {
                !files.busy()
                    && !chat.sending()
                    && !chat.loading()
                    && !chat.settings_busy()
                    && !voice.active()
                    && (editing.with(Option::is_none) || !chat.busy())
                    && (!files.empty() || draft.with(|value| !value.trim().is_empty()))
            }
        });
        view()
            .self_center()
            .w_full()
            .max_w(px(800.0))
            .px(px(12.0))
            .pt(px(8.0))
            .pb(px(10.0))
            .gap(px(8.0))
            .child(self.pending_messages())
            .child(when(
                move || editing.with(Option::is_some),
                move || {
                    let panel = cancel.clone();
                    let state = cancel.chat.clone();
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(14.0))
                        .bg(move || p.raised())
                        .pl(px(14.0))
                        .pr(px(4.0))
                        .child(composer_note(
                            p,
                            || "Editing message",
                            || "Sending replaces later messages and replies.".into(),
                        ))
                        .child(
                            ui::icon_button(p, ui::CLOSE, "Cancel edit", move || {
                                panel.cancel_edit()
                            })
                            .disabled(move || state.sending()),
                        )
                },
            ))
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
                        self.attachments
                            .draft_strip(move || draft_files.busy() || loading.sending()),
                    )
                    .child(
                        input()
                            .element_ref(&input_ref)
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
                            .on_paste(move |event: &PasteEvent| {
                                if !pasting_chat.sending() && !pasting_chat.loading() {
                                    pasting.paste(event);
                                }
                            })
                            .h(px(56.0))
                            .pt(px(18.0))
                            .pb(px(10.0))
                            .px(px(18.0))
                            .text_size(px(16.0))
                            .line_height(px(24.0))
                            .text_color(move || p.ink())
                            .readonly({
                                let loading = self.chat.clone();
                                move || loading.loading()
                            }),
                    )
                    .child(self.voice.preview(p))
                    .child(when(
                        move || reasoning_open.get() && selection.with(Option::is_some),
                        {
                            let chat = self.chat.clone();
                            move || reasoning_panel(p, chat.clone(), selection, reasoning_open)
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(7.0))
                            .child(
                                press()
                                    .size(px(42.0))
                                    .rounded(px(21.0))
                                    .items_center()
                                    .justify_center()
                                    .label("Attach files")
                                    .disabled({
                                        let pick = pick.clone();
                                        move || {
                                            pick.busy()
                                                || pick_chat.loading()
                                                || pick_chat.sending()
                                        }
                                    })
                                    .on_press(move || {
                                        input_ref.blur();
                                        pick.show_actions();
                                    })
                                    .child(ui::icon(PAPERCLIP, 22.0, move || p.ink())),
                            )
                            .child(reasoning_toggle(
                                p,
                                self.chat.clone(),
                                selection,
                                reasoning_open,
                            ))
                            .child(view().flex_1())
                            .child(self.voice.button(p, self.chat.clone(), input_ref, {
                                let reading = self.reading.clone();
                                move || reading.stop()
                            }))
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
                                    .opacity(move || if enabled.get() { 1.0_f32 } else { 0.45 })
                                    .disabled(move || !enabled.get())
                                    .label({
                                        let chat = self.chat.clone();
                                        move || {
                                            if chat.busy() {
                                                "Queue message"
                                            } else if editing.with(Option::is_some) {
                                                "Send edited message"
                                            } else {
                                                "Send message"
                                            }
                                        }
                                    })
                                    .on_press(move || {
                                        let submitted = draft.get_untracked();
                                        let files = send_files.clone();
                                        let replacing = editing.get_untracked().map(|(id, _)| id);
                                        send.send(
                                            submitted.clone(),
                                            files.ids(),
                                            replacing,
                                            move || {
                                                reasoning_open.set(false);
                                                if draft.get_untracked() == submitted {
                                                    draft.set(String::new());
                                                }
                                                files.sent();
                                                files.finish_edit();
                                                editing.set(None);
                                            },
                                        );
                                    })
                                    .child(ui::icon(ui::UP, 22.0, move || p.background())),
                            ),
                    )
                    .child(self.voice.status(p)),
            )
            .child(self.voice.error(p))
            .child(self.attachments.notice())
    }

    fn pending_messages(&self) -> If {
        let chat = self.chat.clone();
        let pending = computed(move || {
            chat.current()
                .map(|thread| thread.pending_messages.clone())
                .unwrap_or_default()
        });
        let chat = self.chat.clone();
        let p = self.palette;
        when(
            move || pending.with(|messages| !messages.is_empty()),
            move || {
                let chat = chat.clone();
                scroll()
                    .max_h(px(160.0))
                    .flex_shrink(1.0)
                    .keyboard_taps(KeyboardTaps::Handled)
                    .child(view().gap(px(8.0)).child(each(
                        pending,
                        |message| message.id.clone(),
                        move |message, _| {
                            let running = chat.clone();
                            let state = chat.clone();
                            let send = chat.clone();
                            press()
                                .px(px(14.0))
                                .py(px(8.0))
                                .rounded(px(14.0))
                                .bg(move || p.raised())
                                .label(move || {
                                    if running.busy() {
                                        "Queued message"
                                    } else {
                                        "Send queued message"
                                    }
                                })
                                .disabled(move || {
                                    state.busy() || state.sending() || state.loading()
                                })
                                .on_press(move || {
                                    send.send_queued(
                                        message.with_untracked(|message| message.id.clone()),
                                    )
                                })
                                .child(composer_note(
                                    p,
                                    {
                                        let chat = chat.clone();
                                        move || {
                                            if chat.busy() {
                                                "Queued · steers after tool results"
                                            } else {
                                                "Not sent · tap to send"
                                            }
                                        }
                                    },
                                    move || {
                                        message.with(|message| {
                                            if message.content.is_empty() {
                                                format!(
                                                    "{} attached files",
                                                    message.attachments.len()
                                                )
                                            } else {
                                                message.content.clone()
                                            }
                                        })
                                    },
                                ))
                        },
                    )))
            },
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

type ReasoningContext = Memo<Option<(String, String)>>;

fn reasoning_context(chat: &Chat) -> Option<(String, String)> {
    let current = chat.current();
    let config = chat.config();
    let provider = current
        .as_ref()
        .map(|thread| thread.provider_id.as_str())
        .filter(|id| !id.is_empty())
        .or_else(|| config["activeId"].as_str())?;
    if let Some(group) = config["groups"]
        .as_array()?
        .iter()
        .find(|group| group["id"] == provider && group["connected"] == true)
    {
        return Some((provider.into(), group["name"].as_str()?.into()));
    }
    let connection = config["providers"].as_array()?.iter().find(|item| {
        item["id"] == provider && item["connected"] == true && item["needsReauth"] != true
    })?;
    let model = current
        .as_ref()
        .map(|thread| thread.model.as_str())
        .filter(|model| !model.is_empty())
        .or_else(|| connection["model"].as_str())
        .filter(|model| !model.is_empty())?;
    Some((provider.into(), model.into()))
}

pub(crate) fn effort_name(effort: Option<&str>) -> String {
    match effort {
        None | Some("") => "Default".into(),
        Some("none") => "Off".into(),
        Some("xhigh") => "Extra high".into(),
        Some(level) => {
            let mut chars = level.chars();
            let first = chars.next().expect("nonempty reasoning effort");
            format!("{}{}", first.to_uppercase(), chars.as_str())
        }
    }
}

fn reasoning_toggle(p: Palette, chat: Chat, context: ReasoningContext, open: Signal<bool>) -> View {
    let value = computed({
        let chat = chat.clone();
        move || effort_name(chat.reasoning().as_deref())
    });
    let disabled = computed({
        let chat = chat.clone();
        move || context.with(Option::is_none) || chat.loading() || chat.settings_busy()
    });
    press()
        .h(px(42.0))
        .px(px(8.0))
        .rounded(px(21.0))
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .disabled(disabled)
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0 })
        .label(move || format!("Reasoning effort: {}", value.get()))
        .on_press(move || open.update(|open| *open = !*open))
        .child(ui::icon(BRAIN, 16.0, move || p.ink()))
        .child(
            text(move || value.get())
                .text_size(px(12.0))
                .text_color(move || p.ink()),
        )
        .child(ui::icon(ui::DOWN, 12.0, move || p.muted()))
}

#[derive(Deserialize)]
struct ReasoningOptions {
    levels: Vec<String>,
    discovered: bool,
}

fn reasoning_panel(p: Palette, chat: Chat, context: ReasoningContext, open: Signal<bool>) -> View {
    let levels = signal(None);
    let discovered = signal(true);
    let error = signal(None::<String>);
    let core = chat.core();
    if let Some((id, model)) = context.get_untracked() {
        spawn(async move {
            let result = core
                .ai::<ReasoningOptions>(json!({"op":"ai_reasoning_options","id":id,"model":model}))
                .await;
            if context.get_untracked() != Some((id, model)) {
                return;
            }
            match result {
                Ok(options) => {
                    discovered.set(options.discovered);
                    levels.set(Some(
                        std::iter::once(String::new())
                            .chain(options.levels)
                            .collect::<Vec<_>>(),
                    ));
                }
                Err(problem) => error.set(Some(problem.to_string())),
            }
        });
    }
    let busy = chat.clone();
    let effort = computed({
        let chat = chat.clone();
        move || chat.reasoning()
    });
    view()
        .py(px(5.0))
        .gap(px(4.0))
        .child(when(
            move || busy.busy(),
            move || {
                reasoning_help(
                    p,
                    "Applies to your next message. This response keeps its current setting.",
                )
            },
        ))
        .child(when(
            move || levels.with(Option::is_none) && error.with(Option::is_none),
            move || {
                view()
                    .h(px(44.0))
                    .items_center()
                    .justify_center()
                    .child(ui::icon("M12 3a9 9 0 1 1-9 9", 18.0, move || p.ink()))
            },
        ))
        .child(when(
            move || levels.with(Option::is_some),
            move || {
                let chat = chat.clone();
                scroll()
                    .horizontal()
                    .h(px(44.0))
                    .keyboard_taps(KeyboardTaps::Handled)
                    .child(view().flex_row().gap(px(4.0)).px(px(10.0)).child(each(
                        move || levels.get().unwrap_or_default(),
                        Clone::clone,
                        move |level, _| {
                            let level = level.get_untracked();
                            let value = (!level.is_empty()).then(|| level.clone());
                            let selected = computed({
                                let value = value.clone();
                                move || value == effort.get()
                            });
                            let reason = effort_name(value.as_deref());
                            let accessible = if level.is_empty() {
                                "Reasoning model default".into()
                            } else {
                                format!("Reasoning {reason}")
                            };
                            let name = if level == "xhigh" {
                                "XHigh".into()
                            } else {
                                reason
                            };
                            let save = chat.clone();
                            let enabled = chat.clone();
                            press()
                                .px(px(8.0))
                                .min_w(px(44.0))
                                .h(px(44.0))
                                .rounded(px(12.0))
                                .items_center()
                                .justify_center()
                                .label(accessible)
                                .disabled(move || enabled.settings_busy() || enabled.sending())
                                .bg(move || {
                                    if selected.get() {
                                        p.background()
                                    } else {
                                        rgba(0x00000000).into()
                                    }
                                })
                                .accessibility_state(move || AccessibilityState {
                                    selected: selected.get(),
                                    ..Default::default()
                                })
                                .on_press(move || {
                                    save.set_reasoning(value.clone(), move || open.set(false))
                                })
                                .child(
                                    text(name)
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || {
                                            if selected.get() { p.ink() } else { p.muted() }
                                        }),
                                )
                        },
                    )))
            },
        ))
        .child(when(
            move || !discovered.get(),
            move || {
                reasoning_help(
                    p,
                    "Supported levels vary by model. Default uses its saved settings.",
                )
            },
        ))
        .child(when(
            move || error.with(Option::is_some),
            move || {
                text(move || error.get().unwrap_or_default())
                    .px(px(16.0))
                    .text_size(px(12.0))
                    .text_color(rgb(0xff0000))
            },
        ))
}

fn reasoning_help(p: Palette, value: &'static str) -> Text {
    text(value)
        .px(px(16.0))
        .text_size(px(11.0))
        .line_height(px(16.0))
        .text_color(move || p.muted())
}

fn composer_note(
    p: Palette,
    title: impl Fn() -> &'static str + 'static,
    detail: impl Fn() -> String + 'static,
) -> View {
    view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(2.0))
        .child(
            text(title)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
        .child(
            text(detail)
                .number_of_lines(2)
                .text_size(px(11.0))
                .text_color(move || p.muted()),
        )
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
