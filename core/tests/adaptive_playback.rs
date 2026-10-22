#![cfg(test)]

use serde_json::{Value, json};

fn call(command: Value) -> Value {
    let reply: Value =
        serde_json::from_str(&govue_core::execute(command.to_string(), String::new())).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn stream(kind: &str, height: u64, codec: &str) -> Value {
    let webm = codec.starts_with("vp09");
    json!({"kind":kind,"height":height,"width":height*2,"fps":30,"bytes":4096,
        "mimeType":format!("{kind}/{}; codecs=\"{codec}\"", if webm {"webm"} else {"mp4"}),
        "bitrate":height*1000+64000,"url":format!("https://r1.googlevideo.com/videoplayback?fixture={codec}-{height}"),
        "initRange":{"start":0,"end":99},"indexRange":{"start":100,"end":199}})
}

#[test]
fn auto_keeps_lower_sizes_for_the_selected_codec_and_manual_quality_stays_pinned() {
    let catalog = json!({"origin":"https://m.youtube.com","duration":60,"streams":[
        stream("audio",0,"mp4a.40.2"), stream("video",1440,"vp09.00.51.08"),
        stream("video",720,"avc1.640028"), stream("video",720,"vp09.00.31.08"),
        stream("video",360,"avc1.4d401e"), stream("video",360,"vp09.00.21.08") ]});
    let choices = call(json!({"op":"media_playback_choices","catalog":catalog}));
    assert_eq!(
        choices.as_array().unwrap().len(),
        3,
        "The menu still deduplicates codec variants"
    );
    for height in [0, 720] {
        let playback = call(
            json!({"op":"media_playback","catalog":catalog,"height":height,
            "manifest":"dash","userAgent":"fixture"}),
        );
        assert_eq!(playback["qualityHeight"], height);
        if height == 0 {
            assert_eq!(playback["startupHeight"], 360);
        } else {
            assert_eq!(playback["startupHeight"], 0);
        }
        let uri = playback["uri"].as_str().unwrap();
        let text = ureq::get(uri)
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        assert!(!text.contains("googlevideo"));
        let document = roxmltree::Document::parse(&text).unwrap();
        let videos: Vec<_> = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("AdaptationSet") && node.attribute("contentType") == Some("video")
            })
            .collect();
        assert_eq!(
            videos.len(),
            1,
            "Auto needs one complete, switchable codec ladder"
        );
        let formats: Vec<_> = videos[0]
            .children()
            .filter(|node| node.has_tag_name("Representation"))
            .collect();
        if height == 0 {
            assert_eq!(
                formats
                    .iter()
                    .map(|node| node.attribute("height").unwrap())
                    .collect::<Vec<_>>(),
                ["1440", "720", "360"]
            );
            assert!(
                formats
                    .iter()
                    .all(|node| node.attribute("codecs").unwrap().starts_with("vp09"))
            );
        } else {
            assert_eq!(formats.len(), 1);
            assert_eq!(formats[0].attribute("height"), Some("720"));
            assert!(formats[0].attribute("codecs").unwrap().starts_with("avc1"));
        }
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
}

#[test]
fn startup_limit_uses_an_available_height_and_keeps_higher_formats() {
    for (heights, expected) in [(vec![1440, 720], 720), (vec![1080, 356, 180], 356)] {
        let mut streams = vec![stream("audio", 0, "mp4a.40.2")];
        streams.extend(
            heights
                .iter()
                .map(|height| stream("video", *height, "avc1.640028")),
        );
        let playback = call(json!({"op":"media_playback","height":0,"manifest":"dash",
            "userAgent":"fixture","catalog":{"origin":"https://m.youtube.com",
                "duration":60,"streams":streams}}));
        assert_eq!(playback["startupHeight"], expected);
        assert_eq!(playback["qualityHeight"], 0, "Auto remains adaptive");
        let uri = playback["uri"].as_str().unwrap();
        let text = ureq::get(uri)
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        let document = roxmltree::Document::parse(&text).unwrap();
        let video_count = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("Representation") && node.attribute("height").is_some()
            })
            .count();
        assert_eq!(video_count, heights.len());
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
}
