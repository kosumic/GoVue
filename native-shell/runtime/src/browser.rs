use nsh_core::{element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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
pub struct BrowserTab {
    element: ElementRef,
    initial_url: String,
    generation: u64,
}

impl BrowserTab {
    pub fn new(initial_url: String, generation: u64) -> Self {
        Self {
            element: element_ref(),
            initial_url,
            generation,
        }
    }

    pub fn view(
        &self,
        options: impl Fn() -> BrowserOptions + 'static,
        on_event: impl Fn(BrowserEvent) + 'static,
    ) -> NativeView {
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
                    on_event(event);
                }
            })
    }

    pub fn navigate(&self, url: &str) {
        self.element.command("navigate", json!(url));
    }

    pub fn back(&self) {
        self.element.command("back", Value::Null);
    }

    pub fn reload(&self) {
        self.element.command("reload", Value::Null);
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
