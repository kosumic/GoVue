use govue_shell_runtime::Core;
use nsh_core::{element::share, prelude::*, share::ShareRequest};
use serde_json::Value;

use crate::{
    browser::Browser,
    download_library::DOWNLOAD,
    downloads::alert,
    ui::{self, press},
};

const QUEUE: &str = "M3 6h18M3 12h18M3 18h10m4-3v6m-3-3h6";
const SHARE: &str = "m8 10 8-4m-8 8 8 4M8 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0m14-8a3 3 0 1 1-6 0 3 3 0 0 1 6 0m0 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0";

#[derive(Clone, PartialEq)]
pub struct Card {
    pub video_id: String,
    pub title: String,
    pub author: String,
}

impl Card {
    pub fn from_page(core: &Core, value: &Value) -> Option<Self> {
        let id = value["videoId"].as_str()?;
        let address = format!("https://m.youtube.com/watch?v={id}");
        let parsed = core
            .state::<Value>(serde_json::json!({"op":"browser_identity","url":address}))
            .ok()?;
        if parsed["videoId"] != id {
            return None;
        }
        Some(Self {
            video_id: id.into(),
            title: value["title"]
                .as_str()
                .filter(|title| !title.is_empty())
                .unwrap_or("Video")
                .chars()
                .take(1000)
                .collect(),
            author: value["author"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect(),
        })
    }
}

pub fn share_video(url: String) {
    spawn(async move {
        if let Err(error) = share(ShareRequest::new().text(url)).await {
            alert(error.message).await;
        }
    });
}

pub fn sheet(browser: Browser, card: Card) -> View {
    let close = browser.clone();
    let heading = browser.clone();
    let play = browser.clone();
    let queued = browser.clone();
    let download = browser.clone();
    let sharing = card.video_id.clone();
    let playing = card.video_id.clone();
    let enqueue = card.clone();
    let video = card.video_id.clone();
    view()
        .absolute()
        .inset(px(0.0))
        .justify_end()
        .z_index(5)
        .child(
            press()
                .absolute()
                .inset(px(0.0))
                .bg(rgba(0x00000080))
                .label("Close video options")
                .on_press(move || {
                    close.close_card();
                }),
        )
        .child(
            view()
                .self_center()
                .w_full()
                .max_w(px(680.0))
                .max_h(percentage(0.92))
                .rounded(px(18.0))
                .overflow_hidden()
                .bg(rgb(0x202020))
                .child(
                    view()
                        .h(px(48.0))
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .pl(px(16.0))
                        .pr(px(6.0))
                        .child(
                            text(card.title)
                                .flex_1()
                                .truncate()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0xffffff)),
                        )
                        .child(
                            press()
                                .size(px(44.0))
                                .items_center()
                                .justify_center()
                                .label("Close video options")
                                .on_press(move || {
                                    heading.close_card();
                                })
                                .child(ui::icon(ui::CLOSE, 22.0, || rgb(0xffffff).into())),
                        ),
                )
                .child(
                    scroll().flex_shrink(1.0).child(
                        view()
                            .pb(px(6.0))
                            .child(
                                image(format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg"))
                                    .w_full()
                                    .h(px(180.0))
                                    .object_fit(nsh_core::ResizeMode::Cover),
                            )
                            .child(
                                text(card.author)
                                    .p(px(16.0))
                                    .text_size(px(13.0))
                                    .text_color(rgb(0xb5b5b5)),
                            )
                            .child(action("Play", ui::PLAY, move || {
                                play.close_card();
                                play.open(format!("https://m.youtube.com/watch?v={playing}"));
                            }))
                            .child(action("Add to queue", QUEUE, move || {
                                queued.queue_card(enqueue.clone())
                            }))
                            .child(action("Download", DOWNLOAD, move || {
                                download.download_card(video.clone())
                            }))
                            .child(action("Share", SHARE, move || {
                                share_video(format!("https://www.youtube.com/watch?v={sharing}"))
                            })),
                    ),
                ),
        )
}

fn action(title: &'static str, glyph: &'static str, run: impl Fn() + 'static) -> View {
    press()
        .h(px(48.0))
        .px(px(16.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .label(title)
        .on_press(run)
        .child(ui::icon(glyph, 22.0, || rgb(0xffffff).into()))
        .child(text(title).text_size(px(14.0)).text_color(rgb(0xffffff)))
}
