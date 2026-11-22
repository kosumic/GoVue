use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use govue_shell_runtime::{
    BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab, CaptionPage, Core, CoreError,
    WatchPlayback,
};
use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::chat::Chat;
use crate::queue::Queue;
use crate::settings_view::SettingsScreen;

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
    queue: Queue,
    settings_screen: Signal<Option<SettingsScreen>>,
    notice: Signal<Option<(String, u64)>>,
}

impl Browser {
    pub fn new(
        core: Core,
        captions: CaptionPage,
        watch: WatchPlayback,
        settings: Signal<Option<Map<String, Value>>>,
        chat: Chat,
        queue: Queue,
        settings_screen: Signal<Option<SettingsScreen>>,
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
            queue,
            settings_screen,
            notice: signal(None),
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

    pub fn launches(&self) -> NativeView {
        let browser = self.clone();
        nsh_core::element::native("govue/browser-launches")
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .prop("ready", json!(true))
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "launch"
                    && let Some(text) = event.data.as_str().filter(|text| text.len() <= 100_000)
                {
                    browser.post("shared_url", json!({"text":text}));
                }
            })
    }

    pub fn toast(&self) -> If {
        let notice = self.notice;
        when(
            move || notice.with(Option::is_some),
            move || {
                view()
                    .absolute()
                    .left(px(14.0))
                    .right(px(14.0))
                    .bottom(px(20.0))
                    .rounded(px(10.0))
                    .bg(rgba(0x272727f5))
                    .px(px(12.0))
                    .py(px(8.0))
                    .pointer_events(PointerEvents::None)
                    .child(
                        text(move || {
                            notice
                                .get()
                                .map_or_else(String::new, |(message, _)| message)
                        })
                        .text_size(px(13.0))
                        .text_color(rgb(0xffffff)),
                    )
            },
        )
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
        let settings_screen = self.settings_screen;
        native
            .view(
                &self.captions,
                move || BrowserOptions {
                    active: active.get()
                        && !chat.visible()
                        && settings_screen.with(Option::is_none),
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
            Some("settings") => self.settings_screen.set(Some(SettingsScreen::Extensions)),
            Some("context") => {
                if self.core.state::<Value>(json!({"op":"browser_identity","url":document.url}))
                    .is_ok_and(|identity| identity["videoId"].is_string() && identity["videoId"] == payload["videoId"])
                {
                    self.queue.update("context", json!({"videoId":payload["videoId"],
                        "next":payload["next"],"previous":payload["previous"],"autoplay":payload["autoplay"]}));
                }
            }
            Some("queueAdd") => self.queue.update("add", json!({"item":payload})),
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
        let mut notice_version = 0;
        while let Some(mut request) = pending.recv().await {
            if request["op"] == "browser_shared_url" {
                match self.core.execute::<String>(request).await {
                    Ok(url) => {
                        request = json!({"op":"browser_open","url":url});
                    }
                    Err(error) => {
                        notice_version += 1;
                        let notice = self.notice;
                        let version = notice_version;
                        notice.set(Some((error.to_string(), version)));
                        spawn(async move {
                            timer(Duration::from_secs(3)).await;
                            if notice.with_untracked(|notice| {
                                notice
                                    .as_ref()
                                    .is_some_and(|(_, current)| *current == version)
                            }) {
                                notice.set(None);
                            }
                        });
                        continue;
                    }
                }
            }
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
