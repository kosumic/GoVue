#![cfg(test)]

use govue_core::{MediaHttp, MediaRangeReply, execute, install_media_http};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Clone, Debug)]
struct Seen {
    url: String,
    method: String,
    start: u64,
    end: u64,
}
struct Http(Arc<Mutex<Vec<Seen>>>);
impl MediaHttp for Http {
    fn fetch_range(
        &self,
        url: String,
        agent: String,
        origin: String,
        method: String,
        start: u64,
        end: u64,
    ) -> MediaRangeReply {
        assert_eq!(agent, "fixture-client");
        assert_eq!(origin, "https://m.youtube.com");
        self.0.lock().unwrap().push(Seen {
            url: url.clone(),
            method: method.clone(),
            start,
            end,
        });
        let parsed = Url::parse(&url).unwrap();
        let case = parsed
            .query_pairs()
            .find(|(key, _)| key == "case")
            .unwrap()
            .1
            .into_owned();
        let redirect = |status, location: &str| MediaRangeReply {
            status,
            body: Vec::new(),
            location: Some(location.into()),
        };
        match case.as_str() {
            "edge" => redirect(
                302,
                "https://r2.googlevideo.com/videoplayback?case=target&sig=A%2fb+C&range=0-1&rn=8",
            ),
            "relative" => redirect(302, "?case=target&sig=A%2fb+C&range=0-1"),
            "foreign" => redirect(302, "https://example.com/videoplayback"),
            "suffix" => redirect(302, "https://googlevideo.com.evil.test/videoplayback"),
            "http" => redirect(302, "http://r2.googlevideo.com/videoplayback"),
            "credentials" => redirect(302, "https://secret@r2.googlevideo.com/videoplayback"),
            "port" => redirect(302, "https://r2.googlevideo.com:444/videoplayback"),
            "fragment" => redirect(302, "https://r2.googlevideo.com/videoplayback#fragment"),
            "empty" => redirect(302, ""),
            "missing" => MediaRangeReply {
                status: 302,
                body: Vec::new(),
                location: None,
            },
            "loop" => redirect(302, "?case=loop"),
            "post307" | "post303" if method == "GET" => MediaRangeReply {
                status: 403,
                body: Vec::new(),
                location: None,
            },
            "post307" => redirect(
                307,
                "https://r2.googlevideo.com/videoplayback?case=post-target",
            ),
            "post303" => redirect(303, "https://r2.googlevideo.com/videoplayback?case=target"),
            "post-target" => {
                assert_eq!(method, "POST");
                MediaRangeReply {
                    status: 206,
                    body: (start..=end).map(|n| n.to_le_bytes()[0]).collect(),
                    location: None,
                }
            }
            "target" => {
                assert_eq!(method, "GET");
                MediaRangeReply {
                    status: 206,
                    body: (start..=end).map(|n| n.to_le_bytes()[0]).collect(),
                    location: None,
                }
            }
            _ => panic!("Unexpected fixture request"),
        }
    }
}

fn read(uri: &str, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let response = ureq::get(uri)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(3)))
        .build()
        .header("Range", &format!("bytes={start}-{end}"))
        .call()
        .map_err(std::io::Error::other)?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn redirects_follow_validated_edges_preserve_ranges_and_reuse_the_working_target() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    install_media_http(Box::new(Http(seen.clone())));
    let root = std::env::temp_dir().join(format!(
        "govue-redirects-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let call = |command: Value| -> Value {
        let reply: Value =
            serde_json::from_str(&execute(command.to_string(), root.to_string_lossy().into()))
                .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    };
    let source = |case: &str| {
        call(
            json!({"op":"media_proxy","url":format!("https://r1.googlevideo.com/videoplayback?case={case}"),
        "length":1000,"mimeType":"video/mp4","userAgent":"fixture-client","origin":"https://m.youtube.com"}),
        )
    };
    for case in ["edge", "relative"] {
        seen.lock().unwrap().clear();
        let uri = source(case);
        let uri = uri.as_str().unwrap();
        assert_eq!(read(uri, 20, 29).unwrap(), (20u8..30).collect::<Vec<_>>());
        assert_eq!(read(uri, 50, 59).unwrap(), (50u8..60).collect::<Vec<_>>());
        let requests = seen.lock().unwrap().clone();
        assert_eq!(
            requests.len(),
            3,
            "later ranges must bypass the redirecting edge"
        );
        assert!(
            requests[1].url.contains("sig=A%2fb+C"),
            "signed query encoding must survive"
        );
        assert!(!requests[1].url.contains("range=") || requests[1].url.contains("range=20-29"));
        assert!(!requests[1].url.contains("rn="));
        assert_eq!(requests[1].url, requests[2].url);
        assert_eq!((requests[1].start, requests[1].end), (20, 29));
        assert_eq!((requests[2].start, requests[2].end), (50, 59));
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
    for case in [
        "foreign",
        "suffix",
        "http",
        "credentials",
        "port",
        "fragment",
        "empty",
        "missing",
        "loop",
    ] {
        seen.lock().unwrap().clear();
        let uri = source(case);
        assert!(read(uri.as_str().unwrap(), 20, 29).is_err());
        assert_eq!(
            seen.lock().unwrap().len(),
            if case == "loop" { 5 } else { 1 },
            "{case}"
        );
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
    for (case, expected) in [
        ("post307", ["GET", "POST", "POST", "POST"]),
        ("post303", ["GET", "POST", "GET", "GET"]),
    ] {
        seen.lock().unwrap().clear();
        let uri = source(case);
        for (start, end) in [(20, 29), (50, 59)] {
            assert_eq!(
                read(uri.as_str().unwrap(), start, end).unwrap(),
                (start..=end)
                    .map(|n| n.to_le_bytes()[0])
                    .collect::<Vec<_>>()
            );
        }
        let requests = seen.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .map(|r| r.method.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
    let _ = fs::remove_dir_all(root);
}
