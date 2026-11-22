#![cfg(any(target_os = "android", target_os = "ios"))]

#[cfg(target_os = "android")]
extern crate nsh as _;

mod about_view;
#[cfg(target_os = "ios")]
mod ai_ios;
mod attachment_camera;
mod attachments_view;
mod browser;
mod browser_card;
#[cfg(target_os = "ios")]
mod camera_ios;
mod chat;
mod chat_activity;
mod chat_markdown;
mod chat_read_aloud;
mod chat_view;
mod chat_viewport;
mod chat_voice;
mod download_confirm;
mod download_library;
mod downloads;
#[cfg(target_os = "ios")]
mod downloads_ios;
#[cfg(target_os = "ios")]
mod ios_browser;
#[cfg(target_os = "ios")]
mod ios_host;
#[cfg(target_os = "ios")]
mod ios_runtimes;
mod language_view;
mod mini_player;
mod now_playing;
#[cfg(target_os = "ios")]
mod now_playing_ios;
mod pip;
#[cfg(target_os = "ios")]
mod pip_ios;
mod player_gestures;
mod player_view;
mod player_window;
#[cfg(target_os = "ios")]
mod player_window_ios;
mod provider_view;
mod queue;
mod queue_view;
mod reader_clock;
mod reader_view;
mod segments_view;
mod settings_view;
#[cfg(target_os = "ios")]
mod speech_ios;
mod sponsors;
mod subtitle_style;
mod ui;

use std::rc::Rc;

use govue_shell_runtime::{
    Core, PlaybackEvent, PlayerRuntime, TokenRuntime, TranscriptReader, WatchPlayback,
    WatchSubtitles,
};
use nsh_core::{platform::Lifecycle, prelude::*};
use serde_json::{Value, json};

use browser::Browser;
use chat::Chat;
use chat_view::ChatView;
use download_library::DownloadLibrary;
use downloads::Downloads;
use player_view::PlayerControls;
use player_window::PlayerWindow;
use settings_view::{Preferences, SettingsView};

#[cfg(target_os = "android")]
::nsh::ui_sets!(vec![nsh_base::registry()]);
#[cfg(target_os = "android")]
::nsh::main!(nsh_core::element::Root::of::<GoVue>().app_id("io.github.kaminarios.govue.nsh"));

#[cfg(target_os = "ios")]
pub fn run_ios() {
    ai_ios::install();
    downloads_ios::install();
    nsh_platform_ios::set_ui_sets(vec![nsh_base::registry()]);
    nsh_platform_ios::set_components(|components| {
        camera_ios::register(components);
        speech_ios::register(components);
        ios_browser::register(components);
        downloads_ios::register(components);
        now_playing_ios::register(components);
        ios_runtimes::register(components);
        player_window_ios::register(components);
        pip_ios::register(components);
    });
    ios_host::run(nsh_core::element::Root::of::<GoVue>().app_id("io.github.kaminarios.govue.nsh"));
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
    let downloads = Downloads::new(core.clone());
    let subtitles = WatchSubtitles::new(core.clone(), watch.clone());
    let preferences = Preferences::new(core.clone());
    let settings = preferences.state;
    let settings_screen = signal(None);
    let chat = Chat::new(core.clone());
    let queue = queue::Queue::new(core.clone(), watch.clone());
    let browser = Browser::new(
        core.clone(),
        watch.clone(),
        settings,
        chat.clone(),
        queue.clone(),
        settings_screen,
        downloads.clone(),
    );
    let theme = browser.clone();
    let sponsors = sponsors::SponsorBlock::new(core.clone(), watch.clone(), settings, {
        let browser = browser.clone();
        let watch = watch.clone();
        move || {
            if watch.offline() {
                None
            } else {
                browser.active_video()
            }
        }
    });
    let palette = ui::Palette {
        dark: computed(move || theme.dark()),
    };
    let settings_view = SettingsView::new(
        preferences.clone(),
        palette,
        settings_screen,
        downloads.clone(),
        browser.clone(),
    );
    let chat_view = ChatView::new(chat.clone(), palette, watch.clone());
    let has_watch_page = computed({
        let browser = browser.clone();
        let watch = watch.clone();
        move || watch.offline() || browser.active_video().is_some()
    });
    let download_screen = downloads.destination;
    let can_present = computed({
        let chat = chat.clone();
        move || {
            has_watch_page.get()
                && !chat.visible()
                && settings_screen.with(Option::is_none)
                && download_screen.with(Option::is_none)
        }
    });
    let pip = pip::Pip::new(core.clone(), watch.clone(), queue.clone(), settings, {
        let browser = browser.clone();
        move |url| browser.open(url)
    });
    let pip_mode = pip.mode;
    let pip_window = computed(move || cfg!(target_os = "android") && pip_mode.get());
    let now_playing = now_playing::now_playing(watch.clone(), queue.clone(), settings, pip_mode, {
        let browser = browser.clone();
        move |url| browser.open(url)
    });
    let player_window = PlayerWindow::new(
        core.clone(),
        {
            let watch = watch.clone();
            move || watch.video().is_some()
        },
        move || can_present.get() && !pip_window.get(),
    );
    let library = DownloadLibrary::new(
        downloads.clone(),
        watch.clone(),
        palette,
        {
            let window = player_window.clone();
            move || window.set(true)
        },
        {
            let browser = browser.clone();
            move |url| browser.open(url)
        },
    );
    let back_from_offline = Rc::new({
        let library = downloads.clone();
        let watch = watch.clone();
        let window = player_window.clone();
        move || {
            if watch.offline() {
                watch.close();
                window.set(false);
                library.open();
                true
            } else {
                false
            }
        }
    });
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
        queue.clone(),
        preferences.clone(),
    );
    on_back_press({
        let browser = browser.clone();
        let chat_view = chat_view.clone();
        let chat_reader = chat_reader.clone();
        let controls = controls.clone();
        let watch_reader = watch_reader.clone();
        let window = player_window.clone();
        let downloads = downloads.clone();
        let offline = back_from_offline.clone();
        move || {
            if downloads.back() || browser.close_card() {
                return true;
            }
            if let Some(screen) = settings_screen.get_untracked() {
                settings_screen.set(screen.back());
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
            offline() || window.close() || browser.back()
        }
    });
    let target = computed({
        let browser = browser.clone();
        let watch = watch.clone();
        move || {
            (
                watch
                    .video()
                    .filter(|video| video.offline)
                    .map(|video| video.video_id)
                    .or_else(|| browser.active_video()),
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
    effect({
        let watch = watch.clone();
        move || {
            if let Some(settings) = settings.get() {
                let phase = lifecycle();
                let pip = pip_mode.get();
                let background = settings.get("backgroundPlayback") == Some(&Value::Bool(true));
                watch.configure(settings, phase == Lifecycle::Active || pip);
                if phase == Lifecycle::Background
                    && !pip
                    && !background
                    && let Some(video) = watch.video()
                {
                    let watch = watch.clone();
                    spawn(async move {
                        timer(std::time::Duration::from_millis(350)).await;
                        if lifecycle() == Lifecycle::Background && !pip_mode.get_untracked() {
                            watch.player().pause(video.generation);
                        }
                    });
                }
            }
        }
    });

    let width = signal(0_f32);
    let height = signal(0_f32);
    let mini_active = computed({
        let browser = browser.clone();
        move || browser.mini_active() && !pip_window.get()
    });
    let mini = mini_player::MiniPlayer::new(core.clone(), mini_active, width, height);
    let fullscreen = player_window.presented;
    let immersive = computed(move || fullscreen.get() || pip_window.get());
    let beside = computed(move || fullscreen.get() && width.get() > height.get());
    effect({
        let watch = watch.clone();
        let window = player_window.clone();
        let mini = mini.clone();
        move || {
            if pip_window.get() {
                watch_reader.close();
                watch.present(Some(govue_shell_runtime::EmbeddedVideoFrame {
                    left: 0.0,
                    top: 0.0,
                    width: width.get().max(1.0),
                    height: height.get().max(1.0),
                    visible: true,
                }));
            } else if let Some(frame) = mini.frame() {
                watch_reader.close();
                watch.present(Some(frame));
            } else {
                watch.present(window.frame(width.get(), height.get(), watch_reader.visible()));
            }
        }
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
    let video_zoom = controls.zoom();
    let sponsor_ticks = sponsors.clone();
    let navigate_back = browser.clone();
    let navigate_video = browser.clone();
    let player_controls = controls.view(
        move || (width.get(), mini_active.get()),
        move || {
            if !back_from_offline() {
                navigate_back.back();
            }
        },
        move |url| navigate_video.open(url),
        player_window.clone(),
        pip.clone(),
        downloads.clone(),
    );
    let download_notice = downloads.notice;
    let download_export = downloads.export_view();
    let mini_controls = mini.view(watch.clone(), browser.clone(), queue, controls.clone());
    let root = view()
        .flex_1()
        .pt(move || {
            px(if immersive.get() {
                0.0
            } else {
                safe_area().top.0
            })
        })
        .pb(move || {
            px(if immersive.get() {
                0.0
            } else {
                safe_area().bottom.0
            })
        })
        .pl(move || {
            px(if immersive.get() {
                0.0
            } else {
                safe_area().left.0
            })
        })
        .pr(move || {
            px(if immersive.get() {
                0.0
            } else {
                safe_area().right.0
            })
        })
        .bg(move || palette.background())
        .child(download_export)
        .child(
            view()
                .relative()
                .flex_1()
                .on_layout(move |bounds: &nsh_core::Bounds<nsh_core::Points>| {
                    width.set(bounds.size.width.0);
                    height.set(bounds.size.height.0);
                })
                .bg(move || {
                    if immersive.get() {
                        rgb(0x000000).into()
                    } else {
                        palette.background()
                    }
                })
                .child(browser.view(immersive))
                .child(
                    view()
                        .absolute()
                        .inset(px(0.0))
                        .pointer_events(nsh_core::PointerEvents::None)
                        .z_index(move || i32::from(mini_active.get()))
                        .child(watch.view(
                            move || width.get(),
                            move || if mini_active.get() { 16.0 } else { 0.0 },
                            move || {
                                if mini_active.get() || pip_mode.get() {
                                    1.0
                                } else {
                                    video_zoom.get()
                                }
                            },
                            move |event| {
                                let next_video = next_video.clone();
                                playback_controls.playback(&event, move |url| next_video.open(url));
                                if matches!(event, PlaybackEvent::Status { .. }) {
                                    sponsor_ticks.sample();
                                }
                            },
                        )),
                )
                .child(subtitles.view(
                    move || width.get(),
                    move || !pip_mode.get() && !mini_active.get() && caption_controls.visible(),
                ))
                .child(player_controls.z_index(move || if mini_active.get() { 3 } else { 0 }))
                .child(sponsors.view(
                    move || width.get(),
                    move || !pip_mode.get() && !mini_active.get() && !sponsor_controls.menu_open(),
                ))
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
                .child(mini_controls)
                .child(settings_view.view())
                .child(library.view())
                .child(browser.card_view())
                .child(download_confirm::download_confirm(
                    palette, downloads, width, height,
                ))
                .child(when(
                    move || download_notice.with(Option::is_some),
                    move || {
                        view()
                            .absolute()
                            .left(px(14.0))
                            .right(px(14.0))
                            .bottom(px(20.0))
                            .rounded(px(10.0))
                            .bg(rgba(0x272727f5))
                            .px(px(12.0))
                            .py(px(8.0))
                            .pointer_events(nsh_core::PointerEvents::None)
                            .child(
                                text(move || download_notice.get().unwrap_or_default())
                                    .text_size(px(13.0))
                                    .text_color(rgb(0xffffff)),
                            )
                    },
                ))
                .child(browser.toast())
                .child(pip.notice()),
        );
    #[cfg(target_os = "android")]
    let root = root.child(browser.launches());
    let root = root
        .child(now_playing)
        .child(player_window.view())
        .child(pip.view(
            move || watch.frame(width.get()),
            move || can_present.get() || mini_active.get(),
        ));
    root.child(chat_view.camera())
        .child(chat_view.speech())
        .child(chat_view.read_aloud())
        .child(PlayerRuntime::new().view())
        .child(TokenRuntime::new(core).view())
}
