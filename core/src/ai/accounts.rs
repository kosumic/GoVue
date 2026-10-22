//! Browser PKCE and RFC 8628 account connections. Tokens remain in secure storage.
use super::*;

struct Spec {
    client: &'static str,
    authorize: &'static str,
    token: &'static str,
    scope: &'static str,
    port: u16,
    path: &'static str,
}
fn spec(kind: &str) -> Result<Spec> {
    Ok(match kind {
        "codex" => Spec {
            client: "app_EMoamEEZ73f0CkXaXp7hrann",
            authorize: "https://auth.openai.com/oauth/authorize",
            token: "https://auth.openai.com/oauth/token",
            scope: "openid profile email offline_access",
            port: 1455,
            path: "/auth/callback",
        },
        "anthropic" => Spec {
            client: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
            authorize: "https://claude.ai/oauth/authorize",
            token: "https://claude.ai/v1/oauth/token",
            scope: "org:create_api_key user:profile user:inference",
            port: 54545,
            path: "/callback",
        },
        "xai" => Spec {
            client: "b1a00492-073a-47ea-816f-4c329264a828",
            authorize: "https://auth.x.ai/oauth2/authorize",
            token: "https://auth.x.ai/oauth2/token",
            scope: "openid profile email offline_access grok-cli:access api:access",
            port: 56121,
            path: "/callback",
        },
        "openrouter" => Spec {
            client: "",
            authorize: "https://openrouter.ai/auth",
            token: "https://openrouter.ai/api/v1/auth/keys",
            scope: "",
            port: 0,
            path: "/callback",
        },
        "kimi" => Spec {
            client: "17e5f671-d194-4dfb-9706-5516cb48c098",
            authorize: "https://auth.kimi.com/api/oauth/device_authorization",
            token: "https://auth.kimi.com/api/oauth/token",
            scope: "",
            port: 0,
            path: "",
        },
        "copilot" => Spec {
            client: "Iv1.b507a08c87ecfe98",
            authorize: "https://github.com/login/device/code",
            token: "https://github.com/login/oauth/access_token",
            scope: "read:user",
            port: 0,
            path: "",
        },
        _ => return Err("This provider does not support account sign-in.".into()),
    })
}
pub(super) fn claude_headers() -> Vec<(&'static str, String)> {
    [
        ("User-Agent", "claude-cli/2.1.280 (external, cli)"),
        ("X-App", "cli"),
        ("X-Stainless-Lang", "js"),
        ("X-Stainless-Package-Version", "0.106.0"),
        ("X-Stainless-OS", "Linux"),
        ("X-Stainless-Arch", "arm64"),
        ("X-Stainless-Runtime", "node"),
        ("X-Stainless-Runtime-Version", "v24.18.0"),
        ("X-Stainless-Retry-Count", "0"),
        ("X-Stainless-Timeout", "600"),
        ("Anthropic-Dangerous-Direct-Browser-Access", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k, v.into()))
    .collect()
}
pub(super) fn copilot_headers() -> Vec<(&'static str, String)> {
    [
        ("User-Agent", "GitHubCopilotChat/0.31.0"),
        ("Editor-Version", "vscode/1.104.0"),
        ("Editor-Plugin-Version", "copilot-chat/0.31.0"),
        ("X-GitHub-Api-Version", "2025-04-01"),
        ("Copilot-Integration-Id", "vscode-chat"),
        ("Openai-Intent", "conversation-panel"),
    ]
    .into_iter()
    .map(|(k, v)| (k, v.into()))
    .collect()
}
fn raw(
    url: &str,
    body: Option<Value>,
    form: bool,
    mut hs: Vec<(&str, String)>,
    job: &Arc<Job>,
) -> Result<Value> {
    hs.push(("Accept", "application/json".into()));
    let method = if body.is_some() { "POST" } else { "GET" };
    let body = if let Some(b) = body {
        hs.push((
            "Content-Type",
            if form {
                "application/x-www-form-urlencoded"
            } else {
                "application/json"
            }
            .into(),
        ));
        if form {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in b.as_object().ok_or("Invalid authorization request.")? {
                serializer.append_pair(k, v.as_str().ok_or("Invalid authorization request.")?);
            }
            serializer.finish()
        } else {
            b.to_string()
        }
    } else {
        String::new()
    };
    http_impl(url.into(), method, hs, body, None, Some(job), true)
}
fn checked(reply: Value) -> Result<Value> {
    let status = reply["status"].as_i64().unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(format!(
            "Account authorization failed (HTTP {status}). Reconnect or try again later."
        ));
    }
    Ok(reply["data"].clone())
}
fn token_bundle(mut data: Value, previous: &Value) -> Result<Value> {
    let access = string(&data, "access_token")?;
    if access.is_empty() || access.len() > 32_000 || access.chars().any(char::is_control) {
        return Err("Account returned an invalid access token.".into());
    }
    if data["token_type"]
        .as_str()
        .is_some_and(|kind| !kind.eq_ignore_ascii_case("Bearer"))
    {
        return Err("Account returned an unsupported token type.".into());
    }
    let expires = data["expires_in"].as_u64().unwrap_or(3600);
    if expires == 0 || expires > 31_536_000 {
        return Err("Account returned an invalid token expiry.".into());
    }
    for field in [
        "refresh_token",
        "email",
        "subject",
        "account_id",
        "modelOptions",
        "id_token",
        "token_endpoint",
    ] {
        if data.get(field).is_none() {
            data[field] = previous[field].clone();
        }
    }
    data["expires_at"] = json!(now() + expires);
    data["needs_reauth"] = json!(false);
    Ok(data)
}
fn wait_foreground(job: &Arc<Job>, deadline: Instant) -> Result<()> {
    while !FOREGROUND.load(Ordering::Relaxed) {
        pause(job, Duration::from_millis(200), deadline)?;
    }
    Ok(())
}
fn pause(job: &Arc<Job>, delay: Duration, deadline: Instant) -> Result<()> {
    let until = Instant::now() + delay;
    loop {
        job.pending()?;
        if Instant::now() >= deadline {
            return Err("Sign-in timed out. Start again.".into());
        }
        if Instant::now() >= until {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn save_tokens(root: &Path, p: &Provider, job: &Arc<Job>, data: Value) -> Result<Value> {
    let lock = session(&p.id)?;
    let _g = lock
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    connections::check_current(root, p, job)?;
    let previous = read_secret(&p.id)?;
    if let (Some(saved), Some(received)) = (previous["subject"].as_str(), data["subject"].as_str())
        && saved != received
    {
        return Err(
            "Sign-in used a different account. Add a separate connection for that account.".into(),
        );
    }
    let mut data = if p.kind == "openrouter" {
        let key = string(&data, "key")?;
        if key.is_empty() || key.len() > 8192 || key.chars().any(char::is_control) {
            return Err("OpenRouter returned an invalid key.".into());
        }
        json!({"key":key,"modelOptions":previous["modelOptions"]})
    } else {
        token_bundle(data, &previous)?
    };
    // Copilot account tokens are never sent to the inference host.
    if p.kind == "copilot" {
        data["github_token"] = data["access_token"].clone();
        data["expires_at"] = json!(0);
    }
    write_secret(&p.id, &data)?;
    Ok(json!({"connected":true}))
}
pub(super) fn login(root: PathBuf, p: Provider, consent: bool) -> Result<Value> {
    if !catalog::oauth(&p) {
        return Err("Choose account sign-in as the connection's authentication method.".into());
    }
    if p.kind == "copilot" && !consent {
        return Err(
            "Review and accept the unofficial Copilot connection notice before signing in.".into(),
        );
    }
    let s = spec(&p.kind)?;
    cancel_owner(&p.id)?;
    let job = new_job(&p.id)?;
    let worker = job.clone();
    if matches!(p.kind.as_str(), "kimi" | "copilot") {
        thread::spawn(move || {
            let result = device_login(&root, &p, &s, &worker);
            worker.finish(result);
        });
        return Ok(json!({"taskId":job.id}));
    }
    // Bind before opening the browser; PKCE and callback correlation stay in Rust.
    let Ok(server) = tiny_http::Server::http(("127.0.0.1", s.port)) else {
        jobs()
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .remove(&job.id);
        return Err("The account callback port is in use. Close another sign-in and retry.".into());
    };
    let state = random()?;
    let verifier = random()?;
    let nonce = random()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let port = server
        .server_addr()
        .to_ip()
        .ok_or("Local callback is unavailable.")?
        .port();
    let path = if p.kind == "openrouter" {
        format!("{}/{}", s.path, state)
    } else {
        s.path.into()
    };
    let host = if p.kind == "codex" {
        "localhost"
    } else {
        "127.0.0.1"
    };
    let redirect = format!("http://{host}:{port}{path}");
    let return_url = format!("govue://ai-login/{}", job.id);
    let browser_return = return_url.clone();
    // xAI discovery runs in the worker, so the JS bridge never blocks on HTTPS.
    thread::spawn(move || {
        let result = (|| {
            let deadline = Instant::now() + Duration::from_secs(600);
            let mut authorize = s.authorize.to_owned();
            let mut token_endpoint = s.token.to_owned();
            if p.kind == "xai" {
                let meta = checked(raw(
                    "https://auth.x.ai/.well-known/openid-configuration",
                    None,
                    false,
                    vec![],
                    &worker,
                )?)?;
                for key in ["authorization_endpoint", "token_endpoint"] {
                    trusted(string(&meta, key)?, "auth.x.ai")?;
                }
                authorize = string(&meta, "authorization_endpoint")?.into();
                token_endpoint = string(&meta, "token_endpoint")?.into();
            }
            let mut url =
                Url::parse(&authorize).map_err(|_| "Invalid account authorization endpoint.")?;
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("code_challenge", &challenge)
                    .append_pair("code_challenge_method", "S256");
                if p.kind == "openrouter" {
                    q.append_pair("callback_url", &redirect);
                } else {
                    q.append_pair("client_id", s.client)
                        .append_pair("redirect_uri", &redirect)
                        .append_pair("response_type", "code")
                        .append_pair("scope", s.scope)
                        .append_pair("state", &state);
                    if p.kind == "codex" {
                        q.append_pair("id_token_add_organizations", "true")
                            .append_pair("codex_cli_simplified_flow", "true")
                            .append_pair("originator", "codex_cli_rs");
                    }
                    if p.kind == "xai" {
                        q.append_pair("nonce", &nonce)
                            .append_pair("plan", "generic")
                            .append_pair("referrer", "govue");
                    }
                }
            }
            worker
                .state
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?["login"] =
                json!({"url":url.as_str(),"returnUrl":browser_return,"mode":"browser"});
            loop {
                worker.pending()?;
                if Instant::now() >= deadline {
                    return Err("Sign-in timed out. Start again.".into());
                }
                let Some(request) = server
                    .recv_timeout(Duration::from_millis(200))
                    .map_err(|_| "Local callback failed.")?
                else {
                    continue;
                };
                let parsed = Url::parse(&format!("http://127.0.0.1{}", request.url()));
                let valid = parsed.as_ref().is_ok_and(|u| {
                    request.method() == &tiny_http::Method::Get
                        && u.path() == path
                        && (p.kind == "openrouter"
                            || u.query_pairs()
                                .filter(|(k, _)| k == "state")
                                .map(|(_, v)| v.into_owned())
                                .collect::<Vec<_>>()
                                == vec![state.clone()])
                });
                if !valid {
                    let _ = request.respond(tiny_http::Response::empty(400));
                    continue;
                }
                let u = parsed.map_err(|_| "Invalid account callback address.")?;
                let fields: Vec<_> = u.query_pairs().into_owned().collect();
                let codes: Vec<_> = fields.iter().filter(|(k, _)| k == "code").collect();
                let _ = request.respond(
                    tiny_http::Response::empty(302)
                        .with_header(crate::transport::header("Location", &browser_return)?)
                        .with_header(crate::transport::header("Cache-Control", "no-store")?),
                );
                if fields.iter().any(|(k, _)| k == "error") {
                    return Err("Account sign-in was declined.".into());
                }
                if codes.len() != 1 || codes[0].1.is_empty() || codes[0].1.len() > 8192 {
                    return Err(
                        "Account callback did not contain one valid authorization code.".into(),
                    );
                }
                worker
                    .state
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?["message"] =
                    json!("Return to GoVue to finish signing in.");
                wait_foreground(&worker, deadline)?;
                let mut body = json!({"grant_type":"authorization_code","client_id":s.client,"code":codes[0].1,"redirect_uri":redirect,"code_verifier":verifier});
                if p.kind == "anthropic" {
                    body["state"] = json!(state);
                }
                if p.kind == "xai" {
                    body["code_challenge"] = json!(challenge);
                    body["code_challenge_method"] = json!("S256");
                }
                if p.kind == "openrouter" {
                    body = json!({"code":codes[0].1,"code_verifier":verifier,"code_challenge_method":"S256"});
                }
                let mut data = checked(raw(
                    &token_endpoint,
                    Some(body),
                    p.kind == "xai",
                    if p.kind == "anthropic" {
                        claude_headers()
                    } else {
                        vec![]
                    },
                    &worker,
                )?)?;
                if p.kind == "codex" {
                    let claims =
                        verify_identity(string(&data, "id_token")?, s.client, "", &worker)?;
                    data["email"] = claims["email"].clone();
                    data["subject"] = claims["sub"].clone();
                    data["account_id"] =
                        claims["https://api.openai.com/auth"]["chatgpt_account_id"].clone();
                }
                if p.kind == "xai" {
                    let claims = verify_oidc(
                        string(&data, "id_token")?,
                        "https://auth.x.ai",
                        s.client,
                        &nonce,
                        &worker,
                    )?;
                    data["email"] = claims["email"].clone();
                    data["subject"] = claims["sub"].clone();
                    data["token_endpoint"] = json!(token_endpoint);
                }
                return save_tokens(&root, &p, &worker, data);
            }
        })();
        worker.finish(result);
    });
    Ok(json!({"taskId":job.id,"returnUrl":return_url}))
}
fn trusted(value: &str, host: &str) -> Result<()> {
    let u = Url::parse(value).map_err(|_| "Invalid account endpoint.")?;
    if u.scheme() != "https"
        || u.host_str() != Some(host)
        || u.port_or_known_default() != Some(443)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err("Untrusted account endpoint.".into());
    }
    Ok(())
}
fn verify_oidc(
    token: &str,
    issuer: &str,
    client: &str,
    nonce: &str,
    job: &Arc<Job>,
) -> Result<Value> {
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
    let meta = checked(raw(
        &format!("{issuer}/.well-known/openid-configuration"),
        None,
        false,
        vec![],
        job,
    )?)?;
    if meta["issuer"] != issuer {
        return Err("Unexpected account issuer.".into());
    }
    let jwks = string(&meta, "jwks_uri")?;
    let issuer_url = Url::parse(issuer).map_err(|_| "Invalid account issuer.")?;
    trusted(
        jwks,
        issuer_url
            .host_str()
            .ok_or("Missing account issuer host.")?,
    )?;
    let keys: JwkSet = serde_json::from_value(checked(raw(jwks, None, false, vec![], job)?)?)
        .map_err(|_| "Invalid account signing keys.")?;
    let head = decode_header(token).map_err(|_| "Invalid account identity.")?;
    if !matches!(head.alg, Algorithm::RS256 | Algorithm::ES256) {
        return Err("Unsupported account identity signature.".into());
    }
    let key = keys
        .find(head.kid.as_deref().ok_or("Missing signing key.")?)
        .ok_or("Unknown signing key.")?;
    let mut validation = Validation::new(head.alg);
    validation.set_audience(&[client]);
    validation.set_issuer(&[issuer]);
    validation.validate_nbf = true;
    let claims = decode::<Value>(
        token,
        &DecodingKey::from_jwk(key).map_err(|_| "Invalid signing key.")?,
        &validation,
    )
    .map_err(|_| "Could not verify account identity.")?
    .claims;
    if claims["nonce"] != nonce || claims["sub"].as_str().is_none_or(str::is_empty) {
        return Err("Account identity does not match this sign-in.".into());
    }
    Ok(claims)
}
fn device_login(root: &Path, p: &Provider, s: &Spec, job: &Arc<Job>) -> Result<Value> {
    let mut body = json!({"client_id":s.client});
    if !s.scope.is_empty() {
        body["scope"] = json!(s.scope);
    }
    let data = checked(raw(s.authorize, Some(body), true, vec![], job)?)?;
    let device = string(&data, "device_code")?;
    let code = string(&data, "user_code")?;
    let verify = data["verification_uri_complete"]
        .as_str()
        .or_else(|| data["verification_uri"].as_str())
        .or_else(|| data["verification_url"].as_str())
        .ok_or("Account did not return a verification address.")?;
    let host = Url::parse(verify).map_err(|_| "Invalid verification address.")?;
    let valid_host = if p.kind == "copilot" {
        host.host_str() == Some("github.com")
    } else {
        matches!(
            host.host_str(),
            Some("auth.kimi.com" | "www.kimi.com" | "kimi.com" | "kimi.ai" | "www.kimi.ai")
        )
    };
    if host.scheme() != "https"
        || !valid_host
        || !host.username().is_empty()
        || host.password().is_some()
        || code.len() > 100
        || device.len() > 8192
    {
        return Err("Account returned an untrusted verification address.".into());
    }
    let deadline = Instant::now()
        + Duration::from_secs(data["expires_in"].as_u64().unwrap_or(600).clamp(1, 1800));
    let mut interval = data["interval"].as_u64().unwrap_or(5).clamp(1, 60);
    job.state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?["login"] =
        json!({"url":verify,"userCode":code,"mode":"device"});
    loop {
        pause(job, Duration::from_secs(interval), deadline)?;
        wait_foreground(job, deadline)?;
        let reply = raw(
            s.token,
            Some(
                json!({"client_id":s.client,"device_code":device,"grant_type":"urn:ietf:params:oauth:grant-type:device_code"}),
            ),
            true,
            vec![],
            job,
        )?;
        let data = &reply["data"];
        if (200..300).contains(&reply["status"].as_i64().unwrap_or(0))
            && data["access_token"].is_string()
        {
            return save_tokens(root, p, job, data.clone());
        }
        match data["error"].as_str() {
            Some("authorization_pending") => (),
            Some("slow_down") => interval = (interval + 5).min(120),
            Some("access_denied") => return Err("Account authorization was declined.".into()),
            Some("expired_token") => return Err("Device code expired. Start sign-in again.".into()),
            _ => return Err("Device sign-in failed. Start again or check account access.".into()),
        }
    }
}
pub(super) fn credential(root: &Path, p: &Provider, job: &Arc<Job>) -> Result<String> {
    let lock = session(&p.id)?;
    let _g = lock
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    connections::check_current(root, p, job)?;
    let mut s = read_secret(&p.id)?;
    if !catalog::oauth(p) || p.kind == "openrouter" {
        return s["key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| (p.kind == "custom").then(String::new))
            .ok_or("Add an API key for this connection.".into());
    }
    if s["needs_reauth"] == true {
        return Err("This account needs to sign in again.".into());
    }
    if s["expires_at"].as_u64().unwrap_or(0) <= now() + 60 {
        let reply = if p.kind == "copilot" {
            let mut hs = copilot_headers();
            hs.push((
                "Authorization",
                format!("token {}", string(&s, "github_token")?),
            ));
            raw(
                "https://api.github.com/copilot_internal/v2/token",
                None,
                false,
                hs,
                job,
            )
        } else {
            let spec = spec(&p.kind)?;
            let endpoint = if p.kind == "xai" {
                s["token_endpoint"].as_str().unwrap_or(spec.token)
            } else {
                spec.token
            };
            if p.kind == "xai" {
                trusted(endpoint, "auth.x.ai")?;
            }
            raw(
                endpoint,
                Some(
                    json!({"client_id":spec.client,"grant_type":"refresh_token","refresh_token":string(&s,"refresh_token")?}),
                ),
                matches!(p.kind.as_str(), "kimi" | "xai"),
                if p.kind == "anthropic" {
                    claude_headers()
                } else {
                    vec![]
                },
                job,
            )
        };
        let reply = match reply {
            Ok(value) => value,
            Err(error) => {
                job.pending()?;
                if s["expires_at"].as_u64().unwrap_or(0) > now() {
                    return s["access_token"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .ok_or(error);
                }
                return Err(error);
            }
        };
        let status = reply["status"].as_i64().unwrap_or(0);
        if !(200..300).contains(&status) {
            let rejected = matches!(status, 400 | 401 | 403)
                && !matches!(
                    reply["data"]["error"].as_str(),
                    Some("temporarily_unavailable" | "server_error")
                );
            if rejected {
                s["needs_reauth"] = json!(true);
                job.pending()?;
                write_secret(&p.id, &s)?;
            }
            // Transient failures keep a still-valid access token usable.
            if rejected || s["expires_at"].as_u64().unwrap_or(0) <= now() {
                return Err("Could not refresh this account. Reconnect or try again later.".into());
            }
        } else {
            let mut data = reply["data"].clone();
            if p.kind == "copilot" {
                data["access_token"] = data["token"].clone();
                data["expires_in"] = json!(
                    data["expires_at"]
                        .as_u64()
                        .unwrap_or(now() + 1200)
                        .saturating_sub(now())
                );
                data["github_token"] = s["github_token"].clone();
            }
            s = token_bundle(data, &s)?;
            job.pending()?;
            write_secret(&p.id, &s)?;
        }
    }
    s["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or("Sign in to this account first.".into())
}
