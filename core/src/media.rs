use super::{MAX_BYTES, Result, clean, string, trusted_origin, video_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use url::Url;
pub(crate) mod dash;
mod live;
pub(crate) mod playback;

// Reuse a freshly validated extraction for an immediate download. Signed URLs
// stay in this bounded, short-lived memory cache and never enter media storage.
struct CachedCatalog {
    root: String,
    video_id: String,
    at: Instant,
    catalog: Value,
}
static CATALOGS: OnceLock<Mutex<VecDeque<CachedCatalog>>> = OnceLock::new();
fn cached_catalog(root: &str, id: &str) -> Option<Value> {
    let mut entries = CATALOGS.get_or_init(Default::default).lock().ok()?;
    entries.retain(|entry| entry.at.elapsed() < Duration::from_secs(90));
    entries
        .iter()
        .find(|entry| entry.root == root && entry.video_id == id)
        .map(|entry| entry.catalog.clone())
}
fn cache_catalog(root: &str, catalog: &Value) {
    let Some(id) = catalog["videoId"].as_str() else {
        return;
    };
    let Ok(mut entries) = CATALOGS.get_or_init(Default::default).lock() else {
        return;
    };
    entries.retain(|entry| {
        entry.at.elapsed() < Duration::from_secs(90)
            && !(entry.root == root && entry.video_id == id)
    });
    entries.push_front(CachedCatalog {
        root: root.into(),
        video_id: id.into(),
        at: Instant::now(),
        catalog: catalog.clone(),
    });
    while entries.len() > 3 {
        entries.pop_back();
    }
}

pub(crate) fn read(root: &str, name: &str, fallback: Value) -> Value {
    fs::read(Path::new(root).join("media").join(name))
        .ok()
        .filter(|b| b.len() <= MAX_BYTES)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(fallback)
}

pub(crate) fn write(root: &str, name: &str, value: &Value) -> Result<()> {
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

pub(crate) fn container(stream: &Value) -> Option<&str> {
    stream["mimeType"]
        .as_str()?
        .split(';')
        .next()
        .map(str::trim)
        .filter(|mime| {
            matches!(
                *mime,
                "video/mp4" | "audio/mp4" | "video/webm" | "audio/webm"
            )
        })
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
        let media_type = container(format).unwrap_or("");
        let audio = (media_type == "audio/mp4" && mime.contains("mp4a"))
            || (media_type == "audio/webm" && mime.contains("opus"));
        let video = matches!(media_type, "video/mp4" | "video/webm")
            && ["avc1", "vp09", "vp9", "av01", "hvc1", "hev1"]
                .iter()
                .any(|codec| mime.contains(codec));
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
        let audio_id = format["audioTrack"]["id"]
            .as_str()
            .filter(|s| {
                s.len() <= 128
                    && s.chars()
                        .all(|c| c.is_alphanumeric() || "_-.|:".contains(c))
            })
            .unwrap_or("");
        let key = format!("{itag}:{audio_id}");
        if streams.iter().any(|s: &Value| s["key"] == key) {
            continue;
        }
        let user_agent = format["userAgent"]
            .as_str()
            .filter(|s| s.len() <= 1000 && !s.contains(['\r', '\n']))
            .unwrap_or("");
        streams.push(
            json!({"itag":itag,"key":key,"url":url,"kind":kind,"mimeType":mime,"userAgent":user_agent,
            "fps":format["fps"].as_u64().unwrap_or(0).min(240),"audioKey":audio_id,
            "audioSampleRate":format["audioSampleRate"].as_u64().or_else(||format["audioSampleRate"].as_str()?.parse().ok()).unwrap_or(0).min(384000),
            "audioChannels":format["audioChannels"].as_u64().unwrap_or(0).min(32),
            "audioLabel":clean(format["audioTrack"]["displayName"].as_str().unwrap_or("Default")),
            "audioDefault":format["audioTrack"]["audioIsDefault"]==true,
            "audioOriginal":format["audioTrack"]["isOriginal"]==true || format["audioTrack"]["displayName"].as_str().is_some_and(|n|n.to_lowercase().contains("original")),
            "height":height,"width":format["width"].as_u64().unwrap_or(0).min(15_360),
            "initRange":playback::range(&format["initRange"], bytes).map(|(start,end)|json!({"start":start,"end":end})),
            "indexRange":playback::range(&format["indexRange"], bytes).map(|(start,end)|json!({"start":start,"end":end})),
            "bitrate":format["bitrate"].as_u64().unwrap_or(0),"bytes":bytes,
            "label":if audio { if media_type=="audio/webm" {"Audio · WebM".to_string()} else {"Audio · M4A".to_string()} } else { format!("{quality} · {}",if media_type=="video/webm" {"WebM"}else{"MP4"}) }}),
        );
    }
    streams.sort_by_key(|s| std::cmp::Reverse(s["height"].as_u64().unwrap_or(0)));
    let hls = media["hlsManifestUrl"].as_str().and_then(media_url);
    if streams.is_empty() && hls.is_none() {
        return Err("No playable media captured yet. Start the video, then try again.".into());
    }
    let mut audio_tracks = Vec::new();
    for stream in &streams {
        let key = stream["audioKey"].as_str().unwrap_or("");
        if stream["kind"] == "audio"
            && !key.is_empty()
            && !audio_tracks.iter().any(|t: &Value| t["key"] == key)
        {
            audio_tracks.push(json!({"key":key,"label":stream["audioLabel"]}));
        }
    }
    let chapters = chapter_list(&media["chapters"]);
    Ok(
        json!({"videoId":id,"origin":Url::parse(string(command,"origin")?).map_err(|_|"Invalid media origin.")?.origin().ascii_serialization(),"title":clean(media["title"].as_str().unwrap_or("YouTube video")),
        "author":clean(media["author"].as_str().unwrap_or("")),"duration":media["duration"].as_f64().filter(|n|*n>0.0&&*n<=604800.0).unwrap_or_else(||formats.iter().filter_map(|f|f["approxDurationMs"].as_u64().or_else(||f["approxDurationMs"].as_str()?.parse().ok())).filter(|n|*n>0&&*n<=604800000).max().unwrap_or(0) as f64/1000.0),
        "position":media["position"].as_f64().unwrap_or(0.0).max(0.0),"live":media["live"]==true,
        "streams":streams,"hlsUrl":hls,"hlsUserAgent":media["hlsUserAgent"].as_str().filter(|s|s.len()<=1000&&!s.chars().any(char::is_control)).unwrap_or(""),"audioTracks":audio_tracks,
        "subtitles":media["subtitles"].as_array().map(|items|items.iter().take(200).filter_map(|t|{
            let id=t["id"].as_str().filter(|s|s.len()<=512)?;
            Some(json!({"id":id,"label":clean(t["label"].as_str().unwrap_or("Captions")),"language":clean(t["language"].as_str().unwrap_or("")),"automatic":t["automatic"]==true}))
        }).collect::<Vec<_>>()).unwrap_or_default(),
        "chapters":chapters}),
    )
}

fn chapter_list(raw: &Value) -> Vec<Value> {
    let mut chapters=raw.as_array().map(|items|items.iter().take(300).filter_map(|chapter|{
        let parsed=||{let fields=chapter["timestamp"].as_str()?.split(':').map(str::parse::<u32>).collect::<std::result::Result<Vec<_>,_>>().ok()?;if !(2..=3).contains(&fields.len())||fields.iter().skip(1).any(|n|*n>=60){return None;}Some(fields.iter().fold(0.0,|total,n|total*60.0+f64::from(*n)))};
        let start=chapter["start"].as_f64().or_else(parsed).filter(|n|n.is_finite()&&*n>=0.0&&*n<604800.0)?;
        let preview = chapter["previewUrl"].as_str().and_then(|raw| crate::gallery::allowed(raw).ok()).map(|url| url.to_string());
        Some(json!({"start":start,"title":clean(chapter["title"].as_str().unwrap_or("Segments")),"previewUrl":preview}))
    }).collect::<Vec<_>>()).unwrap_or_default();
    chapters.sort_by(|a, b| {
        a["start"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&b["start"].as_f64().unwrap_or(0.0))
    });
    chapters.dedup_by(|a, b| {
        (a["start"].as_f64().unwrap_or(0.0) - b["start"].as_f64().unwrap_or(0.0)).abs() < 0.01
    });
    chapters
}

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match string(command, "op")? {
        "downloads_config"
        | "downloads_options"
        | "downloads_plan"
        | "downloads_preview"
        | "downloads_subtitles"
        | "downloads_view"
        | "downloads_targets"
        | "downloads_export_name"
        | "downloads_enqueue"
        | "downloads_claim"
        | "downloads_prepared"
        | "downloads_event"
        | "downloads_remove"
        | "downloads_restore" => crate::downloads::dispatch(command, root),
        "downloads_clear_history" => crate::downloads::dispatch(command, root),
        "downloads_publication_plan"
        | "downloads_publication_check"
        | "downloads_publication_target"
        | "downloads_publication_verified" => crate::downloads::dispatch(command, root),
        "downloads_transfer_start"
        | "downloads_transfer_poll"
        | "downloads_transfer_pause"
        | "downloads_transfer_cancel"
        | "downloads_transfer_release" => crate::downloads::dispatch(command, root),
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
        "media_catalog" => {
            let result = catalog(command)?;
            cache_catalog(root, &result);
            Ok(result)
        }
        "media_message_origin" => {
            let id = string(command, "videoId")?;
            Ok(json!(
                video_id(id) && trusted_origin(string(command, "origin")?, id)
            ))
        }
        "media_catalog_cached" => {
            let id = string(command, "videoId")?;
            if !video_id(id) {
                return Err("Invalid video identity.".into());
            }
            Ok(cached_catalog(root, id).unwrap_or(Value::Null))
        }
        "media_chapters" => {
            let id = string(command, "videoId")?;
            let request = string(command, "requestId")?;
            let raw = string(command, "raw")?;
            if !video_id(id)
                || request.len() > 200
                || raw.len() > 2_000_000
                || !trusted_origin(string(command, "origin")?, id)
            {
                return Err("Invalid chapter response.".into());
            }
            let data: Value = serde_json::from_str(raw).map_err(|_| "Unreadable chapters.")?;
            if data["videoId"] != id
                || data["requestId"] != request
                || data["type"] != "mediaChapters"
            {
                return Err("The chapter request changed.".into());
            }
            Ok(json!(chapter_list(&data["chapters"])))
        }
        "media_title_request" => {
            let id = string(command, "videoId")?;
            if !video_id(id) {
                return Err("Invalid video identity.".into());
            }
            let mut url = Url::parse("https://www.youtube.com/oembed").unwrap();
            url.query_pairs_mut()
                .append_pair("url", &format!("https://www.youtube.com/watch?v={id}"))
                .append_pair("format", "json");
            Ok(json!(url.to_string()))
        }
        "media_title_accept" => {
            if !video_id(string(command, "videoId")?) {
                return Err("Invalid video identity.".into());
            }
            let raw = string(command, "raw")?;
            if raw.len() > 65536 {
                return Err("Video metadata is too large.".into());
            }
            let data: Value =
                serde_json::from_str(raw).map_err(|_| "Unreadable video metadata.")?;
            let title = clean(string(&data, "title")?)
                .chars()
                .take(1000)
                .collect::<String>();
            if title.is_empty() {
                return Err("Video title is unavailable.".into());
            }
            Ok(json!(title))
        }
        "media_capabilities" => {
            let keys = command["keys"]
                .as_array()
                .filter(|v| v.len() <= 500)
                .ok_or("Invalid device capabilities.")?;
            let mut catalog = command["catalog"].clone();
            let formats = catalog["streams"]
                .as_array_mut()
                .filter(|v| v.len() <= 500)
                .ok_or("Invalid media catalog.")?;
            for format in formats.iter_mut() {
                format["supported"] = json!(keys.iter().any(|key| key == &format["key"]));
            }
            let playable_audio: Vec<_> = formats
                .iter()
                .filter(|s| s["kind"] == "audio" && s["supported"] == true)
                .map(|s| s["audioKey"].clone())
                .collect();
            if let Some(tracks) = catalog["audioTracks"].as_array_mut() {
                tracks.retain(|track| playable_audio.contains(&track["key"]));
            }
            Ok(catalog)
        }
        "media_playback_choices" => playback::choices(&command["catalog"]),
        "media_live_choices" => live::choices(command),
        "media_live_select" => live::select(command),
        "media_playback" => super::transport::playback(command),
        "media_speeds" => Ok(json!(playback::SPEEDS)),
        "media_settings" => crate::player::preferences::settings(command, root),
        "media_settings_catalog" => Ok(crate::player::preferences::catalog()),
        "media_settings_export" => crate::player::preferences::export(root),
        "media_settings_import" => crate::player::preferences::import(&command["data"], root),
        "sponsor_control" => crate::player::sponsor::control(command, root),
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
                .append_pair("categories", r#"["sponsor","selfpromo","interaction","intro","outro","preview","music_offtopic","filler","poi_highlight"]"#)
                .append_pair("actionTypes", r#"["skip","poi"]"#);
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
                        && (end > start || end == start && category == "poi_highlight")
                        && end < 604_800.0
                        && matches!(
                            segment["actionType"].as_str().unwrap_or("skip"),
                            "skip" | "poi"
                        )
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
                                | "poi_highlight"
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
                .find(|s| {
                    s["itag"] == command["itag"]
                        && matches!(container(s), Some("video/mp4" | "audio/mp4"))
                })
                .ok_or("Selected quality is unavailable. Refresh the video.")?;
            if selected["url"].as_str().and_then(media_url).is_none() {
                return Err("Invalid download URL.".into());
            }
            let mut parts = vec![selected.clone()];
            if selected["kind"] == "video" {
                let audio = formats
                    .iter()
                    .filter(|s| s["kind"] == "audio" && container(s) == Some("audio/mp4"))
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
        "downloads_list" => {
            crate::downloads::history(&json!({"entries":read(root,"downloads.json",json!([]))}))
        }
        "downloads_save" => {
            let entries = command["entries"]
                .as_array()
                .ok_or("Invalid download history.")?;
            if entries.len() > 2000
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
            write(root, "downloads.json", &crate::downloads::history(command)?)?;
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
        assert_eq!(accepted["streams"].as_array().unwrap().len(), 2);
        assert_eq!(
            accepted["streams"][1]["itag"], 399,
            "retain AV1 for device capability selection"
        );
        assert_eq!(accepted["streams"][0]["label"], "1080p · MP4");
        assert_eq!(accepted["streams"][0]["userAgent"], "Safari media client");
        command["requestId"] = json!("another-request");
        assert!(catalog(&command).is_err());
        command["requestId"] = json!("request-1");
        command["origin"] = json!("https://youtube.com.evil.org");
        assert!(catalog(&command).is_err());
    }

    #[test]
    fn immediate_download_catalogs_are_ephemeral_scoped_and_expire() {
        let root = format!("catalog-cache-test-{}", std::process::id());
        let response = json!({"type":"media","requestId":"fresh","videoId":"abcdefghijk","ok":true,"media":{"title":"Cached video","formats":[{"itag":18,"url":"https://r1.googlevideo.com/videoplayback?sig=in-memory-only","mimeType":"video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"","height":360,"contentLength":"1000"}]}});
        let command = json!({"op":"media_catalog","origin":"https://m.youtube.com/watch?v=abcdefghijk","videoId":"abcdefghijk","requestId":"fresh","raw":response.to_string()});
        let accepted = dispatch(&command, &root).unwrap();
        assert_eq!(
            dispatch(
                &json!({"op":"media_catalog_cached","videoId":"abcdefghijk"}),
                &root
            )
            .unwrap(),
            accepted
        );
        assert_eq!(
            dispatch(
                &json!({"op":"media_catalog_cached","videoId":"abcdefghijk"}),
                "another-profile"
            )
            .unwrap(),
            Value::Null
        );
        assert_eq!(
            dispatch(
                &json!({"op":"media_catalog_cached","videoId":"0123456789a"}),
                &root
            )
            .unwrap(),
            Value::Null
        );
        let mut stale = command;
        stale["requestId"] = json!("stale");
        assert!(dispatch(&stale, &root).is_err());
        assert_eq!(cached_catalog(&root, "abcdefghijk").unwrap(), accepted);
        assert!(
            !Path::new(&root).exists(),
            "signed catalogs must never create storage"
        );
        for index in 0..4 {
            let mut next = accepted.clone();
            next["videoId"] = json!(format!("{index:011}"));
            cache_catalog(&root, &next);
        }
        assert_eq!(cached_catalog(&root, "abcdefghijk"), None);
        {
            let mut cache = CATALOGS.get().unwrap().lock().unwrap();
            assert!(cache.len() <= 3);
            for entry in cache.iter_mut().filter(|entry| entry.root == root) {
                entry.at = Instant::now() - Duration::from_secs(91);
            }
        }
        assert_eq!(cached_catalog(&root, "00000000003"), None);
    }

    #[test]
    fn download_plans_pair_selected_video_with_best_audio() {
        let mut command = json!({"op":"media_plan","itag":137,"catalog":{"streams":[
            {"itag":137,"kind":"video","mimeType":"video/mp4","url":"https://r1.googlevideo.com/video","label":"1080p · MP4"},
            {"itag":139,"kind":"audio","mimeType":"audio/mp4","url":"https://r1.googlevideo.com/low-audio","bitrate":48000},
            {"itag":140,"kind":"audio","mimeType":"audio/mp4","url":"https://r1.googlevideo.com/audio","bitrate":128000}
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
