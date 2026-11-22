use std::rc::Rc;

use nsh_core::{
    AccessibilityActionDef, Checked, Dialog, DialogButton, DialogButtonStyle, PressEvent,
    element::{ClaimRequest, ResponderEvent, TouchClaim, dialog},
    prelude::*,
};
use serde_json::json;

use crate::{
    download_library::DOWNLOAD,
    downloads::Downloads,
    queue::Queue,
    ui::{self, Palette, press},
};

const LOOP: &str = "m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4m14-1v2a3 3 0 0 1-3 3H3";
const DELETE: &str = "M3 6h18m-2 0-1.5 15h-11L5 6m4 0V3h6v3M10 10v7m4-7v7";
const ROW_HEIGHT: f32 = 72.0;

#[derive(Clone, PartialEq)]
struct Drag {
    video: String,
    from: usize,
    to: usize,
    origin: f32,
    captured: bool,
}

pub fn queue_sheet(
    p: Palette,
    queue: Queue,
    open: Rc<dyn Fn(String)>,
    downloads: Downloads,
    close: impl Fn() + Clone + 'static,
) -> View {
    let state = queue.signal();
    let mode = computed(move || {
        state.with(|state| match state.loop_mode.as_str() {
            "repeat" => "Loop one",
            "pause" => "Pause at end",
            "shuffle" => "Shuffle",
            _ => "Play next",
        })
    });
    let autoplay = computed(move || state.with(|state| state.enabled));
    let update = queue.clone();
    let loop_mode = queue.clone();
    let clear = queue.clone();
    let remove = queue.clone();
    let error = queue.clone();
    let dismiss = close.clone();
    let download_all = downloads.clone();
    let close_all = close.clone();
    let drag = signal(None::<Drag>);
    let reorder = queue.clone();
    let displayed = computed(move || {
        let mut items = state.with(|state| state.queue.clone());
        if let Some(active) = drag.get()
            && active.to < items.len()
            && let Some(from) = items.iter().position(|item| item.video_id == active.video)
            && from != active.to
        {
            let item = items.remove(from);
            items.insert(active.to, item);
        }
        items
    });
    view()
        .absolute()
        .inset(px(0.0))
        .child(
            press()
                .absolute()
                .inset(px(0.0))
                .bg(rgba(0x00000066))
                .label("Close queue")
                .on_press(dismiss),
        )
        .child(
            view()
                .absolute()
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .max_h(percentage(0.75))
                .rounded(px(16.0))
                .bg(move || p.background())
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .px(px(12.0))
                        .min_h(px(48.0))
                        .child(
                            text("Queue")
                                .flex_1()
                                .text_size(px(17.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || p.ink()),
                        )
                        .child(ui::icon_button(p, ui::CLOSE, "Close queue", {
                            let close = close.clone();
                            move || close()
                        })),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .px(px(12.0))
                        .gap(px(8.0))
                        .child(
                            press()
                                .size(px(40.0))
                                .label(move || mode.get())
                                .items_center()
                                .justify_center()
                                .on_press(move || loop_mode.update("loop", json!({})))
                                .child(ui::icon(LOOP, 22.0, move || p.ink())),
                        )
                        .child(ui::icon_button(p, DELETE, "Clear queue", move || {
                            if state.with(|state| state.queue.is_empty()) {
                                return;
                            }
                            let clear = clear.clone();
                            spawn(async move {
                                if dialog(Dialog {
                                    id: 0,
                                    title: Some("Clear queue?".into()),
                                    message: Some("Remove all videos from the queue?".into()),
                                    buttons: vec![
                                        DialogButton {
                                            text: "Cancel".into(),
                                            style: DialogButtonStyle::Cancel,
                                        },
                                        DialogButton {
                                            text: "OK".into(),
                                            style: DialogButtonStyle::Destructive,
                                        },
                                    ],
                                    cancelable: true,
                                    sheet: false,
                                })
                                .await
                                    == 1
                                {
                                    clear.update("clear", json!({}));
                                }
                            });
                        }))
                        .child(ui::icon_button(p, DOWNLOAD, "Download queue", move || {
                            let items = state.with_untracked(|state| {
                                state.queue.iter().map(|item| json!({
                                    "videoId":item.video_id,"title":item.title,"author":item.author
                                })).collect::<Vec<_>>()
                            });
                            if !items.is_empty() {
                                close_all();
                                download_all.confirm("Queue", json!(items));
                            }
                        }))
                        .child(view().flex_1())
                        .child(
                            text("Autoplay")
                                .text_size(px(13.0))
                                .text_color(move || p.ink()),
                        )
                        .child(
                            press()
                                .role(AccessibilityRole::Switch)
                                .label("Queue autoplay")
                                .w(px(42.0))
                                .h(px(24.0))
                                .relative()
                                .rounded(px(12.0))
                                .bg(move || if autoplay.get() { p.ink() } else { p.muted() })
                                .accessibility_state(move || AccessibilityState {
                                    checked: Some(if autoplay.get() {
                                        Checked::True
                                    } else {
                                        Checked::False
                                    }),
                                    ..Default::default()
                                })
                                .on_press(move || {
                                    update
                                        .update("queue_enabled", json!({"enabled":!autoplay.get()}))
                                })
                                .child(
                                    view()
                                        .absolute()
                                        .top(px(3.0))
                                        .left(move || px(if autoplay.get() { 21.0 } else { 3.0 }))
                                        .size(px(18.0))
                                        .rounded(px(9.0))
                                        .bg(move || p.background()),
                                ),
                        ),
                )
                .child(when(
                    move || state.with(|state| state.queue.is_empty()),
                    move || {
                        text("Queue is empty")
                            .py(px(24.0))
                            .text_center()
                            .text_size(px(13.0))
                            .text_color(move || p.muted())
                    },
                ))
                .child(scroll().max_h(px(390.0)).child(view()
                    .claim_touches(TouchClaim::on_move().capture().ask(move |request: &ClaimRequest| {
                        if drag.with_untracked(Option::is_some) { request.claim(); }
                    }))
                    .on_responder_grant(move |_: &ResponderEvent| drag.update(|drag| {
                        if let Some(current) = drag { current.captured = true; }
                    }))
                    .on_responder_move(move |event: &ResponderEvent| {
                        let length = state.with_untracked(|state| state.queue.len());
                        drag.update(|drag| {
                            if let Some(active) = drag {
                                let delta = ((event.page.y.0 - active.origin) / ROW_HEIGHT).floor() as isize;
                                active.to = active.from.saturating_add_signed(delta).min(length.saturating_sub(1));
                            }
                        });
                    })
                    .on_responder_terminate(move |_: &ResponderEvent| drag.set(None))
                    .on_responder_release(move |_: &ResponderEvent| {
                        if let Some(active) = drag.get_untracked() && active.to != active.from {
                            reorder.update("move", json!({"id":active.video,"offset":active.to as isize - active.from as isize}));
                        }
                        drag.set(None);
                    })
                    .child(each(
                    displayed,
                    |item| item.video_id.clone(),
                    move |item, _| {
                        let navigate = open.clone();
                        let close = close.clone();
                        let close_download = close.clone();
                        let download = downloads.clone();
                        let remove = remove.clone();
                        let move_item = remove.clone();
                        let picked = move || drag.with(|drag| drag.as_ref().is_some_and(|active| {
                            item.with(|item| item.video_id == active.video)
                        }));
                        view()
                            .px(px(12.0))
                            .h(px(ROW_HEIGHT))
                            .flex_row()
                            .items_center()
                            .bg(move || if picked() { p.raised() } else { rgba(0x00000000).into() })
                            .child(
                                press()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .h_full()
                                    .justify_center()
                                    .py(px(8.0))
                                    .gap(px(4.0))
                                    .label(move || item.with(|item| format!("Play {}", item.title)))
                                    .hint("Hold and drag to reorder")
                                    .accessibility_actions(vec![
                                        AccessibilityActionDef { name: "decrement".into(), label: Some("Move up".into()) },
                                        AccessibilityActionDef { name: "increment".into(), label: Some("Move down".into()) },
                                    ])
                                    .on_accessibility_action(move |action: &str| {
                                        let offset = match action { "increment" => 1, "decrement" => -1, _ => return };
                                        move_item.update("move", json!({"id":item.with_untracked(|item| item.video_id.clone()),"offset":offset}));
                                    })
                                    .on_long_press(move |event: &PressEvent| {
                                        let video = item.with_untracked(|item| item.video_id.clone());
                                        if let Some(from) = state.with_untracked(|state| state.queue.iter().position(|item| item.video_id == video)) {
                                            let origin = event.page_position.y.0 - event.position.y.0.clamp(0.0, ROW_HEIGHT);
                                            drag.set(Some(Drag { video, from, to: from, origin, captured: false }));
                                        }
                                    })
                                    .on_press_up(move || {
                                        if drag.with_untracked(|drag| drag.as_ref().is_some_and(|active| !active.captured)) {
                                            drag.set(None);
                                        }
                                    })
                                    .on_press(move || {
                                        if drag.with_untracked(Option::is_some) { return; }
                                        close();
                                        navigate(item.with(|item| item.address()));
                                    })
                                    .child(
                                        text(move || item.with(|item| item.title.clone()))
                                            .truncate()
                                            .text_size(px(14.0))
                                            .text_color(move || p.ink()),
                                    )
                                    .child(
                                        text(move || item.with(|item| item.author.clone()))
                                            .truncate()
                                            .text_size(px(12.0))
                                            .text_color(move || p.muted()),
                                    ),
                            )
                            .child(ui::icon_button(p, DOWNLOAD, "Download video", move || {
                                close_download();
                                let item = item.get_untracked();
                                download.confirm("", json!([{
                                    "videoId":item.video_id,"title":item.title,"author":item.author
                                }]));
                            }))
                            .child(ui::icon_button(
                                p,
                                ui::CLOSE,
                                "Remove from queue",
                                move || {
                                    remove.update(
                                        "remove",
                                        json!({"id":item.with(|item|item.video_id.clone())}),
                                    );
                                },
                            ))
                    },
                ))))
                .child(when(move || error.error().is_some(), {
                    let queue = queue.clone();
                    move || {
                        let queue = queue.clone();
                        text(move || queue.error().unwrap_or_default())
                            .px(px(12.0))
                            .py(px(6.0))
                            .text_size(px(13.0))
                            .text_color(rgb(0xff0000))
                    }
                })),
        )
}
