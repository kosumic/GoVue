//! Provider settings, migration, validation and credential-safe presentation.
use super::{catalog, connections, has_plan, read_secret};
use crate::{Error, Result, json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, sync::Mutex};
use url::Url;

pub(super) static CONFIG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Provider {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) kind: String,
    #[serde(default)]
    pub(super) base_url: String,
    #[serde(default)]
    pub(super) model: String,
    #[serde(default = "catalog::enabled")]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) auth: String,
    #[serde(default)]
    pub(super) protocol: String,
    #[serde(default)]
    pub(super) azure: bool,
    #[serde(default)]
    pub(super) append_v1_suffix: bool,
    #[serde(default)]
    pub(super) custom_user_agent: String,
    #[serde(default)]
    pub(super) models: Vec<catalog::Model>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Config {
    pub(super) providers: Vec<Provider>,
    pub(super) active_id: Option<String>,
    #[serde(default)]
    pub(super) groups: Vec<catalog::Group>,
    #[serde(default)]
    pub(super) agent_md: String,
}
pub(super) fn config(root: &Path) -> Result<Config> {
    let path = root.join("ai/providers.json");
    match fs::read(&path) {
        Ok(b) if b.len() <= 2_000_000 => {
            let mut cfg: Config = serde_json::from_slice(&b).map_err(|cause| {
                Error::caused_by(
                    "Provider settings are unreadable.",
                    "decode provider settings",
                    cause,
                )
            })?;
            catalog::migrate(&mut cfg);
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(cause) => Err(Error::caused_by(
            "Could not read provider settings.",
            "read provider settings",
            cause,
        )),
        Ok(_) => Err("Could not read provider settings.".into()),
    }
}
pub(super) fn save(root: &Path, c: &Config) -> Result<()> {
    let dir = root.join("ai");
    fs::create_dir_all(&dir).map_err(|cause| {
        Error::caused_by(
            "Could not save provider settings.",
            "create provider directory",
            cause,
        )
    })?;
    let temp = dir.join("providers.tmp");
    let bytes = serde_json::to_vec(c).map_err(|cause| {
        Error::caused_by(
            "Invalid provider settings.",
            "encode provider settings",
            cause,
        )
    })?;
    if bytes.len() > 2_000_000 {
        return Err("Provider settings are full. Remove unused models or connections.".into());
    }
    fs::write(&temp, bytes).map_err(|cause| {
        Error::caused_by(
            "Could not save provider settings.",
            "write provider settings",
            cause,
        )
    })?;
    fs::rename(temp, dir.join("providers.json")).map_err(|cause| {
        Error::caused_by(
            "Could not save provider settings.",
            "commit provider settings",
            cause,
        )
    })
}
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(super) fn validate_provider(p: &mut Provider) -> Result<()> {
    if p.id == "host" {
        return Err("Reserved provider identity.".into());
    }
    if !valid_id(&p.id)
        || p.label.trim().is_empty()
        || p.label.len() > 100
        || p.model.len() > 200
        || p.model.chars().any(char::is_control)
    {
        return Err("Invalid provider settings.".into());
    }
    p.label = p.label.trim().into();
    p.model = p.model.trim().into();
    if p.kind != "custom" && catalog::default_base(&p.kind).is_none() {
        return Err("Unsupported provider type.".into());
    }
    if p.base_url.is_empty() {
        p.base_url = catalog::default_base(&p.kind).unwrap_or_default().into();
    }
    p.base_url = catalog::validate_url(&p.base_url)?;
    let u = Url::parse(&p.base_url).map_err(|_| "Invalid provider endpoint.")?;
    if u.query().is_some() && (!p.azure || u.query_pairs().any(|(k, _)| k != "api-version")) {
        return Err("Only Azure endpoints may contain an api-version query.".into());
    }
    if !matches!(p.auth.as_str(), "" | "key" | "oauth")
        || !matches!(
            p.protocol.as_str(),
            "" | "responses" | "chat" | "anthropic" | "gemini"
        )
    {
        return Err("Invalid authentication or API protocol.".into());
    }
    connections::validate(p)?;
    if catalog::oauth(p)
        && !matches!(
            p.kind.as_str(),
            "chatgpt" | "codex" | "anthropic" | "openrouter" | "xai" | "kimi" | "copilot"
        )
    {
        return Err("This provider does not support account sign-in.".into());
    }
    if catalog::oauth(p) && p.base_url != catalog::default_base(&p.kind).unwrap_or_default() {
        return Err(
            "Account credentials can only be used with their provider's own endpoint.".into(),
        );
    }
    if p.azure && (catalog::oauth(p) || !matches!(catalog::protocol(p), "chat" | "responses")) {
        return Err("Azure requires an API key and an OpenAI protocol.".into());
    }
    if matches!(p.kind.as_str(), "chatgpt" | "codex") && catalog::protocol(p) != "responses" {
        return Err("This account connection requires Responses API.".into());
    }
    catalog::validate_models(p)?;
    Ok(())
}
pub(super) fn provider(root: &Path, id: &str) -> Result<Provider> {
    let _g = CONFIG_LOCK
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let cfg = config(root)?;
    if let Some(g) = cfg.groups.iter().find(|g| g.id == id) {
        return Ok(catalog::group_provider(g));
    }
    let mut p = cfg
        .providers
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("Provider was removed.")?;
    validate_provider(&mut p)?;
    Ok(p)
}
pub(super) fn public_config(root: &Path) -> Result<Value> {
    let c = config(root)?;
    let mut rows = Vec::new();
    for p in &c.providers {
        let mut row = serde_json::to_value(p).map_err(|_| "Invalid provider settings.")?;
        row["connection"] = connections::preview(p);
        let s = match read_secret(&p.id) {
            Ok(s) => s,
            Err(_) => {
                row["credentialError"] = json!(
                    "Stored credentials could not be read. Reconnect or remove this provider."
                );
                json!({})
            }
        };
        row["connected"] = json!(if catalog::oauth(p) && p.kind != "openrouter" {
            s["access_token"].as_str().is_some_and(|s| !s.is_empty())
        } else {
            s["key"].as_str().is_some_and(|s| !s.is_empty())
                || (p.kind == "custom"
                    && !catalog::oauth(p)
                    && row.get("credentialError").is_none())
        });
        row["needsReauth"] = json!(s["needs_reauth"] == true);
        row["account"] = s["email"].clone();
        row["planEnabled"] = json!(has_plan(&s));
        rows.push(row);
    }
    let groups: Vec<_> = c
        .groups
        .iter()
        .map(|g| {
            let mut row = serde_json::to_value(g).unwrap_or(Value::Null);
            row["connected"] = json!(g.members.iter().any(|member| rows.iter().any(|p| {
                p["id"] == member.provider_id
                    && p["enabled"] == true
                    && p["connected"] == true
                    && p["needsReauth"] != true
                    && p["models"]
                        .as_array()
                        .is_some_and(|models| models.iter().any(|m| m["id"] == member.model))
            })));
            row
        })
        .collect();
    Ok(
        json!({"providers":rows,"groups":groups,"activeId":c.active_id,"choices":catalog::choices(),"agentMd":c.agent_md}),
    )
}

#[cfg(test)]
mod tests {
    use super::config;
    use std::{error::Error as _, fs};

    #[test]
    fn unreadable_settings_retain_the_decoder_cause_without_exposing_it() {
        let root =
            std::env::temp_dir().join(format!("govue-provider-diagnostics-{}", std::process::id()));
        fs::create_dir_all(root.join("ai")).unwrap();
        fs::write(root.join("ai/providers.json"), b"not json").unwrap();
        let error = config(&root).err().unwrap();
        fs::remove_dir_all(&root).unwrap();

        let cause = error
            .source()
            .unwrap()
            .downcast_ref::<serde_json::Error>()
            .unwrap();
        assert!(cause.is_syntax());
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            "Provider settings are unreadable."
        );
    }
}
