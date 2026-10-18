//! Download choices and configuration follow LiTube's confirmation sheet.
//! Signed addresses occur only in transient plans, never in preferences.
use crate::{Result, clean, media, string, video_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
mod publication;
mod store;
mod transfer;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Config {
    audio_only: bool,
    quality_height: u32,
    quality_fps: u32,
    video_key: String,
    audio_key: String,
    audio_bitrate: u64,
    include_subtitle: bool,
    subtitle_key: String,
    include_cover: bool,
    file_name: String,
    attachments_only: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            audio_only: false,
            quality_height: 1080,
            quality_fps: 0,
            video_key: String::new(),
            audio_key: String::new(),
            audio_bitrate: 0,
            include_subtitle: false,
            subtitle_key: String::new(),
            include_cover: false,
            file_name: String::new(),
            attachments_only: false,
        }
    }
}
impl Config {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = if value.is_null() {
            Self::default()
        } else {
            serde_json::from_value(value.clone()).map_err(|_| "Invalid download options.")?
        };
        if config.quality_height > 8640
            || config.quality_fps > 240
            || config.audio_bitrate > 10_000_000
            || config.video_key.len() > 512
            || config.audio_key.len() > 512
            || config.subtitle_key.len() > 512
            || config.file_name.len() > 1000
        {
            return Err("Invalid download options.".into());
        }
        if [&config.video_key, &config.audio_key, &config.subtitle_key]
            .iter()
            .any(|s| {
                !s.chars()
                    .all(|c| c.is_alphanumeric() || "_-.|:".contains(c))
            })
        {
            return Err("Invalid download track identity.".into());
        }
        Ok(config)
    }
}

pub(crate) fn history(command: &Value) -> Result<Value> {
    let entries = command["entries"]
        .as_array()
        .filter(|e| e.len() <= 2000)
        .ok_or("Invalid download history.")?;
    let safe_file = |value: &Value| {
        value.as_str().is_some_and(|s| {
            s.len() <= 255
                && !s.starts_with('.')
                && !s.contains("..")
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
    };
    let mut output = Vec::new();
    for entry in entries {
        if !entry["id"].as_str().is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) || !entry["videoId"].as_str().is_some_and(video_id)
        {
            return Err("Invalid download identity.".into());
        }
        let state = entry["state"].as_str().unwrap_or("failed");
        if !matches!(
            state,
            "queued"
                | "resolving"
                | "downloading"
                | "paused"
                | "combining"
                | "complete"
                | "failed"
                | "cancelled"
                | "skipped"
        ) {
            return Err("Invalid download state.".into());
        }
        if !safe_file(&entry["file"]) {
            return Err("Invalid download file.".into());
        }
        let mut record = json!({"id":entry["id"],"videoId":entry["videoId"],"title":clean(entry["title"].as_str().unwrap_or("Video")),"author":clean(entry["author"].as_str().unwrap_or("")),"label":clean(entry["label"].as_str().unwrap_or("")),"itag":entry["itag"].as_u64().unwrap_or(0),"state":state,"received":entry["received"].as_u64().unwrap_or(0).min(200_000_000_000),"total":entry["total"].as_u64().unwrap_or(0).min(200_000_000_000),"file":entry["file"],"mimeType":clean(entry["mimeType"].as_str().unwrap_or("video/mp4")),"watchPageDownloaded":state=="complete"&&entry["config"]["attachmentsOnly"]!=true});
        if let Some(generation) = entry["publicationGeneration"].as_u64() {
            if generation > 9_007_199_254_740_991 {
                return Err("Invalid publication generation.".into());
            }
            record["publicationGeneration"] = json!(generation);
        }
        for field in ["name", "batchId"] {
            if let Some(text) = entry[field].as_str() {
                record[field] = json!(filename(text));
            }
        }
        if let Some(error) = entry["error"].as_str() {
            record["error"] = json!(if error.contains("://") {
                "Download failed. Refresh the video and try again.".to_string()
            } else {
                clean(error).chars().take(500).collect()
            });
        }
        if entry["config"].is_object() {
            record["config"] = json!(Config::parse(&entry["config"])?);
        }
        if let Some(artifacts) = entry["artifacts"].as_array() {
            if artifacts.len() > 3 {
                return Err("Too many download artifacts.".into());
            }
            let mut safe = Vec::new();
            for artifact in artifacts {
                if !safe_file(&artifact["file"]) {
                    return Err("Invalid saved attachment.".into());
                }
                let mut saved = json!({"file":artifact["file"],"mimeType":clean(artifact["mimeType"].as_str().unwrap_or("application/octet-stream")),"label":clean(artifact["label"].as_str().unwrap_or("File"))});
                publication::sanitize(artifact, &mut saved)?;
                safe.push(saved);
            }
            record["artifacts"] = json!(safe);
        }
        output.push(record);
    }
    Ok(json!(output))
}

fn filename(value: &str) -> String {
    let mut output = String::new();
    for c in clean(value).chars() {
        if output.len() + c.len_utf8() > 180 {
            break;
        }
        output.push(if c.is_control() || "\\/:*?\"<>|".contains(c) {
            '_'
        } else {
            c
        });
    }
    let output = output.trim_matches(['.', ' ']);
    if output.is_empty() {
        "video".into()
    } else {
        output.into()
    }
}

fn preferences(root: &str) -> Value {
    let saved = media::read(root, "download-settings.json", json!({}));
    json!({"wifiOnly":saved["wifiOnly"].as_bool().unwrap_or(false),"maxConnections":saved["maxConnections"].as_u64().filter(|n|(1..=16).contains(n)).unwrap_or(4),"defaultQuality":saved["defaultQuality"].as_u64().filter(|n|*n<=8640).unwrap_or(1080),"lastConfig":Config::parse(&saved["lastConfig"]).unwrap_or_default()})
}

fn formats(catalog: &Value) -> Result<&Vec<Value>> {
    catalog["streams"]
        .as_array()
        .filter(|v| v.len() <= 500)
        .ok_or_else(|| "Missing download formats.".into())
}
fn videos(catalog: &Value) -> Result<Vec<&Value>> {
    let has_audio = formats(catalog)?
        .iter()
        .any(|s| s["kind"] == "audio" && media::container(s) == Some("audio/mp4"));
    let mut selected: Vec<_> = formats(catalog)?
        .iter()
        .filter(|s| {
            (s["kind"] == "video" && has_audio || s["kind"] == "combined")
                && media::container(s) == Some("video/mp4")
                && s["height"].as_u64().is_some_and(|n| n > 0 && n <= 8640)
                && s["bytes"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= 100_000_000_000)
                && s["url"].as_str().and_then(media::media_url).is_some()
        })
        .collect();
    selected.sort_by_key(|s| {
        std::cmp::Reverse((
            s["height"].as_u64().unwrap_or(0),
            s["fps"].as_u64().unwrap_or(0),
            s["kind"] == "video",
            s["bitrate"].as_u64().unwrap_or(0),
        ))
    });
    selected.dedup_by_key(|s| (s["height"].as_u64(), s["fps"].as_u64()));
    Ok(selected)
}
fn audio<'a>(catalog: &'a Value, config: &Config) -> Result<Option<&'a Value>> {
    Ok(formats(catalog)?
        .iter()
        .filter(|s| {
            s["kind"] == "audio"
                && media::container(s) == Some("audio/mp4")
                && (config.audio_key.is_empty() || s["audioKey"] == config.audio_key)
                && s["url"].as_str().and_then(media::media_url).is_some()
        })
        .max_by_key(|s| {
            (
                s["audioOriginal"] == true,
                s["audioDefault"] == true,
                config.audio_bitrate == 0
                    || s["bitrate"].as_u64().unwrap_or(0) <= config.audio_bitrate,
                s["bitrate"].as_u64().unwrap_or(0),
            )
        }))
}
fn label(stream: &Value) -> String {
    let height = stream["height"].as_u64().unwrap_or(0);
    let fps = stream["fps"].as_u64().unwrap_or(0);
    format!(
        "{height}p{}",
        if fps > 30 {
            fps.to_string()
        } else {
            String::new()
        }
    )
}
fn selected<'a>(catalog: &'a Value, config: &Config) -> Result<Vec<&'a Value>> {
    if config.attachments_only {
        return Ok(vec![]);
    }
    if catalog["live"] == true {
        return Err("Live streams cannot be saved as files.".into());
    }
    let audio = audio(catalog, config)?;
    if config.audio_only {
        return audio
            .map(|s| vec![s])
            .ok_or_else(|| "The selected audio track is unavailable.".into());
    }
    let videos = videos(catalog)?;
    let video = if !config.video_key.is_empty() {
        videos
            .iter()
            .copied()
            .find(|s| s["key"] == config.video_key)
    } else {
        None
    }
    .or_else(|| {
        videos.iter().copied().find(|s| {
            config.quality_height == 0
                || s["height"].as_u64().unwrap_or(0) <= u64::from(config.quality_height)
        })
    })
    .or_else(|| videos.last().copied())
    .ok_or("No downloadable video formats are available.")?;
    if video["kind"] == "combined" {
        return Ok(vec![video]);
    }
    Ok(vec![
        video,
        audio.ok_or("The selected audio track is unavailable.")?,
    ])
}

fn plan(catalog: &Value, config: &Config) -> Result<Value> {
    let id = string(catalog, "videoId")?;
    if !video_id(id) {
        return Err("Invalid download video.".into());
    }
    let parts = selected(catalog, config)?;
    if config.attachments_only && !config.include_cover && !config.include_subtitle {
        return Err("Select a cover image or subtitles to download.".into());
    }
    for part in &parts {
        if !part["bytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 100_000_000_000)
            || part["url"].as_str().and_then(media::media_url).is_none()
        {
            return Err("The media length is unavailable. Refresh formats.".into());
        }
    }
    let subtitle = if config.include_subtitle {
        let tracks = catalog["subtitles"]
            .as_array()
            .ok_or("No subtitles are available.")?;
        tracks
            .iter()
            .find(|t| {
                config.subtitle_key.is_empty()
                    || t["id"] == config.subtitle_key
                    || t["language"] == config.subtitle_key
            })
            .cloned()
            .ok_or("The selected subtitle language is unavailable.")?
    } else {
        Value::Null
    };
    let title = parts
        .first()
        .map(|s| {
            if config.audio_only {
                format!("Audio · {} kbps", s["bitrate"].as_u64().unwrap_or(0) / 1000)
            } else {
                format!("{} · MP4", label(s))
            }
        })
        .unwrap_or_else(|| "Attachments".into());
    Ok(
        json!({"parts":parts,"extension":if config.audio_only{"m4a"}else{"mp4"},"mimeType":if config.audio_only{"audio/mp4"}else{"video/mp4"},"label":title,"name":filename(if config.file_name.trim().is_empty(){catalog["title"].as_str().unwrap_or(id)}else{&config.file_name}),"bytes":parts.iter().map(|s|s["bytes"].as_u64().unwrap_or(0)).sum::<u64>(),"coverUrl":if config.include_cover{Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"))}else{None},"subtitle":subtitle,"attachmentsOnly":config.attachments_only}),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep each operation and its shared state validation and persistence in one dispatcher."
)]
pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match string(command, "op")? {
        "downloads_export_name" => {
            let entries = media::read(root, "downloads.json", json!([]));
            let id = string(command, "id")?;
            let entry = entries
                .as_array()
                .and_then(|e| e.iter().find(|e| e["id"] == id))
                .ok_or("Download no longer exists.")?;
            let file = string(command, "file")?;
            Ok(json!(publication::name(entry, file)?))
        }
        "downloads_targets" => {
            let items = command["items"]
                .as_array()
                .filter(|i| i.len() <= 500)
                .ok_or("Invalid download selection.")?;
            let mut safe = Vec::new();
            for item in items {
                if item["videoId"].as_str().is_some_and(video_id)
                    && !safe.iter().any(|s: &Value| s["videoId"] == item["videoId"])
                {
                    safe.push(json!({"videoId":item["videoId"],"title":clean(item["title"].as_str().unwrap_or("Video")).chars().take(1000).collect::<String>(),"author":clean(item["author"].as_str().unwrap_or("")).chars().take(500).collect::<String>()}));
                }
            }
            if safe.is_empty() {
                return Err("No downloadable videos were found.".into());
            }
            Ok(
                json!({"name":clean(command["name"].as_str().unwrap_or("Playlist")).chars().take(200).collect::<String>(),"items":safe}),
            )
        }
        "downloads_transfer_start"
        | "downloads_transfer_poll"
        | "downloads_transfer_pause"
        | "downloads_transfer_cancel"
        | "downloads_transfer_release" => transfer::dispatch(command, root),
        "downloads_enqueue"
        | "downloads_claim"
        | "downloads_prepared"
        | "downloads_event"
        | "downloads_remove"
        | "downloads_restore"
        | "downloads_clear_history"
        | "downloads_publication_plan"
        | "downloads_publication_check"
        | "downloads_publication_target"
        | "downloads_publication_verified" => store::dispatch(command, root),
        "downloads_config" => {
            let mut settings = preferences(root);
            if let Some(update) = command["settings"].as_object() {
                for (key, value) in update {
                    match key.as_str() {
                        "wifiOnly" if value.is_boolean() => settings[key] = value.clone(),
                        "maxConnections"
                            if value.as_u64().is_some_and(|n| (1..=16).contains(&n)) =>
                        {
                            settings[key] = value.clone();
                        }
                        "defaultQuality" if value.as_u64().is_some_and(|n| n <= 8640) => {
                            settings[key] = value.clone();
                        }
                        "lastConfig" => {
                            let mut config = Config::parse(value)?;
                            config.video_key.clear();
                            settings[key] = json!(config);
                        }
                        _ => return Err("Invalid download preferences.".into()),
                    }
                }
                media::write(root, "download-settings.json", &settings)?;
            }
            Ok(settings)
        }
        "downloads_options" => {
            let catalog = &command["catalog"];
            let config = Config::parse(&command["config"])?;
            let qualities=videos(catalog)?.iter().map(|s|json!({"key":s["key"],"height":s["height"],"fps":s["fps"],"label":label(s)})).collect::<Vec<_>>();
            let mut bitrates: Vec<_> = formats(catalog)?
                .iter()
                .filter(|s| {
                    s["kind"] == "audio"
                        && media::container(s) == Some("audio/mp4")
                        && (config.audio_key.is_empty() || s["audioKey"] == config.audio_key)
                })
                .filter_map(|s| s["bitrate"].as_u64())
                .collect();
            bitrates.sort_unstable_by(|a, b| b.cmp(a));
            bitrates.dedup();
            let preview = plan(catalog, &config);
            let audio_tracks = catalog["audioTracks"]
                .as_array()
                .map(|tracks| {
                    tracks
                        .iter()
                        .filter(|track| {
                            formats(catalog).is_ok_and(|formats| {
                                formats.iter().any(|s| {
                                    s["kind"] == "audio"
                                        && media::container(s) == Some("audio/mp4")
                                        && s["audioKey"] == track["key"]
                                })
                            })
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(
                json!({"qualities":qualities,"audioTracks":audio_tracks,"subtitles":catalog["subtitles"],"bitrates":bitrates,"preview":preview.as_ref().ok(),"error":preview.err()}),
            )
        }
        "downloads_plan" | "downloads_preview" => {
            plan(&command["catalog"], &Config::parse(&command["config"])?)
        }
        "downloads_subtitles" => {
            let data = crate::load(
                root,
                string(command, "videoId")?,
                string(command, "trackId")?,
            )?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "Subtitle timestamps intentionally round nonnegative seconds to saturating integer milliseconds."
            )]
            let stamp = |seconds: f64| {
                let ms = (seconds.max(0.0) * 1000.0).round() as u64;
                format!(
                    "{:02}:{:02}:{:02},{:03}",
                    ms / 3_600_000,
                    ms / 60_000 % 60,
                    ms / 1000 % 60,
                    ms % 1000
                )
            };
            let mut result = String::new();
            for (i, cue) in data.segments.iter().enumerate() {
                let end = if cue.duration > 0.0 {
                    cue.start + cue.duration
                } else {
                    data.segments
                        .get(i + 1)
                        .map_or(cue.start + 3.0, |n| n.start)
                };
                let _ = write!(
                    result,
                    "{}\n{} --> {}\n{}\n\n",
                    i + 1,
                    stamp(cue.start),
                    stamp(end),
                    cue.text
                );
            }
            Ok(json!(result))
        }
        "downloads_view" => {
            let entries = command["entries"]
                .as_array()
                .filter(|v| v.len() <= 2000)
                .ok_or("Invalid downloads list.")?;
            let query = command["query"].as_str().unwrap_or("").to_lowercase();
            let filter = command["filter"].as_str().unwrap_or("all");
            let batch_id = command["batchId"].as_str().unwrap_or("");
            if batch_id.len() > 100
                || !batch_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err("Invalid download batch.".into());
            }
            let items: Vec<_> = entries
                .iter()
                .filter(|e| batch_id.is_empty() || e["batchId"] == batch_id)
                .filter(|e| {
                    let state = e["state"].as_str().unwrap_or("");
                    (filter == "all"
                        || filter == "active"
                            && matches!(
                                state,
                                "downloading" | "paused" | "combining" | "queued" | "resolving"
                            )
                        || filter == "complete" && state == "complete"
                        || filter == "terminal"
                            && matches!(state, "complete" | "failed" | "cancelled" | "skipped")
                        || filter == "failed" && matches!(state, "failed" | "cancelled"))
                        && (query.is_empty()
                            || format!(
                                "{} {}",
                                e["title"].as_str().unwrap_or(""),
                                e["author"].as_str().unwrap_or("")
                            )
                            .to_lowercase()
                            .contains(&query))
                })
                .collect();
            let batch = if batch_id.is_empty() {
                Value::Null
            } else {
                let ids = |states: &[&str]| {
                    items
                        .iter()
                        .filter(|e| e["state"].as_str().is_some_and(|s| states.contains(&s)))
                        .map(|e| e["id"].clone())
                        .collect::<Vec<_>>()
                };
                json!({"name":items.first().and_then(|e|e["name"].as_str()).filter(|s|!s.is_empty()).unwrap_or("Downloads"),"total":items.len(),"completed":items.iter().filter(|e|e["state"]=="complete").count(),"failed":items.iter().filter(|e|e["state"]=="failed").count(),"pauseIds":ids(&["queued","resolving","downloading"]),"resumeIds":ids(&["paused"]),"ids":items.iter().map(|e|e["id"].clone()).collect::<Vec<_>>()})
            };
            Ok(
                json!({"items":items,"batch":batch,"total":entries.len(),"active":entries.iter().filter(|e|matches!(e["state"].as_str(),Some("downloading"|"paused"|"combining"|"queued"|"resolving"))).count(),"completed":entries.iter().filter(|e|e["state"]=="complete").count()}),
            )
        }
        _ => Err("Unknown download action.".into()),
    }
}
