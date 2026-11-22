#![cfg(target_os = "android")]

extern crate nsh as _;

mod browser;
mod chat;
mod chat_view;
mod player_view;
mod provider_view;
mod reader_clock;
mod reader_view;
mod ui;

use govue_shell_runtime::{
    CaptionPage, Core, PlayerRuntime, TokenRuntime, TranscriptReader, WatchPlayback, WatchSubtitles,
};
use nsh_core::{platform::Lifecycle, prelude::*};
use serde_json::{Map, Value, json};

use browser::Browser;
use chat::Chat;
use chat_view::ChatView;
use player_view::PlayerControls;

::nsh::ui_sets!(vec![nsh_base::registry()]);
::nsh::main!(nsh_core::element::Root::of::<GoVue>().app_id("io.github.kaminarios.govue.nsh"));

#[component]
fn GoVue() -> impl IntoElement {
    match Core::android().and_then(|core| Ok((WatchPlayback::new(core.clone())?, core))) {
        Ok((watch, core)) => workspace(core, watch),
        Err(error) => view().p_safe_area().child(text(error.to_string())),
    }
}

fn workspace(core: Core, watch: WatchPlayback) -> View {
    let captions = CaptionPage::new(core.clone());
    let subtitles = WatchSubtitles::new(core.clone(), watch.clone());
    let signatures = PlayerRuntime::new();
    let tokens = TokenRuntime::new(core.clone());
    let settings = signal(None::<Map<String, Value>>);
    let chat = Chat::new(core.clone());
    let browser = Browser::new(
        core.clone(),
        captions,
        watch.clone(),
        settings,
        chat.clone(),
    );
    let theme = browser.clone();
    let palette = ui::Palette {
        dark: computed(move || theme.dark()),
    };
    let chat_view = ChatView::new(chat.clone(), palette);
    let chat_reader = TranscriptReader::new(core.clone());
    let chat_transcript = reader_view::ReaderView::new(chat_reader.clone(), palette);
    let watch_reader = TranscriptReader::new(core.clone());
    let (changes, mut writes) = tokio::sync::mpsc::unbounded_channel();
    let pause_setting = changes.clone();
    let clock = reader_clock::ReaderClock::new(
        core.clone(),
        watch_reader.clone(),
        watch.clone(),
        move || {
            settings.with(|settings| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.get("pauseAfterLine"))
                    == Some(&Value::Bool(true))
            })
        },
        move |enabled| {
            let _ = pause_setting.send(json!({"pauseAfterLine":enabled}));
        },
    );
    let watch_transcript =
        reader_view::ReaderView::new(watch_reader.clone(), palette).with_playback(clock);
    let controls = PlayerControls::new(
        watch.clone(),
        subtitles.clone(),
        watch_reader.clone(),
        chat,
        palette,
        move |change| {
            let _ = changes.send(change);
        },
    );
    on_back_press({
        let browser = browser.clone();
        let chat_view = chat_view.clone();
        let chat_reader = chat_reader.clone();
        let controls = controls.clone();
        let watch_reader = watch_reader.clone();
        move || {
            if chat_reader.visible() {
                chat_reader.close();
                return true;
            }
            if chat_view.back() || controls.close_menu() {
                return true;
            }
            if watch_reader.visible() {
                watch_reader.close();
                return true;
            }
            browser.back()
        }
    });
    spawn(async move {
        match core.execute(json!({"op":"media_settings"})).await {
            Ok(preferences) => settings.set(Some(preferences)),
            Err(error) => browser::report(error),
        }
        while let Some(change) = writes.recv().await {
            match core
                .execute(json!({"op":"media_settings","settings":change}))
                .await
            {
                Ok(preferences) => settings.set(Some(preferences)),
                Err(error) => browser::report(error),
            }
        }
    });
    let target = computed({
        let browser = browser.clone();
        move || {
            (
                browser.active_video(),
                settings.with(|settings| {
                    settings
                        .as_ref()
                        .and_then(|settings| settings.get("audioKey"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                }),
            )
        }
    });
    effect(move || {
        let (video, audio) = target.get();
        watch_reader.select(video.as_deref(), &audio);
    });
    effect({
        let watch = watch.clone();
        move || {
            if let Some(settings) = settings.get() {
                watch.configure(settings, lifecycle() == Lifecycle::Active);
            }
        }
    });

    let width = signal(0_f32);
    let chat_panel = chat_view.view(
        {
            let browser = browser.clone();
            move |video| browser.open(format!("https://m.youtube.com/watch?v={video}"))
        },
        move |context| {
            chat_reader.select_track(&context.video_id, &context.track_id);
            if !chat_reader.visible() {
                chat_reader.toggle();
            }
        },
    );
    let watch_bounds = watch.clone();
    let caption_controls = controls.clone();
    let navigate_back = browser.clone();
    let player_controls = controls.view(
        move || width.get(),
        move || {
            navigate_back.back();
        },
    );
    view()
        .flex_1()
        .p_safe_area()
        .bg(move || palette.background())
        .child(
            view()
                .relative()
                .flex_1()
                .on_layout(move |bounds: &nsh_core::Bounds<nsh_core::Points>| {
                    width.set(bounds.size.width.0)
                })
                .child(browser.view())
                .child(watch.view(move || width.get(), |_| {}))
                .child(subtitles.view(move || width.get(), move || caption_controls.visible()))
                .child(player_controls)
                .child(watch_transcript.view(move || {
                    let frame = watch_bounds.frame(width.get());
                    (frame.top + frame.height).max(0.0)
                }))
                .child(chat_panel)
                .child(chat_transcript.view(|| 0.0)),
        )
        .child(signatures.view())
        .child(tokens.view())
}
