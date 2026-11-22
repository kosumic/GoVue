#![cfg(any(target_os = "android", target_os = "ios"))]

#[cfg(target_os = "android")]
extern crate nsh as _;

mod browser;
mod chat;
mod chat_view;
#[cfg(target_os = "ios")]
mod ios_browser;
mod player_view;
mod player_window;
mod provider_view;
mod queue;
mod queue_view;
mod reader_clock;
mod reader_view;
mod settings_view;
mod sponsors;
mod ui;

use govue_shell_runtime::{
    CaptionPage, Core, PlaybackEvent, TranscriptReader, WatchPlayback, WatchSubtitles,
};
#[cfg(target_os = "android")]
use govue_shell_runtime::{PlayerRuntime, TokenRuntime};
#[cfg(target_os = "ios")]
use nsh_core::prelude::*;
#[cfg(target_os = "android")]
use nsh_core::{platform::Lifecycle, prelude::*};
use serde_json::{Value, json};

use browser::Browser;
use chat::Chat;
use chat_view::ChatView;
use player_view::PlayerControls;
use player_window::PlayerWindow;
use settings_view::{Preferences, SettingsView};

#[cfg(target_os = "android")]
::nsh::ui_sets!(vec![nsh_base::registry()]);
#[cfg(target_os = "android")]
::nsh::main!(nsh_core::element::Root::of::<GoVue>().app_id("io.github.kaminarios.govue.nsh"));

#[cfg(target_os = "ios")]
pub fn run_ios() {
    nsh_platform_ios::set_ui_sets(vec![nsh_base::registry()]);
    nsh_platform_ios::set_components(ios_browser::register);
    nsh_ios_app::run_rust(
        nsh_core::element::Root::of::<GoVue>().app_id("io.github.kaminarios.govue.nsh"),
    );
}

#[component]
fn GoVue() -> impl IntoElement {
    #[cfg(target_os = "android")]
    let core = Core::android();
    #[cfg(target_os = "ios")]
    let core = Core::ios();
    match core.and_then(|core| Ok((WatchPlayback::new(core.clone())?, core))) {
        Ok((watch, core)) => workspace(core, watch),
        Err(error) => view().p_safe_area().child(text(error.to_string())),
    }
}

fn workspace(core: Core, watch: WatchPlayback) -> View {
    let captions = CaptionPage::new(core.clone());
    let subtitles = WatchSubtitles::new(core.clone(), watch.clone());
    let preferences = Preferences::new(core.clone());
    let settings = preferences.state;
    let settings_screen = signal(None);
    let chat = Chat::new(core.clone());
    let queue = queue::Queue::new(core.clone(), watch.clone());
    let browser = Browser::new(
        core.clone(),
        captions,
        watch.clone(),
        settings,
        chat.clone(),
        queue.clone(),
        settings_screen,
    );
    let theme = browser.clone();
    let sponsors = sponsors::SponsorBlock::new(core.clone(), watch.clone(), settings, {
        let browser = browser.clone();
        move || browser.active_video()
    });
    let palette = ui::Palette {
        dark: computed(move || theme.dark()),
    };
    let settings_view = SettingsView::new(preferences.clone(), palette, settings_screen);
    let chat_view = ChatView::new(chat.clone(), palette);
    let has_watch_page = computed({
        let browser = browser.clone();
        move || browser.active_video().is_some()
    });
    let player_window = PlayerWindow::new(
        core.clone(),
        {
            let watch = watch.clone();
            move || watch.video().is_some()
        },
        {
            let chat = chat.clone();
            move || has_watch_page.get() && !chat.visible() && settings_screen.with(Option::is_none)
        },
    );
    effect({
        let window = player_window.clone();
        move || {
            if !has_watch_page.get() {
                window.close();
            }
        }
    });
    let chat_reader = TranscriptReader::new(core.clone());
    let chat_transcript = reader_view::ReaderView::new(chat_reader.clone(), palette);
    let watch_reader = TranscriptReader::new(core.clone());
    let pause_setting = preferences.clone();
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
            pause_setting.update(json!({"pauseAfterLine":enabled}));
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
        queue,
        move |change| preferences.update(change),
    );
    on_back_press({
        let browser = browser.clone();
        let chat_view = chat_view.clone();
        let chat_reader = chat_reader.clone();
        let controls = controls.clone();
        let watch_reader = watch_reader.clone();
        let window = player_window.clone();
        move || {
            if settings_screen.with_untracked(Option::is_some) {
                settings_screen.set(None);
                return true;
            }
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
            window.close() || browser.back()
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
    effect({
        let reader = watch_reader.clone();
        move || {
            let (video, audio) = target.get();
            reader.select(video.as_deref(), &audio);
        }
    });
    #[cfg(target_os = "android")]
    effect({
        let watch = watch.clone();
        move || {
            if let Some(settings) = settings.get() {
                watch.configure(settings, lifecycle() == Lifecycle::Active);
            }
        }
    });

    let width = signal(0_f32);
    let height = signal(0_f32);
    let fullscreen = player_window.presented;
    let beside = computed(move || fullscreen.get() && width.get() > height.get());
    effect({
        let watch = watch.clone();
        let window = player_window.clone();
        move || watch.present(window.frame(width.get(), height.get(), watch_reader.visible()))
    });
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
    let playback_controls = controls.clone();
    let next_video = browser.clone();
    let caption_controls = controls.clone();
    let sponsor_controls = controls.clone();
    let sponsor_ticks = sponsors.clone();
    let navigate_back = browser.clone();
    let navigate_video = browser.clone();
    let player_controls = controls.view(
        move || width.get(),
        move || {
            navigate_back.back();
        },
        move |url| navigate_video.open(url),
        player_window.clone(),
    );
    let root = view()
        .flex_1()
        .pt(move || {
            px(if fullscreen.get() {
                0.0
            } else {
                safe_area().top.0
            })
        })
        .pb(move || {
            px(if fullscreen.get() {
                0.0
            } else {
                safe_area().bottom.0
            })
        })
        .pl(move || {
            px(if fullscreen.get() {
                0.0
            } else {
                safe_area().left.0
            })
        })
        .pr(move || {
            px(if fullscreen.get() {
                0.0
            } else {
                safe_area().right.0
            })
        })
        .bg(move || palette.background())
        .child(
            view()
                .relative()
                .flex_1()
                .on_layout(move |bounds: &nsh_core::Bounds<nsh_core::Points>| {
                    width.set(bounds.size.width.0);
                    height.set(bounds.size.height.0);
                })
                .bg(move || {
                    if fullscreen.get() {
                        rgb(0x000000).into()
                    } else {
                        palette.background()
                    }
                })
                .child(browser.view(fullscreen))
                .children(cfg!(target_os = "android").then(|| {
                    watch.view(
                        move || width.get(),
                        move |event| {
                            let next_video = next_video.clone();
                            playback_controls.playback(&event, move |url| next_video.open(url));
                            if matches!(event, PlaybackEvent::Status { .. }) {
                                sponsor_ticks.sample();
                            }
                        },
                    )
                }))
                .children(cfg!(target_os = "android").then(|| {
                    subtitles.view(move || width.get(), move || caption_controls.visible())
                }))
                .child(player_controls)
                .child(sponsors.view(move || width.get(), move || !sponsor_controls.menu_open()))
                .child(watch_transcript.view(
                    move || {
                        if beside.get() {
                            0.0
                        } else {
                            let frame = watch_bounds.frame(width.get());
                            (frame.top + frame.height).max(0.0)
                        }
                    },
                    move || {
                        if beside.get() {
                            width.get() - (width.get() * 0.4).min(400.0)
                        } else {
                            0.0
                        }
                    },
                ))
                .child(controls.error_view())
                .child(chat_panel)
                .child(chat_transcript.view(|| 0.0, || 0.0))
                .child(settings_view.view())
                .child(browser.toast()),
        );
    #[cfg(target_os = "android")]
    let root = root
        .child(browser.launches())
        .child(player_window.view())
        .child(PlayerRuntime::new().view())
        .child(TokenRuntime::new(core).view());
    root
}
