#![cfg(test)]

use govue_core::{MediaHttp, MediaRangeReply, execute, install_media_http};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

#[derive(Default)]
struct State {
    ranges: Vec<(u64, u64)>,
    released: bool,
}
#[derive(Default)]
struct Control {
    state: Mutex<State>,
    wake: Condvar,
}
impl Control {
    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.wake.notify_all();
    }
}
struct Release(Arc<Control>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Http(Arc<Control>);
impl MediaHttp for Http {
    fn fetch_range(
        &self,
        _: String,
        _: String,
        _: String,
        _: String,
        start: u64,
        end: u64,
    ) -> MediaRangeReply {
        let mut state = self.0.state.lock().unwrap();
        state.ranges.push((start, end));
        self.0.wake.notify_all();
        while start > 0 && !state.released {
            let (next, timeout) = self
                .0
                .wake
                .wait_timeout(state, Duration::from_secs(10))
                .unwrap();
            assert!(!timeout.timed_out(), "The fixture was never unblocked");
            state = next;
        }
        drop(state);
        MediaRangeReply {
            status: 206,
            body: (start..=end).map(|n| n.to_le_bytes()[0]).collect(),
            location: None,
        }
    }
}

#[test]
fn the_player_receives_initial_bytes_while_later_upstream_ranges_are_still_blocked() {
    let control = Arc::new(Control::default());
    install_media_http(Box::new(Http(control.clone())));
    let release = Release(control.clone());
    let call = |command: Value| -> Value {
        let result: Value =
            serde_json::from_str(&execute(command.to_string(), String::new())).unwrap();
        assert_eq!(result["ok"], true, "{result}");
        result["value"].clone()
    };
    let length: u64 = 3 * 1_048_576 + 123;
    let uri = call(
        json!({"op":"media_proxy","url":"https://r1.googlevideo.com/videoplayback?sig=fixture",
        "length":length,"mimeType":"video/mp4","userAgent":"fixture","origin":"https://www.youtube.com"}),
    );
    let uri = uri.as_str().unwrap().to_owned();
    let (prefix_tx, prefix_rx) = mpsc::channel();
    let (complete_tx, complete_rx) = mpsc::channel();
    let client_uri = uri.clone();
    let client = std::thread::spawn(move || {
        let response = ureq::get(&client_uri)
            .config()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .call()
            .unwrap();
        let mut reader = response.into_body().into_reader();
        let mut bytes = vec![0; 1024];
        reader.read_exact(&mut bytes).unwrap();
        prefix_tx.send(bytes.clone()).unwrap();
        reader.read_to_end(&mut bytes).unwrap();
        complete_tx.send(bytes).unwrap();
    });
    let prefix = prefix_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("Startup bytes must be available before the remaining stream finishes downloading");
    assert_eq!(
        prefix,
        (0u64..1024).map(|n| n.to_le_bytes()[0]).collect::<Vec<_>>()
    );
    let mut state = control.state.lock().unwrap();
    while state.ranges.len() < 2 {
        let (next, timeout) = control
            .wake
            .wait_timeout(state, Duration::from_secs(3))
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "The proxy never advanced to the next range"
        );
        state = next;
    }
    assert!(
        state.ranges[0].1 < 65_536,
        "Startup must not buffer an entire MiB"
    );
    drop(state);
    assert!(matches!(
        complete_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    control.release();
    let complete = complete_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(complete.len() as u64, length);
    assert!(
        complete
            .iter()
            .enumerate()
            .all(|(n, byte)| *byte == n.to_le_bytes()[0])
    );
    client.join().unwrap();
    let ranges = control.state.lock().unwrap().ranges.clone();
    assert_eq!(ranges[0].0, 0);
    assert_eq!(ranges.last().unwrap().1, length - 1);
    assert!(ranges.iter().all(|(start, end)| end - start < 1_048_576));
    assert!(ranges.windows(2).all(|pair| pair[0].1 + 1 == pair[1].0));
    call(json!({"op":"media_proxy_release","uri":uri}));
    drop(release);
}
