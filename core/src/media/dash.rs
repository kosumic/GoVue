//! Synthetic VOD DASH manifests. Only opaque loopback-relative file names cross
//! into the manifest; signed upstream addresses remain in the transport registry.
use super::{Result, container, playback};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn manifest(duration: f64, tracks: &[(&Value, String)]) -> Result<String> {
    if !duration.is_finite()
        || !(0.0..=604800.0).contains(&duration)
        || duration == 0.0
        || tracks.is_empty()
        || tracks.len() > 17
    {
        return Err("The playback duration or track pool is unavailable.".into());
    }
    let mut groups: BTreeMap<(&str, &str), Vec<(&Value, &str)>> = BTreeMap::new();
    for (stream, uri) in tracks {
        let mime = container(stream).ok_or("Invalid playback container.")?;
        let codec = playback::codecs(stream)?;
        if !playback::indexed(stream)
            || uri.len() > 160
            || uri.is_empty()
            || !uri
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("Invalid indexed playback track.".into());
        }
        // Keep container and codec families in separate adaptation sets.
        groups
            .entry((mime, codec.split('.').next().unwrap_or(codec)))
            .or_default()
            .push((stream, uri));
    }
    let mut text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MPD xmlns=\"urn:mpeg:DASH:schema:MPD:2011\" profiles=\"urn:mpeg:dash:profile:full:2011\" type=\"static\" minBufferTime=\"PT1.500S\" mediaPresentationDuration=\"PT{duration:.3}S\"><Period>"
    );
    let mut representation = 0;
    for (set, ((mime, _), streams)) in groups.into_iter().enumerate() {
        let audio = mime.starts_with("audio/");
        text.push_str(&format!("<AdaptationSet id=\"{set}\" mimeType=\"{mime}\" contentType=\"{}\" subsegmentAlignment=\"true\">",if audio{"audio"}else{"video"}));
        for (stream, uri) in streams {
            let length = stream["bytes"].as_u64().unwrap_or(0);
            let (init_start, init_end) = playback::range(&stream["initRange"], length)
                .ok_or("Missing playback initialization.")?;
            let (index_start, index_end) =
                playback::range(&stream["indexRange"], length).ok_or("Missing playback index.")?;
            let bandwidth = stream["bitrate"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 1_000_000_000)
                .unwrap_or(if audio { 128000 } else { 4_000_000 });
            text.push_str(&format!(
                "<Representation id=\"{representation}\" bandwidth=\"{bandwidth}\" codecs=\"{}\"",
                playback::codecs(stream)?
            ));
            representation += 1;
            for (field, attribute, max) in if audio {
                vec![("audioSampleRate", "audioSamplingRate", 384000)]
            } else {
                vec![
                    ("width", "width", 15360),
                    ("height", "height", 8640),
                    ("fps", "frameRate", 240),
                ]
            } {
                if let Some(n) = stream[field].as_u64().filter(|n| *n > 0 && *n <= max) {
                    text.push_str(&format!(" {attribute}=\"{n}\""));
                }
            }
            text.push('>');
            if audio
                && let Some(channels) = stream["audioChannels"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 32)
            {
                text.push_str(&format!("<AudioChannelConfiguration schemeIdUri=\"urn:mpeg:dash:23003:3:audio_channel_configuration:2012\" value=\"{channels}\"/>"));
            }
            text.push_str(&format!("<BaseURL>{uri}</BaseURL><SegmentBase indexRange=\"{index_start}-{index_end}\"><Initialization range=\"{init_start}-{init_end}\"/></SegmentBase></Representation>"));
        }
        text.push_str("</AdaptationSet>");
    }
    text.push_str("</Period></MPD>");
    Ok(text)
}
