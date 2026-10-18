//! Android Downloads publication state. Content URIs are owned local artifacts,
//! never upstream addresses. Platform adapters supply copy/verification results.
use super::filename;
use crate::{Result, string};
use serde_json::{Value, json};
use url::Url;

pub(super) fn uri(value: &str) -> bool {
    let Ok(parsed) = Url::parse(value) else {
        return false;
    };
    let parts: Vec<_> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();
    parsed.scheme() == "content"
        && parsed.host_str() == Some("media")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parts.len() == 3
        && matches!(parts[0], "external" | "external_primary")
        && parts[1] == "downloads"
        && !parts[2].is_empty()
        && parts[2].len() <= 20
        && parts[2].bytes().all(|b| b.is_ascii_digit())
        && parsed.path() == format!("/{}/{}/{}", parts[0], parts[1], parts[2])
}
pub(super) fn invalidate(entry: &mut Value) -> Result<u64> {
    let next = entry["publicationGeneration"]
        .as_u64()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|v| *v <= 9_007_199_254_740_991)
        .ok_or("The publication generation is exhausted.")?;
    entry["publicationGeneration"] = json!(next);
    Ok(next)
}
fn digest(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub(super) fn name(entry: &Value, file: &str) -> Result<String> {
    if entry["file"] != file
        && !entry["artifacts"]
            .as_array()
            .is_some_and(|a| a.iter().any(|a| a["file"] == file))
    {
        return Err("This file does not belong to the download.".into());
    }
    let ext = file
        .rsplit('.')
        .next()
        .filter(|e| matches!(*e, "mp4" | "m4a" | "jpg" | "srt"))
        .ok_or("Unsupported saved file.")?;
    Ok(format!(
        "{}.{ext}",
        filename(
            entry["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(entry["title"].as_str())
                .unwrap_or("video")
        )
    ))
}

pub(super) fn sanitize(source: &Value, target: &mut Value) -> Result<()> {
    if let Some(address) = source["publishedUri"].as_str() {
        if !uri(address) {
            return Err("Invalid published download identity.".into());
        }
        target["publishedUri"] = json!(address);
        target["published"] = json!(source["published"] == true);
        if let Some(bytes) = source["publishedBytes"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 200_000_000_000)
        {
            target["publishedBytes"] = json!(bytes);
        }
        if let Some(hash) = digest(&source["publishedSha256"]) {
            target["publishedSha256"] = json!(hash.to_ascii_lowercase());
        }
        if source["published"] == true
            && (target["publishedBytes"].is_null() || target["publishedSha256"].is_null())
        {
            return Err("Published download verification is missing.".into());
        }
    }
    Ok(())
}

pub(super) fn dispatch(entry: &mut Value, command: &Value) -> Result<Value> {
    if !matches!(
        entry["state"].as_str(),
        Some("downloading" | "combining" | "complete" | "failed")
    ) {
        return Err("This download cannot be published now.".into());
    }
    let op = string(command, "op")?;
    if op == "downloads_publication_plan" {
        let generation = invalidate(entry)?;
        let artifacts = entry["artifacts"]
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 3)
            .ok_or("Saved download files are not available.".to_string())?;
        let plan: Vec<_> = artifacts.iter().map(|a| {
            let file = string(a,"file")?;
            Ok(json!({"file":file,"name":name(entry,file)?,"mimeType":a["mimeType"],"uri":a["publishedUri"],"published":a["published"]==true,"bytes":a["publishedBytes"],"sha256":a["publishedSha256"]}))
        }).collect::<Result<_>>()?;
        return Ok(json!({"state":entry["state"],"generation":generation,"artifacts":plan}));
    }
    if command["generation"].as_u64().is_none()
        || command["generation"] != entry["publicationGeneration"]
    {
        return Err("This publication attempt was superseded.".into());
    }
    if op == "downloads_publication_check" {
        return Ok(Value::Null);
    }
    let file = string(command, "file")?;
    let artifact = entry["artifacts"]
        .as_array_mut()
        .and_then(|a| a.iter_mut().find(|a| a["file"] == file))
        .ok_or("This file does not belong to the download.".to_string())?;
    let address = string(command, "uri")?;
    if !uri(address) {
        return Err("Invalid published download identity.".into());
    }
    match op {
        "downloads_publication_target" => {
            if !artifact["publishedUri"].is_null()
                && artifact["publishedUri"] != address
                && artifact["publishedUri"] != command["previousUri"]
            {
                return Err("The publication target changed.".into());
            }
            artifact["publishedUri"] = json!(address);
            artifact["published"] = json!(false);
            artifact
                .as_object_mut()
                .ok_or("Invalid download artifact.")?
                .remove("publishedBytes");
            artifact
                .as_object_mut()
                .ok_or("Invalid download artifact.")?
                .remove("publishedSha256");
        }
        "downloads_publication_verified" => {
            if artifact["publishedUri"] != address {
                return Err("The publication target changed.".into());
            }
            let source =
                digest(&command["sourceSha256"]).ok_or("The source checksum is unavailable.")?;
            let copied =
                digest(&command["copiedSha256"]).ok_or("The saved checksum is unavailable.")?;
            let bytes = command["sourceBytes"]
                .as_u64()
                .filter(|v| *v > 0 && *v <= 200_000_000_000)
                .ok_or("Invalid saved file length.")?;
            if !source.eq_ignore_ascii_case(copied)
                || command["copiedBytes"].as_u64() != Some(bytes)
            {
                return Err("The saved file did not pass verification.".into());
            }
            artifact["published"] = json!(true);
            artifact["publishedBytes"] = json!(bytes);
            artifact["publishedSha256"] = json!(source.to_ascii_lowercase());
        }
        _ => return Err("Unknown publication action.".into()),
    }
    Ok(Value::Null)
}
