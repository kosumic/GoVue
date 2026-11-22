#![cfg(target_os = "android")]

extern crate nsh as _;

mod browser;

use govue_shell_runtime::{
    CaptionPage, Core, PlayerRuntime, TokenRuntime, WatchPlayback, WatchSubtitles,
};
use nsh_core::{platform::Lifecycle, prelude::*};
use serde_json::{Map, Value, json};

use browser::Browser;

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
    let browser = Browser::new(core.clone(), captions, watch.clone(), settings);
    spawn(async move {
        match core.execute(json!({"op":"media_settings"})).await {
            Ok(preferences) => settings.set(Some(preferences)),
            Err(error) => browser::report(error),
        }
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
    let theme = browser.clone();
    view()
        .flex_1()
        .p_safe_area()
        .bg(move || {
            if theme.dark() {
                rgb(0x0f0f0f)
            } else {
                rgb(0xffffff)
            }
        })
        .child(
            view()
                .relative()
                .flex_1()
                .on_layout(move |bounds: &nsh_core::Bounds<nsh_core::Points>| {
                    width.set(bounds.size.width.0)
                })
                .child(browser.view())
                .child(watch.view(move || width.get(), |_| {}))
                .child(subtitles.view(move || width.get(), || false)),
        )
        .child(signatures.view())
        .child(tokens.view())
}
