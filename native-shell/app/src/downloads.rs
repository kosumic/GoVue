use std::rc::Rc;

use govue_shell_runtime::{Core, WatchPlayback};
use nsh_core::{
    Dialog, DialogButton, DialogButtonStyle, PointerEvents,
    element::{NativeEvent, dialog},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Notify, broadcast};

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub video_id: String,
    pub title: String,
    pub author: String,
    pub state: String,
    pub label: String,
    pub file: String,
    pub mime_type: String,
    pub artifacts: Vec<Artifact>,
    pub total: u64,
    pub received: u64,
    pub error: Option<String>,
    pub batch_id: Option<String>,
    pub watch_page_downloaded: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub file: String,
    pub label: String,
    pub mime_type: String,
}

impl Entry {
    pub fn pausable(&self) -> bool {
        matches!(
            self.state.as_str(),
            "queued" | "resolving" | "downloading" | "paused"
        )
    }
}

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Snapshot {
    pub ready: bool,
    pub entries: Vec<Entry>,
    pub waiting_for_wifi: Vec<String>,
    pub preferences: Value,
}

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub name: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub ids: Vec<String>,
    pub pause_ids: Vec<String>,
    pub resume_ids: Vec<String>,
}

#[derive(Clone, Default, Deserialize, PartialEq)]
pub struct Listing {
    pub items: Vec<Entry>,
    pub batch: Option<Batch>,
}

#[derive(Clone, PartialEq)]
pub enum Destination {
    Library,
    Settings,
    History,
    Batch(String),
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct Target {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub video_id: String,
    pub title: String,
    pub author: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Picker {
    Quality,
    Audio,
    Subtitles,
}

#[derive(Clone)]
pub struct Downloads {
    pub core: Core,
    owner: Owner,
    pub destination: Signal<Option<Destination>>,
    pub filter: Signal<&'static str>,
    pub selected: Signal<Option<String>>,
    pub state: Signal<Snapshot>,
    pub listing: Signal<Listing>,
    pub error: Signal<Option<String>>,
    pub target: Signal<Option<Target>>,
    pub picker: Signal<Option<Picker>>,
    pub notice: Signal<Option<String>>,
    export_picker: ElementRef,
    refresh: Rc<Notify>,
}

impl Downloads {
    pub fn new(core: Core) -> Self {
        let updates = core.subscribe();
        let downloads = Self {
            core,
            owner: Owner::current(),
            destination: signal(None),
            filter: signal("all"),
            selected: signal(None),
            state: signal(Snapshot::default()),
            listing: signal(Listing::default()),
            error: signal(None),
            target: signal(None),
            picker: signal(None),
            notice: signal(None),
            export_picker: element_ref(),
            refresh: Rc::new(Notify::new()),
        };
        effect({
            let downloads = downloads.clone();
            move || {
                let _ = downloads.destination.get();
                let _ = downloads.filter.get();
                downloads.refresh.notify_one();
            }
        });
        spawn(downloads.clone().observe(updates));
        downloads
    }

    async fn observe(self, mut updates: broadcast::Receiver<String>) {
        loop {
            tokio::select! {
                () = self.refresh.notified() => {},
                update = updates.recv() => match update {
                    Ok(channel) if channel == "downloads" => {},
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => break,
                    _ => continue,
                },
            }
            match self
                .core
                .execute::<Snapshot>(json!({"op":"downloads_runner_snapshot"}))
                .await
            {
                Ok(state) => {
                    self.error.set(None);
                    self.state.set(state);
                }
                Err(error) => self.error.set(Some(error.to_string())),
            }
            let route = self.destination.get_untracked();
            let filter = self.filter.get_untracked();
            let query = match &route {
                Some(Destination::Library) => json!({"op":"downloads_view","filter":filter}),
                Some(Destination::History) => json!({"op":"downloads_view","filter":"terminal"}),
                Some(Destination::Batch(id)) => json!({"op":"downloads_view","batchId":id}),
                _ => continue,
            };
            if let Ok(listing) = self.core.execute::<Listing>(query).await
                && self.destination.get_untracked() == route
                && self.filter.get_untracked() == filter
            {
                self.listing.set(listing);
            }
        }
    }

    pub fn open(&self) {
        self.selected.set(None);
        if self.destination.get_untracked().is_none() {
            self.listing.set(Listing::default());
        }
        self.destination.set(Some(Destination::Library));
    }

    pub fn close(&self) {
        self.selected.set(None);
        self.destination.set(None);
    }

    pub fn back(&self) -> bool {
        if self.picker.with_untracked(Option::is_some) {
            self.picker.set(None);
            return true;
        }
        if self.target.with_untracked(Option::is_some) {
            self.target.set(None);
            return true;
        }
        if self.selected.with_untracked(Option::is_some) {
            self.selected.set(None);
            return true;
        }
        match self.destination.get_untracked() {
            Some(Destination::Library) => self.close(),
            Some(_) => self.open(),
            None => return false,
        }
        true
    }

    pub fn active_entry(&self) -> Option<Entry> {
        let selected = self.selected.get()?;
        self.state.with(|state| {
            state
                .entries
                .iter()
                .find(|entry| entry.id == selected)
                .cloned()
        })
    }

    pub fn completed(&self, video: &str) -> bool {
        self.state.with(|state| {
            state
                .entries
                .iter()
                .any(|entry| entry.video_id == video && entry.watch_page_downloaded)
        })
    }

    pub fn confirm(&self, name: &str, items: Value) {
        let downloads = self.clone();
        let request = json!({"op":"downloads_targets","name":name,"items":items});
        self.owner.run(|| {
            spawn(async move {
                match downloads.core.execute(request).await {
                    Ok(target) => {
                        downloads.picker.set(None);
                        downloads.target.set(Some(target));
                    }
                    Err(error) => alert(error.to_string()).await,
                }
            })
        });
    }

    pub async fn enqueue(
        &self,
        items: Vec<Item>,
        name: &str,
        config: Value,
    ) -> Result<(), govue_shell_runtime::CoreError> {
        #[cfg(target_os = "android")]
        let _ = nsh_core::element::request_notification_permission().await;
        self.core
            .execute::<Value>(json!({"op":"downloads_config","settings":{"lastConfig":config}}))
            .await?;
        let result: Value = self
            .core
            .execute(json!({
                "op":"downloads_runner_enqueue","name":name,"config":config,"items":items
            }))
            .await?;
        self.refresh.notify_one();
        let count = result["added"].as_u64().unwrap_or(0);
        let skipped = result["skipped"].as_u64().unwrap_or(0);
        let message = match (count, skipped) {
            (0, _) => "Already in downloads".into(),
            (count, 0) => format!("{count} added to downloads"),
            (count, skipped) => format!("{count} added to downloads · {skipped} already present"),
        };
        self.target.set(None);
        self.notice.set(Some(message.clone()));
        let notice = self.notice;
        self.owner.run(|| {
            spawn(async move {
                timer(std::time::Duration::from_secs(2)).await;
                if notice.with_untracked(|current| current.as_ref() == Some(&message)) {
                    notice.set(None);
                }
            })
        });
        Ok(())
    }

    pub fn run(&self, command: Value) {
        let downloads = self.clone();
        self.owner.run(|| {
            spawn(async move {
                if let Err(error) = downloads.core.execute::<Value>(command).await {
                    alert(error.to_string()).await;
                }
                downloads.refresh.notify_one();
            })
        });
    }

    pub fn remove(&self, title: &str, description: String, ids: Vec<String>) {
        let downloads = self.clone();
        let title = title.to_owned();
        self.owner.run(|| {
            spawn(async move {
                let answer = dialog(Dialog {
                    id: 0,
                    title: Some(title),
                    message: Some(description),
                    buttons: vec![
                        DialogButton {
                            text: "Cancel".into(),
                            style: DialogButtonStyle::Cancel,
                        },
                        DialogButton {
                            text: "Keep files".into(),
                            style: DialogButtonStyle::Default,
                        },
                        DialogButton {
                            text: "Delete files".into(),
                            style: DialogButtonStyle::Destructive,
                        },
                    ],
                    cancelable: true,
                    sheet: false,
                })
                .await;
                if matches!(answer, 1 | 2) {
                    downloads.selected.set(None);
                    downloads.run(
                        json!({"op":"downloads_runner_remove","ids":ids,"deleteMedia":answer==2}),
                    );
                }
            })
        });
    }

    pub fn play(&self, entry: Entry, watch: WatchPlayback, started: impl FnOnce() + 'static) {
        let downloads = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let page = downloads.destination.get_untracked();
                let uri = downloads
                    .core
                    .execute::<String>(json!({"op":"downloads_runner_uri","id":entry.id}))
                    .await;
                if downloads.destination.get_untracked() != page {
                    return;
                }
                match uri.and_then(|uri| {
                    watch.play_offline(entry.video_id, entry.title, entry.author, uri)
                }) {
                    Ok(()) => {
                        downloads.close();
                        started();
                    }
                    Err(error) => alert(error.to_string()).await,
                }
            })
        });
    }

    pub fn export(&self, id: String, artifact: Artifact, title: String, save: bool) {
        let core = self.core.clone();
        let picker = self.export_picker;
        self.owner.run(|| {
            spawn(async move {
                let exported = core
                    .execute::<Value>(json!({"op":"downloads_runner_export","id":id,"file":artifact.file}))
                    .await;
                match exported {
                    Ok(value) if save => {
                        picker.command("save", json!({"uri":value["uri"],"name":value["name"],"mime":artifact.mime_type}));
                    }
                    Ok(value) => {
                        let path = value["uri"]
                            .as_str()
                            .and_then(|uri| url::Url::parse(uri).ok())
                            .and_then(|uri| uri.to_file_path().ok());
                        if let Some(path) = path {
                            let request =
                                nsh_core::share::ShareRequest::new().title(title).file(path);
                            if let Err(error) = nsh_core::element::share(request).await {
                                alert(error.message).await;
                            }
                        }
                    }
                    Err(error) => alert(error.to_string()).await,
                }
            })
        });
    }

    pub fn export_view(&self) -> NativeView {
        nsh_core::element::native("govue/download-export")
            .element_ref(&self.export_picker)
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "error"
                    && let Some(error) = event.data.as_str()
                {
                    spawn(alert(error.to_owned()));
                }
            })
    }
}

pub async fn alert(message: String) {
    let _ = dialog(Dialog {
        id: 0,
        title: Some("GoVue".into()),
        message: Some(message),
        buttons: vec![DialogButton {
            text: "OK".into(),
            style: DialogButtonStyle::Default,
        }],
        cancelable: true,
        sheet: false,
    })
    .await;
}
