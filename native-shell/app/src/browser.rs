use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use govue_shell_runtime::{
    BrowserDocument, BrowserEvent, BrowserOptions, BrowserTab, CaptionPage, Core, CoreError,
    WatchPlayback,
};
#[cfg(target_os = "android")]
use nsh_core::element::NativeEvent;
use nsh_core::{PointerEvents, platform::Appearance, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::browser_card::{self, Card};
use crate::chat::Chat;
use crate::downloads::Downloads;
use crate::gallery_view::{self, GalleryTarget};
use crate::queue::Queue;
use crate::settings_view::SettingsScreen;
use crate::ui::{Palette, press};

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct State {
    tabs: Vec<Tab>,
    active_id: u64,
    can_go_back: bool,
    suspended_watch_id: Option<u64>,
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
    #[cfg(target_os = "android")]
    launch_ready: Signal<bool>,
    actions: mpsc::UnboundedSender<Value>,
    captions: CaptionPage,
    watch: WatchPlayback,
    chat: Chat,
    queue: Queue,
    downloads: Downloads,
    settings_screen: Signal<Option<SettingsScreen>>,
    notice: Signal<Option<(String, u64)>>,
    notice_revision: Signal<u64>,
    card: Signal<Option<Card>>,
    gallery: Signal<Option<GalleryTarget>>,
    gallery_revision: Signal<u64>,
}

impl Browser {
    pub fn new(
        core: Core,
        watch: WatchPlayback,
        settings: Signal<Option<Map<String, Value>>>,
        chat: Chat,
        queue: Queue,
        settings_screen: Signal<Option<SettingsScreen>>,
        downloads: Downloads,
    ) -> Self {
        let (actions, pending) = mpsc::unbounded_channel();
        let browser = Self {
            captions: CaptionPage::new(core.clone()),
            core,
            state: signal(State::default()),
            retained: Rc::new(RefCell::new(HashMap::new())),
            settings,
            #[cfg(target_os = "android")]
            launch_ready: signal(false),
            actions,
            watch,
            chat,
            queue,
            downloads,
            settings_screen,
            notice: signal(None),
            notice_revision: signal(0),
            card: signal(None),
            gallery: signal(None),
            gallery_revision: signal(0),
        };
        spawn(browser.clone().run(pending));
        #[cfg(target_os = "ios")]
        {
            let browser = browser.clone();
            spawn(async move {
                loop {
                    let text = nsh_core::element::next_url_scheme_open().await;
                    if text.len() <= 100_000 {
                        browser.post("shared_url", json!({"text":text}));
                    }
                }
            });
        }
        browser.post("state", json!({}));
        effect({
            let browser = browser.clone();
            move || {
                if browser
                    .state
                    .with(|state| state.suspended_watch_id.is_some())
                    && !browser.mini_eligible()
                {
                    browser.dismiss_mini();
                }
            }
        });
        browser
    }

    #[cfg(target_os = "android")]
    pub fn launch_ready(&self) -> bool {
        self.launch_ready.get()
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

    fn mini_eligible(&self) -> bool {
        !self.watch.remote()
            && self.watch.video().is_some_and(|video| !video.offline)
            && self.settings.with(|settings| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.get("miniPlayer"))
                    == Some(&Value::Bool(true))
            })
    }

    pub fn mini_active(&self) -> bool {
        self.state.with(|state| state.suspended_watch_id.is_some()) && self.mini_eligible()
    }

    pub fn restore_mini(&self) {
        self.chat.close();
        self.post("restore_watch", json!({}));
    }

    pub fn dismiss_mini(&self) {
        if self
            .state
            .with_untracked(|state| state.suspended_watch_id.is_some())
        {
            if !self.watch.offline() {
                self.watch.close();
            }
            self.post("drop_watch", json!({}));
        }
    }

    pub fn view(&self, fullscreen: Memo<bool>) -> View {
        let state = self.state;
        let browser = self.clone();
        view().absolute().inset(px(0.0)).child(each(
            move || state.with(|state| state.tabs.clone()),
            |tab| tab.id,
            move |tab, _| browser.tab(tab.get_untracked(), fullscreen),
        ))
    }

    #[cfg(target_os = "android")]
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

    fn tab(&self, tab: Tab, fullscreen: Memo<bool>) -> View {
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
                    if state.with(|state| state.suspended_watch_id.is_none()) {
                        browser.watch.select(Some(&native));
                    }
                }
            }
        });
        let settings = self.settings;
        let chat = self.chat.clone();
        let settings_screen = self.settings_screen;
        let download_screen = self.downloads.destination;
        let visible = computed(move || {
            active.get()
                && !fullscreen.get()
                && !chat.visible()
                && settings_screen.with(Option::is_none)
                && download_screen.with(Option::is_none)
        });
        let bottom_inset = computed(move || {
            if fullscreen.get() {
                0.0
            } else {
                safe_area().bottom.0
            }
        });
        let loading = signal(true);
        let failure = signal::<Option<String>>(None);
        let summary = computed({
            let native = native.clone();
            let browser = self.clone();
            move || {
                if !active.get() || loading.get() {
                    return None;
                }
                let document = native.current_document()?;
                let video = browser.video_id(&document)?;
                let completed = browser.downloads.completed(&video);
                Some((document, video, completed))
            }
        });
        effect({
            let native = native.clone();
            move || {
                if let Some((document, video, completed)) = summary.get() {
                    download_status(&native, &document, &video, completed);
                }
            }
        });
        let renderer = native
            .view(
                &self.captions,
                move || BrowserOptions {
                    active: visible.get(),
                    bottom_inset: f64::from(bottom_inset.get()),
                    settings: settings.get().unwrap_or_default(),
                },
                {
                    let browser = self.clone();
                    let native = native.clone();
                    move |event| {
                        match &event {
                            BrowserEvent::Navigation {
                                loading: pending, ..
                            } => {
                                loading.set(*pending);
                                failure.set(None);
                            }
                            BrowserEvent::Ready { .. } => loading.set(false),
                            BrowserEvent::Error { message, .. } => {
                                loading.set(false);
                                failure.set(Some(message.clone()));
                            }
                            _ => {}
                        }
                        browser.event(id, &native, event);
                    }
                },
            )
            .absolute()
            .inset(px(0.0))
            // Let YouTube paint behind the gesture handle and apply the inset to its controls.
            .bottom(move || px(-bottom_inset.get()));
        let palette = Palette {
            dark: computed(dark),
        };
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(PointerEvents::BoxNone)
            .child(renderer)
            .child(when(
                move || visible.get() && loading.get(),
                || {
                    view()
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .right(px(0.0))
                        .h(px(2.0))
                        .bg(rgb(0xff0000))
                        .pointer_events(PointerEvents::None)
                },
            ))
            .child(when(
                move || visible.get() && failure.with(Option::is_some),
                move || {
                    let native = native.clone();
                    view()
                        .absolute()
                        .bottom(px(0.0))
                        .left(px(0.0))
                        .right(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .p(px(16.0))
                        .bg(move || palette.raised())
                        .child(
                            text(move || failure.get().unwrap_or_default())
                                .flex_1()
                                .text_size(px(13.0))
                                .text_color(move || palette.ink()),
                        )
                        .child(
                            press()
                                .p(px(12.0))
                                .label("Reload page")
                                .on_press(move || {
                                    failure.set(None);
                                    loading.set(true);
                                    native.reload();
                                })
                                .child(
                                    text("Reload")
                                        .text_size(px(13.0))
                                        .text_color(move || palette.ink()),
                                ),
                        )
                },
            ))
    }

    fn event(&self, id: u64, tab: &BrowserTab, event: BrowserEvent) {
        self.watch.receive(tab, &event);
        match event {
            BrowserEvent::Navigation { document, .. } => self.post(
                "document",
                json!({"id":id,"url":document.url,"generation":document.generation}),
            ),
            BrowserEvent::ExternalUrl { url } if self.active(id) => self.open(url),
            #[cfg(target_os = "android")]
            BrowserEvent::Painted { document }
                if self.active(id)
                    && tab.current_document().is_some_and(|current| {
                        current.url == document.url && current.generation == document.generation
                    }) =>
            {
                self.launch_ready.set(true);
            }
            BrowserEvent::Error { .. } => {
                #[cfg(target_os = "android")]
                if self.active(id) {
                    self.launch_ready.set(true);
                }
                self.post("invalidate", json!({"id":id}));
            }
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
            Some("share") => {
                if let Some(url) = payload["url"].as_str()
                    && govue_core::browser_url_policy(url.to_owned()) == "internal"
                { browser_card::share_video(url.to_owned()); }
            }
            Some("card") => {
                if let Some(card) = Card::from_page(&self.core, payload) { self.card.set(Some(card)); }
            }
            Some("gallery") => self.open_gallery(id, document, payload),
            Some("chat") => {
                self.chat.show(self.video_id(&document));
            }
            Some("settings") => self.settings_screen.set(Some(SettingsScreen::Extensions)),
            Some("about") => self.settings_screen.set(Some(SettingsScreen::About)),
            Some("library") => self.downloads.open(),
            Some("download") if payload["type"] == "openManager" => self.downloads.open(),
            Some("download") if payload["type"] == "openConfirm" => {
                self.downloads.confirm("", json!([payload]));
            }
            Some("download") if payload["type"] == "openBatch" => {
                self.downloads.confirm(payload["name"].as_str().unwrap_or("Playlist"), payload["items"].clone());
            }
            Some("download") if payload["type"] == "requestStatus" => {
                if let Some(video) = payload["videoId"].as_str()
                    && self.video_id(&document).as_deref() == Some(video)
                    && let Some(tab) = self.retained.borrow().get(&id)
                {
                    download_status(tab, &document, video, self.downloads.completed(video));
                }
            }
            Some("context") => {
                if self.core.state::<Value>(json!({"op":"browser_identity","url":document.url}))
                    .is_ok_and(|identity| identity["videoId"].is_string() && identity["videoId"] == payload["videoId"])
                {
                    self.queue.update("context", json!({"videoId":payload["videoId"],
                        "next":payload["next"],"previous":payload["previous"],"autoplay":payload["autoplay"]}));
                }
            }
            Some("queueAdd") => {
                let browser = self.clone();
                self.queue.add(payload.clone(), move || browser.notify("Added to queue".into()));
            }
            Some("queueError") => self.notify("Could not add this video to the queue.".into()),
            Some("shortsReady" | "shortsBlocked") if payload["url"] == document.url => {
                if url::Url::parse(&document.url).is_ok_and(|url| url.path().starts_with("/shorts/"))
                    && self.video_id(&document).is_some()
                {
                    if message["action"] == "shortsReady" && !self.watch.offline() { self.watch.close(); }
                    if message["action"] == "shortsBlocked" { self.notify("Tap the Short to start playback.".into()); }
                }
            }
            Some("appearance") if payload["url"] == document.url => {
                if let Some(dark) = payload["dark"].as_bool() {
                    set_color_scheme(if dark { Appearance::Dark } else { Appearance::Light });
                }
            }
            Some("transient") => self.post("transient", json!({"id":id,"origin":origin,
                "documentUrl":document.url,"generation":document.generation,"kind":payload["kind"]})),
            _ => {}
        }
    }

    pub fn card_view(&self) -> impl IntoElement {
        let card = self.card;
        let browser = self.clone();
        dynamic(move || {
            card.get()
                .map(|card| browser_card::sheet(browser.clone(), card))
        })
    }

    fn open_gallery(&self, id: u64, document: BrowserDocument, payload: &Value) {
        let revision = self.gallery_revision.get_untracked() + 1;
        self.gallery_revision.set(revision);
        let command =
            json!({"op":"gallery_validate","urls":payload["urls"],"index":payload["index"]});
        let browser = self.clone();
        spawn(async move {
            let result = browser.core.execute::<GalleryTarget>(command).await;
            if browser.gallery_revision.get_untracked() != revision || !browser.active(id) {
                return;
            }
            let still_current = browser
                .retained
                .borrow()
                .get(&id)
                .and_then(BrowserTab::current_document)
                .is_some_and(|current| {
                    current.generation == document.generation && current.url == document.url
                });
            if still_current {
                match result {
                    Ok(gallery) => browser.gallery.set(Some(gallery)),
                    Err(error) => crate::downloads::alert(error.to_string()).await,
                }
            }
        });
    }

    pub fn gallery_view(&self) -> impl IntoElement {
        let gallery = self.gallery;
        let browser = self.clone();
        dynamic(move || {
            let target = gallery.get()?;
            let close = browser.clone();
            Some(gallery_view::gallery(
                browser.core.clone(),
                browser.downloads.clone(),
                target,
                move || {
                    close.close_gallery();
                },
            ))
        })
    }

    pub fn close_gallery(&self) -> bool {
        let open = self.gallery.with_untracked(Option::is_some);
        if open {
            self.gallery_revision.update(|revision| *revision += 1);
            self.gallery.set(None);
        }
        open
    }

    pub fn close_card(&self) -> bool {
        let open = self.card.with_untracked(Option::is_some);
        if open {
            self.card.set(None);
        }
        open
    }

    pub fn queue_card(&self, card: Card) {
        let browser = self.clone();
        let selected = card.clone();
        self.queue.add(
            json!({"videoId":card.video_id,"title":card.title,"author":card.author}),
            move || {
                if browser
                    .card
                    .with_untracked(|card| card.as_ref() == Some(&selected))
                {
                    browser.close_card();
                }
                browser.notify("Added to queue".into());
            },
        );
    }

    pub fn download_card(&self, video: String) {
        self.close_card();
        self.downloads.confirm("", json!([{"videoId":video}]));
    }

    fn video_id(&self, document: &BrowserDocument) -> Option<String> {
        self.core
            .state::<Value>(json!({"op":"browser_identity","url":document.url}))
            .ok()
            .and_then(|identity| identity["videoId"].as_str().map(str::to_owned))
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

    pub fn clear_cache(&self) {
        let id = self.state.with_untracked(|state| state.active_id);
        if let Some(browser) = self.retained.borrow().get(&id) {
            browser.clear_cache();
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
        if state.can_go_back && self.mini_eligible() && self.active_video().is_some() {
            self.post("back", json!({}));
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
        while let Some(mut request) = pending.recv().await {
            if request["op"] == "browser_shared_url" {
                match self.core.execute::<String>(request).await {
                    Ok(url) => {
                        request = json!({"op":"browser_open","url":url});
                    }
                    Err(error) => {
                        self.notify(error.to_string());
                        continue;
                    }
                }
            }
            if matches!(
                request["op"].as_str(),
                Some("browser_open" | "browser_back")
            ) {
                request["suspendWatch"] = json!(self.mini_eligible());
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

    fn notify(&self, message: String) {
        let notice = self.notice;
        let revision = self.notice_revision.get_untracked() + 1;
        self.notice_revision.set(revision);
        notice.set(Some((message, revision)));
        spawn(async move {
            timer(Duration::from_secs(3)).await;
            if notice.with_untracked(|notice| {
                notice
                    .as_ref()
                    .is_some_and(|(_, current)| *current == revision)
            }) {
                notice.set(None);
            }
        });
    }
}

fn download_status(tab: &BrowserTab, document: &BrowserDocument, video: &str, completed: bool) {
    let status = json!({"videoId":video,"watchPageDownloaded":completed});
    tab.inject(
        document,
        &format!("window.dispatchEvent(new CustomEvent('downloadStatus',{{detail:{status}}}));"),
    );
}

pub fn report(error: CoreError) {
    // The browser stays mounted if an action is rejected or storage fails.
    eprintln!("{error}");
}
