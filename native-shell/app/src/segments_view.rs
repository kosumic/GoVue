use govue_shell_runtime::{Chapter, WatchPlayback};
use nsh_core::{
    PointerEvents, ResizeMode,
    element::{ImageSrc, set_clipboard, share},
    prelude::*,
    share::ShareRequest,
};

use crate::{
    reader_view::timestamp,
    ui::{self, Palette, press},
};

pub const SEGMENTS: &str = "M3 6h18M3 12h11m3 0h4M3 18h5m3 0h10M14 10v4M8 16v4";
const SHARE: &str = "M8 10l8-4m-8 8 8 4M6 9a3 3 0 1 0 0 6m12 2a3 3 0 1 0 0 6m0-6a3 3 0 1 0 0 6m0-21a3 3 0 1 0 0 6m0-6a3 3 0 1 0 0 6";

pub fn segments_view(
    p: Palette,
    watch: WatchPlayback,
    preview: Signal<Option<Chapter>>,
    close: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let jumping = watch.clone();
    let reading = watch.clone();
    let chapters = computed(move || {
        reading
            .video()
            .map(|video| (video.video_id, video.chapters))
    });
    let has_chapters = computed(move || {
        chapters.with(|list| {
            list.as_ref()
                .is_some_and(|(_, chapters)| !chapters.is_empty())
        })
    });
    let active = computed({
        let watch = watch.clone();
        move || {
            let position = watch.status()?.position;
            chapters.with(|list| {
                let (_, chapters) = list.as_ref()?;
                chapters
                    .partition_point(|chapter| chapter.start <= position)
                    .checked_sub(1)
            })
        }
    });
    let width =
        computed(|| ((nsh_core::element::window().width.0 - 48.0) * 0.42).clamp(1.0, 156.0));
    let list = element_ref();
    spawn(async move {
        timer(std::time::Duration::from_millis(30)).await;
        if let Some(index) = active.get_untracked() {
            let row_height = width.get_untracked() * 9.0 / 16.0 + 20.0;
            list.scroll_to(
                point(px(0.0), px(index.saturating_sub(2) as f32 * row_height)),
                false,
            );
        }
    });
    let body = dynamic(move || {
        let Some((video, chapters)) = chapters.get() else {
            return view();
        };
        if chapters.is_empty() {
            let segments = watch.sponsor_segments(&video);
            if segments.is_empty() {
                return view().p(px(24.0)).child(
                    text("No chapters or segments available")
                        .text_size(px(14.0))
                        .text_color(move || p.muted()),
                );
            }
            return view().children(
                segments
                    .into_iter()
                    .map(|segment| {
                        let watch = watch.clone();
                        let (title, color) = category(&segment.category);
                        let start = segment.start;
                        row(p, title, start, || false)
                            .child(
                                view()
                                    .size(px(12.0))
                                    .rounded(px(6.0))
                                    .mt(px(5.0))
                                    .bg(rgb(color)),
                            )
                            .child(label(p, title.to_owned(), start))
                            .on_press(move || jump(&watch, start, close))
                    })
                    .collect::<Vec<_>>(),
            );
        }
        view().children(
            chapters
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, chapter)| {
                    let watch = watch.clone();
                    let selected = move || active.get() == Some(index);
                    let start = chapter.start;
                    let link = format!(
                        "https://m.youtube.com/watch?v={video}&t={}s",
                        start.floor() as u64
                    );
                    let title = chapter.title.clone();
                    row(p, &chapter.title, start, selected)
                        .child(thumbnail(p, &video, &chapter, width.get(), selected))
                        .child(label(p, chapter.title.clone(), start).child(when(
                            selected,
                            move || {
                                let title = title.clone();
                                let link = link.clone();
                                press()
                                    .size(px(44.0))
                                    .items_center()
                                    .justify_center()
                                    .label(format!("Share chapter: {title}"))
                                    .on_press(move || {
                                        let request = ShareRequest::new()
                                            .title(title.clone())
                                            .url(link.clone());
                                        let link = link.clone();
                                        spawn(async move {
                                            if share(request).await.is_err() {
                                                set_clipboard(link);
                                            }
                                        });
                                    })
                                    .child(ui::icon(SHARE, 20.0, move || p.ink()))
                            },
                        )))
                        .on_press(move || jump(&watch, start, close))
                        .on_long_press(move || preview.set(Some(chapter.clone())))
                })
                .collect::<Vec<_>>(),
        )
    });
    view()
        .absolute()
        .inset(px(0.0))
        .z_index(2)
        .child(
            press()
                .absolute()
                .inset(px(0.0))
                .bg(rgba(0x00000066))
                .label("Close chapters")
                .on_press(close),
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
                        .px(px(16.0))
                        .min_h(px(48.0))
                        .child(
                            text(move || {
                                if has_chapters.get() {
                                    "Chapters"
                                } else {
                                    "Segments"
                                }
                            })
                            .flex_1()
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || p.ink()),
                        )
                        .child(ui::icon_button(p, ui::CLOSE, "Close chapters", close)),
                )
                .child(view().h(px(0.5)).bg(move || p.raised()))
                .child(
                    scroll()
                        .element_ref(&list)
                        .max_h(percentage(0.65))
                        .child(view().py(px(8.0)).child(body)),
                ),
        )
        .child(dynamic(move || {
            let chapter = preview.get()?;
            let video = jumping.video()?;
            let watch = jumping.clone();
            Some(
                view()
                    .absolute()
                    .inset(px(0.0))
                    .items_center()
                    .justify_center()
                    .child(
                        press()
                            .absolute()
                            .inset(px(0.0))
                            .bg(rgba(0x00000066))
                            .label("Close chapter preview")
                            .on_press(move || preview.set(None)),
                    )
                    .child(
                        view()
                            .w(percentage(0.88))
                            .max_w(px(360.0))
                            .p(px(16.0))
                            .rounded(px(16.0))
                            .bg(move || p.background())
                            .child(
                                text("Chapter preview")
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .mb(px(12.0))
                                    .text_color(move || p.ink()),
                            )
                            .child(thumbnail(p, &video.video_id, &chapter, 280.0, || false))
                            .child(
                                text(chapter.title)
                                    .mt(px(12.0))
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(
                                text(timestamp(chapter.start))
                                    .mt(px(8.0))
                                    .text_size(px(14.0))
                                    .text_color(rgb(0x3ea6ff)),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .justify_end()
                                    .mt(px(8.0))
                                    .child(button("Close", move || preview.set(None)))
                                    .child(button("Play chapter", move || {
                                        jump(&watch, chapter.start, close)
                                    })),
                            ),
                    ),
            )
        }))
}

fn row(p: Palette, title: &str, start: f64, active: impl Fn() -> bool + 'static) -> View {
    press()
        .label(format!("{title}, {}", timestamp(start)))
        .flex_row()
        .gap(px(16.0))
        .min_h(px(64.0))
        .px(px(16.0))
        .py(px(10.0))
        .bg(move || if active() { p.raised() } else { p.background() })
}

fn label(p: Palette, title: String, start: f64) -> View {
    view()
        .flex_1()
        .min_w(px(0.0))
        .items_start()
        .gap(px(6.0))
        .child(
            text(title)
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
        .child(
            text(timestamp(start))
                .px(px(4.0))
                .py(px(2.0))
                .rounded(px(3.0))
                .bg(rgba(0x3ea6ff26))
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x3ea6ff)),
        )
}

fn thumbnail(
    p: Palette,
    video: &str,
    chapter: &Chapter,
    width: f32,
    active: impl Fn() -> bool + 'static,
) -> View {
    let fallback = format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg");
    let address = chapter
        .preview_url
        .clone()
        .unwrap_or_else(|| fallback.clone());
    let failed = signal(false);
    view()
        .w(px(width))
        .max_w(percentage(1.0))
        .aspect_ratio(16.0 / 9.0)
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(move || p.raised())
        .child(
            image(move || {
                ImageSrc::from(if failed.get() {
                    fallback.clone()
                } else {
                    address.clone()
                })
            })
            .absolute()
            .inset(px(0.0))
            .w_full()
            .h_full()
            .object_fit(ResizeMode::Cover)
            .pointer_events(PointerEvents::None)
            .on_error(move |_: &str| failed.set(true)),
        )
        .child(when(active, || {
            view()
                .absolute()
                .inset(px(0.0))
                .items_center()
                .justify_center()
                .bg(rgba(0x00000033))
                .pointer_events(PointerEvents::None)
                .child(
                    view()
                        .size(px(36.0))
                        .rounded(px(18.0))
                        .bg(rgba(0x00000099))
                        .items_center()
                        .justify_center()
                        .child(ui::icon(ui::PLAY, 22.0, || rgb(0xffffff).into())),
                )
        }))
}

fn button(title: &'static str, click: impl Fn() + 'static) -> View {
    press()
        .min_h(px(48.0))
        .px(px(12.0))
        .items_center()
        .justify_center()
        .on_press(click)
        .child(
            text(title)
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0x3ea6ff)),
        )
}

fn jump(watch: &WatchPlayback, position: f64, close: impl Fn()) {
    if let Some(video) = watch.video() {
        watch.seek(video.generation, position);
        watch.player().play(video.generation);
        close()
    }
}

pub fn category(value: &str) -> (&str, u32) {
    match value {
        "sponsor" => ("Sponsor", 0x00d400),
        "selfpromo" => ("Self promotion", 0xffff00),
        "interaction" => ("Interaction", 0xcc00ff),
        "intro" => ("Intro", 0x00ffff),
        "outro" => ("Outro", 0x0202ed),
        "preview" => ("Preview", 0x008fd6),
        "music_offtopic" => ("Non-music section", 0xff9900),
        "filler" => ("Filler", 0x7300ff),
        "poi_highlight" => (value, 0xff1684),
        _ => (value, 0x00ff00),
    }
}
