#![cfg(test)]

use govue_core::{ExtractionHeader, ExtractionHttp, ExtractionReply, ExtractionRequest};
use serde_json::{Value, json};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    prefix: bool,
    limit: u32,
    visitor: String,
    cookie: String,
}
struct Http(Arc<Mutex<Vec<Seen>>>);
impl ExtractionHttp for Http {
    fn request(&self, request: ExtractionRequest) -> ExtractionReply {
        let url = Url::parse(&request.url).unwrap();
        let data: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
        let id = data["videoId"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                url.query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, id)| id.into_owned())
            })
            .unwrap();
        let visitor = data["context"]["client"]["visitorData"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        self.0.lock().unwrap().push(Seen {
            path: url.path().into(),
            prefix: request.prefix_only,
            limit: request.max_bytes,
            visitor: visitor.clone(),
            cookie: request
                .headers
                .iter()
                .find(|h| h.name == "Cookie")
                .map_or_else(String::new, |h| h.value.clone()),
        });
        let mut headers = Vec::new();
        let body = match url.path() {
            "/watch" => {
                headers.push(ExtractionHeader { name: "Set-Cookie".into(),
                    value: format!("VISITOR_INFO1_LIVE=guest-{id}; Path=/; Secure") });
                let context = json!({"client":{"clientName":"WEB","clientVersion":"fixture","visitorData":id}});
                let web = json!({"videoDetails":{"videoId":id},"captions":{"playerCaptionsTracklistRenderer":{
                    "captionTracks":[{"baseUrl":format!("https://www.youtube.com/api/timedtext?v={id}&lang=en"),
                        "vssId":".en","languageCode":"en","name":{"simpleText":"English"}}]}}});
                // The complete nested context fits near the start even when
                // the enclosing ytcfg object and WEB metadata do not.
                let late = if id == "lateConfig1" { " ".repeat(200_000) } else { String::new() };
                format!("{late}ytcfg.set({{\"INNERTUBE_CONTEXT\":{context},\"TAIL\":\"{}\"}});var ytInitialPlayerResponse = {web};", "é".repeat(150_000))
            }
            "/youtubei/v1/player" if id == "needsFresh1" && visitor != id => {
                json!({"playabilityStatus":{"status":"LOGIN_REQUIRED"}}).to_string()
            }
            "/youtubei/v1/player" => {
                assert_eq!(data["context"]["client"]["clientName"], "VISIONOS");
                json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":id,"title":"Startup fixture","author":"Fixture","lengthSeconds":"10"},
                    "streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?sig=memory-only","contentLength":"2048","bitrate":64000}]}}).to_string()
            }
            "/api/timedtext" => json!({"events":[{"tStartMs":0,"dDurationMs":1000,"segs":[{"utf8":"Fixture caption"}]}]}).to_string(),
            _ => panic!("Unexpected fixture endpoint"),
        };
        let mut body = body.into_bytes();
        body.truncate(request.max_bytes as usize + usize::from(!request.prefix_only));
        ExtractionReply {
            status: 200,
            body,
            headers,
        }
    }
}
fn call(root: &str, command: Value) -> Value {
    let result: Value =
        serde_json::from_str(&govue_core::execute(command.to_string(), root.into())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn extract(root: &str, id: &str, kind: &str, fresh: bool) -> Value {
    call(
        root,
        json!({"op":"extract_start","kind":kind,"requestId":"fixture","videoId":id,"fresh":fresh}),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = call(root, json!({"op":"extract_poll","requestId":"fixture"}));
        if result["done"] == true {
            assert!(result["error"].is_null(), "{result}");
            call(root, json!({"op":"extract_release","requestId":"fixture"}));
            return result["value"].clone();
        }
        assert!(Instant::now() < deadline, "Extraction stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn playback_uses_bounded_configuration_but_transcripts_keep_the_full_web_metadata() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    govue_core::install_extraction_http(Box::new(Http(seen.clone())));
    let root = std::env::temp_dir().join(format!(
        "govue-startup-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = root.to_string_lossy().into_owned();
    let take = || std::mem::take(&mut *seen.lock().unwrap());

    let first = extract(&root, "abcdefghijk", "media", false);
    assert_eq!(first["streams"].as_array().unwrap().len(), 1);
    let requests = take();
    assert_eq!(
        requests.len(),
        2,
        "The large page tail must not be needed for playback"
    );
    assert_eq!(requests[0].path, "/watch");
    assert!(requests[0].prefix && requests[0].limit <= 262_144);
    assert_eq!(requests[1].visitor, "abcdefghijk");

    extract(&root, "ABCDEFGHIJK", "media", false);
    let requests = take();
    assert_eq!(requests.len(), 1, "Another video must reuse configuration");
    assert_eq!(requests[0].path, "/youtubei/v1/player");
    assert_eq!(requests[0].visitor, "abcdefghijk");
    assert!(requests[0].cookie.contains("guest-abcdefghijk"));
    extract(&root, "abcdefghijk", "media", false);
    assert!(
        take().is_empty(),
        "An unchanged video still uses its media cache"
    );

    let transcript = extract(&root, "abcdefghijk", "transcript", false);
    assert_eq!(transcript["segments"][0]["text"], "Fixture caption");
    assert_eq!(transcript["tracks"][0]["language"], "en");
    let requests = take();
    assert_eq!(
        requests.len(),
        3,
        "Transcript metadata must be fetched in full after prefix-only playback"
    );
    assert_eq!(requests[0].path, "/watch");
    assert!(!requests[0].prefix);
    assert_eq!(requests[2].path, "/api/timedtext");

    extract(&root, "ABCDEFGHIJK", "media", true);
    let requests = take();
    assert_eq!(
        requests.len(),
        2,
        "Explicit refresh must bypass configuration and media caches"
    );
    assert_eq!(requests[0].path, "/watch");
    assert!(requests[0].prefix);

    extract(&root, "needsFresh1", "media", false);
    let requests = take();
    assert_eq!(
        requests.len(),
        3,
        "A rejected cached context gets one fresh configuration attempt"
    );
    assert_eq!(requests[0].path, "/youtubei/v1/player");
    assert_eq!(requests[1].path, "/watch");
    assert_eq!(requests[2].visitor, "needsFresh1");

    let other = format!("{root}-isolated");
    fs::create_dir_all(&other).unwrap();
    extract(&other, "lateConfig1", "media", false);
    let requests = take();
    assert_eq!(
        requests.len(),
        3,
        "Configuration outside the prefix needs a bounded full-page fallback"
    );
    assert_eq!(requests[0].path, "/watch");
    assert!(
        requests[0].prefix && requests[0].cookie.is_empty(),
        "Guest sessions must remain isolated by app root"
    );
    assert_eq!(requests[1].path, "/watch");
    assert!(!requests[1].prefix);
    assert_eq!(requests[2].visitor, "lateConfig1");
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(other);
}
