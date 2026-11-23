//! A single speculative video's starting ranges, kept in memory for playback.

use super::{Source, playback, range_bytes, select_video, stream_source};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod webm;

const VIDEO_BYTES: u64 = 768 * 1024;
const AUDIO_BYTES: u64 = 128 * 1024;
const LIFETIME: Duration = Duration::from_secs(30);

struct Track {
    source: Arc<Source>,
    ranges: Arc<[Range]>,
}

pub(super) struct Range {
    pub(super) start: u64,
    pub(super) bytes: Vec<u8>,
}

impl Range {
    pub(super) fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }
}

struct Prefetched {
    at: Instant,
    root: String,
    video_id: String,
    complete: bool,
    tracks: Vec<Track>,
}

impl Prefetched {
    fn matches(&self, root: &str, video_id: &str) -> bool {
        self.at.elapsed() < LIFETIME && self.root == root && self.video_id == video_id
    }
}

static PREFETCHED: Mutex<Option<Prefetched>> = Mutex::new(None);
static LOADING: AtomicBool = AtomicBool::new(false);

struct Loading;

impl Drop for Loading {
    fn drop(&mut self) {
        LOADING.store(false, Ordering::Release);
    }
}

pub(crate) fn ready(root: &str, video_id: &str) -> bool {
    PREFETCHED.lock().is_ok_and(|cache| {
        cache
            .as_ref()
            .is_some_and(|prepared| prepared.complete && prepared.matches(root, video_id))
    })
}

pub(crate) fn available(root: &str, video_id: &str) -> bool {
    PREFETCHED.lock().is_ok_and(|cache| {
        cache
            .as_ref()
            .is_some_and(|prepared| prepared.matches(root, video_id))
    })
}

pub(super) fn lookup(source: &Source) -> Option<Arc<[Range]>> {
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
            .then(|| track.ranges.clone())
    })
}

fn fetch(
    source: Arc<Source>,
    stream: &Value,
    position: f64,
    limit: u64,
    active: &impl Fn() -> bool,
) -> Option<Track> {
    if !active() {
        return None;
    }
    let _span = crate::trace::Section::begin(c"GoVue transport: prefetch startup range");
    let seek_index = (position > 0.25)
        .then(|| {
            let (_, init) = playback::range(&stream["initRange"], source.length)?;
            let (index, end) = playback::range(&stream["indexRange"], source.length)?;
            (init < index && end < limit / 2).then_some((index, end))
        })
        .flatten();
    let end = seek_index.map_or(source.length.min(limit) - 1, |(_, end)| end);
    let bytes = range_bytes(&source, 0, end).ok()?;
    let seek = seek_index.and_then(|(index, end)| {
        let index = usize::try_from(index).ok()?;
        let end = usize::try_from(end).ok()?;
        let data = bytes.get(index..=end)?;
        if source.mime.ends_with("/webm") {
            webm::cluster_at(bytes.get(..index)?, data, position)
        } else {
            playback::segment_index(stream, data)
                .ok()?
                .into_iter()
                .find(|segment| {
                    position >= segment.start && position < segment.start + segment.duration
                })
                .map(|segment| segment.offset)
        }
    });
    let mut ranges = vec![Range { start: 0, bytes }];
    if let Some(start) = seek.filter(|n| *n > end && *n < source.length)
        && active()
        && let Ok(bytes) = range_bytes(
            &source,
            start,
            (start + limit - end - 2).min(source.length - 1),
        )
    {
        ranges.push(Range { start, bytes });
    }
    Some(Track {
        source,
        ranges: ranges.into(),
    })
}

pub(crate) fn warm(
    root: &str,
    catalog: &Value,
    settings: &Value,
    active: impl Fn() -> bool + Sync,
) {
    if catalog["live"] == true || !active() || LOADING.swap(true, Ordering::AcqRel) {
        return;
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
    let video_format = video;
    let Ok(video) = stream_source(catalog, video_format, "") else {
        return;
    };
    let audio = (!combined)
        .then(|| {
            playback::audio_for(catalog, settings["audioKey"].as_str().unwrap_or(""))
                .or_else(|| playback::audio(catalog))
                .and_then(|audio| Some((audio, stream_source(catalog, audio, "").ok()?)))
        })
        .flatten();
    let position = crate::player::dispatch(
        &json!({"op":"player_resume","videoId":catalog["videoId"],"duration":catalog["duration"],"position":0}),
        root,
    )
    .ok()
    .and_then(|position| position.as_f64())
    .unwrap_or(0.0);
    let active = &active;
    let tracks: Vec<_> = std::thread::scope(|threads| {
        let audio = audio.map(|(format, audio)| {
            threads.spawn(move || fetch(audio, format, position, AUDIO_BYTES, active))
        });
        let video = fetch(video, video_format, position, VIDEO_BYTES, active);
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
            root: root.to_owned(),
            video_id: catalog["videoId"].as_str().unwrap_or_default().to_owned(),
            complete: tracks.len() == if combined { 1 } else { 2 }
                && (position <= 0.25 || tracks.iter().all(|track| track.ranges.len() > 1)),
            tracks,
        });
    }
}
