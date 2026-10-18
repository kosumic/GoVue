//! Authoritative transfer state. Platform adapters execute the returned effects.
use super::{Config, history, plan, preferences};
use crate::{Result, media, string};
use serde_json::{Value, json};
use std::sync::Mutex;

static WRITES: Mutex<()> = Mutex::new(());

#[allow(
    clippy::too_many_lines,
    reason = "Keep each operation and its shared state validation and persistence in one dispatcher."
)]
pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let _guard = WRITES.lock().map_err(|_| "Download storage is busy.")?;
    let mut entries = history(&json!({"entries":media::read(root,"downloads.json",json!([]))}))?
        .as_array()
        .cloned()
        .unwrap_or_default();
    let op = string(command, "op")?;
    let mut claimed = Vec::new();
    let mut prepared = Value::Null;
    let mut removed = Vec::new();
    let mut added = 0;
    let mut skipped = 0;
    let mut publication = Value::Null;
    match op {
        "downloads_enqueue" => {
            let items = command["items"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 500)
                .ok_or("Choose up to 500 videos.")?;
            if entries.len() + items.len() > 2000 {
                return Err("Remove old downloads before adding more.".into());
            }
            let config = Config::parse(&command["config"])?;
            let mut batch_bytes = [0u8; 8];
            getrandom::getrandom(&mut batch_bytes)
                .map_err(|_| "Could not create download identity.")?;
            let batch = crate::hex(&batch_bytes);
            for (index, item) in items.iter().enumerate() {
                let id = format!("{batch}-{index}");
                if command["replace"] != true
                    && entries.iter().any(|e| {
                        e["videoId"] == item["videoId"]
                            && e["config"] == json!(config)
                            && !matches!(
                                e["state"].as_str(),
                                Some("failed" | "cancelled" | "skipped")
                            )
                    })
                {
                    skipped += 1;
                    continue;
                }
                entries.insert(0,json!({"id":id,"videoId":item["videoId"],"title":item["title"],"author":item["author"],"label":"","itag":0,"state":"queued","received":0,"total":0,"file":format!("{id}.{}",if config.audio_only{"m4a"}else{"mp4"}),"mimeType":if config.audio_only{"audio/mp4"}else{"video/mp4"},"config":config,"batchId":batch,"name":command["name"].as_str().unwrap_or("")}));
                added += 1;
            }
        }
        "downloads_claim" => {
            let can_run = preferences(root)["wifiOnly"] != true || command["wifi"] == true;
            if can_run {
                let active = entries
                    .iter()
                    .filter(|e| {
                        matches!(
                            e["state"].as_str(),
                            Some("resolving" | "downloading" | "combining")
                        )
                    })
                    .count();
                for entry in entries
                    .iter_mut()
                    .rev()
                    .filter(|e| e["state"] == "queued")
                    .take(2usize.saturating_sub(active))
                {
                    super::publication::invalidate(entry)?;
                    entry["state"] = json!("resolving");
                    entry
                        .as_object_mut()
                        .ok_or("Invalid download entry.")?
                        .remove("error");
                    claimed.push(entry["id"].clone());
                }
            }
        }
        "downloads_restore" => {
            super::transfer::reset(root);
            for entry in &mut entries {
                if matches!(
                    entry["state"].as_str(),
                    Some("resolving" | "downloading" | "combining")
                ) {
                    super::publication::invalidate(entry)?;
                    if entry["artifacts"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty() && a.iter().all(|a| a["published"] == true))
                    {
                        entry["state"] = json!("complete");
                        entry["received"] = entry["total"].clone();
                        entry
                            .as_object_mut()
                            .ok_or("Invalid download entry.")?
                            .remove("error");
                    } else {
                        entry["state"] = json!("paused");
                        entry["error"] =
                            json!("Download interrupted. Resume to refresh its media address.");
                    }
                }
            }
        }
        "downloads_clear_history" => {
            let selected = if command["ids"].is_null() {
                None
            } else {
                Some(
                    command["ids"]
                        .as_array()
                        .filter(|a| {
                            a.len() <= 2000
                                && a.iter().all(|v| {
                                    v.as_str().is_some_and(|s| {
                                        s.len() <= 100
                                            && !s.is_empty()
                                            && s.bytes()
                                                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                                    })
                                })
                        })
                        .ok_or("Invalid history selection.")?,
                )
            };
            entries.retain(|entry| {
                let terminal = matches!(
                    entry["state"].as_str(),
                    Some("complete" | "failed" | "cancelled" | "skipped")
                ) && selected.is_none_or(|a| a.iter().any(|id| *id == entry["id"]));
                if terminal {
                    removed.push(entry.clone());
                }
                !terminal
            });
        }
        _ => {
            let id = string(command, "id")?;
            let index = entries
                .iter()
                .position(|e| e["id"] == id)
                .ok_or("This download no longer exists.")?;
            let entry = &mut entries[index];
            let state = entry["state"].as_str().unwrap_or("failed").to_string();
            match op {
                "downloads_publication_plan"
                | "downloads_publication_check"
                | "downloads_publication_target"
                | "downloads_publication_verified" => {
                    publication = super::publication::dispatch(entry, command)?;
                    if op == "downloads_publication_check" {
                        return Ok(json!({}));
                    }
                }
                "downloads_prepared" => {
                    if state != "resolving" {
                        return Err("Download was paused or cancelled.".into());
                    }
                    if command["catalog"]["videoId"] != entry["videoId"] {
                        return Err("The resolved video changed.".into());
                    }
                    let config = Config::parse(&entry["config"])?;
                    prepared = plan(&command["catalog"], &config)?;
                    entry["state"] = json!("downloading");
                    entry["total"] = prepared["bytes"].clone();
                    entry["received"] = json!(0);
                    entry["label"] = prepared["label"].clone();
                    entry["name"] = prepared["name"].clone();
                    entry["mimeType"] = prepared["mimeType"].clone();
                    for field in ["title", "author"] {
                        if let Some(text) = command["catalog"][field]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                        {
                            entry[field] =
                                json!(crate::clean(text).chars().take(1000).collect::<String>());
                        }
                    }
                    let mut artifacts = Vec::new();
                    if !config.attachments_only {
                        artifacts.push(json!({"file":entry["file"],"mimeType":prepared["mimeType"],"label":prepared["label"]}));
                    }
                    if config.include_subtitle {
                        artifacts.push(json!({"file":format!("{id}.srt"),"mimeType":"application/x-subrip","label":"Subtitles"}));
                    }
                    if config.include_cover {
                        artifacts.push(json!({"file":format!("{id}.jpg"),"mimeType":"image/jpeg","label":"Cover image"}));
                    }
                    for artifact in &mut artifacts {
                        if let Some(previous) = entry["artifacts"]
                            .as_array()
                            .and_then(|a| a.iter().find(|a| a["file"] == artifact["file"]))
                        {
                            // A fresh transfer may resolve a different rendition. Reuse
                            // its owned target, but verify the newly written bytes again.
                            if previous["publishedUri"].is_string() {
                                artifact["publishedUri"] = previous["publishedUri"].clone();
                                artifact["published"] = json!(false);
                            }
                        }
                    }
                    entry["artifacts"] = json!(artifacts);
                }
                "downloads_event" => {
                    let event = string(command, "event")?;
                    if matches!(event, "pause" | "resume" | "retry" | "cancel") {
                        super::publication::invalidate(entry)?;
                    }
                    match event {
                        "progress" if state == "downloading" => {
                            entry["received"] = json!(
                                command["received"]
                                    .as_u64()
                                    .unwrap_or(0)
                                    .min(entry["total"].as_u64().unwrap_or(0))
                            );
                        }
                        "pause"
                            if matches!(state.as_str(), "queued" | "resolving" | "downloading") =>
                        {
                            entry["state"] = json!("paused");
                        }
                        "resume" if state == "paused" => {
                            entry["state"] = json!(if command["retained"] == true {
                                "downloading"
                            } else {
                                "queued"
                            });
                            entry
                                .as_object_mut()
                                .ok_or("Invalid download entry.")?
                                .remove("error");
                        }
                        "retry"
                            if matches!(
                                state.as_str(),
                                "failed" | "cancelled" | "complete" | "skipped"
                            ) =>
                        {
                            entry["state"] = json!("queued");
                            entry["received"] = json!(0);
                            entry
                                .as_object_mut()
                                .ok_or("Invalid download entry.")?
                                .remove("error");
                        }
                        "cancel" if !matches!(state.as_str(), "complete" | "cancelled") => {
                            entry["state"] = json!("cancelled");
                            entry
                                .as_object_mut()
                                .ok_or("Invalid download entry.")?
                                .remove("error");
                        }
                        "combining" if state == "downloading" => {
                            entry["state"] = json!("combining");
                        }
                        "complete" if matches!(state.as_str(), "downloading" | "combining") => {
                            if command["publicationRequired"] == true
                                && !entry["artifacts"].as_array().is_some_and(|a| {
                                    !a.is_empty() && a.iter().all(|a| a["published"] == true)
                                })
                            {
                                return Err(
                                    "The download has not been saved to Android Downloads.".into(),
                                );
                            }
                            entry["state"] = json!("complete");
                            entry["received"] = entry["total"].clone();
                            entry
                                .as_object_mut()
                                .ok_or("Invalid download entry.")?
                                .remove("error");
                        }
                        "failed"
                            if !matches!(state.as_str(), "cancelled" | "paused" | "complete") =>
                        {
                            entry["state"] = json!("failed");
                            entry["error"] = json!(
                                command["error"]
                                    .as_str()
                                    .unwrap_or("Download failed. Try again.")
                            );
                        }
                        "missing" if state == "complete" => {
                            entry["state"] = json!("failed");
                            entry["error"] = json!("Saved file is missing. Download again.");
                        }
                        _ => return Err("Download action is no longer available.".into()),
                    }
                }
                "downloads_remove" => {
                    if matches!(state.as_str(), "resolving" | "downloading" | "combining") {
                        return Err("Cancel the download before removing it.".into());
                    }
                    entries.remove(index);
                }
                _ => return Err("Unknown download operation.".into()),
            }
        }
    }
    let safe = history(&json!({"entries":entries}))?;
    if safe.to_string().len() > crate::MAX_BYTES {
        return Err("Download history is full. Remove old entries before adding more.".into());
    }
    media::write(root, "downloads.json", &safe)?;
    Ok(
        json!({"entries":safe,"claimed":claimed,"plan":prepared,"publication":publication,"removed":removed,"added":added,"skipped":skipped}),
    )
}
