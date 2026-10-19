#![cfg(test)]

use govue_core::{MediaStreamHttp, MediaStreamReply, execute, install_media_stream_http};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::Read,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

struct Stream {
    next: u64,
    end: u64,
    case: String,
}
#[derive(Default)]
struct State {
    ranges: Vec<(u64, u64)>,
    streams: HashMap<u64, Stream>,
    sequence: u64,
    released: bool,
    closed: usize,
}
#[derive(Default)]
struct Control(Mutex<State>, Condvar);
impl Control {
    fn release(&self) {
        self.0.lock().unwrap().released = true;
        self.1.notify_all();
    }
}
struct Release(Arc<Control>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Http(Arc<Control>);
impl MediaStreamHttp for Http {
    fn open_range(
        &self,
        url: String,
        _: String,
        _: String,
        method: String,
        start: u64,
        end: u64,
    ) -> MediaStreamReply {
        assert_eq!(method, "GET");
        assert!(end - start < 1_048_576);
        let case = url::Url::parse(&url)
            .unwrap()
            .query_pairs()
            .find(|(key, _)| key == "case")
            .unwrap()
            .1
            .into_owned();
        let mut state = self.0.0.lock().unwrap();
        state.sequence += 1;
        let handle = state.sequence;
        state.ranges.push((start, end));
        state.streams.insert(
            handle,
            Stream {
                next: start,
                end,
                case: case.clone(),
            },
        );
        MediaStreamReply {
            status: if case == "redirect" { 302 } else { 206 },
            handle,
            length: if case == "length" { 1 } else { -1 },
            location: (case == "redirect").then(|| "https://example.org/private".into()),
        }
    }
    fn read(&self, handle: u64, max_bytes: u32) -> Vec<u8> {
        assert!((1..=65_536).contains(&max_bytes));
        let mut state = self.0.0.lock().unwrap();
        while !state.released && state.streams[&handle].next >= 8192 {
            let (next, timeout) = self
                .0
                .1
                .wait_timeout(state, Duration::from_secs(5))
                .unwrap();
            assert!(!timeout.timed_out(), "The stream fixture was not unblocked");
            state = next;
        }
        let stream = state.streams.get_mut(&handle).unwrap();
        if stream.case == "oversized" {
            return vec![0; max_bytes as usize + 1];
        }
        if stream.case == "truncated" && stream.next >= 8192 {
            return Vec::new();
        }
        let end = (stream.next + u64::from(max_bytes)).min(stream.end + 1);
        let bytes = (stream.next..end).map(|n| n.to_le_bytes()[0]).collect();
        stream.next = end;
        drop(state);
        bytes
    }
    fn close(&self, handle: u64) {
        let mut state = self.0.0.lock().unwrap();
        assert!(
            state.streams.remove(&handle).is_some(),
            "A handle must close exactly once"
        );
        state.closed += 1;
        drop(state);
        self.0.1.notify_all();
    }
}

fn call(command: Value) -> Value {
    let reply: Value = serde_json::from_str(&execute(command.to_string(), String::new())).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn source(case: &str, length: u64) -> String {
    call(json!({"op":"media_proxy","url":format!("https://r1.googlevideo.com/videoplayback?case={case}"),
        "length":length,"mimeType":"video/mp4","userAgent":"fixture","origin":"https://www.youtube.com"}))
        .as_str().unwrap().into()
}
fn read(uri: &str) -> std::io::Result<Vec<u8>> {
    let response = ureq::get(uri)
        .timeout(Duration::from_secs(5))
        .call()
        .map_err(std::io::Error::other)?;
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn streams_the_first_range_incrementally_and_closes_rejected_or_incomplete_bodies() {
    let control = Arc::new(Control::default());
    let _release = Release(control.clone());
    install_media_stream_http(Box::new(Http(control.clone())));
    let length = 2 * 1_048_576 + 123;
    let uri = source("blocked", length);
    let (prefix_tx, prefix_rx) = mpsc::channel();
    let client_uri = uri.clone();
    let client = std::thread::spawn(move || {
        let response = ureq::get(&client_uri)
            .timeout(Duration::from_secs(5))
            .call()
            .unwrap();
        let mut reader = response.into_reader();
        let mut bytes = vec![0; 1024];
        reader.read_exact(&mut bytes).unwrap();
        prefix_tx.send(bytes.clone()).unwrap();
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    });
    assert_eq!(
        prefix_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        (0u64..1024).map(|n| n.to_le_bytes()[0]).collect::<Vec<_>>()
    );
    assert_eq!(
        control.0.lock().unwrap().ranges,
        [(0, 1_048_575)],
        "The initial MiB body must still be open when the player receives bytes"
    );
    control.release();
    let bytes = client.join().unwrap();
    assert_eq!(bytes.len() as u64, length);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(n, byte)| *byte == n.to_le_bytes()[0])
    );
    let ranges = control.0.lock().unwrap().ranges.clone();
    assert_eq!(ranges.last().unwrap().1, length - 1);
    assert!(ranges.windows(2).all(|pair| pair[0].1 + 1 == pair[1].0));
    call(json!({"op":"media_proxy_release","uri":uri}));

    for case in ["length", "redirect", "oversized", "truncated"] {
        let uri = source(case, 16_384);
        assert!(
            read(&uri).is_err(),
            "{case} must not return a successful complete body"
        );
        call(json!({"op":"media_proxy_release","uri":uri}));
    }
    // The writer can finish just after the client receives its final bytes.
    let mut state = control.0.lock().unwrap();
    while !state.streams.is_empty() {
        let (next, timeout) = control
            .1
            .wait_timeout(state, Duration::from_secs(3))
            .unwrap();
        assert!(!timeout.timed_out(), "An upstream response handle leaked");
        state = next;
    }
    assert_eq!(state.closed as u64, state.sequence);
}
