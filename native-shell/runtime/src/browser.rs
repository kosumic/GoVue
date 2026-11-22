use nsh_core::{element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::CaptionPage;

const COMPONENT: &str = "govue/browser-tab";
const BOOTSTRAP: &str = include_str!("../../../src/youtube/generated/nativeShellBootstrap.js");
const PAGE_BRIDGE: &str = include_str!("../../../src/youtube/generated/nativeShellBrowser.js");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDocument {
    pub url: String,
    pub generation: u64,
    pub can_go_back: bool,
}

impl BrowserDocument {
    pub(crate) fn same_document(&self, other: &Self) -> bool {
        self.generation == other.generation && self.url == other.url
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct DocumentState {
    pub document: BrowserDocument,
    pub ready: bool,
}

/// Origin and document identity are supplied by the native WebView. Page data
/// still needs the owning tab, video and pending request checks in Rust.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BrowserEvent {
    Navigation {
        document: BrowserDocument,
        loading: bool,
    },
    Ready {
        document: BrowserDocument,
    },
    Progress {
        fraction: f64,
    },
    PageMessage {
        document: BrowserDocument,
        origin: String,
        raw: String,
    },
    ExternalUrl {
        url: String,
    },
    Error {
        message: String,
        terminated: bool,
    },
}

pub struct BrowserOptions {
    pub active: bool,
    pub bottom_inset: f64,
    pub settings: Map<String, Value>,
}

/// Keep the same view mounted while the tab is covered by a native screen.
#[derive(Clone)]
pub struct BrowserTab {
    element: ElementRef,
    initial_url: String,
    generation: u64,
    pub(crate) document: Signal<Option<DocumentState>>,
}

impl BrowserTab {
    pub fn new(initial_url: String, generation: u64) -> Self {
        Self {
            element: element_ref(),
            initial_url,
            generation,
            document: signal(None),
        }
    }

    pub fn view(
        &self,
        captions: &CaptionPage,
        options: impl Fn() -> BrowserOptions + 'static,
        on_event: impl Fn(BrowserEvent) + 'static,
    ) -> NativeView {
        let tab = self.clone();
        let captions = captions.clone();
        nsh_core::element::native(COMPONENT)
            .element_ref(&self.element)
            .prop(
                "source",
                json!({"url": self.initial_url, "generation": self.generation,
                    "bootstrap": BOOTSTRAP, "pageBridge": PAGE_BRIDGE}),
            )
            .prop("options", move || {
                let options = options();
                let settings = Value::Object(options.settings);
                let inset = options.bottom_inset.clamp(0.0, 200.0);
                let inset = if inset.is_finite() { inset } else { 0.0 };
                let script = format!(
                    "window.__govueMedia?.configure({settings});\n\
                    window.__govueBrowser?.configure({settings});\n\
                    window.__govueBrowser?.setBottomInset({inset});"
                );
                json!({"active": options.active, "script": script})
            })
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "browser"
                    && let Ok(event) = BrowserEvent::deserialize(&event.data)
                {
                    tab.record_document(&event);
                    if !captions.receive(&tab, &event) {
                        on_event(event);
                    }
                }
            })
    }

    fn record_document(&self, event: &BrowserEvent) {
        match event {
            BrowserEvent::Navigation { document, .. } => {
                let ready = self
                    .document
                    .get_untracked()
                    .is_some_and(|state| state.ready && state.document.same_document(document));
                self.document.set(Some(DocumentState {
                    document: document.clone(),
                    ready,
                }));
            }
            BrowserEvent::Ready { document } => {
                self.document.set(Some(DocumentState {
                    document: document.clone(),
                    ready: true,
                }));
            }
            BrowserEvent::Error { .. } => self.document.set(None),
            _ => {}
        }
    }

    pub(crate) fn is_same(&self, other: &Self) -> bool {
        self.element == other.element
    }

    pub fn navigate(&self, url: &str) {
        self.element.command("navigate", json!(url));
    }

    pub fn current_document(&self) -> Option<BrowserDocument> {
        self.document.get().map(|state| state.document)
    }

    pub fn back(&self) {
        self.element.command("back", Value::Null);
    }

    pub fn reload(&self) {
        self.element.command("reload", Value::Null);
    }

    pub fn clear_cache(&self) {
        self.element.command("clearCache", Value::Null);
    }

    /// Evaluate only while the target YouTube document is current.
    pub fn inject(&self, document: &BrowserDocument, script: &str) {
        self.element.command(
            "inject",
            json!({"generation": document.generation, "url": document.url, "script": script}),
        );
    }
}

#[cfg(target_os = "android")]
mod android {
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::{jint, jstring};
    use serde_json::{Value, json};

    #[unsafe(no_mangle)]
    extern "system" fn Java_io_github_kaminarios_govue_shell_BrowserTab_urlPolicy(
        mut env: JNIEnv,
        _class: JClass,
        address: JString,
    ) -> jint {
        let Ok(url) = env.get_string(&address) else {
            return 0;
        };
        match govue_core::browser_url_policy(url.into()).as_str() {
            "internal" => 1,
            "external" => 2,
            _ => 0,
        }
    }

    #[unsafe(no_mangle)]
    extern "system" fn Java_io_github_kaminarios_govue_shell_BrowserTab_browserUserAgent(
        mut env: JNIEnv,
        _class: JClass,
        agent: JString,
    ) -> jstring {
        let Ok(agent) = env.get_string(&agent) else {
            return std::ptr::null_mut();
        };
        let request = json!({"op":"browser_user_agent", "agent":String::from(agent)});
        let reply = govue_core::state_execute(request.to_string());
        serde_json::from_str::<Value>(&reply)
            .ok()
            .and_then(|reply| env.new_string(reply["value"].as_str()?).ok())
            .map_or(std::ptr::null_mut(), JString::into_raw)
    }
}
