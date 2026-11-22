use std::time::Duration;

use govue_shell_runtime::{Core, PlaybackSelection, PlaybackStatus, WatchPlayback};
#[cfg(target_os = "android")]
use nsh_core::element::NativeEvent;
use nsh_core::{
    PointerEvents,
    element::{ElementRef, share},
    platform::Lifecycle,
    prelude::*,
    share::ShareRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::{
    downloads::alert,
    ui::{self, Palette, press},
};

pub const CAST: &str = "M2 8V5h20v14h-7M2 12a7 7 0 0 1 7 7m-7-3a3 3 0 0 1 3 3m-3 0h.01";
const CAST_CONNECTED: &str =
    "M2 8V5h20v14h-7m-3-8h7v5h-5M2 12a7 7 0 0 1 7 7m-7-3a3 3 0 0 1 3 3m-3 0h.01";

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Snapshot {
    available: bool,
    id: Option<String>,
    video_id: Option<String>,
    device: Option<String>,
    device_id: Option<String>,
    active: bool,
    acknowledged: bool,
    link: bool,
    link_connected: bool,
    url: Option<String>,
    phase: String,
    position: Option<f64>,
    duration: Option<f64>,
    speed: Option<f64>,
    error: Option<String>,
}

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Discovery {
    available: bool,
    error: Option<String>,
    transport_available: bool,
    devices: Vec<Device>,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
struct Device {
    id: String,
    name: String,
    host: String,
    port: u16,
}

#[derive(Clone, PartialEq)]
struct LocalResume {
    generation: u64,
    video_id: String,
    position: f64,
    playing: bool,
}

struct Control {
    id: String,
    generation: u64,
    request: Value,
}

#[derive(Clone)]
pub struct CastView {
    core: Core,
    watch: WatchPlayback,
    platform: ElementRef,
    state: Signal<Snapshot>,
    discovery: Signal<Discovery>,
    open: Signal<bool>,
    pending: Signal<bool>,
    share_pending: Signal<bool>,
    revision: Signal<u64>,
    restore: Signal<Option<LocalResume>>,
    error: Signal<Option<String>>,
}

impl CastView {
    pub fn new(core: Core, watch: WatchPlayback) -> Self {
        let cast = Self {
            core,
            watch,
            platform: element_ref(),
            state: signal(Snapshot::default()),
            discovery: signal(Discovery::default()),
            open: signal(false),
            pending: signal(false),
            share_pending: signal(false),
            revision: signal(0),
            restore: signal(None),
            error: signal(None),
        };
        let (send, receive) = mpsc::unbounded_channel();
        let state = cast.state;
        let restore = cast.restore;
        cast.watch
            .player()
            .remote_commands(move |generation, action, value| {
                let current = state.get_untracked();
                let selected = restore.get_untracked();
                let Some(id) = current
                    .id
                    .filter(|_| selected.is_some_and(|owner| owner.generation == generation))
                else {
                    return false;
                };
                if !current.active || current.link {
                    return false;
                }
                if !current.acknowledged {
                    return true;
                }
                let (action, field) = match action {
                    "rate" => ("speed", "speed"),
                    "seek" => ("seek", "position"),
                    action => (action, ""),
                };
                let mut request = json!({"op":"cast_control","id":id,"action":action});
                if !field.is_empty() {
                    request[field] = value.clone();
                }
                let _ = send.send(Control {
                    generation,
                    id,
                    request,
                });
                true
            });
        spawn(cast.clone().control_loop(receive));
        spawn(cast.clone().poll());
        let video = computed({
            let watch = cast.watch.clone();
            move || {
                watch
                    .video()
                    .filter(|video| !video.offline)
                    .map(|video| video.video_id)
            }
        });
        effect({
            let cast = cast.clone();
            move || cast.sync(video.get())
        });
        cast
    }

    fn advance(&self) -> u64 {
        let revision = self.revision.get_untracked() + 1;
        self.revision.set(revision);
        revision
    }

    fn sync(&self, video: Option<String>) {
        let revision = self.advance();
        let cast = self.clone();
        spawn(async move {
            if let Ok(snapshot) = cast
                .core
                .execute(json!({"op":"cast_sync","videoId":video}))
                .await
                && cast.revision.get_untracked() == revision
            {
                cast.apply(snapshot);
            }
        });
    }

    async fn poll(self) {
        loop {
            if self.open.get_untracked() && lifecycle() == Lifecycle::Active {
                self.platform.command("snapshot", Value::Null);
            }
            if !self.pending.get_untracked()
                && (self.open.get_untracked() || self.state.with_untracked(|state| state.active))
            {
                let revision = self.revision.get_untracked();
                if let Ok(snapshot) = self.core.execute(json!({"op":"cast_status"})).await
                    && self.revision.get_untracked() == revision
                {
                    self.apply(snapshot);
                }
            }
            timer(Duration::from_millis(1000)).await;
        }
    }

    async fn control_loop(self, mut receive: mpsc::UnboundedReceiver<Control>) {
        while let Some(action) = receive.recv().await {
            let now = self.restore.get_untracked();
            if self.pending.get_untracked()
                || !now.is_some_and(|owner| owner.generation == action.generation)
                || !self
                    .state
                    .with_untracked(|state| state.id.as_deref() == Some(&action.id))
            {
                continue;
            }
            let revision = self.advance();
            let result = self.core.execute(action.request).await;
            if self.revision.get_untracked() == revision {
                match result {
                    Ok(snapshot) => self.apply(snapshot),
                    Err(error) => self.error.set(Some(error.to_string())),
                }
            }
        }
    }

    fn apply(&self, next: Snapshot) {
        let old = self.restore.get_untracked();
        let remote = self.watch.video().filter(|video| {
            next.active
                && !next.link
                && !video.offline
                && next.video_id.as_deref() == Some(&video.video_id)
                && (next.acknowledged
                    || old
                        .as_ref()
                        .is_some_and(|old| old.video_id == video.video_id))
        });
        if let Some(video) = remote {
            let position = next.position.unwrap_or(0.0);
            let playing = matches!(next.phase.as_str(), "playing" | "buffering");
            if old
                .as_ref()
                .is_none_or(|old| old.generation != video.generation)
            {
                self.watch.player().suspend_local(video.generation);
            }
            self.restore.set(Some(LocalResume {
                generation: video.generation,
                video_id: video.video_id,
                position,
                playing,
            }));
            self.watch.set_remote_status(
                video.generation,
                Some(PlaybackStatus {
                    state: next.phase.clone(),
                    position,
                    buffered_position: position,
                    duration: next.duration,
                    playing,
                    rate: next.speed.unwrap_or(1.0),
                    error_code: None,
                    live_offset: None,
                    diagnostics: Value::Null,
                }),
            );
        } else if let Some(old) = old {
            self.restore.set(None);
            self.watch.set_remote_status(old.generation, None);
            if self.watch.video().is_some_and(|video| {
                video.generation == old.generation && video.video_id == old.video_id
            }) {
                self.watch
                    .seek(old.generation, next.position.unwrap_or(old.position));
                if old.playing {
                    self.watch.player().play(old.generation);
                }
            }
        }
        if let Some(error) = &next.error
            && !self
                .state
                .with_untracked(|state| state.error.as_ref() == Some(error))
        {
            self.share_pending.set(false);
            self.error.set(Some(error.clone()));
        }
        if next.active
            && next.link
            && self.open.get_untracked()
            && self.share_pending.get_untracked()
            && let Some(url) = next.url.clone()
        {
            self.share_pending.set(false);
            share_link(url);
        }
        self.state.set(next);
    }

    pub fn show(&self) {
        self.error.set(None);
        self.discovery.set(Discovery::default());
        self.open.set(true);
    }

    pub fn close(&self) -> bool {
        let open = self.open.get_untracked();
        self.open.set(false);
        self.share_pending.set(false);
        open
    }

    fn start(&self, device: Option<Device>) {
        if self.pending.get_untracked() {
            return;
        }
        if device.is_none()
            && let Some(link) = self
                .state
                .with_untracked(|state| state.url.clone().filter(|_| state.active && state.link))
        {
            share_link(link);
            return;
        }
        let Some(video) = self.watch.video().filter(|video| !video.offline) else {
            return;
        };
        let Some((catalog, user_agent)) = self.watch.cast_catalog(video.generation) else {
            self.error.set(Some(
                "Wait for the video to load before starting Cast.".into(),
            ));
            return;
        };
        let position = self.watch.status().map_or(0.0, |status| status.position);
        let rate = self.watch.status().map_or(1.0, |status| status.rate);
        let link = device.is_none();
        let revision = self.advance();
        self.pending.set(true);
        self.share_pending.set(link);
        self.error.set(None);
        let cast = self.clone();
        spawn(async move {
            #[cfg(target_os = "android")]
            let _ = nsh_core::element::request_notification_permission().await;
            if cast.revision.get_untracked() != revision || !cast.open.get_untracked() {
                cast.pending.set(false);
                return;
            }
            let command = json!({"op":"cast_start","catalog":&*catalog,"device":device,"link":link,
                "userAgent":user_agent,"position":position,"speed":rate,"height":video.quality_height,
                "fps":video.quality_fps,"audioKey":video.audio_key});
            let result: Result<Snapshot, _> = cast.core.execute(command).await;
            cast.pending.set(false);
            if cast.revision.get_untracked() != revision {
                if let Ok(snapshot) = result
                    && let Some(id) = snapshot.id
                {
                    let _ = cast
                        .core
                        .execute::<Value>(json!({"op":"cast_stop","id":id}))
                        .await;
                }
                return;
            }
            match result {
                Ok(snapshot) => cast.apply(snapshot),
                Err(error) => {
                    cast.share_pending.set(false);
                    cast.error.set(Some(error.to_string()));
                }
            }
        });
    }

    fn stop(&self) {
        if self.pending.get_untracked() {
            return;
        }
        let Some(id) = self.state.with_untracked(|state| state.id.clone()) else {
            return;
        };
        let revision = self.advance();
        self.pending.set(true);
        let cast = self.clone();
        spawn(async move {
            let result = cast.core.execute(json!({"op":"cast_stop","id":id})).await;
            cast.pending.set(false);
            if cast.revision.get_untracked() == revision {
                match result {
                    Ok(snapshot) => cast.apply(snapshot),
                    Err(error) => cast.error.set(Some(error.to_string())),
                }
            }
        });
    }

    pub fn replace_stream(
        &self,
        selection: &PlaybackSelection,
        accepted: impl FnOnce(Result<(), String>) + 'static,
    ) {
        let state = self.state.get_untracked();
        if self.pending.get_untracked() || !state.acknowledged {
            accepted(Err("The Cast receiver is still loading.".into()));
            return;
        }
        let Some(video) = self.watch.video() else {
            return;
        };
        let Some((catalog, agent)) = self.watch.cast_catalog(video.generation) else {
            accepted(Err("This video is no longer available to cast.".into()));
            return;
        };
        let (height, fps, audio) = match selection {
            PlaybackSelection::Quality { height, fps } => (*height, *fps, video.audio_key),
            PlaybackSelection::Audio(key) => (video.quality_height, video.quality_fps, key.clone()),
        };
        let request = json!({"op":"cast_replace","id":state.id,"catalog":&*catalog,"userAgent":agent,
            "position":state.position.unwrap_or_default(),"speed":state.speed.unwrap_or(1.0),
            "height":height,"fps":fps,"audioKey":audio});
        let revision = self.advance();
        self.pending.set(true);
        let cast = self.clone();
        spawn(async move {
            let result: Result<Snapshot, _> = cast.core.execute(request).await;
            cast.pending.set(false);
            if cast.revision.get_untracked() == revision {
                match result {
                    Ok(snapshot) => {
                        cast.apply(snapshot);
                        cast.watch
                            .cast_selection(video.generation, height, fps, audio);
                        accepted(Ok(()));
                    }
                    Err(error) => accepted(Err(error.to_string())),
                }
            } else if let Ok(snapshot) = result
                && let Some(id) = snapshot.id
            {
                let _ = cast
                    .core
                    .execute::<Value>(json!({"op":"cast_stop","id":id}))
                    .await;
            }
        });
    }

    #[cfg(target_os = "android")]
    pub fn discovery(&self) -> NativeView {
        let cast = self.clone();
        let open = self.open;
        nsh_core::element::native("govue/cast-discovery")
            .element_ref(&self.platform)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .prop("enabled", move || {
                json!(open.get() && lifecycle() == Lifecycle::Active)
            })
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "devices"
                    && open.get_untracked()
                    && let Some(raw) = event.data.as_str().filter(|raw| raw.len() < 100_000)
                {
                    let request = json!({"op":"player_cast_devices","raw":raw});
                    let core = cast.core.clone();
                    let state = cast.discovery;
                    spawn(async move {
                        if let Ok(devices) = core.execute(request).await
                            && open.get_untracked()
                        {
                            state.set(devices);
                        }
                    });
                }
            })
    }

    pub fn panel(&self, p: Palette) -> If {
        let cast = self.clone();
        let open = self.open;
        when(open, move || cast.dialog(p))
    }

    fn dialog(&self, p: Palette) -> View {
        let close = self.clone();
        let backdrop = self.clone();
        let state = self.state;
        let discovery = self.discovery;
        let pending = self.pending;
        let error = self.error;
        let start = self.clone();
        let stop = self.clone();
        let casting = computed(move || state.with(|state| state.active && !state.link));
        let busy = computed(move || {
            pending.get()
                || state.with(|state| {
                    state.active
                        && matches!(state.phase.as_str(), "preparing" | "connecting" | "loading")
                })
        });
        view().absolute().inset(px(0.0)).items_center().justify_center().z_index(7)
            .child(press().absolute().inset(px(0.0)).bg(rgba(0x00000099)).label("Dismiss Cast")
                .on_press(move || { backdrop.close(); }))
            .child(view().w(px(360.0)).max_w(percentage(0.9)).max_h(percentage(0.87))
                .rounded(px(12.0)).bg(move || p.raised()).px(px(24.0)).pt(px(24.0)).pb(px(12.0)).gap(px(12.0))
                .child(text("Cast").text_size(px(24.0)).font_weight(FontWeight::MEDIUM).text_color(move || p.ink()))
                .child(scroll().flex_shrink(1.0).child(view().gap(px(10.0))
                    .child(when(casting, move || {
                        let stop = stop.clone();
                        view().gap(px(4.0))
                            .child(text(move || state.with(|state| format!("{} {}", if state.acknowledged { "Casting to" } else { "Connecting to" }, state.device.as_deref().unwrap_or("device"))))
                                .text_size(px(15.0)).text_color(move || p.ink()))
                            .child(cast_action(p, "Stop casting", move || pending.get(), move || stop.stop()))
                    }))
                    .child(text("Available devices").font_weight(FontWeight::MEDIUM).text_size(px(14.0)).text_color(move || p.ink()))
                    .child(each(move || discovery.with(|state| state.devices.clone()), |device| device.id.clone(), move |device, _| {
                        let cast = start.clone();
                        let selected = computed(move || casting.get() && state.with(|state| device.with(|device| state.device_id.as_deref() == Some(&device.id))));
                        let disabled = computed(move || busy.get() || !discovery.with(|state| state.transport_available));
                        press().h(px(48.0)).items_center().flex_row().gap(px(10.0)).disabled(disabled)
                            .label(move || device.with(|device| device.name.clone())).on_press(move || cast.start(Some(device.get_untracked())))
                            .child(dynamic(move || ui::icon(if selected.get() { CAST_CONNECTED } else { CAST }, 22.0, move || p.muted())))
                            .child(text(move || device.with(|device| device.name.clone())).text_size(px(14.0)).text_color(move || p.ink()))
                    }))
                    .child(when(move || discovery.with(|state| state.devices.is_empty()), move || {
                        view().gap(px(4.0))
                            .child(text(move || if discovery.with(|state| state.available) { "No Cast devices found" } else { "Cast unavailable" })
                                .text_size(px(14.0)).text_color(move || p.ink()))
                            .child(text(move || discovery.with(|state| state.error.clone()).unwrap_or_else(|| "Make sure casting is set up on the same network.".into()))
                                .text_size(px(13.0)).text_color(move || p.muted()))
                            .child(cast_action(p, "Learn about casting", || false, || { spawn(async { open_url("https://support.google.com/googlecast/answer/6102923").await; }); }))
                    }))
                    .child(self.link_options(p, casting, busy))
                    .child(when(move || error.with(Option::is_some), move || {
                        text(move || error.get().unwrap_or_default()).text_size(px(13.0)).text_color(rgb(0xe63939))
                    }))
                ))
                .child(cast_action(p, "Close", || false, move || { close.close(); })))
    }

    fn link_options(&self, p: Palette, casting: Memo<bool>, busy: Memo<bool>) -> If {
        let cast = self.clone();
        let state = self.state;
        when(
            move || !casting.get(),
            move || {
                let start = cast.clone();
                let stop = cast.clone();
                view()
                    .pt(px(12.0))
                    .border_t(px(0.5))
                    .border_color(move || p.muted())
                    .gap(px(5.0))
                    .child(
                        text("Open on any device")
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(14.0))
                            .text_color(move || p.ink()),
                    )
                    .child(
                        text("On the same Wi-Fi network.")
                            .text_size(px(13.0))
                            .text_color(move || p.muted()),
                    )
                    .child(dynamic(move || {
                        state.with(|state| state.url.clone()).map(|url| {
                            text(url)
                                .selectable(true)
                                .number_of_lines(3)
                                .text_size(px(13.0))
                                .text_color(move || p.muted())
                        })
                    }))
                    .child(when(
                        move || state.with(|state| state.link_connected),
                        move || {
                            text("Device connected")
                                .text_size(px(13.0))
                                .text_color(move || p.ink())
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(8.0))
                            .child(cast_action(
                                p,
                                "Share casting link",
                                move || busy.get() || !state.with(|state| state.available),
                                move || start.start(None),
                            ))
                            .child(when(
                                move || state.with(|state| state.active && state.link),
                                move || {
                                    let stop = stop.clone();
                                    cast_action(
                                        p,
                                        "Close link",
                                        move || busy.get(),
                                        move || stop.stop(),
                                    )
                                },
                            )),
                    )
            },
        )
    }

    pub fn overlay(&self, width: Signal<f32>) -> If {
        let watch = self.watch.clone();
        let frame = computed(move || watch.frame(width.get()));
        let state = self.state;
        let watch = self.watch.clone();
        when(
            move || watch.remote() && frame.get().visible,
            move || {
                view()
                    .absolute()
                    .left(move || px(frame.get().left))
                    .top(move || px(frame.get().top))
                    .w(move || px(frame.get().width))
                    .h(move || px(frame.get().height))
                    .bg(rgb(0x000000))
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .pointer_events(PointerEvents::None)
                    .child(ui::icon(CAST_CONNECTED, 36.0, || rgb(0xffffff).into()))
                    .child(
                        text(move || {
                            state.with(|state| {
                                format!(
                                    "Casting to {}",
                                    state.device.as_deref().unwrap_or("device")
                                )
                            })
                        })
                        .text_size(px(15.0))
                        .text_color(rgb(0xffffff)),
                    )
            },
        )
    }
}

fn share_link(url: String) {
    spawn(async move {
        if let Err(error) = share(ShareRequest::new().title("GoVue").text(url)).await {
            alert(error.message).await;
        }
    });
}

fn cast_action(
    p: Palette,
    title: &'static str,
    disabled: impl Fn() -> bool + 'static,
    action: impl Fn() + 'static,
) -> View {
    let disabled = computed(disabled);
    press()
        .min_h(px(44.0))
        .self_start()
        .justify_center()
        .px(px(8.0))
        .label(title)
        .disabled(disabled)
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0 })
        .on_press(action)
        .child(
            text(title)
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || p.ink()),
        )
}
