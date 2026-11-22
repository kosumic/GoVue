use std::{
    cell::Cell,
    collections::HashSet,
    time::{Duration, Instant},
};

use govue_shell_runtime::Core;
use nsh_core::{
    PressEvent, ResizeMode,
    element::{ScrollEvent, share},
    prelude::*,
    share::ShareRequest,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    download_library::DOWNLOAD,
    downloads::{Downloads, alert},
    ui::{self, press},
};

const SHARE: &str = "m8 10 8-4m-8 8 8 4M8 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0m14-8a3 3 0 1 1-6 0 3 3 0 0 1 6 0m0 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0";
const RESET: &str = "M3 12a9 9 0 1 0 3-6.7L3 8m0-5v5h5";

#[derive(Clone, Deserialize, PartialEq)]
pub struct GalleryTarget {
    pub urls: Vec<String>,
    pub index: usize,
}

#[derive(Clone, Deserialize, PartialEq)]
struct GalleryImage {
    uri: String,
}

#[derive(Clone)]
struct Pages {
    current: Signal<usize>,
    zoomed: Signal<bool>,
    loaded: Signal<HashSet<usize>>,
    width: Signal<f32>,
    height: Signal<f32>,
}

pub fn gallery(
    core: Core,
    downloads: Downloads,
    targets: GalleryTarget,
    close: impl Fn() + 'static,
) -> View {
    let urls = std::rc::Rc::new(targets.urls);
    let current = signal(targets.index);
    let loaded = signal(HashSet::new());
    let width = signal(0.0);
    let height = signal(0.0);
    let zoomed = signal(false);
    let pages = Pages {
        current,
        loaded,
        width,
        height,
        zoomed,
    };
    let scroll_ref = element_ref();
    let count = urls.len();
    let busy = signal(false);
    let export_core = core.clone();
    let export_urls = urls.clone();
    let actions = move |save, name, glyph| {
        let core = export_core.clone();
        let downloads = downloads.clone();
        let urls = export_urls.clone();
        gallery_button(
            name,
            glyph,
            move || busy.get() || !loaded.with(|images| images.contains(&current.get())),
            move || {
                let url = urls[current.get_untracked()].clone();
                let core = core.clone();
                let downloads = downloads.clone();
                busy.set(true);
                spawn(async move {
                    match core
                        .execute::<Value>(json!({"op":"gallery_export_file","url":url}))
                        .await
                    {
                        Ok(file) if save => {
                            let mime = file["mimeType"].as_str().unwrap_or("image/jpeg").to_owned();
                            downloads.save_export(file, &mime);
                        }
                        Ok(file) => {
                            if let Some(path) = file["uri"]
                                .as_str()
                                .and_then(|uri| url::Url::parse(uri).ok())
                                .and_then(|uri| uri.to_file_path().ok())
                                && let Err(error) =
                                    share(ShareRequest::new().title("Share image").file(path)).await
                            {
                                alert(error.message).await;
                            }
                        }
                        Err(error) => alert(error.to_string()).await,
                    }
                    busy.set(false);
                });
            },
        )
    };
    view()
        .absolute()
        .inset(px(0.0))
        .bg(rgb(0x0f0f0f))
        .z_index(6)
        .child(
            view()
                .h(px(56.0))
                .flex_row()
                .items_center()
                .px(px(4.0))
                .child(gallery_button(
                    "Back from gallery",
                    ui::BACK,
                    || false,
                    close,
                ))
                .child(
                    text(move || format!("{} / {count}", current.get() + 1))
                        .flex_1()
                        .pl(px(8.0))
                        .text_size(px(20.0))
                        .text_color(rgb(0xeeeeee)),
                )
                .child(actions(true, "Save image", DOWNLOAD))
                .child(actions(false, "Share image", SHARE)),
        )
        .child(
            scroll()
                .element_ref(&scroll_ref)
                .flex_1()
                .min_h(px(0.0))
                .horizontal()
                .paging(true)
                .scrollbar(false)
                .scrollable(move || !zoomed.get())
                .on_layout(move |frame: &Bounds<Points>| {
                    let size = frame.size;
                    height.set(size.height.0);
                    if size.width.0 > 0.0 && (size.width.0 - width.get_untracked()).abs() > 0.5 {
                        width.set(size.width.0);
                        spawn(async move {
                            timer(Duration::ZERO).await;
                            scroll_ref.scroll_to(
                                nsh_core::Point::new(
                                    px(size.width.0 * current.get_untracked() as f32),
                                    px(0.0),
                                ),
                                false,
                            );
                        });
                    }
                })
                .on_scroll_end(move |offset: &nsh_core::Point<Points>| {
                    let width = width.get_untracked();
                    if width <= 0.0 || zoomed.get_untracked() {
                        return;
                    }
                    current.set(((offset.x.0 / width).round().max(0.0) as usize).min(count - 1));
                })
                .child(
                    view().flex_row().children(
                        urls.iter()
                            .enumerate()
                            .map(|(index, url)| {
                                let url = url.clone();
                                let core = core.clone();
                                let state = pages.clone();
                                view()
                                    .w(move || px(width.get().max(1.0)))
                                    .h(move || px(height.get().max(1.0)))
                                    .flex_shrink(0.0)
                                    .relative()
                                    .bg(rgb(0x000000))
                                    .overflow_hidden()
                                    .child(when(
                                        move || {
                                            current.with(|current| current.abs_diff(index) <= 1)
                                        },
                                        move || {
                                            image_page(
                                                core.clone(),
                                                url.clone(),
                                                index,
                                                state.clone(),
                                            )
                                        },
                                    ))
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
        )
}

fn image_page(core: Core, url: String, page: usize, pages: Pages) -> View {
    let image_state = signal(None::<GalleryImage>);
    let failed = signal(false);
    let attempt = signal(0);
    let zoom = signal(1.0_f32);
    let zoom_ref = element_ref();
    let active = computed(move || pages.current.get() == page);
    let last_tap = stored(Cell::new(None::<Instant>));
    effect(move || {
        attempt.get();
        pages.loaded.update(|loaded| {
            loaded.remove(&page);
        });
        failed.set(false);
        let core = core.clone();
        let url = url.clone();
        spawn(async move {
            match core
                .execute::<GalleryImage>(json!({"op":"gallery_fetch","url":url}))
                .await
            {
                Ok(item) => {
                    image_state.set(Some(item));
                    pages.loaded.update(|loaded| {
                        loaded.insert(page);
                    });
                }
                Err(_) => failed.set(true),
            }
        });
    });
    effect(move || {
        if active.get() {
            pages.zoomed.set(zoom.get() > 1.01);
        } else {
            zoom.set(1.0);
            zoom_ref.zoom_to(1.0, None, false);
        }
    });
    cleanup(move || {
        pages.loaded.update(|loaded| {
            loaded.remove(&page);
        })
    });
    view()
        .absolute()
        .inset(px(0.0))
        .items_center()
        .justify_center()
        .child(when(failed, move || {
            view()
                .items_center()
                .gap(px(12.0))
                .child(
                    text("Image could not load")
                        .text_size(px(14.0))
                        .text_color(rgb(0xaaaaaa)),
                )
                .child(
                    press()
                        .p(px(16.0))
                        .label("Retry")
                        .on_press(move || attempt.update(|retry| *retry += 1))
                        .child(text("Retry").text_size(px(14.0)).text_color(rgb(0xeeeeee))),
                )
        }))
        .child(when(
            move || !failed.get() && image_state.with(Option::is_none),
            || ui::icon("M12 3a9 9 0 1 1-9 9", 22.0, || rgb(0xeeeeee).into()),
        ))
        .child(when(
            move || !failed.get() && image_state.with(Option::is_some),
            move || {
                scroll()
                    .absolute()
                    .inset(px(0.0))
                    .element_ref(&zoom_ref)
                    .zoom_range(1.0, 5.0)
                    .bounces_zoom(false)
                    .scrollbar(false)
                    .on_scroll(move |event: &ScrollEvent| zoom.set(event.zoom_scale))
                    .child(
                        press()
                            .w(move || px(pages.width.get()))
                            .h(move || px(pages.height.get()))
                            .label(format!("Image {}", page + 1))
                            .on_press(move |event: &PressEvent| {
                                let now = Instant::now();
                                let previous = last_tap.with(|time| time.replace(Some(now)));
                                if event.clicks == 2
                                    || previous.is_some_and(|time| {
                                        now.duration_since(time) < Duration::from_millis(300)
                                    })
                                {
                                    last_tap.with(|time| time.set(None));
                                    let next = if zoom.get_untracked() > 1.01 {
                                        1.0
                                    } else {
                                        2.0
                                    };
                                    zoom_ref.zoom_to(next, None, true);
                                }
                            })
                            .child(
                                image(move || {
                                    image_state
                                        .get()
                                        .map_or_else(String::new, |image| image.uri)
                                })
                                .w_full()
                                .h_full()
                                .object_fit(ResizeMode::Contain)
                                .on_error(move |_: &str| {
                                    failed.set(true);
                                    pages.loaded.update(|loaded| {
                                        loaded.remove(&page);
                                    });
                                }),
                            ),
                    )
            },
        ))
        .child(when(
            move || active.get() && zoom.get() > 1.01,
            move || {
                press()
                    .absolute()
                    .bottom(px(24.0))
                    .self_center()
                    .rounded(px(24.0))
                    .p(px(12.0))
                    .bg(rgba(0x282828dd))
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .label("Reset image zoom")
                    .on_press(move || zoom_ref.zoom_to(1.0, None, true))
                    .child(ui::icon(RESET, 18.0, || rgb(0xeeeeee).into()))
                    .child(
                        text(move || format!("Reset zoom · {:.1}×", zoom.get()))
                            .text_size(px(14.0))
                            .text_color(rgb(0xeeeeee)),
                    )
            },
        ))
}

fn gallery_button(
    title: &'static str,
    glyph: &'static str,
    disabled: impl Fn() -> bool + 'static,
    action: impl Fn() + 'static,
) -> View {
    let disabled = computed(disabled);
    press()
        .size(px(44.0))
        .items_center()
        .justify_center()
        .label(title)
        .disabled(disabled)
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0 })
        .on_press(action)
        .child(ui::icon(glyph, 22.0, || rgb(0xeeeeee).into()))
}
