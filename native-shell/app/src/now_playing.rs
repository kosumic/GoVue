use govue_shell_runtime::WatchPlayback;
use nsh_core::{PointerEvents, element::NativeEvent, platform::Lifecycle, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::queue::Queue;

#[derive(Deserialize)]
struct Control {
    action: String,
    generation: u64,
    video: String,
    position: f64,
}

pub fn now_playing(
    watch: WatchPlayback,
    queue: Queue,
    settings: Signal<Option<Map<String, Value>>>,
    pip: Signal<bool>,
    open: impl Fn(String) + Clone + 'static,
) -> NativeView {
    let controls = watch.clone();
    let next = queue.clone();
    let queue = queue.signal();
    let foreground = computed(move || lifecycle() == Lifecycle::Active);
    let can_resume = computed(move || {
        foreground.get()
            || pip.get()
            || settings.with(|settings| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.get("backgroundPlayback"))
                    == Some(&Value::Bool(true))
            })
    });
    nsh_core::element::native("govue/now-playing")
        .absolute()
        .size(px(1.0))
        .opacity(0.0)
        .pointer_events(PointerEvents::None)
        .prop("snapshot", move || {
            if watch.remote() {
                return Value::Null;
            }
            let undisplayed = || {
                if watch.queue_transition() {
                    json!({"transition":true})
                } else {
                    Value::Null
                }
            };
            let Some(video) = watch.video() else {
                return undisplayed();
            };
            let Some(status) = watch.status() else {
                return undisplayed();
            };
            let (previous, next) = queue.with(|queue| {
                (
                    !video.offline && queue.has_previous,
                    !video.offline && queue.has_next,
                )
            });
            json!({
                "generation":video.generation,
                "video":video.video_id,
                "title":video.title,
                "author":video.author,
                "position":status.position,
                "duration":status.duration,
                "rate":status.rate,
                "playing":status.playing,
                "buffering":status.state == "buffering",
                "previous":previous,
                "next":next,
                "foreground":foreground.get(),
            })
        })
        .on_native_event(move |event: &NativeEvent| {
            if event.name.as_ref() != "control" {
                return;
            }
            let Ok(action) = Control::deserialize(&event.data) else {
                return;
            };
            let Some(video) = controls.video() else {
                return;
            };
            if action.generation != video.generation || action.video != video.video_id {
                return;
            }
            match action.action.as_str() {
                "play" if can_resume.get() => controls.player().play(video.generation),
                "pause" => controls.player().pause(video.generation),
                "seek" => controls.seek(video.generation, action.position),
                "next" | "previous" if !video.offline && can_resume.get() => {
                    next.advance(&action.action, open.clone());
                }
                _ => {}
            }
        })
}
