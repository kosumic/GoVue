use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use govue_shell_runtime::{
    BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab, CaptionPage, Core, CoreError,
    WatchPlayback,
};
use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::chat::Chat;

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct State {
    tabs: Vec<Tab>,
    active_id: u64,
    can_go_back: bool,
    transient: String,
    target: Option<Target>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Tab {
    id: u64,
    url: String,
    generation: u64,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Target {
    id: u64,
    url: String,
    load: bool,
    #[serde(default)]
    dismiss: bool,
}

#[derive(Clone)]
pub struct Browser {
    core: Core,
    state: Signal<State>,
    retained: Rc<RefCell<HashMap<u64, BrowserTab>>>,
    settings: Signal<Option<Map<String, Value>>>,
    dark: Signal<bool>,
    actions: mpsc::UnboundedSender<Value>,
    captions: CaptionPage,
    watch: WatchPlayback,
    chat: Chat,
}

impl Browser {
    pub fn new(
        core: Core,
        captions: CaptionPage,
        watch: WatchPlayback,
        settings: Signal<Option<Map<String, Value>>>,
        chat: Chat,
    ) -> Self {
        let (actions, pending) = mpsc::unbounded_channel();
        let browser = Self {
            core,
            state: signal(State::default()),
            retained: Rc::new(RefCell::new(HashMap::new())),
            settings,
            dark: signal(false),
            actions,
            captions,
            watch,
            chat,
        };
        spawn(browser.clone().run(pending));
        browser.post("state", json!({}));
        browser
    }

    pub fn dark(&self) -> bool {
        self.dark.get()
    }

    pub fn active_video(&self) -> Option<String> {
        let url = self.state.with(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == state.active_id)
                .map(|tab| tab.url.clone())
        })?;
        self.core
            .state::<Value>(json!({"op":"browser_identity","url":url}))
            .ok()
            .and_then(|identity| identity["videoId"].as_str().map(str::to_owned))
    }

    pub fn view(&self) -> View {
        let state = self.state;
        let browser = self.clone();
        view().absolute().inset(px(0.0)).child(each(
            move || state.with(|state| state.tabs.clone()),
            |tab| tab.id,
            move |tab, _| browser.tab(tab.get_untracked()),
        ))
    }

    fn tab(&self, tab: Tab) -> NativeView {
        let id = tab.id;
        let native = BrowserTab::new(tab.url, tab.generation);
        self.retained.borrow_mut().insert(id, native.clone());
        cleanup({
            let retained = self.retained.clone();
            move || {
                retained.borrow_mut().remove(&id);
            }
        });
        let state = self.state;
        let active = computed(move || state.with(|snapshot| snapshot.active_id == id));
        effect({
            let browser = self.clone();
            let native = native.clone();
            move || {
                if active.get() {
                    browser.captions.select(Some(&native));
                    browser.watch.select(Some(&native));
                }
            }
        });
        let settings = self.settings;
        let chat = self.chat.clone();
        native
            .view(
                &self.captions,
                move || BrowserOptions {
                    active: active.get() && !chat.visible(),
                    bottom_inset: f64::from(safe_area().bottom.0),
                    settings: settings.get().unwrap_or_default(),
                },
                {
                    let browser = self.clone();
                    let native = native.clone();
                    move |event| browser.event(id, &native, event)
                },
            )
            .absolute()
            .inset(px(0.0))
    }

    fn event(&self, id: u64, tab: &BrowserTab, event: BrowserEvent) {
        self.watch.receive(tab, &event);
        match event {
            BrowserEvent::Navigation { document, .. } => self.post(
                "document",
                json!({"id":id,"url":document.url,"generation":document.generation}),
            ),
            BrowserEvent::ExternalUrl { url } if self.active(id) => self.open(url),
            BrowserEvent::Error { .. } => self.post("invalidate", json!({"id":id})),
            BrowserEvent::PageMessage {
                document,
                origin,
                raw,
            } if self.active(id) => self.page_message(id, document, origin, &raw),
            _ => {}
        }
    }

    fn page_message(&self, id: u64, document: BrowserDocument, origin: String, raw: &str) {
        if raw.len() > 65536 {
            return;
        }
        let Ok(message) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        if message["type"] != "litube"
            || message["documentUrl"] != document.url
            || message["documentGeneration"] != document.generation
        {
            return;
        }
        let payload = &message["payload"];
        match message["action"].as_str() {
            Some("openTab") => {
                if let Some(url) = payload["url"].as_str() {
                    self.open(url.to_owned());
                }
            }
            Some("back") => { self.back(); }
            Some("chat") => {
                let video = self.core.state::<Value>(json!({"op":"browser_identity","url":document.url}))
                    .ok().and_then(|identity| identity["videoId"].as_str().map(str::to_owned));
                self.chat.show(video);
            }
            Some("appearance") if payload["url"] == document.url => {
                if let Some(dark) = payload["dark"].as_bool() {
                    self.dark.set(dark);
                }
            }
            Some("transient") => self.post("transient", json!({"id":id,"origin":origin,
                "documentUrl":document.url,"generation":document.generation,"kind":payload["kind"]})),
            _ => {}
        }
    }

    pub fn open(&self, url: String) {
        match govue_core::browser_url_policy(url.clone()).as_str() {
            "internal" => self.post("open", json!({"url":url})),
            "external" => {
                spawn(async move {
                    open_url(url).await;
                });
            }
            _ => {}
        }
    }

    fn active(&self, id: u64) -> bool {
        self.state.with_untracked(|state| state.active_id == id)
    }

    pub fn back(&self) -> bool {
        let state = self.state.get_untracked();
        if !state.transient.is_empty() && state.transient != "none" {
            self.post("dismiss", json!({}));
            return true;
        }
        if let Some(tab) = self.retained.borrow().get(&state.active_id)
            && tab
                .current_document()
                .is_some_and(|document| document.can_go_back)
        {
            tab.back();
            return true;
        }
        if state.can_go_back {
            self.post("back", json!({}));
            return true;
        }
        false
    }

    fn post(&self, action: &str, mut request: Value) {
        request["op"] = json!(format!("browser_{action}"));
        let _ = self.actions.send(request);
    }

    async fn run(self, mut pending: mpsc::UnboundedReceiver<Value>) {
        while let Some(request) = pending.recv().await {
            match self.core.execute::<State>(request).await {
                Ok(mut state) => {
                    let target = state.target.take();
                    self.state.set(state);
                    if let Some(target) = target
                        && let Some(tab) = self.retained.borrow().get(&target.id)
                    {
                        if target.load {
                            tab.navigate(&target.url);
                        }
                        if target.dismiss
                            && let Some(document) = tab.current_document()
                        {
                            tab.inject(&document, "window.Lite?.ui?.dismiss();");
                        }
                    }
                }
                Err(error) => report(error),
            }
        }
    }
}

pub fn report(error: CoreError) {
    // The browser stays mounted if an action is rejected or storage fails.
    eprintln!("{error}");
}
