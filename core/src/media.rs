use super::{MAX_BYTES, Result, clean, string, trusted_origin, video_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use url::Url;
pub(crate) mod playback;

fn read(root: &str, name: &str, fallback: Value) -> Value {
    fs::read(Path::new(root).join("media").join(name))
        .ok()
        .filter(|b| b.len() <= MAX_BYTES)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(fallback)
}

fn write(root: &str, name: &str, value: &Value) -> Result<()> {
    let directory = Path::new(root).join("media");
    fs::create_dir_all(&directory).map_err(|_| "Could not create media storage.")?;
    let path = directory.join(name);
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value.to_string())
        .and_then(|_| fs::rename(temporary, path))
        .map_err(|_| "Could not save media settings.".into())
}

pub(super) fn media_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || raw.len() > 30_000
        || !url.host_str()?.ends_with(".googlevideo.com")
        || raw.trim() != raw
        || raw.contains(['\r', '\n'])
        || url.fragment().is_some()
    {
        return None;
    }
    // A player's byte range is only one chunk, not the complete media file.
    // Preserve signed query bytes exactly; URL form re-encoding can change
    // the meaning of signature material. Remove only player chunk parameters.
    let Some((base, query)) = raw.split_once('?') else {
        return Some(raw.to_owned());
    };
    let pairs: Vec<_> = query
        .split('&')
        .filter(|pair| {
            !url::form_urlencoded::parse(pair.as_bytes())
                .next()
                .is_some_and(|(key, _)| matches!(key.as_ref(), "range" | "rn" | "rbuf"))
        })
        .collect();
    Some(format!("{base}?{}", pairs.join("&")))
}

fn catalog(command: &Value) -> Result<Value> {
    let id = string(command, "videoId")?;
    let raw = string(command, "raw")?;
    if !video_id(id) || raw.len() > MAX_BYTES || !trusted_origin(string(command, "origin")?, id) {
        return Err("Untrusted media response.".into());
    }
    let data: Value = serde_json::from_str(raw).map_err(|_| "Unreadable media response.")?;
    if data["type"] != "media" || data["requestId"] != command["requestId"] || data["videoId"] != id
    {
        return Err("Media response belongs to another request.".into());
    }
    if data["ok"] != true {
        return Err(data["error"]
            .as_str()
            .unwrap_or("Play the video and try again.")
            .chars()
            .take(500)
            .collect());
    }
    let media = &data["media"];
    let formats = media["formats"]
        .as_array()
        .ok_or("Missing media formats.")?;
    if formats.len() > 500 {
        return Err("Too many media formats.".into());
    }
    let mut streams = Vec::new();
    for format in formats {
        let Some(url) = format["url"].as_str().and_then(media_url) else {
            continue;
        };
        let Some(itag) = format["itag"].as_u64().filter(|i| *i < 100_000) else {
            continue;
        };
        let mime = format["mimeType"].as_str().unwrap_or("");
        // AVC/AAC can be played and remuxed on both target platforms.
        let audio = mime.starts_with("audio/mp4") && mime.contains("mp4a");
        let video = mime.starts_with("video/mp4") && mime.contains("avc1");
        if !audio && !video {
            continue;
        }
        let combined = video && (mime.contains("mp4a") || format["audioQuality"].is_string());
        let kind = if audio {
            "audio"
        } else if combined {
            "combined"
        } else {
            "video"
        };
        let height = format["height"].as_u64().unwrap_or(0).min(8_640);
        let quality = format["qualityLabel"]
            .as_str()
            .filter(|label| {
                label.len() <= 20 && label.bytes().all(|b| b.is_ascii_digit() || b == b'p')
            })
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{height}p"));
        let bytes = format["contentLength"]
            .as_u64()
            .or_else(|| format["contentLength"].as_str()?.parse().ok())
            .unwrap_or(0);
        if streams.iter().any(|s: &Value| s["itag"] == itag) {
            continue;
        }
        let user_agent = format["userAgent"]
            .as_str()
            .filter(|s| s.len() <= 1000 && !s.contains(['\r', '\n']))
            .unwrap_or("");
        streams.push(
            json!({"itag":itag,"url":url,"kind":kind,"mimeType":mime,"userAgent":user_agent,
            "height":height,"width":format["width"].as_u64().unwrap_or(0).min(15_360),
            "initRange":playback::range(&format["initRange"], bytes).map(|(start,end)|json!({"start":start,"end":end})),
            "indexRange":playback::range(&format["indexRange"], bytes).map(|(start,end)|json!({"start":start,"end":end})),
            "bitrate":format["bitrate"].as_u64().unwrap_or(0),"bytes":bytes,
            "label":if audio { "Audio · M4A".to_string() } else { format!("{quality} · MP4") }}),
        );
    }
    streams.sort_by_key(|s| std::cmp::Reverse(s["height"].as_u64().unwrap_or(0)));
    let hls = media["hlsManifestUrl"].as_str().and_then(media_url);
    if streams.is_empty() && hls.is_none() {
        return Err("No playable media captured yet. Start the video, then try again.".into());
    }
    Ok(
        json!({"videoId":id,"origin":Url::parse(string(command,"origin")?).map_err(|_|"Invalid media origin.")?.origin().ascii_serialization(),"title":clean(media["title"].as_str().unwrap_or("YouTube video")),
        "author":clean(media["author"].as_str().unwrap_or("")),"duration":media["duration"].as_f64().unwrap_or(0.0).max(0.0),
        "position":media["position"].as_f64().unwrap_or(0.0).max(0.0),"live":media["live"]==true,
        "streams":streams,"hlsUrl":hls}),
    )
}

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match string(command, "op")? {
        "media_proxy" => super::transport::proxy(command),
        "media_proxy_release" => super::transport::release(command),
        "media_transport_status" => Ok(super::transport::status()),
        "media_range" => {
            let length = command["length"].as_u64().ok_or("Missing media length.")?;
            let offset = command["offset"].as_u64().ok_or("Missing media offset.")?;
            if length == 0 || offset >= length {
                return Err("Invalid media range.".into());
            }
            Ok(json!({"start":offset,"end":(offset + 262_143).min(length-1)}))
        }
        "media_catalog" => catalog(command),
        "media_playback_choices" => playback::choices(&command["catalog"]),
        "media_playback" => super::transport::playback(command),
        "media_speeds" => Ok(json!(playback::SPEEDS)),
        "media_settings" => {
            let defaults = json!({"adBlock":true,"sponsorBlock":true,"backgroundPlayback":true,"autoPip":false,"pauseAfterLine":false,
                "playbackSpeed":1.0,"qualityHeight":0,"categories":["sponsor","selfpromo","interaction"]});
            let mut settings = read(root, "settings.json", defaults.clone());
            if !settings.is_object() {
                settings = defaults.clone();
            }
            for key in [
                "adBlock",
                "sponsorBlock",
                "backgroundPlayback",
                "autoPip",
                "pauseAfterLine",
            ] {
                if !settings[key].is_boolean() {
                    settings[key] = defaults[key].clone();
                }
            }
            if !settings["playbackSpeed"]
                .as_f64()
                .is_some_and(|n| playback::SPEEDS.contains(&n))
            {
                settings["playbackSpeed"] = json!(1.0);
            }
            if !settings["qualityHeight"]
                .as_u64()
                .is_some_and(|n| n <= 8640)
            {
                settings["qualityHeight"] = json!(0);
            }
            if let Some(update) = command["settings"].as_object() {
                if let Some(speed) = update.get("playbackSpeed") {
                    if !speed
                        .as_f64()
                        .is_some_and(|n| playback::SPEEDS.contains(&n))
                    {
                        return Err("Unsupported playback speed.".into());
                    }
                    settings["playbackSpeed"] = speed.clone();
                }
                if let Some(height) = update.get("qualityHeight") {
                    if !height.as_u64().is_some_and(|n| n <= 8640) {
                        return Err("Invalid playback resolution.".into());
                    }
                    settings["qualityHeight"] = height.clone();
                }
                for key in [
                    "adBlock",
                    "sponsorBlock",
                    "backgroundPlayback",
                    "autoPip",
                    "pauseAfterLine",
                ] {
                    if let Some(value) = update.get(key).filter(|v| v.is_boolean()) {
                        settings[key] = value.clone();
                    }
                }
                if let Some(categories) = update.get("categories").and_then(Value::as_array) {
                    settings["categories"] = json!(
                        categories
                            .iter()
                            .filter(|v| matches!(
                                v.as_str(),
                                Some(
                                    "sponsor"
                                        | "selfpromo"
                                        | "interaction"
                                        | "intro"
                                        | "outro"
                                        | "preview"
                                        | "music_offtopic"
                                        | "filler"
                                )
                            ))
                            .take(8)
                            .collect::<Vec<_>>()
                    );
                }
                write(root, "settings.json", &settings)?;
            }
            if !settings["categories"].is_array() {
                settings["categories"] = defaults["categories"].clone();
            }
            Ok(settings)
        }
        "sponsor_request" => {
            let id = string(command, "videoId")?;
            if !video_id(id) {
                return Err("Invalid SponsorBlock video.".into());
            }
            let prefix = format!("{:x}", Sha256::digest(id.as_bytes()));
            let mut url = Url::parse(&format!(
                "https://sponsor.ajay.app/api/skipSegments/{}",
                &prefix[..4]
            ))
            .unwrap();
            url.query_pairs_mut().append_pair("service", "YouTube")
                .append_pair("categories", r#"["sponsor","selfpromo","interaction","intro","outro","preview","music_offtopic","filler"]"#)
                .append_pair("actionType", "skip");
            Ok(json!(url.to_string()))
        }
        "sponsor_parse" => {
            let id = string(command, "videoId")?;
            let raw = string(command, "raw")?;
            if !video_id(id) || raw.len() > 2_000_000 {
                return Err("Invalid SponsorBlock response.".into());
            }
            let data: Value =
                serde_json::from_str(raw).map_err(|_| "Unreadable SponsorBlock response.")?;
            let records = data.as_array().ok_or("Missing SponsorBlock records.")?;
            let mut segments = Vec::new();
            for record in records.iter().filter(|r| r["videoID"] == id) {
                for segment in record["segments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(10_000)
                {
                    let start = segment["segment"][0].as_f64().unwrap_or(-1.0);
                    let end = segment["segment"][1].as_f64().unwrap_or(-1.0);
                    let category = segment["category"].as_str().unwrap_or("");
                    if start.is_finite()
                        && end.is_finite()
                        && start >= 0.0
                        && end > start
                        && end < 604_800.0
                        && segment["actionType"].as_str().unwrap_or("skip") == "skip"
                        && matches!(
                            category,
                            "sponsor"
                                | "selfpromo"
                                | "interaction"
                                | "intro"
                                | "outro"
                                | "preview"
                                | "music_offtopic"
                                | "filler"
                        )
                    {
                        segments.push(json!({"start":start,"end":end,"category":category}));
                    }
                }
            }
            segments.sort_by(|a, b| {
                a["start"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b["start"].as_f64().unwrap())
            });
            Ok(json!(segments))
        }
        "media_plan" => {
            let catalog = &command["catalog"];
            if catalog["live"] == true {
                return Err("Live streams cannot be downloaded as a file.".into());
            }
            let formats = catalog["streams"]
                .as_array()
                .ok_or("Missing download formats.")?;
            let selected = formats
                .iter()
                .find(|s| s["itag"] == command["itag"])
                .ok_or("Selected quality is unavailable. Refresh the video.")?;
            if selected["url"].as_str().and_then(media_url).is_none() {
                return Err("Invalid download URL.".into());
            }
            let mut parts = vec![selected.clone()];
            if selected["kind"] == "video" {
                let audio = formats
                    .iter()
                    .filter(|s| s["kind"] == "audio")
                    .max_by_key(|s| s["bitrate"].as_u64().unwrap_or(0))
                    .ok_or("No compatible audio track captured. Play the video and refresh.")?;
                if audio["url"].as_str().and_then(media_url).is_none() {
                    return Err("Invalid audio URL.".into());
                }
                parts.push(audio.clone());
            }
            Ok(
                json!({"parts":parts,"extension":if selected["kind"]=="audio" {"m4a"} else {"mp4"},
                "mimeType":if selected["kind"]=="audio" {"audio/mp4"} else {"video/mp4"},"label":selected["label"]}),
            )
        }
        "downloads_list" => Ok(read(root, "downloads.json", json!([]))),
        "downloads_save" => {
            let entries = command["entries"]
                .as_array()
                .ok_or("Invalid download history.")?;
            if entries.len() > 200
                || entries.iter().any(|e| {
                    !e["id"].as_str().is_some_and(|s| {
                        s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    }) || !e["videoId"].as_str().is_some_and(video_id)
                        || e.to_string().len() > 15_000
                        || e.get("url").is_some()
                        || e.get("parts").is_some()
                })
            {
                return Err("Invalid download history entry.".into());
            }
            write(root, "downloads.json", &command["entries"])?;
            Ok(Value::Null)
        }
        _ => Err("Unsupported media operation.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_queries_keep_their_original_encoding() {
        let raw = "https://r1.googlevideo.com/videoplayback?sig=A%2fb+C&range=0-99&pot=X%3d%3D&rn=3&rbuf=10";
        assert_eq!(
            media_url(raw).unwrap(),
            "https://r1.googlevideo.com/videoplayback?sig=A%2fb+C&pot=X%3d%3D"
        );
        for invalid in [
            "http://r1.googlevideo.com/a",
            "https://r1.googlevideo.com.evil.org/a",
            "https://user@r1.googlevideo.com/a",
            "https://r1.googlevideo.com:444/a",
            "https://r1.googlevideo.com/a#fragment",
        ] {
            assert!(media_url(invalid).is_none());
        }
    }

    #[test]
    fn catalogs_correlate_identity_and_keep_compatible_client_headers() {
        let response = json!({"type":"media","requestId":"request-1","videoId":"QVv3mQzlrN0","ok":true,"media":{"title":"Video","formats":[
            {"itag":137,"url":"https://r1.googlevideo.com/videoplayback?sig=valid","mimeType":"video/mp4; codecs=\"avc1.640028\"","height":960,"qualityLabel":"1080p","contentLength":"12345","userAgent":"Safari media client"},
            {"itag":399,"url":"https://r1.googlevideo.com/videoplayback","mimeType":"video/mp4; codecs=\"av01.0.08M.08\""},
            {"itag":140,"url":"https://evil.org/audio","mimeType":"audio/mp4; codecs=\"mp4a.40.2\""}
        ]}});
        let mut command = json!({"op":"media_catalog","origin":"https://m.youtube.com","videoId":"QVv3mQzlrN0","requestId":"request-1","raw":response.to_string()});
        let accepted = catalog(&command).unwrap();
        assert_eq!(accepted["streams"].as_array().unwrap().len(), 1);
        assert_eq!(accepted["streams"][0]["label"], "1080p · MP4");
        assert_eq!(accepted["streams"][0]["userAgent"], "Safari media client");
        command["requestId"] = json!("another-request");
        assert!(catalog(&command).is_err());
        command["requestId"] = json!("request-1");
        command["origin"] = json!("https://youtube.com.evil.org");
        assert!(catalog(&command).is_err());
    }

    #[test]
    fn download_plans_pair_selected_video_with_best_audio() {
        let mut command = json!({"op":"media_plan","itag":137,"catalog":{"streams":[
            {"itag":137,"kind":"video","url":"https://r1.googlevideo.com/video","label":"1080p · MP4"},
            {"itag":139,"kind":"audio","url":"https://r1.googlevideo.com/low-audio","bitrate":48000},
            {"itag":140,"kind":"audio","url":"https://r1.googlevideo.com/audio","bitrate":128000}
        ]}});
        let plan = dispatch(&command, "").unwrap();
        assert_eq!(plan["parts"][0]["itag"], 137);
        assert_eq!(plan["parts"][1]["itag"], 140);
        assert_eq!(plan["extension"], "mp4");
        command["catalog"]["live"] = json!(true);
        assert!(dispatch(&command, "").is_err());
    }

    #[test]
    fn sponsor_segments_match_video_and_reject_invalid_windows() {
        let response = json!([
            {"videoID":"other-video","segments":[{"segment":[0,999],"category":"sponsor"}]},
            {"videoID":"QVv3mQzlrN0","segments":[
                {"segment":[30,40],"category":"selfpromo","actionType":"skip"},
                {"segment":[5,10],"category":"sponsor","actionType":"skip"},
                {"segment":[-1,2],"category":"sponsor"},
                {"segment":[10,9],"category":"sponsor"},
                {"segment":[12,15],"category":"sponsor","actionType":"mute"},
                {"segment":[20,30],"category":"unknown"}
            ]}
        ]);
        let segments = dispatch(
            &json!({"op":"sponsor_parse","videoId":"QVv3mQzlrN0","raw":response.to_string()}),
            "",
        )
        .unwrap();
        assert_eq!(
            segments,
            json!([{"start":5.0,"end":10.0,"category":"sponsor"},{"start":30.0,"end":40.0,"category":"selfpromo"}])
        );
        let request =
            dispatch(&json!({"op":"sponsor_request","videoId":"QVv3mQzlrN0"}), "").unwrap();
        assert!(!request.as_str().unwrap().contains("QVv3mQzlrN0"));
    }

    #[test]
    fn playback_preferences_migrate_validate_and_survive_reload() {
        let mut unique = [0u8; 8];
        getrandom::getrandom(&mut unique).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "lingoplay-playback-{:x}",
            u64::from_ne_bytes(unique)
        ));
        let root = directory.to_str().unwrap();
        write(root, "settings.json", &json!({"adBlock":false,"sponsorBlock":true,"backgroundPlayback":true,"autoPip":false,"categories":["sponsor"]})).unwrap();
        let migrated = dispatch(&json!({"op":"media_settings"}), root).unwrap();
        assert_eq!(migrated["playbackSpeed"], 1.0);
        assert_eq!(migrated["qualityHeight"], 0);
        assert_eq!(migrated["pauseAfterLine"], false);
        dispatch(&json!({"op":"media_settings","settings":{"playbackSpeed":1.5,"qualityHeight":1080,"pauseAfterLine":true,"url":"https://r1.googlevideo.com/private"}}), root).unwrap();
        let restored = dispatch(&json!({"op":"media_settings"}), root).unwrap();
        assert_eq!(restored["playbackSpeed"], 1.5);
        assert_eq!(restored["qualityHeight"], 1080);
        assert_eq!(restored["pauseAfterLine"], true);
        assert_eq!(restored["adBlock"], false);
        assert_eq!(restored["categories"], json!(["sponsor"]));
        for invalid in [
            json!({"playbackSpeed":0}),
            json!({"playbackSpeed":"1.5"}),
            json!({"playbackSpeed":16}),
            json!({"qualityHeight":-1}),
            json!({"qualityHeight":8641}),
        ] {
            assert!(dispatch(&json!({"op":"media_settings","settings":invalid}), root).is_err());
        }
        assert_eq!(
            dispatch(&json!({"op":"media_settings"}), root).unwrap(),
            restored
        );
        assert!(
            !fs::read_to_string(directory.join("media/settings.json"))
                .unwrap()
                .contains("private")
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
