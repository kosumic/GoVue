use govue_shell_runtime::{Core, WatchPlayback};
use nsh_core::{PointerEvents, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::ui::{self, press};

const SKIP: &str = "m5 4 10 8-10 8zM19 4v16";

#[derive(Clone, PartialEq)]
struct Video {
    id: String,
    enabled: bool,
    categories: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Segment {
    start: f64,
    end: f64,
    category: String,
}

#[derive(Clone, Default, Deserialize, PartialEq)]
struct Prompt {
    countdown: Option<u32>,
    chip: bool,
    highlight: bool,
    seek: Option<f64>,
}

enum Command {
    Select(Option<Video>),
    Segments(u64, Vec<Segment>),
    Sample {
        action: &'static str,
        position: Option<f64>,
    },
}

#[derive(Clone)]
pub struct SponsorBlock {
    watch: WatchPlayback,
    desired: Memo<Option<Video>>,
    prompt: Signal<Prompt>,
    sampling: Signal<bool>,
    loaded: Signal<bool>,
    tx: mpsc::UnboundedSender<Command>,
}

impl SponsorBlock {
    pub fn new(
        core: Core,
        watch: WatchPlayback,
        settings: Signal<Option<Map<String, Value>>>,
        video_id: impl Fn() -> Option<String> + 'static,
    ) -> Self {
        let desired = computed(move || {
            let id = video_id()?;
            settings.with(|settings| {
                let settings = settings.as_ref()?;
                let enabled = settings.get("sponsorBlock") == Some(&Value::Bool(true));
                let categories = settings
                    .get("categories")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(Video {
                    id,
                    enabled,
                    categories,
                })
            })
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let sponsor = Self {
            watch,
            desired,
            tx,
            prompt: signal(Prompt::default()),
            sampling: signal(false),
            loaded: signal(false),
        };
        spawn(sponsor.clone().run(core, settings, rx));
        effect({
            let tx = sponsor.tx.clone();
            move || {
                let _ = tx.send(Command::Select(desired.get()));
            }
        });
        effect({
            let sponsor = sponsor.clone();
            move || {
                if let Some((generation, position)) = sponsor.watch.seek_intent()
                    && sponsor
                        .watch
                        .video()
                        .is_some_and(|video| video.generation == generation)
                    && sponsor.loaded.get_untracked()
                {
                    sponsor.send("seek", Some(position));
                }
            }
        });
        sponsor
    }

    pub fn sample(&self) {
        if self.loaded.get_untracked() && !self.sampling.get_untracked() {
            self.sampling.set(true);
            self.send("tick", None);
        }
    }

    fn send(&self, action: &'static str, position: Option<f64>) {
        let _ = self.tx.send(Command::Sample { action, position });
    }

    async fn run(
        self,
        core: Core,
        settings: Signal<Option<Map<String, Value>>>,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        let mut owner = None::<Video>;
        let mut revision = 0;
        let mut fetch = None::<Spawned<()>>;
        while let Some(command) = commands.recv().await {
            match command {
                Command::Select(video) => {
                    revision += 1;
                    if let Some(task) = fetch.take() {
                        task.cancel();
                    }
                    self.loaded.set(false);
                    self.prompt.set(Prompt::default());
                    owner = video;
                    if let Some(video) = owner.as_ref() {
                        self.watch.set_sponsor_segments(video.id.clone(), json!([]));
                        let _ = core.execute::<Prompt>(json!({
                            "op":"sponsor_control","action":"segments","videoId":video.id,"segments":[]
                        })).await;
                        if video.enabled {
                            fetch = Some(spawn(load_segments(
                                core.clone(),
                                self.tx.clone(),
                                video.clone(),
                                revision,
                            )));
                        }
                    }
                }
                Command::Segments(current, segments) => {
                    if current == revision
                        && owner == self.desired.get_untracked()
                        && let Some(video) = owner.as_ref()
                    {
                        self.watch
                            .set_sponsor_segments(video.id.clone(), json!(segments));
                        if core.execute::<Prompt>(json!({
                            "op":"sponsor_control","action":"segments","videoId":video.id,"segments":segments
                        })).await.is_ok() {
                            self.loaded.set(!segments.is_empty());
                        }
                    }
                }
                Command::Sample { action, position } => {
                    if self.loaded.get_untracked()
                        && owner == self.desired.get_untracked()
                        && let Some((video, status)) = self.watch.video().zip(self.watch.status())
                        && owner
                            .as_ref()
                            .is_some_and(|owner| owner.id == video.video_id)
                    {
                        let countdown = settings.with_untracked(|value| {
                            value
                                .as_ref()
                                .and_then(|prefs| prefs.get("sponsorCountdown"))
                                != Some(&Value::Bool(false))
                        });
                        let result = core.execute::<Prompt>(json!({
                            "op":"sponsor_control","action":action,"videoId":video.video_id,
                            "position":position.unwrap_or(status.position),"playing":status.playing,"countdown":countdown
                        })).await;
                        if let Ok(prompt) = result
                            && owner == self.desired.get_untracked()
                            && self
                                .watch
                                .video()
                                .is_some_and(|current| current.generation == video.generation)
                        {
                            if let Some(seek) = prompt.seek {
                                self.watch.player().seek(video.generation, seek);
                            }
                            self.prompt.set(prompt);
                        }
                    }
                    if action == "tick" {
                        self.sampling.set(false);
                    }
                }
            }
        }
    }

    pub fn view(
        &self,
        width: impl Fn() -> f32 + 'static,
        visible: impl Fn() -> bool + 'static,
    ) -> If {
        let sponsor = self.clone();
        let watch = self.watch.clone();
        let frame = computed(move || watch.frame(width()));
        let prompt = self.prompt;
        when(
            move || {
                visible()
                    && frame.get().visible
                    && prompt.with(|prompt| prompt.countdown.is_some() || prompt.chip)
            },
            move || {
                let skip = sponsor.clone();
                let cancel = sponsor.clone();
                let countdown = computed(move || prompt.with(|prompt| prompt.countdown.is_some()));
                view()
                    .absolute()
                    .left(move || px(frame.get().left))
                    .top(move || px(frame.get().top))
                    .w(move || px(frame.get().width))
                    .h(move || px(frame.get().height))
                    .pointer_events(PointerEvents::BoxNone)
                    .child(
                        view()
                            .absolute()
                            .right(move || px((frame.get().width * 0.12).max(24.0)))
                            .bottom(px(64.0))
                            .bg(rgba(0x000000cc))
                            .rounded(px(12.0))
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .p(px(8.0))
                            .child(
                                press()
                                    .px(px(4.0))
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .label(move || {
                                        if countdown.get() {
                                            "Skip now"
                                        } else if prompt.with(|p| p.highlight) {
                                            "Jump"
                                        } else {
                                            "Skip"
                                        }
                                    })
                                    .on_press(move || skip.send("skip", None))
                                    .child(
                                        text(move || {
                                            prompt.with(|prompt| {
                                                if let Some(seconds) = prompt.countdown {
                                                    format!("Skipping in {seconds} s")
                                                } else if prompt.highlight {
                                                    "Jump".into()
                                                } else {
                                                    "Skip".into()
                                                }
                                            })
                                        })
                                        .text_size(px(13.0))
                                        .text_color(rgb(0xffffff)),
                                    )
                                    .child(when(
                                        move || !countdown.get(),
                                        || ui::icon(SKIP, 14.0, || rgb(0xffffff).into()),
                                    )),
                            )
                            .child(when(countdown, move || {
                                let cancel = cancel.clone();
                                press()
                                    .px(px(8.0))
                                    .py(px(3.0))
                                    .rounded(px(10.0))
                                    .bg(rgba(0xffffff29))
                                    .label("Cancel automatic skip")
                                    .on_press(move || cancel.send("cancel", None))
                                    .child(
                                        text("Cancel")
                                            .text_size(px(12.0))
                                            .text_color(rgb(0xffffff)),
                                    )
                            })),
                    )
            },
        )
    }
}

async fn load_segments(
    core: Core,
    tx: mpsc::UnboundedSender<Command>,
    video: Video,
    revision: u64,
) {
    if let Ok(mut segments) = core
        .execute::<Vec<Segment>>(json!({
            "op":"sponsor_fetch","videoId":video.id
        }))
        .await
    {
        segments.retain(|segment| video.categories.contains(&segment.category));
        let _ = tx.send(Command::Segments(revision, segments));
    }
}
