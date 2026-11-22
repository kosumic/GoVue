use std::rc::Rc;

use nsh_core::{AudioSession, PointerEvents, ResizeMode, prelude::*};

use super::{EmbeddedVideoFrame, Load, PlaybackEvent, PlaybackFit, PlaybackStatus, VideoPlayer};

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
    emit: Rc<dyn Fn(PlaybackEvent)>,
) -> Video {
    let source = load.source.expect("a mounted video has a source");
    let uri = source.uri.clone();
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
                },
            });
        }
    });
    let seek = player.element.clone();
    video(uri)
        .test_id(&format!("govue-video-{generation}"))
        .element_ref(&player.element)
        .absolute()
        .left(move || px(decoded.get().0))
        .top(move || px(decoded.get().1))
        .w(move || px(decoded.get().2))
        .h(move || px(decoded.get().3))
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
            let size = info.size;
            if size.width.0 > 0 && size.height.0 > 0 {
                aspect.set(size.width.0 as f32 / size.height.0 as f32);
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
