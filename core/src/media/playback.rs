//! Playback choices and HLS byte-range playlists for YouTube's AVC/AAC files.
//! Playlists and signed source addresses remain in the loopback proxy's memory.
use super::{media_url, Result};
use serde_json::{json, Value};

pub(super) const SPEEDS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

pub(crate) fn range(value: &Value, length: u64) -> Option<(u64, u64)> {
    let number = |v: &Value| v.as_u64().or_else(|| v.as_str()?.parse().ok());
    let start = number(&value["start"])?;
    let end = number(&value["end"])?;
    (start <= end && end < length && end - start < 1_048_576).then_some((start, end))
}

pub(crate) fn indexed(stream: &Value) -> bool {
    let length = stream["bytes"].as_u64().unwrap_or(0);
    matches!((range(&stream["initRange"], length), range(&stream["indexRange"], length)),
        (Some((0, init_end)), Some((index_start, _))) if init_end < index_start)
}

pub(crate) fn videos(catalog: &Value) -> Result<Vec<&Value>> {
    if catalog["live"] == true {
        return Err("Resolution selection is unavailable for this live stream.".into());
    }
    let streams = catalog["streams"]
        .as_array()
        .ok_or("Missing playback formats.")?;
    if streams.len() > 500 {
        return Err("Too many playback formats.".into());
    }
    let has_audio = audio(catalog).is_some();
    let mut videos: Vec<_> = streams
        .iter()
        .filter(|s| {
            let mime = s["mimeType"].as_str().unwrap_or("");
            s["bytes"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 100_000_000_000)
                && s["height"].as_u64().is_some_and(|h| h > 0 && h <= 8640)
                && s["url"].as_str().and_then(media_url).is_some()
                && mime.starts_with("video/mp4")
                && mime.contains("avc1")
                && (s["kind"] == "combined" || (s["kind"] == "video" && has_audio && indexed(s)))
        })
        .collect();
    videos.sort_by_key(|s| {
        std::cmp::Reverse((
            s["height"].as_u64().unwrap_or(0),
            s["kind"] == "video",
            s["bitrate"].as_u64().unwrap_or(0),
        ))
    });
    videos.dedup_by_key(|s| s["height"].as_u64());
    videos.truncate(16);
    if videos.is_empty() {
        return Err("No playable video formats are available. Refresh the video.".into());
    }
    Ok(videos)
}

pub(crate) fn audio(catalog: &Value) -> Option<&Value> {
    catalog["streams"]
        .as_array()?
        .iter()
        .filter(|s| {
            let mime = s["mimeType"].as_str().unwrap_or("");
            s["kind"] == "audio"
                && mime.starts_with("audio/mp4")
                && mime.contains("mp4a")
                && indexed(s)
                && s["url"].as_str().and_then(media_url).is_some()
        })
        .max_by_key(|s| s["bitrate"].as_u64().unwrap_or(0))
}

pub(super) fn choices(catalog: &Value) -> Result<Value> {
    Ok(json!(videos(catalog)?
        .iter()
        .map(|s| json!({"height":s["height"],
        "label":format!("{}p", s["height"].as_u64().unwrap())}))
        .collect::<Vec<_>>()))
}

pub(crate) fn codecs(stream: &Value) -> Result<&str> {
    stream["mimeType"]
        .as_str()
        .and_then(|mime| mime.split("codecs=\"").nth(1))
        .and_then(|s| s.split('"').next())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"., ".contains(&b))
        })
        .ok_or("Invalid playback codec.".into())
}

// ISO BMFF Segment Index. Reject nested indexes and references outside the file.
pub(crate) fn playlist(stream: &Value, data: &[u8], uri: &str) -> Result<String> {
    let invalid = || "Invalid MP4 segment index.".to_string();
    let length = stream["bytes"].as_u64().ok_or_else(invalid)?;
    let (init_start, init_end) = range(&stream["initRange"], length).ok_or_else(invalid)?;
    let (index_start, index_end) = range(&stream["indexRange"], length).ok_or_else(invalid)?;
    if data.len() as u64 != index_end - index_start + 1 {
        return Err(invalid());
    }
    let u32_at = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            data.get(at..at + 4)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        ))
    };
    let u64_at = |at: usize| -> Result<u64> {
        Ok(u64::from_be_bytes(
            data.get(at..at + 8)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        ))
    };
    let mut at = 0;
    loop {
        let size = u32_at(at)? as usize;
        if size < 8 || at.checked_add(size).is_none_or(|end| end > data.len()) {
            return Err(invalid());
        }
        if data.get(at + 4..at + 8) == Some(b"sidx") {
            break;
        }
        at += size;
    }
    let end = at + u32_at(at)? as usize;
    let version = *data.get(at + 8).ok_or_else(invalid)?;
    let timescale = u32_at(at + 16)?;
    if timescale == 0 || version > 1 {
        return Err(invalid());
    }
    let (first_offset, count_at) = if version == 0 {
        (u32_at(at + 24)? as u64, at + 28)
    } else {
        (u64_at(at + 28)?, at + 36)
    };
    let count = u32_at(count_at)? as u16 as usize;
    if count == 0 || count > 50_000 || count_at + 4 + count * 12 != end {
        return Err(invalid());
    }
    let mut offset = index_start
        .checked_add(end as u64)
        .and_then(|n| n.checked_add(first_offset))
        .ok_or_else(invalid)?;
    let mut segments = String::new();
    let mut target = 1.0f64;
    for i in 0..count {
        let entry = count_at + 4 + i * 12;
        let size = u32_at(entry)?;
        let duration = u32_at(entry + 4)? as f64 / timescale as f64;
        if size == 0 || size & 0x8000_0000 != 0 || duration <= 0.0 || duration > 3600.0 {
            return Err(invalid());
        }
        let next = offset
            .checked_add(size as u64)
            .filter(|n| *n <= length)
            .ok_or_else(invalid)?;
        target = target.max(duration.ceil());
        segments.push_str(&format!(
            "#EXTINF:{duration:.6},\n#EXT-X-BYTERANGE:{size}@{offset}\n{uri}\n"
        ));
        offset = next;
    }
    Ok(format!("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target:.0}\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"{uri}\",BYTERANGE=\"{}@{init_start}\"\n{segments}#EXT-X-ENDLIST\n", init_end - init_start + 1))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn stream(kind: &str, height: u64) -> Value {
        json!({"itag":height,"kind":kind,"height":height,"width":height * 2,
            "bytes":556,"bitrate":100_000 + height * 1000,"userAgent":"test-client",
            "mimeType":if kind == "audio" { "audio/mp4; codecs=\"mp4a.40.2\"" } else { "video/mp4; codecs=\"avc1.640028\"" },
            "url":format!("https://r1.googlevideo.com/test-{kind}-{height}?sig=private"),
            "initRange":{"start":0,"end":99},"indexRange":{"start":100,"end":155}})
    }

    pub(crate) fn index(version: u8) -> Vec<u8> {
        let size = if version == 0 { 56u32 } else { 64 };
        let mut bytes = size.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"sidx");
        bytes.extend_from_slice(&[version, 0, 0, 0]);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&1000u32.to_be_bytes());
        bytes.extend_from_slice(&vec![0; if version == 0 { 8 } else { 16 }]);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        for duration in [1500u32, 2200] {
            bytes.extend_from_slice(&200u32.to_be_bytes());
            bytes.extend_from_slice(&duration.to_be_bytes());
            bytes.extend_from_slice(&0x9000_0000u32.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn choices_require_playable_audio_and_deduplicate_heights() {
        let video = stream("video", 1080);
        let mut combined = stream("combined", 360);
        combined["initRange"] = Value::Null;
        let mut catalog = json!({"streams":[video,combined]});
        assert_eq!(
            choices(&catalog).unwrap(),
            json!([{"height":360,"label":"360p"}])
        );
        let audio = stream("audio", 0);
        catalog["streams"].as_array_mut().unwrap().push(audio);
        catalog["streams"]
            .as_array_mut()
            .unwrap()
            .push(stream("video", 360));
        assert_eq!(
            choices(&catalog).unwrap(),
            json!([{"height":1080,"label":"1080p"},{"height":360,"label":"360p"}])
        );
        assert_eq!(videos(&catalog).unwrap()[1]["kind"], "video");
        catalog["streams"][0]["url"] = json!("https://evil.org/video");
        assert_eq!(choices(&catalog).unwrap().as_array().unwrap().len(), 1);
        catalog["live"] = json!(true);
        assert!(choices(&catalog).is_err());
    }

    #[test]
    fn playlists_use_segment_durations_offsets_and_both_index_versions() {
        for version in [0, 1] {
            let bytes = index(version);
            let mut stream = stream("video", 1080);
            stream["indexRange"]["end"] = json!(99 + bytes.len());
            stream["bytes"] = json!(100 + bytes.len() + 400);
            let result = playlist(&stream, &bytes, "opaque.mp4").unwrap();
            assert!(result.contains("#EXT-X-TARGETDURATION:3\n"));
            assert!(result.contains("#EXT-X-MAP:URI=\"opaque.mp4\",BYTERANGE=\"100@0\""));
            assert!(result.contains(&format!(
                "#EXTINF:1.500000,\n#EXT-X-BYTERANGE:200@{}",
                100 + bytes.len()
            )));
            assert!(result.contains(&format!(
                "#EXTINF:2.200000,\n#EXT-X-BYTERANGE:200@{}",
                300 + bytes.len()
            )));
            assert!(result.ends_with("#EXT-X-ENDLIST\n"));
            assert!(!result.contains("private"));
        }
        let mut bytes = 8u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"free");
        bytes.extend(index(0));
        let mut stream = stream("video", 1080);
        stream["indexRange"]["end"] = json!(163);
        stream["bytes"] = json!(564);
        assert!(playlist(&stream, &bytes, "opaque.mp4")
            .unwrap()
            .contains("200@164"));
    }

    #[test]
    fn malformed_segment_indexes_fail_without_panicking() {
        let stream = stream("video", 1080);
        let original = index(0);
        for length in 0..original.len() {
            assert!(playlist(&stream, &original[..length], "opaque.mp4").is_err());
        }
        for (at, replacement) in [
            (16, 0u32),
            (32, 0x8000_00c8),
            (36, 0),
            (24, u32::MAX),
            (28, 50_001),
        ] {
            let mut bytes = original.clone();
            bytes[at..at + 4].copy_from_slice(&replacement.to_be_bytes());
            assert!(playlist(&stream, &bytes, "opaque.mp4").is_err());
        }
        let mut outside = stream.clone();
        outside["bytes"] = json!(555);
        assert!(playlist(&outside, &original, "opaque.mp4").is_err());
        let mut version = original;
        version[8] = 2;
        assert!(playlist(&stream, &version, "opaque.mp4").is_err());
    }
}
