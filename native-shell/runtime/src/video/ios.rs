use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use nsh_core::{AudioSession, PointerEvents, ResizeMode, prelude::*};
use serde_json::json;

use super::{EmbeddedVideoFrame, Load, PlaybackEvent, PlaybackFit, PlaybackStatus, VideoPlayer};

unsafe extern "C" {
    fn govue_ios_player_sample(
        generation: u64,
        offset: *mut f64,
        video_bitrate: *mut f64,
        audio_bitrate: *mut f64,
    );
}

#[derive(Clone, Copy, PartialEq)]
struct Status {
    ready: bool,
    buffering: bool,
    ended: bool,
    failed: bool,
    position: f64,
    duration: Option<f64>,
}

pub(super) fn surface(
    player: VideoPlayer,
    bounds: Memo<EmbeddedVideoFrame>,
    fit: Memo<PlaybackFit>,
    corner_radius: impl Fn() -> f32 + 'static,
    zoom: Memo<f32>,
    on_event: impl Fn(PlaybackEvent) + 'static,
) -> View {
    let source = player.load;
    let on_event: Rc<dyn Fn(PlaybackEvent)> = Rc::new(on_event);
    view()
        .absolute()
        .left(move || px(bounds.get().left))
        .top(move || px(bounds.get().top))
        .w(move || px(bounds.get().width))
        .h(move || px(bounds.get().height))
        .bg(rgb(0x000000))
        .rounded(move || px(corner_radius()))
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .opacity(move || {
            if bounds.get().visible {
                1.0_f32
            } else {
                0.0_f32
            }
        })
        .child(each(
            move || {
                source.with(|load| {
                    load.source
                        .is_some()
                        .then(|| load.clone())
                        .into_iter()
                        .collect::<Vec<_>>()
                })
            },
            |load| load.generation,
            move |load, _| {
                clip(
                    player.clone(),
                    load.get_untracked(),
                    bounds,
                    fit,
                    zoom,
                    on_event.clone(),
                )
            },
        ))
}

fn clip(
    player: VideoPlayer,
    load: Load,
    bounds: Memo<EmbeddedVideoFrame>,
    fit: Memo<PlaybackFit>,
    zoom: Memo<f32>,
    emit: Rc<dyn Fn(PlaybackEvent)>,
) -> Video {
    let source = load.source.expect("a mounted video has a source");
    let uri = source.uri.clone();
    let caption = source.caption_token.is_some();
    // The AVPlayer instance may still be reading this generation's loopback
    // route while the watch owner is preparing a replacement.
    cleanup(move || drop(source));
    let generation = load.generation;
    let start_position = load.position;
    let settings = player.load;
    let status = signal(Status {
        ready: false,
        buffering: true,
        ended: false,
        failed: false,
        position: start_position.unwrap_or_default(),
        duration: None,
    });
    let aspect = signal(16.0_f32 / 9.0);
    let size = signal((0.0_f64, 0.0_f64));
    let decoded = computed(move || {
        let frame = bounds.get();
        let ratio = aspect.get();
        match fit.get() {
            PlaybackFit::Width => {
                let height = frame.width / ratio;
                (0.0, (frame.height - height) / 2.0, frame.width, height)
            }
            PlaybackFit::Height => {
                let width = frame.height * ratio;
                ((frame.width - width) / 2.0, 0.0, width, frame.height)
            }
            _ => (0.0, 0.0, frame.width, frame.height),
        }
    });
    effect({
        let emit = emit.clone();
        move || {
            let (owner, requested, rate) =
                settings.with(|load| (load.generation, load.playing, load.rate));
            if owner != generation {
                return;
            }
            let status = status.get();
            let playing =
                status.ready && !status.buffering && !status.ended && !status.failed && requested;
            let state = if status.failed {
                "error"
            } else if status.ended {
                "ended"
            } else if !status.ready || (requested && status.buffering) {
                "buffering"
            } else if playing {
                "playing"
            } else {
                "paused"
            };
            let size: (f64, f64) = size.get();
            let mut live_offset = -1.0;
            let mut video_bitrate = -1.0;
            let mut audio_bitrate = -1.0;
            // SAFETY: The native playback effect runs on the UI thread. Swift only writes these out parameters during the call.
            if !caption {
                // SAFETY: This effect runs on the UI thread and Swift only writes the out parameters.
                unsafe {
                    govue_ios_player_sample(
                        generation,
                        &raw mut live_offset,
                        &raw mut video_bitrate,
                        &raw mut audio_bitrate,
                    );
                }
            }
            emit(PlaybackEvent::Status {
                generation,
                status: PlaybackStatus {
                    state: state.into(),
                    position: status.position,
                    buffered_position: status.position,
                    duration: status.duration,
                    playing,
                    rate,
                    error_code: status.failed.then(|| "PlaybackFailed".into()),
                    live_offset: (live_offset.is_finite() && live_offset >= 0.0)
                        .then_some(live_offset),
                    diagnostics: json!({
                        "sampledAt": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|time| time.as_millis() as u64),
                        "width": size.0,
                        "height": size.1,
                        "videoBitrate": video_bitrate,
                        "audioBitrate": audio_bitrate,
                    }),
                },
            });
        }
    });
    let seek = player.element.clone();
    video(uri)
        .test_id(&format!(
            "govue-{}-{generation}",
            if caption { "caption" } else { "video" }
        ))
        .element_ref(&player.element)
        .absolute()
        .left(move || px(decoded.get().0))
        .top(move || px(decoded.get().1))
        .w(move || px(decoded.get().2))
        .h(move || px(decoded.get().3))
        .scale(zoom)
        .object_fit(move || match fit.get() {
            PlaybackFit::Contain => ResizeMode::Contain,
            PlaybackFit::Cover => ResizeMode::Cover,
            _ => ResizeMode::Stretch,
        })
        .controls(false)
        .paused(move || !status.with(|status| status.ready) || !settings.with(|load| load.playing))
        .rate(move || settings.with(|load| load.rate as f32))
        .volume(player.gain)
        .audio_session(|| AudioSession::Playback)
        .on_load_start({
            let emit = emit.clone();
            move || emit(PlaybackEvent::Applied { generation })
        })
        .on_load(move |info: &VideoInfo| {
            let dimensions = info.size;
            if dimensions.width.0 > 0 && dimensions.height.0 > 0 {
                size.set((
                    f64::from(dimensions.width.0),
                    f64::from(dimensions.height.0),
                ));
                aspect.set(dimensions.width.0 as f32 / dimensions.height.0 as f32);
            }
            if let Some(position) = start_position {
                seek.seek(position);
            }
            status.update(|status| {
                status.ready = true;
                status.buffering = false;
                status.duration = duration(info.duration);
            });
        })
        .on_progress(move |progress: &VideoProgress| {
            if progress.current_time.is_finite() {
                status.update(|status| {
                    status.position = f64::from(progress.current_time.max(0.0));
                    status.duration = duration(progress.duration).or(status.duration);
                    if status.ended
                        && status
                            .duration
                            .is_some_and(|end| status.position < end - 0.25)
                    {
                        status.ended = false;
                    }
                });
            }
        })
        .on_buffer(move |buffering: &bool| status.update(|status| status.buffering = *buffering))
        .on_end(move || status.update(|status| status.ended = true))
        .on_error(move |_: &str| status.update(|status| status.failed = true))
}

fn duration(value: f32) -> Option<f64> {
    (value.is_finite() && value > 0.0).then(|| f64::from(value))
}
