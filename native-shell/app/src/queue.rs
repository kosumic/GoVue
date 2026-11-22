use govue_shell_runtime::{Core, WatchPlayback};
use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub queue: Vec<QueueItem>,
    pub enabled: bool,
    pub loop_mode: String,
    pub has_previous: bool,
    pub has_next: bool,
    pub next: Option<QueueItem>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub video_id: String,
    pub title: String,
    pub author: String,
    pub position: f64,
    pub playlist_id: Option<String>,
}

impl QueueItem {
    pub fn address(&self) -> String {
        let mut url = format!("https://m.youtube.com/watch?v={}", self.video_id);
        if let Some(list) = &self.playlist_id {
            // The shared queue only stores YouTube playlist identifiers.
            url.push_str("&list=");
            url.push_str(list);
        }
        if self.position > 0.0 {
            use std::fmt::Write;
            let _ = write!(url, "&t={}s", self.position.floor() as u64);
        }
        url
    }
}

type After = Box<dyn FnOnce(QueueState)>;

struct Request {
    value: Value,
    then: Option<After>,
}

#[derive(Clone)]
pub struct Queue {
    watch: WatchPlayback,
    queue: Signal<QueueState>,
    error: Signal<Option<String>>,
    tx: mpsc::UnboundedSender<Request>,
}

impl Queue {
    pub fn new(core: Core, watch: WatchPlayback) -> Self {
        let (tx, mut commands) = mpsc::unbounded_channel::<Request>();
        let queue = Self {
            watch,
            queue: signal(QueueState::default()),
            error: signal(None),
            tx,
        };
        let state = queue.queue;
        let error = queue.error;
        spawn(async move {
            while let Some(request) = commands.recv().await {
                match core.execute::<QueueState>(request.value).await {
                    Ok(snapshot) => {
                        state.set(snapshot.clone());
                        error.set(None);
                        if let Some(then) = request.then {
                            then(snapshot);
                        }
                    }
                    Err(message) => error.set(Some(message.to_string())),
                }
            }
        });
        let video = computed({
            let watch = queue.watch.clone();
            move || watch.video().map(|video| video.video_id)
        });
        effect({
            let queue = queue.clone();
            move || {
                let _ = video.get();
                queue.update("state", json!({}));
            }
        });
        queue
    }

    pub fn signal(&self) -> Signal<QueueState> {
        self.queue
    }

    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    pub fn update(&self, operation: &str, value: Value) {
        self.request(operation, value, None);
    }

    pub fn add(&self, item: Value, finished: impl FnOnce() + 'static) {
        self.request(
            "add",
            json!({"item":item}),
            Some(Box::new(move |_| finished())),
        );
    }

    pub fn advance(&self, direction: &str, open: impl FnOnce(String) + 'static) {
        let watch = self.watch.clone();
        let owner = watch.video();
        let ended = direction == "ended";
        self.request(
            direction,
            json!({}),
            Some(Box::new(move |result: QueueState| {
                let current = watch.video();
                if owner.as_ref().map(|video| video.generation)
                    != current.as_ref().map(|video| video.generation)
                {
                    return;
                }
                if ended && result.loop_mode == "repeat" {
                    if let Some(video) = current {
                        let player = watch.player();
                        player.seek(video.generation, 0.0);
                        player.play(video.generation);
                    }
                } else if let Some(item) = result.next {
                    let address = item.address();
                    if let Some(video) = current {
                        watch.expect_queue_target(video.generation, &address);
                    }
                    open(address);
                }
            })),
        );
    }

    fn request(&self, operation: &str, mut value: Value, then: Option<After>) {
        if !value.is_object() {
            return;
        }
        value["op"] = json!(format!("player_{operation}"));
        if value.get("videoId").is_none() {
            value["videoId"] = json!(
                self.watch
                    .video()
                    .map_or_else(String::new, |video| video.video_id)
            );
        }
        let _ = self.tx.send(Request { value, then });
    }
}
