//! HTTPS allowlist, byte limits and per-job cookie handling.
use super::{
    ExtractionHeader, ExtractionHttp, ExtractionReply, ExtractionRequest, Job, MAX_BYTES, ORIGIN,
    UA,
};
use crate::Result;
use serde_json::Value;
use std::{
    io::Read,
    sync::OnceLock,
    time::{Duration, Instant},
};
use url::Url;

pub(super) static HTTP: OnceLock<Box<dyn ExtractionHttp>> = OnceLock::new();

pub(super) fn allowed_address(raw: &str, interpreter: bool) -> Result<()> {
    let url = Url::parse(raw).map_err(|_| "Invalid extraction address.")?;
    if raw.len() > 30_000
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || raw.chars().any(char::is_control)
    {
        return Err("Unsupported extraction address.".into());
    }
    let host = url.host_str().unwrap_or("");
    let allowed = if interpreter {
        [
            "www.youtube.com",
            "m.youtube.com",
            "www.google.com",
            "www.gstatic.com",
        ]
        .contains(&host)
            && (url.path().starts_with("/js/") || url.path().starts_with("/s/"))
    } else {
        ["www.youtube.com", "m.youtube.com", "youtube.com"].contains(&host)
            && matches!(
                url.path(),
                "/watch"
                    | "/youtubei/v1/player"
                    | "/youtubei/v1/att/get"
                    | "/api/timedtext"
                    | "/api/jnn/v1/GenerateIT"
                    | "/api/jnn/v1/Create"
            )
    };
    if allowed {
        Ok(())
    } else {
        Err("Unsupported extraction address.".into())
    }
}

pub(super) fn fetch_response_bounded(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
    limit: usize,
) -> Result<ExtractionReply> {
    if limit == 0 || limit > MAX_BYTES {
        return Err("Invalid extraction size limit.".into());
    }
    job.check()?;
    allowed_address(url, interpreter)?;
    let browser_request = interpreter
        || url.contains("/api/jnn/v1/")
        || url.contains("/api/timedtext")
        || url.contains("/youtubei/v1/att/get");
    let mut headers = vec![
        ExtractionHeader {
            name: "User-Agent".into(),
            value: if browser_request {
                job.user_agent.clone()
            } else {
                UA.into()
            },
        },
        ExtractionHeader {
            name: "Referer".into(),
            value: format!("{ORIGIN}/"),
        },
    ];
    for (name, value) in extra {
        headers.push(ExtractionHeader {
            name: (*name).into(),
            value: (*value).into(),
        });
    }
    let method = if body.is_some() { "POST" } else { "GET" };
    if !interpreter {
        let session = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        if !session.cookies.is_empty() {
            headers.push(ExtractionHeader {
                name: "Cookie".into(),
                value: session
                    .cookies
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
    }
    if body.is_some()
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
    {
        headers.push(ExtractionHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        });
    }
    let request = ExtractionRequest {
        url: url.into(),
        method: method.into(),
        headers,
        body: body.map_or_else(String::new, |b| b.to_string()),
        max_bytes: u32::try_from(limit).map_err(|_| "Invalid extraction size limit.")?,
        prefix_only: limit < MAX_BYTES,
    };
    let reply = if let Some(http) = HTTP.get() {
        http.request(request)
    } else {
        // Host CLI uses the same bounded contract as Android/iOS.
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .new_agent();
        let mut call = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(&request.url);
        for header in request.headers {
            call = call.header(&header.name, &header.value);
        }
        let call = call
            .body(request.body.as_bytes())
            .map_err(|_| "Invalid extraction request.")?;
        match agent.run(call) {
            Ok(response) => {
                let status = i32::from(response.status().as_u16());
                let headers = response
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .filter_map(|value| value.to_str().ok())
                    .map(|value| ExtractionHeader {
                        name: "Set-Cookie".into(),
                        value: value.into(),
                    })
                    .collect();
                let mut body = Vec::new();
                response
                    .into_body()
                    .into_reader()
                    .take(u64::from(request.max_bytes) + u64::from(!request.prefix_only))
                    .read_to_end(&mut body)
                    .map_err(|_| "YouTube extraction connection failed.")?;
                ExtractionReply {
                    status,
                    body,
                    headers,
                }
            }
            Err(_) => return Err("YouTube extraction connection failed.".into()),
        }
    };
    job.check()?;
    if !interpreter {
        let mut session = job
            .session
            .lock()
            .map_err(|_| "Extraction session unavailable.")?;
        for header in reply
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("Set-Cookie"))
        {
            for cookie in header.value.split(',') {
                if let Some((name, value)) = cookie
                    .trim()
                    .split(';')
                    .next()
                    .and_then(|cookie| cookie.split_once('='))
                    && name.len() <= 100
                    && value.len() <= 8192
                    && !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    && !value.chars().any(char::is_control)
                    && (session.cookies.len() < 20 || session.cookies.contains_key(name))
                {
                    if name == "VISITOR_INFO1_LIVE"
                        && session
                            .cookies
                            .get(name)
                            .is_none_or(|previous| previous != value)
                    {
                        session.since = Instant::now();
                    }
                    session.cookies.insert(name.into(), value.into());
                }
            }
        }
    }
    if reply.body.len() > limit + usize::from(limit == MAX_BYTES) {
        return Err("YouTube extraction response is too large.".into());
    }
    Ok(reply)
}
pub(super) fn fetch_response(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
) -> Result<ExtractionReply> {
    let reply = fetch_response_bounded(job, url, body, interpreter, extra, MAX_BYTES)?;
    if reply.body.len() > MAX_BYTES {
        return Err("YouTube extraction response is too large.".into());
    }
    Ok(reply)
}
pub(super) fn response_text(reply: ExtractionReply) -> Result<String> {
    if reply.status != 200 {
        return Err(format!("YouTube extraction request failed (HTTP {}).", reply.status).into());
    }
    String::from_utf8(reply.body).map_err(|_| "Unreadable YouTube extraction response.".into())
}
pub(super) fn fetch(
    job: &Job,
    url: &str,
    body: Option<Value>,
    interpreter: bool,
    extra: &[(&str, &str)],
) -> Result<String> {
    response_text(fetch_response(job, url, body, interpreter, extra)?)
}
