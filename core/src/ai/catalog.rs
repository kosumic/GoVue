//! Saved model library, routing groups and portable, credential-free settings.
use super::*;
use std::collections::HashSet;

pub(super) fn enabled() -> bool {
    true
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Model {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Member {
    pub provider_id: String,
    pub model: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Group {
    pub id: String,
    pub name: String,
    pub members: Vec<Member>,
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub fallback_on_any_error: bool,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub context_limit: Option<u64>,
}

pub(super) fn default_base(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "chatgpt" | "openai" => RESOURCE,
        "codex" => "https://chatgpt.com/backend-api/codex",
        "anthropic" => "https://api.anthropic.com/v1",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        "openrouter" => "https://openrouter.ai/api/v1",
        "xai" => "https://api.x.ai/v1",
        "kimi" => "https://api.kimi.com/coding/v1",
        "copilot" => "https://api.githubcopilot.com",
        "custom" => return None,
        _ => return None,
    })
}
pub(super) fn oauth(p: &Provider) -> bool {
    matches!(p.kind.as_str(), "chatgpt" | "codex" | "copilot") || p.auth == "oauth"
}
pub(super) fn protocol(p: &Provider) -> &str {
    if !p.protocol.is_empty() {
        return &p.protocol;
    }
    match p.kind.as_str() {
        "chatgpt" | "codex" | "openai" => "responses",
        "anthropic" => "anthropic",
        "gemini" => "gemini",
        _ => "chat",
    }
}
pub(super) fn validate_url(base: &str) -> Result<String> {
    let u = Url::parse(base.trim()).map_err(|_| "Enter an HTTPS API base URL.")?;
    if !(u.scheme() == "https"
        || (u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "localhost"))))
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err("Use HTTPS, or HTTP on localhost, without credentials or fragment.".into());
    }
    Ok(u.to_string().trim_end_matches('/').to_owned())
}
fn valid_model(m: &mut Model) -> Result<()> {
    m.id = m.id.trim().into();
    m.name = m.name.trim().into();
    if m.id.is_empty()
        || m.id.len() > 200
        || m.id.chars().any(char::is_control)
        || m.name.chars().count() > 200
        || m.name.chars().any(char::is_control)
        || m.context_window
            .is_some_and(|n| !(256..=10_000_000).contains(&n))
        || m.max_output_tokens
            .is_some_and(|n| !(1..=2_000_000).contains(&n))
        || m.temperature
            .is_some_and(|n| !n.is_finite() || !(0.0..=2.0).contains(&n))
        || m.top_p
            .is_some_and(|n| !n.is_finite() || !(0.0..=1.0).contains(&n))
        || !valid_reasoning(&m.reasoning)
        || m.reasoning_efforts.len() > 12
        || m.reasoning_efforts
            .iter()
            .any(|s| !valid_reasoning(s) || s.is_empty())
        || m.input_modalities.len() > 10
        || m.output_modalities.len() > 10
        || m.input_modalities
            .iter()
            .chain(&m.output_modalities)
            .any(|s| !matches!(s.as_str(), "text" | "image" | "audio" | "video" | "pdf"))
    {
        return Err("Invalid model settings or limits.".into());
    }
    if m.name.is_empty() {
        m.name = m.id.clone();
    }
    Ok(())
}
pub(super) fn valid_reasoning(s: &str) -> bool {
    matches!(
        s,
        "" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    )
}
pub(super) fn reasoning<'a>(model: &'a Model, requested: &'a str) -> &'a str {
    if !requested.is_empty()
        && model.reasoning_efforts.is_empty()
        && let Some(levels) = known_reasoning(&model.id)
    {
        return levels
            .iter()
            .copied()
            .filter(|s| reasoning_rank(s) <= reasoning_rank(requested))
            .max_by_key(|s| reasoning_rank(s))
            .unwrap_or(levels[0]);
    }
    if requested.is_empty()
        || model.reasoning_efforts.is_empty()
        || model.reasoning_efforts.iter().any(|s| s == requested)
    {
        return requested;
    }
    let levels = [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ];
    let rank = |s: &str| levels.iter().position(|e| e == &s).unwrap_or(0);
    model
        .reasoning_efforts
        .iter()
        .filter(|e| rank(e) <= rank(requested))
        .max_by_key(|e| rank(e))
        .or_else(|| model.reasoning_efforts.iter().min_by_key(|e| rank(e)))
        .map(String::as_str)
        .unwrap_or("")
}

fn reasoning_rank(s: &str) -> usize {
    [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ]
    .iter()
    .position(|v| v == &s)
    .unwrap_or(0)
}
fn known_reasoning(id: &str) -> Option<&'static [&'static str]> {
    // Public Responses model metadata omits efforts. Keep the documented fallback
    // narrow; account discovery still wins when it provides its own tier list.
    (id == "gpt-6.1-sol" || id.starts_with("gpt-6.1-sol-"))
        .then_some(&["low", "medium", "high", "xhigh", "max"][..])
}
// Discovery takes precedence. These common choices also work for compatible
// endpoints that don't publish effort metadata; their provider validates support.
fn reasoning_choices(model: &Model) -> Vec<String> {
    if !model.reasoning_efforts.is_empty() {
        return model.reasoning_efforts.clone();
    }
    let levels = known_reasoning(&model.id).unwrap_or(&["none", "low", "medium", "high"]);
    levels.iter().map(|s| s.to_string()).collect()
}

fn reasoning_options(cfg: &Config, c: &Value) -> Result<Value> {
    let id = string(c, "id")?;
    if let Some(group) = cfg.groups.iter().find(|g| g.id == id) {
        let mut levels = Vec::new();
        let mut discovered = true;
        for member in &group.members {
            if let Some(model) = cfg
                .providers
                .iter()
                .filter(|p| p.id == member.provider_id && p.enabled)
                .flat_map(|p| &p.models)
                .find(|m| m.id == member.model)
            {
                discovered &=
                    !model.reasoning_efforts.is_empty() || known_reasoning(&model.id).is_some();
                for level in reasoning_choices(model) {
                    if !levels.contains(&level) {
                        levels.push(level);
                    }
                }
            }
        }
        levels.sort_by_key(|s| reasoning_rank(s));
        return Ok(
            json!({"levels":levels,"defaultEffort":group.reasoning,"group":true,"discovered":discovered}),
        );
    }
    let p = cfg
        .providers
        .iter()
        .find(|p| p.id == id)
        .ok_or("Provider was removed.")?;
    let model_id = c["model"].as_str().unwrap_or(&p.model);
    let m = p
        .models
        .iter()
        .find(|m| m.id == model_id)
        .ok_or("Model was removed.")?;
    Ok(
        json!({"levels":reasoning_choices(m),"defaultEffort":m.reasoning,"group":false,"discovered":!m.reasoning_efforts.is_empty() || known_reasoning(&m.id).is_some()}),
    )
}
pub(super) fn migrate(cfg: &mut Config) {
    for p in &mut cfg.providers {
        migrate_provider(p);
    }
}
pub(super) fn migrate_provider(p: &mut Provider) {
    if !p.model.is_empty() && !p.models.iter().any(|m| m.id == p.model) {
        p.models.push(Model {
            id: p.model.clone(),
            name: p.model.clone(),
            ..Default::default()
        });
    }
}
pub(super) fn group_provider(g: &Group) -> Provider {
    Provider {
        id: g.id.clone(),
        label: g.name.clone(),
        kind: "group".into(),
        base_url: String::new(),
        model: g.name.clone(),
        enabled: true,
        auth: String::new(),
        protocol: String::new(),
        azure: false,
        append_v1_suffix: false,
        custom_user_agent: String::new(),
        models: Vec::new(),
    }
}
pub(super) fn selected(p: &Provider) -> Model {
    p.models
        .iter()
        .find(|m| m.id == p.model)
        .cloned()
        .unwrap_or_else(|| Model {
            id: p.model.clone(),
            name: p.model.clone(),
            ..Default::default()
        })
}
pub(super) fn validate_models(p: &mut Provider) -> Result<()> {
    if p.models.len() > 512 {
        return Err("At most 512 models can be saved per provider.".into());
    }
    let mut ids = HashSet::new();
    for m in &mut p.models {
        valid_model(m)?;
        if !ids.insert(m.id.clone()) {
            return Err("A model can only be saved once per connection.".into());
        }
    }
    Ok(())
}
fn validate_group(g: &mut Group, cfg: &Config) -> Result<()> {
    g.name = g.name.trim().into();
    if !valid_id(&g.id)
        || cfg.providers.iter().any(|p| p.id == g.id)
        || g.name.is_empty()
        || g.name.len() > 100
        || !matches!(g.strategy.as_str(), "fallback" | "loadBalance")
        || !valid_reasoning(&g.reasoning)
        || g.context_limit
            .is_some_and(|n| !(256..=10_000_000).contains(&n))
        || g.members.is_empty()
        || g.members.len() > 32
    {
        return Err("Give the group a name, strategy and 1–32 saved models.".into());
    }
    let mut seen = HashSet::new();
    for m in &g.members {
        let exists = cfg
            .providers
            .iter()
            .any(|p| p.id == m.provider_id && p.models.iter().any(|e| e.id == m.model));
        if !exists || !seen.insert((m.provider_id.clone(), m.model.clone())) {
            return Err("Group members must be distinct saved models.".into());
        }
    }
    Ok(())
}
pub(super) fn options(p: &Provider) -> Result<Value> {
    let s = read_secret(&p.id)?;
    Ok(s["modelOptions"][&p.model].clone())
}
fn validate_options(v: &Value) -> Result<()> {
    if v.to_string().len() > 16_000 {
        return Err("Custom model parameters are too large.".into());
    }
    if let Some(headers) = v.get("headers").filter(|v| !v.is_null()) {
        let headers = headers
            .as_object()
            .ok_or("Headers must be a JSON object.")?;
        if headers.len() > 24
            || headers.iter().any(|(k, v)| {
                k.is_empty()
                    || k.len() > 100
                    || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || matches!(
                        k.to_ascii_lowercase().as_str(),
                        "host" | "content-length" | "connection" | "transfer-encoding"
                    )
                    || v.as_str()
                        .is_none_or(|s| s.len() > 8192 || s.chars().any(char::is_control))
            })
        {
            return Err("Invalid custom HTTP headers.".into());
        }
    }
    if let Some(body) = v.get("body").filter(|v| !v.is_null()) {
        let body = body
            .as_object()
            .ok_or("Extra request parameters must be a JSON object.")?;
        if body.keys().any(|k| {
            matches!(
                k.as_str(),
                "model"
                    | "messages"
                    | "input"
                    | "instructions"
                    | "system"
                    | "systemInstruction"
                    | "contents"
                    | "stream"
                    | "store"
                    | "tools"
                    | "previous_response_id"
            )
        }) {
            return Err("Custom parameters cannot replace the model, conversation, streaming or storage settings.".into());
        }
    }
    Ok(())
}
pub(super) fn operation(op: &str) -> bool {
    matches!(
        op,
        "ai_save_model"
            | "ai_reasoning_options"
            | "ai_remove_model"
            | "ai_model_options"
            | "ai_save_group"
            | "ai_remove_group"
            | "ai_reorder"
            | "ai_export"
            | "ai_import"
    )
}
pub(super) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    let op = string(&c, "op")?;
    let lock = session(c["id"].as_str().unwrap_or("catalog"));
    let _lock = lock.lock().unwrap_or_else(|e| e.into_inner());
    let _g = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = config(&root)?;
    match op {
        "ai_reasoning_options" => return reasoning_options(&cfg, &c),
        "ai_export" => {
            return Ok(
                json!({"format":"lingoplay.ai","version":1,"providers":cfg.providers,"groups":cfg.groups,"activeId":cfg.active_id}),
            );
        }
        "ai_import" => {
            let value = &c["data"];
            if value["format"] != "lingoplay.ai" || value["version"] != 1 {
                return Err("Unsupported provider configuration file.".into());
            }
            // Credentials and arbitrary unknown fields are rejected by Provider/Model.
            let mut imported: Vec<Provider> = serde_json::from_value(value["providers"].clone())
                .map_err(|_| "Invalid provider configuration.")?;
            let mut groups: Vec<Group> = serde_json::from_value(value["groups"].clone())
                .map_err(|_| "Invalid model groups.")?;
            if imported.len() + cfg.providers.len() > 32 || groups.len() + cfg.groups.len() > 32 {
                return Err("Import exceeds the 32 connection/group limit.".into());
            }
            let mut mapping = HashMap::new();
            for p in &mut imported {
                let old = p.id.clone();
                p.id = random()?;
                validate_provider(p)?;
                if mapping.insert(old, p.id.clone()).is_some() {
                    return Err("Duplicate imported provider identity.".into());
                }
            }
            cfg.providers.extend(imported);
            migrate(&mut cfg);
            for g in &mut groups {
                g.id = random()?;
                for m in &mut g.members {
                    m.provider_id = mapping
                        .get(&m.provider_id)
                        .ok_or("Group references a provider missing from this file.")?
                        .clone();
                }
                validate_group(g, &cfg)?;
            }
            cfg.groups.extend(groups);
            if cfg.active_id.is_none() {
                cfg.active_id = cfg.providers.first().map(|p| p.id.clone());
            }
        }
        "ai_save_group" => {
            let mut group: Group =
                serde_json::from_value(c["group"].clone()).map_err(|_| "Invalid model group.")?;
            if group.id.is_empty() {
                group.id = random()?;
            }
            validate_group(&mut group, &cfg)?;
            if let Some(g) = cfg.groups.iter_mut().find(|g| g.id == group.id) {
                *g = group;
            } else if cfg.groups.len() < 32 {
                cfg.groups.push(group);
            } else {
                return Err("At most 32 model groups can be saved.".into());
            }
        }
        "ai_remove_group" => {
            let id = string(&c, "id")?;
            cfg.groups.retain(|g| g.id != id);
            if cfg.active_id.as_deref() == Some(id) {
                cfg.active_id = cfg.providers.first().map(|p| p.id.clone());
            }
        }
        "ai_reorder" => {
            let ids: Vec<String> =
                serde_json::from_value(c["ids"].clone()).map_err(|_| "Invalid order.")?;
            if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
                return Err("Duplicate items in order.".into());
            }
            if let Some(id) = c["id"].as_str() {
                let p = cfg
                    .providers
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Provider was removed.")?;
                if ids.len() != p.models.len() || p.models.iter().any(|m| !ids.contains(&m.id)) {
                    return Err("Model list changed. Refresh and try again.".into());
                }
                p.models
                    .sort_by_key(|m| ids.iter().position(|id| id == &m.id).unwrap());
            } else if c["kind"] == "groups" {
                if ids.len() != cfg.groups.len() || cfg.groups.iter().any(|g| !ids.contains(&g.id))
                {
                    return Err("Group list changed. Refresh and try again.".into());
                }
                cfg.groups
                    .sort_by_key(|g| ids.iter().position(|id| id == &g.id).unwrap());
            } else {
                if ids.len() != cfg.providers.len()
                    || cfg.providers.iter().any(|p| !ids.contains(&p.id))
                {
                    return Err("Provider list changed. Refresh and try again.".into());
                }
                cfg.providers
                    .sort_by_key(|p| ids.iter().position(|id| id == &p.id).unwrap());
            }
        }
        "ai_save_model" | "ai_remove_model" | "ai_model_options" => {
            let id = string(&c, "id")?;
            let p = cfg
                .providers
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Provider was removed.")?;
            let model = if op == "ai_save_model" {
                string(&c["model"], "id")?
            } else {
                string(&c, "model")?
            }
            .to_owned();
            let mut secret = read_secret(id)?;
            if op == "ai_model_options" {
                return Ok(secret["modelOptions"][&model].clone());
            }
            if op == "ai_save_model" {
                let mut m: Model = serde_json::from_value(c["model"].clone())
                    .map_err(|_| "Invalid model settings.")?;
                valid_model(&mut m)?;
                if let Some(options) = c.get("options") {
                    validate_options(options)?;
                    if !secret["modelOptions"].is_object() {
                        secret["modelOptions"] = json!({});
                    }
                    secret["modelOptions"][&model] = options.clone();
                    write_secret(id, &secret)?;
                }
                if let Some(old) = p.models.iter_mut().find(|e| e.id == m.id) {
                    *old = m;
                } else if p.models.len() < 512 {
                    p.models.push(m);
                } else {
                    return Err("At most 512 models can be saved per provider.".into());
                }
                if p.model.is_empty() {
                    p.model = model.clone();
                }
                if p.models.iter().any(|m| m.id == p.model && m.hidden) {
                    p.model = p
                        .models
                        .iter()
                        .find(|m| !m.hidden)
                        .map(|m| m.id.clone())
                        .unwrap_or_default();
                }
            } else {
                p.models.retain(|m| m.id != model);
                if let Some(opts) = secret["modelOptions"].as_object_mut() {
                    opts.remove(&model);
                }
                write_secret(id, &secret)?;
                if p.model == model {
                    p.model = p
                        .models
                        .iter()
                        .find(|m| !m.hidden)
                        .map(|m| m.id.clone())
                        .unwrap_or_default();
                }
                for g in &mut cfg.groups {
                    g.members
                        .retain(|m| m.provider_id != id || m.model != model);
                }
            }
        }
        _ => return Err("Unsupported model operation.".into()),
    }
    cfg.groups.retain(|g| !g.members.is_empty());
    if cfg.active_id.as_ref().is_some_and(|id| {
        !cfg.providers.iter().any(|p| &p.id == id) && !cfg.groups.iter().any(|g| &g.id == id)
    }) {
        cfg.active_id = cfg.providers.first().map(|p| p.id.clone());
    }
    save(&root, &cfg)?;
    public_config(&root)
}

pub(super) fn choices() -> Value {
    json!([
        {"kind":"codex","name":"ChatGPT / Codex","auth":["oauth"],"description":"Sign in with your ChatGPT account"},
        {"kind":"openai","name":"OpenAI API","auth":["key"],"description":"API key · Usage-based billing"},
        {"kind":"anthropic","name":"Anthropic / Claude","auth":["key","oauth"],"description":"API key or Claude account"},
        {"kind":"gemini","name":"Google Gemini","auth":["key"],"description":"Google AI Studio API key"},
        {"kind":"openrouter","name":"OpenRouter","auth":["key","oauth"],"description":"API key or browser authorization"},
        {"kind":"xai","name":"xAI / Grok","auth":["key","oauth"],"description":"API key or Grok account"},
        {"kind":"kimi","name":"Kimi Code","auth":["key","oauth"],"description":"Coding Plan device sign-in or API key"},
        {"kind":"copilot","name":"GitHub Copilot","auth":["oauth"],"description":"Optional, unofficial Copilot connection"},
        {"kind":"custom","name":"Compatible endpoint","auth":["key"],"description":"OpenAI, Anthropic, Gemini or Responses over HTTPS"},
        {"kind":"chatgpt","name":"ChatGPT plan sharing","auth":["oauth"],"description":"Alternative sign-in for eligible ChatGPT plans","advanced":true}
    ])
}
