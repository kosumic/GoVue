//! Cast-compatible AVC/AAC DASH with explicit MP4 segment addresses.
use crate::{
    Result,
    media::{container, playback},
    transport::{self, Source},
};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::{collections::HashMap, sync::Arc};

pub struct Part {
    pub source: Arc<Source>,
    pub start: u64,
    pub end: u64,
}
pub struct Bundle {
    pub manifest: String,
    pub parts: HashMap<String, Part>,
    pub duration: f64,
}

// Parse the bounded sidx box, retaining its earliest presentation timestamp.
type SegmentIndex = (u32, u64, Vec<(u64, u64, u32)>);
fn index(stream: &Value, bytes: &[u8]) -> Result<SegmentIndex> {
    let invalid = || "Invalid Cast segment index.".to_string();
    let length = stream["bytes"].as_u64().ok_or_else(invalid)?;
    let (start, end) = playback::range(&stream["indexRange"], length).ok_or_else(invalid)?;
    if bytes.len() as u64 != end - start + 1 {
        return Err(invalid());
    }
    let u32at = |at| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        ))
    };
    let u64at = |at| -> Result<u64> {
        Ok(u64::from_be_bytes(
            bytes
                .get(at..at + 8)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        ))
    };
    let mut at = 0;
    loop {
        let size = u32at(at)? as usize;
        let end = at
            .checked_add(size)
            .filter(|n| *n <= bytes.len())
            .ok_or_else(invalid)?;
        if size < 8 {
            return Err(invalid());
        }
        if bytes.get(at + 4..at + 8) == Some(b"sidx") {
            break;
        }
        at = end;
    }
    let box_end = at + u32at(at)? as usize;
    let scale = u32at(at + 16)?;
    let (time, offset, count_at) = match bytes.get(at + 8) {
        Some(0) => (
            u64::from(u32at(at + 20)?),
            u64::from(u32at(at + 24)?),
            at + 28,
        ),
        Some(1) => (u64at(at + 20)?, u64at(at + 28)?, at + 36),
        _ => return Err(invalid()),
    };
    let count = (u32at(count_at)? & 0xffff) as usize;
    if scale == 0 || count == 0 || count > 3000 || count_at + 4 + count * 12 != box_end {
        return Err(invalid());
    }
    let mut offset = start
        .checked_add(box_end as u64)
        .and_then(|n| n.checked_add(offset))
        .ok_or_else(invalid)?;
    let mut entries = Vec::with_capacity(count);
    for n in 0..count {
        let row = count_at + 4 + n * 12;
        let size = u32at(row)?;
        let duration = u32at(row + 4)?;
        if size == 0
            || size & 0x8000_0000 != 0
            || duration == 0
            || f64::from(duration) / f64::from(scale) > 3600.0
        {
            return Err(invalid());
        }
        let next = offset
            .checked_add(u64::from(size))
            .filter(|n| *n <= length)
            .ok_or_else(invalid)?;
        entries.push((offset, next - 1, duration));
        offset = next;
    }
    Ok((scale, time, entries))
}

pub fn build(command: &Value) -> Result<Bundle> {
    let catalog = &command["catalog"];
    if catalog["live"] == true {
        return Err("Live casting is not available yet.".into());
    }
    let duration = catalog["duration"]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 604_800.0)
        .ok_or("Missing Cast duration.")?;
    let mut compatible = catalog.clone();
    compatible["streams"] = json!(
        catalog["streams"]
            .as_array()
            .ok_or("Missing Cast formats.")?
            .iter()
            .filter(|s| {
                let mime = s["mimeType"].as_str().unwrap_or("");
                (container(s) == Some("video/mp4") && mime.contains("avc1"))
                    || (container(s) == Some("audio/mp4") && mime.contains("mp4a"))
            })
            .collect::<Vec<_>>()
    );
    let videos = playback::videos(&compatible)?;
    let height = command["height"].as_u64().unwrap_or(0);
    let fps = command["fps"].as_u64().unwrap_or(0);
    let selected = videos
        .iter()
        .copied()
        .find(|s| {
            if height == 0 {
                s["height"].as_u64().unwrap_or(0) <= 1080
            } else {
                s["height"] == height && (fps == 0 || s["fps"] == fps)
            }
        })
        .or_else(|| videos.last().copied())
        .ok_or("No Cast-compatible video format.")?;
    let mut parts = HashMap::new();
    let file = |s: &Value| {
        transport::source(
            &json!({"url":s["url"],"length":s["bytes"],"mimeType":s["mimeType"],"origin":catalog["origin"],
        "userAgent":s["userAgent"].as_str().filter(|s|!s.is_empty()).unwrap_or(command["userAgent"].as_str().unwrap_or(""))}),
        )
    };
    if selected["kind"] == "combined" {
        let source = file(selected)?;
        parts.insert(
            "media.mp4".into(),
            Part {
                start: 0,
                end: source.length() - 1,
                source,
            },
        );
        return Ok(Bundle {
            manifest: String::new(),
            parts,
            duration,
        });
    }
    let audio = playback::audio_for(&compatible, command["audioKey"].as_str().unwrap_or(""))
        .or_else(|| playback::audio(&compatible))
        .ok_or("No Cast-compatible audio track.")?;
    let mut manifest = format!(
        "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:MPD:2011\" profiles=\"urn:mpeg:dash:profile:isoff-live:2011\" type=\"static\" minBufferTime=\"PT1.5S\" mediaPresentationDuration=\"PT{duration:.3}S\"><Period>"
    );
    for (n, stream) in [selected, audio].into_iter().enumerate() {
        let source = file(stream)?;
        let (init_start, init_end) = playback::range(&stream["initRange"], source.length())
            .ok_or("Missing Cast initialization.")?;
        let (index_start, index_end) =
            playback::range(&stream["indexRange"], source.length()).ok_or("Missing Cast index.")?;
        let bytes = transport::range_bytes(&source, index_start, index_end)
            .map_err(|_| "Could not read the Cast segment index.")?;
        let mime = container(stream).ok_or("Invalid Cast container.")?;
        let codec = playback::codecs(stream)?;
        let bandwidth = stream["bitrate"]
            .as_u64()
            .unwrap_or(1)
            .clamp(1, 1_000_000_000);
        let _ = write!(
            manifest,
            "<AdaptationSet id=\"{n}\" mimeType=\"{mime}\" contentType=\"{}\"><Representation id=\"{n}\" codecs=\"{codec}\" bandwidth=\"{bandwidth}\"",
            if n == 0 { "video" } else { "audio" }
        );
        for (field, attribute, max) in [
            ("width", "width", 15360),
            ("height", "height", 8640),
            ("fps", "frameRate", 240),
        ] {
            if n == 0
                && let Some(v) = stream[field].as_u64().filter(|v| *v > 0 && *v <= max)
            {
                let _ = write!(manifest, " {attribute}=\"{v}\"");
            }
        }
        manifest.push('>');
        if n == 1 {
            let channels = stream["audioChannels"].as_u64().unwrap_or(2).clamp(1, 32);
            let _ = write!(
                manifest,
                "<AudioChannelConfiguration schemeIdUri=\"urn:mpeg:dash:23003:3:audio_channel_configuration:2012\" value=\"{channels}\"/>"
            );
        }
        if let Ok((scale, time, entries)) = index(stream, &bytes) {
            let init = format!("init{n}.mp4");
            parts.insert(
                init.clone(),
                Part {
                    source: source.clone(),
                    start: init_start,
                    end: init_end,
                },
            );
            let _ = write!(
                manifest,
                "<SegmentList timescale=\"{scale}\" presentationTimeOffset=\"{time}\"><Initialization sourceURL=\"{init}\"/><SegmentTimeline>"
            );
            for (i, (_, _, duration)) in entries.iter().enumerate() {
                let _ = write!(
                    manifest,
                    "<S{} d=\"{duration}\"/>",
                    if i == 0 {
                        format!(" t=\"{time}\"")
                    } else {
                        String::new()
                    }
                );
            }
            manifest.push_str("</SegmentTimeline>");
            for (i, (start, end, _)) in entries.into_iter().enumerate() {
                let name = format!("part{n}-{i}.mp4");
                parts.insert(
                    name.clone(),
                    Part {
                        source: source.clone(),
                        start,
                        end,
                    },
                );
                let _ = write!(manifest, "<SegmentURL media=\"{name}\"/>");
            }
            manifest.push_str("</SegmentList>");
        } else {
            let name = format!("track{n}.mp4");
            parts.insert(
                name.clone(),
                Part {
                    source: source.clone(),
                    start: 0,
                    end: source.length() - 1,
                },
            );
            let _ = write!(
                manifest,
                "<BaseURL>{name}</BaseURL><SegmentBase indexRange=\"{index_start}-{index_end}\"><Initialization range=\"{init_start}-{init_end}\"/></SegmentBase>"
            );
        }
        manifest.push_str("</Representation></AdaptationSet>");
    }
    manifest.push_str("</Period></MPD>");
    Ok(Bundle {
        manifest,
        parts,
        duration,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::playback::tests::{index as fixture, stream};
    #[test]
    fn cast_indexes_preserve_time_offsets_and_reject_nested_or_outside_references() {
        let stream = stream("video", 240);
        let mut bytes = fixture(0);
        bytes[20..24].copy_from_slice(&5000u32.to_be_bytes());
        let (scale, time, segments) = index(&stream, &bytes).unwrap();
        assert_eq!((scale, time), (1000, 5000));
        assert_eq!(segments, vec![(156, 355, 1500), (356, 555, 2200)]);
        let mut outside = bytes.clone();
        outside[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(index(&stream, &outside).is_err());
        let mut nested = bytes.clone();
        nested[32..36].copy_from_slice(&0x8000_0001_u32.to_be_bytes());
        assert!(index(&stream, &nested).is_err());
        bytes[16..20].copy_from_slice(&0u32.to_be_bytes());
        assert!(index(&stream, &bytes).is_err());
    }
}
