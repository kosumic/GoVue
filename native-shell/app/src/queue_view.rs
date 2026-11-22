use std::rc::Rc;

use nsh_core::{Checked, Dialog, DialogButton, DialogButtonStyle, element::dialog, prelude::*};
use serde_json::json;

use crate::{
    download_library::DOWNLOAD,
    downloads::Downloads,
    queue::Queue,
    ui::{self, Palette, press},
};

const LOOP: &str = "m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4m14-1v2a3 3 0 0 1-3 3H3";
const DELETE: &str = "M3 6h18m-2 0-1.5 15h-11L5 6m4 0V3h6v3M10 10v7m4-7v7";

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
                .child(scroll().max_h(px(390.0)).child(view().child(each(
                    move || state.with(|state| state.queue.clone()),
                    |item| item.video_id.clone(),
                    move |item, _| {
                        let navigate = open.clone();
                        let close = close.clone();
                        let close_download = close.clone();
                        let download = downloads.clone();
                        let remove = remove.clone();
                        view()
                            .px(px(12.0))
                            .min_h(px(72.0))
                            .flex_row()
                            .items_center()
                            .child(
                                press()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .py(px(8.0))
                                    .gap(px(4.0))
                                    .on_press(move || {
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
