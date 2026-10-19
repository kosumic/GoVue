#![cfg(test)]

use govue_core::{
    ExtractionHttp, ExtractionReply, ExtractionRequest, execute, install_extraction_http,
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

const SEARCH: &str = "https://m.youtube.com/results?search_query=fixture";
#[derive(Default)]
struct State {
    requests: Vec<String>,
    blocked: HashSet<String>,
}
#[derive(Default)]
struct Control {
    state: Mutex<State>,
    wake: Condvar,
}
impl Control {
    fn block(&self, id: &str) {
        self.state.lock().unwrap().blocked.insert(id.into());
    }
    fn unblock(&self, id: &str) {
        self.state.lock().unwrap().blocked.remove(id);
        self.wake.notify_all();
    }
    fn count(&self, id: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|value| *value == id)
            .count()
    }
    fn entered(&self, id: &str) {
        let mut state = self.state.lock().unwrap();
        while !state.requests.iter().any(|value| value == id) {
            let result = self
                .wake
                .wait_timeout(state, Duration::from_secs(5))
                .unwrap();
            assert!(!result.1.timed_out(), "Preparation never made a request");
            state = result.0;
        }
        drop(state);
    }
}
struct Http(Arc<Control>);
impl ExtractionHttp for Http {
    fn request(&self, request: ExtractionRequest) -> ExtractionReply {
        let url = Url::parse(&request.url).unwrap();
        let watch = url.path() == "/watch";
        let id = if watch {
            url.query_pairs()
                .find(|(key, _)| key == "v")
                .unwrap()
                .1
                .into_owned()
        } else {
            assert_eq!(url.path(), "/youtubei/v1/player");
            serde_json::from_str::<Value>(&request.body).unwrap()["videoId"]
                .as_str()
                .unwrap()
                .into()
        };
        let mut state = self.0.state.lock().unwrap();
        state.requests.push(id.clone());
        self.0.wake.notify_all();
        while state.blocked.contains(&id) {
            let result = self
                .0
                .wake
                .wait_timeout(state, Duration::from_secs(5))
                .unwrap();
            assert!(!result.1.timed_out(), "Fixture request remained blocked");
            state = result.0;
        }
        drop(state);
        let body = if watch {
            format!(
                "ytcfg.set({});",
                json!({"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"fixture"}}})
            )
        } else {
            json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":id,"title":"Prepared video","author":"Fixture","lengthSeconds":"10"},
                "streamingData":{"adaptiveFormats":[{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.googlevideo.com/videoplayback?sig=memory-only","contentLength":"2048","bitrate":64000}]}}).to_string()
        };
        ExtractionReply {
            status: 200,
            body: body.into_bytes(),
            headers: vec![],
        }
    }
}
fn call(root: &str, command: Value) -> Value {
    let result: Value = serde_json::from_str(&execute(command.to_string(), root.into())).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
fn root(name: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "govue-prepare-{name}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    let root = path.to_string_lossy().into_owned();
    call(&root, json!({"op":"browser_state"}));
    call(
        &root,
        json!({"op":"browser_document","id":1,"generation":7,"url":SEARCH}),
    );
    root
}
fn prepare(id: &str) -> Value {
    json!({"op":"extract_prepare","requestId":"prepare:media","id":1,"generation":7,"origin":SEARCH,"documentUrl":SEARCH,"url":format!("https://m.youtube.com/watch?v={id}")})
}
fn start(root: &str, id: &str, fresh: bool) -> Value {
    call(
        root,
        json!({"op":"extract_start","kind":"media","requestId":"playback","videoId":id,"fresh":fresh}),
    )
}
fn finish(root: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value = call(root, json!({"op":"extract_poll","requestId":"playback"}));
        if value["done"] == true {
            assert!(value["error"].is_null(), "{value}");
            assert_eq!(value["value"]["title"], "Prepared video");
            call(root, json!({"op":"extract_release","requestId":"playback"}));
            return;
        }
        assert!(Instant::now() < deadline, "Playback extraction stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn launch_preparation(control: &Control) {
    let root = std::env::temp_dir().join(format!(
        "govue-launch-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = root.to_str().unwrap();
    let launch =
        |text: &str| json!({"op":"extract_launch","requestId":"prepare:media","text":text});
    for text in [
        "",
        "https://example.org/watch?v=launchVid01",
        "https://youtube.com.evil.test/watch?v=launchVid01",
        "https://m.youtube.com/shorts/launchVid01",
    ] {
        assert_eq!(call(root, launch(text))["prepared"], false);
    }
    let id = "launchVid01";
    control.block(id);
    // Android can prepare before a browser document or React navigation exists.
    call(
        root,
        launch("Shared video: https://youtu.be/launchVid01?t=372"),
    );
    control.entered(id);
    call(
        root,
        json!({"op":"browser_open","url":"https://m.youtube.com/watch?v=launchVid01&t=372"}),
    );
    assert_eq!(start(root, id, false)["prepared"], true);
    control.unblock(id);
    finish(root);
    assert_eq!(
        control.count(id),
        2,
        "Launch and playback must share one extraction"
    );
    call(
        root,
        json!({"op":"media_settings","settings":{"adBlock":false,"backgroundPlayback":false,"autoPip":false}}),
    );
    assert_eq!(
        call(root, launch("https://youtu.be/launchVid02"))["prepared"],
        false
    );
    assert_eq!(
        control.count("launchVid02"),
        0,
        "Disabled native playback must not prefetch"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_preparation_is_shared_bounded_and_correlated_with_navigation() {
    let control = Arc::new(Control::default());
    install_extraction_http(Box::new(Http(control.clone())));
    launch_preparation(&control);
    let root = root("handoff");
    let first = "abcdefghijk";
    let request = prepare(first);
    for patch in [
        json!({"generation":6}),
        json!({"id":2}),
        json!({"origin":"https://evil.test"}),
        json!({"documentUrl":"https://evil.test/watch?v=0123456789a"}),
        json!({"url":"https://youtube.com.evil.test/watch?v=abcdefghijk"}),
        json!({"url":"https://sub.youtube.com/watch?v=abcdefghijk"}),
        json!({"url":"https://m.youtube.com/shorts/abcdefghijk"}),
        json!({"url":"http://m.youtube.com/watch?v=abcdefghijk"}),
    ] {
        let mut invalid = request.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(call(&root, invalid)["prepared"], false);
    }
    assert_eq!(control.count(first), 0);
    control.block(first);
    call(&root, request.clone());
    control.entered(first);
    assert_eq!(call(&root, request)["prepared"], true);
    call(
        &root,
        json!({"op":"browser_document","id":1,"generation":8,"url":format!("https://m.youtube.com/watch?v={first}")}),
    );
    call(
        &root,
        json!({"op":"browser_update","id":1,"url":format!("https://m.youtube.com/watch?v={first}")}),
    );
    assert_eq!(start(&root, first, false)["prepared"], true);
    // The old preparation owner must not cancel the playback owner.
    call(
        &root,
        json!({"op":"extract_release","requestId":"prepare:media"}),
    );
    control.unblock(first);
    finish(&root);
    assert_eq!(
        control.count(first),
        2,
        "Expected one watch and one player request"
    );

    let replaced = "0123456789a";
    let selected = "ABCDEFGHIJK";
    let next = self::root("replacement");
    control.block(replaced);
    control.block(selected);
    call(&next, prepare(replaced));
    control.entered(replaced);
    call(&next, prepare(selected));
    control.entered(selected);
    control.unblock(replaced);
    assert_eq!(start(&next, selected, false)["prepared"], true);
    control.unblock(selected);
    finish(&next);
    assert_eq!(
        control.count(replaced),
        1,
        "Superseded work must not request a player"
    );
    assert_eq!(control.count(selected), 2);
    fs::remove_dir_all(next).unwrap();

    for (name, id, refresh) in [
        ("navigation", "abcdefghij0", false),
        ("refresh", "abcdefghij1", true),
    ] {
        let next = self::root(name);
        control.block(id);
        call(&next, prepare(id));
        control.entered(id);
        if !refresh {
            call(
                &next,
                json!({"op":"browser_update","id":1,"url":"https://m.youtube.com/"}),
            );
        }
        assert!(start(&next, id, refresh)["prepared"].is_null());
        control.unblock(id);
        finish(&next);
        assert_eq!(
            control.count(id),
            3,
            "Cancelled preparation must stop before its player request"
        );
        fs::remove_dir_all(next).unwrap();
    }

    let committed = "abcdefghij2";
    let next = self::root("spa");
    control.block(committed);
    call(
        &next,
        json!({"op":"browser_update","id":1,"url":format!("https://m.youtube.com/watch?v={committed}")}),
    );
    call(&next, prepare(committed));
    control.entered(committed);
    assert_eq!(start(&next, committed, false)["prepared"], true);
    control.unblock(committed);
    finish(&next);
    assert_eq!(control.count(committed), 2);
    let before_commit = self::root("before-spa");
    let early = "abcdefghij3";
    let mut request = prepare(early);
    request["documentUrl"] = request["url"].clone();
    control.block(early);
    call(&before_commit, request);
    control.entered(early);
    assert_eq!(start(&before_commit, early, false)["prepared"], true);
    control.unblock(early);
    finish(&before_commit);
    assert_eq!(control.count(early), 2);
    fs::remove_dir_all(before_commit).unwrap();
    for root in [root, next] {
        fs::remove_dir_all(root).unwrap();
    }
}
