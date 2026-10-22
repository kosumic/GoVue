//! Credential storage, account authentication and token lifecycle.
use crate::Result;
use crate::ai::commands::FOREGROUND;
use crate::ai::jobs::Job;
use crate::ai::providers::{CONFIG_LOCK, Provider};
use crate::ai::{accounts, catalog, connections, http, identity, jobs, providers};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

const AUTH: &str = "https://auth.openai.com";
pub(super) const RESOURCE: &str = "https://api.openai.com/v1";

static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

pub(super) fn session(id: &str) -> Result<Arc<Mutex<()>>> {
    Ok(SESSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .entry(id.into())
        .or_default()
        .clone())
}
pub(super) fn read_secret(id: &str) -> Result<Value> {
    let reply = http::platform()?.secret_read(format!("ai.{id}"));
    if !reply.ok {
        return Err("Could not unlock provider credentials.".into());
    }
    if reply.value.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&reply.value)
        .map_err(|_| "Stored provider credentials are unreadable. Reconnect this provider.".into())
}
pub(super) fn write_secret(id: &str, value: &Value) -> Result<()> {
    if http::platform()?.secret_write(
        format!("ai.{id}"),
        if value.is_null() {
            String::new()
        } else {
            value.to_string()
        },
    ) {
        Ok(())
    } else {
        Err("Could not save provider credentials securely.".into())
    }
}

pub(super) fn has_plan(s: &Value) -> bool {
    s["scope"].as_str().is_some_and(|s| {
        s.split_whitespace()
            .any(|s| s == "chatgpt.tokens.use.direct")
    })
}
pub(super) fn form_request(fields: &[(&str, &str)], job: &Arc<Job>) -> Result<Value> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish();
    http::http(
        format!("{AUTH}/api/accounts/oauth/token"),
        "POST",
        vec![("Content-Type", "application/x-www-form-urlencoded".into())],
        body,
        None,
        Some(job),
    )
}
pub(super) fn token_set(mut response: Value, previous: &Value) -> Result<Value> {
    if let (Some(saved), Some(received)) = (
        previous["client_id"].as_str(),
        response["client_id"].as_str(),
    ) && saved != received
    {
        return Err("Login returned another account registration.".into());
    }
    if response["access_token"].as_str().is_none_or(str::is_empty)
        || !response["token_type"]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case("Bearer"))
    {
        return Err("Login did not return a usable access token.".into());
    }
    let expires = response["expires_in"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 86400)
        .ok_or("Login returned an invalid expiry.")?;
    for field in [
        "refresh_token",
        "id_token",
        "scope",
        "client_id",
        "subject",
        "email",
        "modelOptions",
    ] {
        if response.get(field).is_none() {
            response[field] = previous[field].clone();
        }
    }
    response["expires_at"] = json!(identity::now() + expires);
    Ok(response)
}
pub(super) fn credential(root: &Path, p: &Provider, job: &Arc<Job>) -> Result<String> {
    if p.kind != "chatgpt" {
        return accounts::credential(root, p, job);
    }
    let guard = session(&p.id)?;
    let _g = guard
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    connections::check_current(root, p, job)?;
    let mut s = read_secret(&p.id)?;
    if p.kind != "chatgpt" {
        return s["key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or("Add an API key for this provider.".into());
    }
    if !has_plan(&s) {
        return Err("ChatGPT plan access is not enabled. Reconnect and grant plan usage.".into());
    }
    if s["expires_at"].as_u64().unwrap_or(0) <= identity::now() + 60 {
        let fresh = form_request(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", crate::string(&s, "client_id")?),
                ("refresh_token", crate::string(&s, "refresh_token")?),
                ("resource", RESOURCE),
            ],
            job,
        )?;
        s = token_set(fresh, &s)?;
        job.pending()?;
        write_secret(&p.id, &s)?;
    }
    s["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or("Sign in to ChatGPT again.".into())
}

pub(super) fn verify_identity(
    token: &str,
    client: &str,
    nonce: &str,
    job: &Arc<Job>,
) -> Result<Value> {
    use jsonwebtoken::jwk::JwkSet;
    use jsonwebtoken::{Algorithm, DecodingKey, Validation};
    let metadata = http::json_http(
        format!("{AUTH}/.well-known/openid-configuration"),
        vec![],
        None,
        job,
    )?;
    if metadata["issuer"] != AUTH {
        return Err("Unexpected login issuer.".into());
    }
    let jwks_url = Url::parse(crate::string(&metadata, "jwks_uri")?)
        .map_err(|_| "Invalid login signing keys.")?;
    if jwks_url.scheme() != "https"
        || jwks_url.host_str() != Some("auth.openai.com")
        || jwks_url.port_or_known_default() != Some(443)
        || !jwks_url.username().is_empty()
        || jwks_url.password().is_some()
    {
        return Err("Untrusted login signing keys.".into());
    }
    let jwks: JwkSet =
        serde_json::from_value(http::json_http(jwks_url.to_string(), vec![], None, job)?)
            .map_err(|_| "Invalid login signing keys.")?;
    let head = jsonwebtoken::decode_header(token).map_err(|_| "Invalid login identity.")?;
    if head.alg != Algorithm::RS256 {
        return Err("Unsupported login signature algorithm.".into());
    }
    let key = jwks
        .find(head.kid.as_deref().ok_or("Missing login signing key.")?)
        .ok_or("Unknown login signing key.")?;
    let key = DecodingKey::from_jwk(key).map_err(|_| "Invalid login signing key.")?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[AUTH]);
    validation.set_audience(&[client]);
    validation.validate_nbf = true;
    let claims = jsonwebtoken::decode::<Value>(token, &key, &validation)
        .map_err(|_| "Could not verify login identity.")?
        .claims;
    if (!nonce.is_empty() && claims["nonce"] != nonce)
        || claims["sub"].as_str().is_none_or(str::is_empty)
    {
        return Err("Login identity does not match this request.".into());
    }
    Ok(claims)
}
pub(super) fn callback(
    url: &str,
    expected: &str,
    client: Option<&str>,
) -> Result<(String, String)> {
    if url.len() > 16384 {
        return Err("Invalid login callback.".into());
    }
    let u = Url::parse(&format!("http://127.0.0.1{url}")).map_err(|_| "Invalid login callback.")?;
    if u.path() != "/auth/callback" {
        return Err("Invalid login callback path.".into());
    }
    let mut fields = HashMap::new();
    for (k, v) in u.query_pairs() {
        if fields.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err("Duplicate login callback parameter.".into());
        }
    }
    if fields.get("state").map(String::as_str) != Some(expected) {
        return Err("Login state did not match. Start sign-in again.".into());
    }
    if fields.contains_key("error") {
        return Err("ChatGPT sign-in was declined or unavailable for this app.".into());
    }
    let issued = match (client, fields.get("client_id")) {
        (Some(saved), Some(id)) if id != saved => {
            return Err("Login returned another account registration.".into());
        }
        (Some(saved), _) => saved.to_owned(),
        (None, Some(id)) if id != "dynamic_agent_client" && !id.is_empty() && id.len() <= 200 => {
            id.clone()
        }
        _ => return Err("ChatGPT did not return an app registration. Start sign-in again.".into()),
    };
    let code = fields
        .remove("code")
        .filter(|c| !c.is_empty())
        .ok_or("Missing login authorization code.")?;
    Ok((code, issued))
}
pub(super) fn login(root: PathBuf, p: Provider) -> Result<Value> {
    use tiny_http::{Method, Response, Server};
    if p.kind != "chatgpt" {
        return Err("Account login is only available for ChatGPT.".into());
    }
    let guard = session(&p.id)?;
    let _g = guard
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    jobs::cancel_owner(&p.id)?;
    let saved = read_secret(&p.id)?;
    let host_lock = session("host")?;
    let host_guard = host_lock
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let host = read_secret("host")?;
    let host_id = if let Some(id) = host["id"].as_str() {
        id.to_owned()
    } else {
        let id = format!("urn:uuid:{}", identity::uuid()?);
        write_secret("host", &json!({"id":id}))?;
        id
    };
    let server =
        Server::http("127.0.0.1:0").map_err(|_| "Could not start the local login callback.")?;
    let port = server
        .server_addr()
        .to_ip()
        .ok_or("Local login callback is unavailable.")?
        .port();
    drop(host_guard);
    let redirect = format!("http://127.0.0.1:{port}/auth/callback");
    let state = identity::random()?;
    let nonce = identity::random()?;
    let verifier = identity::random()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let client = saved["client_id"].as_str().map(str::to_owned);
    let mut url = Url::parse(&format!("{AUTH}/api/accounts/authorize"))
        .map_err(|_| "Invalid account authorization endpoint.")?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair(
            "client_id",
            client.as_deref().unwrap_or("dynamic_agent_client"),
        )
        .append_pair("ext_agent_host_id", &host_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect)
        .append_pair(
            "scope",
            "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        )
        .append_pair("resource", RESOURCE)
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", &challenge);
        if client.is_none() {
            q.append_pair("agent_name_hint", "GoVue");
        } else if let Some(hint) = saved["id_token"].as_str() {
            q.append_pair("id_token_hint", hint);
        }
    }
    let job = jobs::new_job(&p.id)?;
    let worker = job.clone();
    // This carries only a task identity, never the authorization code or tokens.
    let return_url = format!("govue://ai-login/{}", job.id);
    let browser_return = return_url.clone();
    thread::spawn(move || {
        let result = (|| {
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                worker.pending()?;
                if Instant::now() >= deadline {
                    return Err("Sign-in timed out. Please start again.".into());
                }
                let Some(request) = server
                    .recv_timeout(Duration::from_millis(200))
                    .map_err(|_| "Login callback failed.")?
                else {
                    continue;
                };
                if request.method() != &Method::Get || !request.url().starts_with("/auth/callback?")
                {
                    let _ =
                        request.respond(Response::from_string("Not found").with_status_code(404));
                    continue;
                }
                let callback_url = request.url().to_owned();
                let received = callback(&callback_url, &state, client.as_deref());
                let accepted = received.is_ok();
                let correlated = Url::parse(&format!("http://127.0.0.1{callback_url}"))
                    .is_ok_and(|u| u.query_pairs().any(|(k, v)| k == "state" && v == state));
                if accepted || correlated {
                    let _ = request.respond(
                        Response::empty(302)
                            .with_header(crate::transport::header("Location", &browser_return)?)
                            .with_header(crate::transport::header("Cache-Control", "no-store")?),
                    );
                } else {
                    let _ = request.respond(
                        Response::from_string("Sign-in could not be verified.")
                            .with_status_code(400),
                    );
                }
                // Invalid unsolicited callbacks cannot consume the pending attempt.
                let (code, issued) = match received {
                    Ok(v) => v,
                    Err(e) => {
                        let matches = Url::parse(&format!("http://127.0.0.1{callback_url}"))
                            .is_ok_and(|u| {
                                u.query_pairs().any(|(k, v)| k == "state" && v == state)
                            });
                        if matches {
                            return Err(e);
                        }
                        continue;
                    }
                };
                worker
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?["message"] =
                    json!("Return to GoVue to finish signing in.");
                while !FOREGROUND.load(Ordering::Relaxed) {
                    worker.pending()?;
                    if Instant::now() >= deadline {
                        return Err("Sign-in timed out. Please start again.".into());
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                let lock = session(&p.id)?;
                let _lock = lock
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
                // Save the registration before exchange so a failed exchange can reauthorize it.
                let mut previous = saved.clone();
                previous["client_id"] = json!(issued);
                worker.pending()?;
                providers::provider(&root, &p.id)?;
                write_secret(&p.id, &previous)?;
                let response = form_request(
                    &[
                        ("grant_type", "authorization_code"),
                        ("client_id", &issued),
                        ("code", &code),
                        ("code_verifier", &verifier),
                        ("redirect_uri", &redirect),
                        ("resource", RESOURCE),
                    ],
                    &worker,
                )?;
                let claims = verify_identity(
                    crate::string(&response, "id_token")?,
                    &issued,
                    &nonce,
                    &worker,
                )?;
                if let Some(subject) = saved["subject"].as_str()
                    && claims["sub"] != subject
                {
                    return Err("Sign-in used a different ChatGPT account. Add another provider for that account.".into());
                }
                let mut tokens = token_set(response, &json!({"client_id":issued}))?;
                tokens["subject"] = claims["sub"].clone();
                tokens["email"] = claims["email"].clone();
                worker.pending()?;
                providers::provider(&root, &p.id)?;
                write_secret(&p.id, &tokens)?;
                return Ok(json!({"planEnabled":has_plan(&tokens)}));
            }
        })();
        worker.finish(result);
    });
    Ok(json!({"taskId":job.id,"url":url.as_str(),"returnUrl":return_url}))
}

pub(super) fn disconnect(root: &Path, p: &Provider, remove: bool, job: &Arc<Job>) -> Result<Value> {
    let lock = session(&p.id)?;
    let _g = lock
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    let (s, readable) = match read_secret(&p.id) {
        Ok(s) => (s, true),
        Err(_) => (json!({}), false),
    };
    let mut revoked = !catalog::oauth(p) || (p.kind == "chatgpt" && readable);
    if p.kind == "chatgpt"
        && let Some(token) = s["refresh_token"].as_str()
    {
        let metadata = http::json_http(
            format!("{AUTH}/.well-known/openid-configuration"),
            vec![],
            None,
            job,
        );
        revoked = (|| {
            let metadata = metadata?;
            let url = Url::parse(crate::string(&metadata, "revocation_endpoint")?)
                .map_err(|_| "Invalid revocation endpoint.")?;
            if url.scheme() != "https"
                || url.host_str() != Some("auth.openai.com")
                || url.port_or_known_default() != Some(443)
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("Invalid revocation endpoint.".into());
            }
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("token", token),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", crate::string(&s, "client_id")?),
                ])
                .finish();
            http::http(
                url.into(),
                "POST",
                vec![("Content-Type", "application/x-www-form-urlencoded".into())],
                body,
                None,
                Some(job),
            )
        })()
        .is_ok();
    }
    // Always finish an explicitly requested local sign-out, even if revocation is offline.
    let remaining = if remove {
        Value::Null
    } else {
        json!({"client_id":s["client_id"],"subject":s["subject"],"email":s["email"],"modelOptions":s["modelOptions"]})
    };
    write_secret(&p.id, &remaining)?;
    if remove {
        let _g = CONFIG_LOCK
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let mut c = providers::config(root)?;
        c.providers.retain(|v| v.id != p.id);
        for g in &mut c.groups {
            g.members.retain(|m| m.provider_id != p.id);
        }
        c.groups.retain(|g| !g.members.is_empty());
        if c.active_id.as_deref() == Some(&p.id)
            || c.active_id.as_ref().is_some_and(|id| {
                !c.providers.iter().any(|p| &p.id == id) && !c.groups.iter().any(|g| &g.id == id)
            })
        {
            c.active_id = c.providers.first().map(|p| p.id.clone());
        }
        providers::save(root, &c)?;
    }
    Ok(json!({"revoked":revoked}))
}
