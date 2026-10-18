//! Connection settings shared by discovery, inference and both native platforms.
use super::*;

pub(super) fn can_append_v1(p: &Provider) -> bool {
    !catalog::oauth(p)
        && !p.azure
        && matches!(catalog::protocol(p), "chat" | "responses" | "anthropic")
}

pub(super) fn validate(p: &mut Provider) -> Result<()> {
    if p.auth.is_empty() {
        p.auth = if catalog::oauth(p) { "oauth" } else { "key" }.into();
    }
    let choices = catalog::choices();
    if !choices.as_array().into_iter().flatten().any(|c| {
        c["kind"] == p.kind
            && c["auth"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v == &p.auth)
    }) {
        return Err("This provider does not support that authentication method.".into());
    }
    p.custom_user_agent = p.custom_user_agent.trim().into();
    if p.custom_user_agent.len() > 512
        || !p
            .custom_user_agent
            .bytes()
            .all(|b| (0x20..=0x7e).contains(&b))
    {
        return Err("User-Agent must contain at most 512 printable ASCII characters.".into());
    }
    if p.append_v1_suffix && !can_append_v1(p) {
        return Err("Automatic /v1 is available for API-key Chat Completions, Responses and Anthropic connections without Azure.".into());
    }
    Ok(())
}

pub(super) fn effective_base(p: &Provider) -> Result<String> {
    let base = if p.base_url.trim().is_empty() {
        catalog::default_base(&p.kind).unwrap_or_default()
    } else {
        p.base_url.trim()
    };
    let base = catalog::validate_url(base)?;
    if !p.append_v1_suffix {
        return Ok(base);
    }
    if !can_append_v1(p) {
        return Err("Automatic /v1 is unavailable for this connection.".into());
    }
    let mut url = Url::parse(&base).map_err(|_| "Invalid provider endpoint.")?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/v1") {
        url.set_path(&format!("{path}/v1"));
    }
    Ok(url.to_string().trim_end_matches('/').into())
}

pub(super) fn preview(p: &Provider) -> Value {
    let mut checked = p.clone();
    if checked.id.is_empty() {
        checked.id = "preview".into();
    }
    let result = validate_provider(&mut checked).and_then(|_| effective_base(&checked));
    json!({
        "canEditEndpoint": !catalog::oauth(p),
        "canAppendV1": can_append_v1(p),
        "effectiveBaseUrl": result.as_ref().ok(),
        "error": result.err(),
    })
}

pub(super) fn changed(a: &Provider, b: &Provider) -> bool {
    a.kind != b.kind
        || catalog::oauth(a) != catalog::oauth(b)
        || effective_base(a).ok() != effective_base(b).ok()
        || catalog::protocol(a) != catalog::protocol(b)
        || a.azure != b.azure
        || a.custom_user_agent != b.custom_user_agent
}

// Called while holding the credential/session lock. A queued task must not
// combine its old endpoint snapshot with credentials from a newer connection.
pub(super) fn check_current(root: &Path, p: &Provider, job: &Arc<Job>) -> Result<()> {
    job.pending()?;
    let current = provider(root, &p.id)?;
    if !current.enabled || changed(&current, p) {
        return Err("Connection settings changed. Start the request again.".into());
    }
    Ok(())
}

pub(super) fn save_connection(c: &Value, root: &Path) -> Result<Value> {
    let mut p: Provider =
        serde_json::from_value(c["provider"].clone()).map_err(|_| "Invalid provider settings.")?;
    if p.id.is_empty() {
        p.id = random()?;
    }
    validate_provider(&mut p)?;
    let key = c["key"].as_str();
    if key.is_some_and(|key| {
        catalog::oauth(&p)
            || (key.trim().is_empty() && p.kind != "custom")
            || key.len() > 8192
            || key.chars().any(char::is_control)
    }) {
        return Err("Enter a valid API key.".into());
    }
    // Interrupt refresh/login work before waiting for its session lock.
    {
        let _g = CONFIG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(old) = config(root)?.providers.iter().find(|v| v.id == p.id) {
            if old.kind != p.kind {
                return Err("Add a separate connection to change its provider type.".into());
            }
            if changed(old, &p) || (old.enabled && !p.enabled) || key.is_some() {
                cancel_owner(&p.id);
            }
        }
    }
    let lock = session(&p.id);
    let _session = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = CONFIG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut cfg = config(root)?;
    let old = cfg.providers.iter().find(|v| v.id == p.id).cloned();
    let auth_changed = old
        .as_ref()
        .is_some_and(|old| catalog::oauth(old) != catalog::oauth(&p));
    if let Some(old) = &old {
        if old.kind != p.kind {
            return Err("Add a separate connection to change its provider type.".into());
        }
        if c["provider"].get("models").is_none() {
            p.models.clone_from(&old.models);
        }
        if changed(old, &p) || (old.enabled && !p.enabled) || key.is_some() {
            cancel_owner(&p.id);
        }
    } else if cfg.providers.len() >= 32 {
        return Err("At most 32 providers can be saved.".into());
    }
    let credentials = if auth_changed || key.is_some() {
        let previous = read_secret(&p.id)?;
        let mut next = if auth_changed {
            json!({"modelOptions":previous["modelOptions"]})
        } else {
            previous.clone()
        };
        if let Some(key) = key {
            next["key"] = json!(key.trim());
        }
        Some((previous, next))
    } else {
        None
    };
    catalog::migrate_provider(&mut p);
    if let Some(i) = cfg.providers.iter().position(|v| v.id == p.id) {
        cfg.providers[i] = p.clone();
    } else {
        cfg.providers.push(p.clone());
    }
    if cfg.active_id.is_none() {
        cfg.active_id = Some(p.id.clone());
    }
    if let Some((_, next)) = &credentials {
        write_secret(&p.id, next)?;
    }
    if let Err(error) = super::save(root, &cfg) {
        if let Some((previous, _)) = &credentials
            && write_secret(&p.id, previous).is_err()
        {
            return Err(format!(
                "{error} Credentials could not be restored; reconnect this provider."
            ));
        }
        return Err(error);
    }
    public_config(root)
}
