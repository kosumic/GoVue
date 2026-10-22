//! Normalized transcript documents and their shared JSON operations.
use crate::{
    MAX_BYTES, MAX_SEGMENTS, Result, caption_tracks, caption_words, captions, clean, folded, json,
    string, timestamp, trace, transcript_playback, transcript_reader, trusted_origin, video_id,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

mod store;
pub(crate) use store::{load, remember, remember_shared, save, with_transcript};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Track {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) language: String,
    pub(crate) automatic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) translatable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) audio_track_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) translated_from: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Segment {
    pub(crate) start: f64,
    pub(crate) duration: f64,
    pub(crate) text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) words: Vec<caption_words::Word>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) runs: Vec<captions::Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "windowId")]
    pub(crate) window_id: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Transcript {
    pub(crate) video_id: String,
    pub(crate) title: String,
    pub(crate) language: String,
    pub(crate) track_id: String,
    pub(crate) tracks: Vec<Track>,
    pub(crate) segments: Vec<Segment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caption_metadata: Option<captions::Metadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) track_metadata: Option<caption_tracks::Metadata>,
}

pub(crate) fn normalize(data: &mut Transcript) -> Result<()> {
    if !video_id(&data.video_id)
        || data.title.len() > 10_000
        || data.language.len() > 500
        || data.track_id.len() > 1_000
        || data.tracks.is_empty()
        || data.tracks.len() > 300
        || data.segments.is_empty()
        || data.segments.len() > MAX_SEGMENTS
    {
        return Err("Invalid transcript metadata or line count.".into());
    }
    caption_tracks::validate(data)?;
    if data
        .tracks
        .iter()
        .any(|t| t.id.len() > 1_000 || t.label.len() > 1_000 || t.language.len() > 100)
        || !data.tracks.iter().any(|t| t.id == data.track_id)
    {
        return Err("Invalid caption track.".into());
    }
    if data.segments.iter().any(|s| {
        !s.start.is_finite()
            || s.start < 0.0
            || !s.duration.is_finite()
            || s.duration < 0.0
            || s.text.len() > 50_000
    }) {
        return Err("Invalid caption line.".into());
    }
    for segment in &mut data.segments {
        segment.text = captions::clean_text(&segment.text);
        if !segment.runs.is_empty() {
            let text = segment.text.clone();
            if segment.runs.len() > 50_000
                || segment.runs.iter().any(|run| {
                    run.text.len() > 50_000
                        || run
                            .start
                            .is_some_and(|n| !n.is_finite() || n < segment.start)
                        || run.reading.as_ref().is_some_and(|s| s.len() > 50_000)
                })
            {
                return Err("Invalid caption formatting.".into());
            }
            captions::refresh(segment);
            if segment.text != text {
                return Err("Caption formatting does not match its text.".into());
            }
        }
        if !segment.words.is_empty() && !caption_words::valid(segment) {
            segment.words.clear();
        }
    }
    if data
        .caption_metadata
        .as_ref()
        .is_some_and(|metadata| !captions::validate(metadata))
    {
        return Err("Invalid caption structure.".into());
    }
    data.segments.retain(|s| !s.text.is_empty());
    data.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    if data.segments.is_empty() {
        return Err("This video has no caption text available.".into());
    }
    data.title = clean(&data.title);
    Ok(())
}

pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let op = string(command, "op")?;
    let id = string(command, "videoId")?;
    if !video_id(id) {
        return Err("Invalid video identity.".into());
    }
    if op == "accept" {
        let origin = string(command, "origin")?;
        let request_id = string(command, "requestId")?;
        let raw = string(command, "raw")?;
        if raw.len() > MAX_BYTES || request_id.len() > 200 || !trusted_origin(origin, id) {
            return Err("Untrusted transcript response.".into());
        }
        let reply: Value = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.reply_parse");
            serde_json::from_str(raw).map_err(|_| "Unreadable transcript response.")?
        };
        if reply["type"] != "transcript"
            || reply["requestId"] != request_id
            || reply["videoId"] != id
        {
            return Err("Transcript response belongs to another request.".into());
        }
        if reply["ok"] == false {
            let error = string(&reply, "error")?
                .chars()
                .take(500)
                .collect::<String>();
            return Ok(
                json!({"type":"transcript","requestId":request_id,"videoId":id,"ok":false,"error":error}),
            );
        }
        if reply["ok"] != true {
            return Err("Invalid transcript response.".into());
        }
        let mut data: Transcript = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.decode");
            serde_json::from_value(reply["transcript"].clone())
                .map_err(|_| "Invalid transcript data.")?
        };
        if data.video_id != id {
            return Err("Transcript belongs to another video.".into());
        }
        if let Some(source) = reply["transcript"]["captionSource"].as_str() {
            let _span = trace::Section::begin(c"GoVue transcript: rust.caption_parse");
            let (segments, metadata) = captions::parse(source)?;
            data.segments = segments;
            data.caption_metadata = Some(metadata);
        }
        {
            let _span = trace::Section::begin(c"GoVue transcript: rust.normalize");
            normalize(&mut data)?;
        }
        // Captions remain usable when device storage is full or unavailable.
        let data = Arc::new(data);
        remember_shared(root, Arc::clone(&data))?;
        let cache_saved = {
            let _span = trace::Section::begin(c"GoVue transcript: rust.cache_save");
            save(root, &data).is_ok()
        };
        return Ok(
            json!({"type":"transcript","requestId":request_id,"videoId":id,"ok":true,"transcript":data.as_ref(),"cacheSaved":cache_saved}),
        );
    }
    let track = string(command, "trackId")?;
    if op == "transcript_playback" {
        return with_transcript(root, id, track, |data| {
            transcript_playback::step(data, command)
        })?;
    }
    let data = load(root, id, track)?;
    match op {
        "transcript_select" | "transcript_reader" | "transcript_export" => {
            transcript_reader::dispatch(op, &data, command, root)
        }
        "transcript_playback" => transcript_playback::step(&data, command),
        "load" => Ok(serde_json::to_value(data.as_ref())
            .map_err(|_| "Could not encode cached transcript.")?),
        "search" => {
            let query = string(command, "query")?;
            if query.len() > 10_000 {
                return Err("Search query is too long.".into());
            }
            let query = folded(query);
            let indices: Vec<usize> = data
                .segments
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    (query.is_empty() || captions::matches(s, &query)).then_some(i)
                })
                .collect();
            Ok(json!(indices))
        }
        "export" => {
            let lines: Vec<_> = data
                .segments
                .iter()
                .map(|s| format!("{}  {}", timestamp(s.start), captions::readable_text(s)))
                .collect();
            Ok(json!(format!(
                "{}\nhttps://www.youtube.com/watch?v={}\n\n{}",
                data.title,
                data.video_id,
                lines.join("\n")
            )))
        }
        _ => Err("Unsupported core operation.".into()),
    }
}

/// Retain a consistent cache version, then decode and serialize without blocking
/// unrelated storage operations. Explicit tracks keep the legacy null-on-miss policy.
pub(crate) fn restore(command: &Value, root: &str) -> Result<Value> {
    let id = string(command, "videoId")?;
    if !video_id(id) {
        return Err("Invalid video identity.".into());
    }
    let track = command["trackId"].as_str();
    let snapshot = {
        let _guard = crate::REQUEST_LOCK
            .lock(c"GoVue core request: transcript.restore")
            .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
        let _span = trace::Section::begin(c"GoVue core op: transcript_restore");
        store::snapshot(root, id, track)?
    };
    let _span = trace::Section::begin(c"GoVue transcript: rust.cache_restore");
    let resolved = snapshot.resolve(root, id, track, command["audioKey"].as_str().unwrap_or(""));
    if track.is_some() {
        return Ok(resolved
            .ok()
            .flatten()
            .map_or(Value::Null, |data| json!(data.as_ref())));
    }
    Ok(resolved?.map_or(Value::Null, |data| json!(data.as_ref())))
}
