//! A single speculative video's starting ranges, kept in memory for playback.

use super::{Source, playback, range_bytes, select_video, stream_source};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VIDEO_BYTES: u64 = 768 * 1024;
const AUDIO_BYTES: u64 = 128 * 1024;
const LIFETIME: Duration = Duration::from_secs(30);

struct Track {
    source: Arc<Source>,
    bytes: Arc<[u8]>,
}

struct Prefetched {
    at: Instant,
    tracks: Vec<Track>,
}

static PREFETCHED: Mutex<Option<Prefetched>> = Mutex::new(None);
static LOADING: AtomicBool = AtomicBool::new(false);

pub(super) fn lookup(source: &Source) -> Option<Arc<[u8]>> {
    let mut cache = PREFETCHED.lock().ok()?;
    if cache.as_ref()?.at.elapsed() >= LIFETIME {
        *cache = None;
        return None;
    }
    cache.as_ref()?.tracks.iter().find_map(|track| {
        (track.source.url == source.url
            && track.source.length == source.length
            && track.source.user_agent == source.user_agent
            && track.source.origin == source.origin)
            .then(|| track.bytes.clone())
    })
}

fn fetch(source: Arc<Source>, limit: u64, active: &impl Fn() -> bool) -> Option<Track> {
    if !active() {
        return None;
    }
    let _span = crate::trace::Section::begin(c"GoVue transport: prefetch startup range");
    let bytes = if let Some(bytes) = &source.startup {
        bytes.clone()
    } else {
        range_bytes(&source, 0, source.length.min(limit) - 1)
            .ok()?
            .into()
    };
    Some(Track { source, bytes })
}

pub(crate) fn warm(catalog: &Value, settings: &Value, active: impl Fn() -> bool + Sync) {
    if catalog["live"] == true || !active() || LOADING.swap(true, Ordering::AcqRel) {
        return;
    }
    struct Loading;
    impl Drop for Loading {
        fn drop(&mut self) {
            LOADING.store(false, Ordering::Release);
        }
    }
    let _loading = Loading;
    let height = if settings["rememberQuality"] == true {
        settings["qualityHeight"].as_u64().unwrap_or(0)
    } else {
        0
    };
    let Ok((_, mut video, requested)) = select_video(
        catalog,
        &json!({"height":height,"fps":settings["qualityFps"],"allowFallback":true}),
    ) else {
        return;
    };
    let combined = video["kind"] == "combined";
    if !combined && requested == 0 {
        let Ok(variants) = playback::adaptive_videos(catalog, video) else {
            return;
        };
        let startup_height = playback::startup_height(&variants);
        if let Some(initial) = variants.into_iter().find(|v| v["height"] == startup_height) {
            video = initial;
        }
    }
    let Ok(video) = stream_source(catalog, video, "") else {
        return;
    };
    let audio = (!combined)
        .then(|| {
            playback::audio_for(catalog, settings["audioKey"].as_str().unwrap_or(""))
                .or_else(|| playback::audio(catalog))
                .and_then(|audio| stream_source(catalog, audio, "").ok())
        })
        .flatten();
    let tracks: Vec<_> = std::thread::scope(|threads| {
        let audio = audio.map(|audio| threads.spawn(|| fetch(audio, AUDIO_BYTES, &active)));
        let video = fetch(video, VIDEO_BYTES, &active);
        video
            .into_iter()
            .chain(audio.and_then(|thread| thread.join().ok().flatten()))
            .collect()
    });
    if !tracks.is_empty()
        && active()
        && let Ok(mut cache) = PREFETCHED.lock()
    {
        *cache = Some(Prefetched {
            at: Instant::now(),
            tracks,
        });
    }
}
