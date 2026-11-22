use std::{cell::RefCell, ffi::c_void, rc::Rc};

use block2::Block;
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::{Allocated, Retained, Weak},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString, NSURL, NSURLRequest};
use objc2_ui_kit::UIView;
use serde_json::{Value, json};

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {
    fn govue_ios_clear_youtube_cache();
}

const HANDLER: &str = "GoVueIosPage";
const MAX_MESSAGE: usize = 8_100_000;

pub fn register(components: &Components) {
    components.register(
        "govue/browser-tab",
        Rc::new(|context| Box::new(Browser::new(context))),
    );
}

#[derive(Default)]
struct TabState {
    url: String,
    generation: u64,
    configuration: Option<String>,
    bootstrap: String,
    page_bridge: String,
    token: Option<String>,
    active: bool,
    started: bool,
    loading: bool,
}

struct DelegateState {
    web: Weak<UIView>,
    context: ComponentContext,
    tab: RefCell<TabState>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateState]
    struct BrowserDelegate;

    unsafe impl NSObjectProtocol for BrowserDelegate {}

    impl BrowserDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(&self, _: &AnyObject, action: &AnyObject, decision: &Block<dyn Fn(isize)>) {
            // SAFETY: These are the fields of the WKNavigationAction delivered by WebKit.
            let (main, new_window, url) = unsafe {
                let frame: Option<Retained<AnyObject>> = msg_send![action, targetFrame];
                let main = frame.as_ref().is_none_or(|frame| {
                    let value: Bool = msg_send![&**frame, isMainFrame];
                    value.as_bool()
                });
                let request: Retained<NSURLRequest> = msg_send![action, request];
                (main, frame.is_none(), request.URL().and_then(|url| url.absoluteString()))
            };
            if !main {
                decision.call((1,));
                return;
            }
            let url = url.map(|value| value.to_string()).unwrap_or_default();
            let policy = govue_core::browser_url_policy(url.clone());
            decision.call(((policy == "internal" && !new_window) as isize,));
            if policy == "external" || (new_window && policy == "internal") {
                self.emit("externalUrl", json!({"url":url}));
            }
        }

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn started(&self, web: &AnyObject, _: &AnyObject) {
            let mut tab = self.ivars().tab.borrow_mut();
            tab.loading = true;
            tab.token = None;
            tab.generation = tab.generation.saturating_add(1);
            drop(tab);
            self.update_document(web, true);
        }

        #[unsafe(method(webView:didCommitNavigation:))]
        fn committed(&self, web: &AnyObject, _: &AnyObject) {
            self.update_document(web, true);
            self.bind(web);
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn finished(&self, web: &AnyObject, _: &AnyObject) {
            self.ivars().tab.borrow_mut().loading = false;
            self.update_document(web, true);
            self.bind(web);
            self.emit("ready", json!({"document":self.document(web)}));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn provisional_error(&self, _: &AnyObject, _: &AnyObject, error: &NSError) {
            self.navigation_error(error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed(&self, _: &AnyObject, _: &AnyObject, error: &NSError) {
            self.navigation_error(error);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, _: &AnyObject) {
            self.failure("The browser stopped. Reload this page to continue.", true);
        }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(&self, _: &NSString, web: &AnyObject, _: *const AnyObject, _: *mut c_void) {
            if self.update_document(web, false) && !self.ivars().tab.borrow().loading {
                self.bind(web);
                let generation = self.ivars().tab.borrow().generation;
                self.evaluate(web, &format!(
                    "window.__govueCaptions?.cancelAll();window.__govueBrowser?.setDocumentGeneration({generation});"
                ));
                self.emit("ready", json!({"document":self.document(web)}));
            }
        }

        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn message(&self, _: &AnyObject, message: &AnyObject) {
            let Some(view) = self.ivars().web.load() else { return; };
            let tab = self.ivars().tab.borrow();
            let Some(token) = &tab.token else { return; };
            let Some(expected) = page_origin(&tab.url) else { return; };
            if web_url(&view).as_deref() != Some(&tab.url) {
                return;
            }
            // SAFETY: All values come from WKScriptMessage and its native frame metadata.
            let (main, origin, body) = unsafe {
                let frame: Retained<AnyObject> = msg_send![message, frameInfo];
                let main: Bool = msg_send![&*frame, isMainFrame];
                let source: Retained<AnyObject> = msg_send![&*frame, securityOrigin];
                let protocol: Retained<NSString> = msg_send![&*source, protocol];
                let host: Retained<NSString> = msg_send![&*source, host];
                let port: isize = msg_send![&*source, port];
                let origin = if (port == 0 || port == 443) && protocol.to_string() == "https" {
                    format!("https://{host}")
                } else {
                    String::new()
                };
                let body: Retained<AnyObject> = msg_send![message, body];
                (main.as_bool(), origin, body.downcast::<NSString>().ok())
            };
            let Some(body) = body.filter(|body| body.len() <= MAX_MESSAGE) else { return; };
            if !main || expected != origin {
                return;
            }
            let body = body.to_string();
            let Some((stamp, raw)) = body.split_once(':') else { return; };
            if stamp != token || raw.len() > MAX_MESSAGE {
                return;
            }
            drop(tab);
            self.emit("pageMessage", json!({"origin":origin,"raw":raw,"document":self.document(&view)}));
        }
    }
);

impl BrowserDelegate {
    fn emit(&self, kind: &str, mut fields: Value) {
        fields["kind"] = json!(kind);
        self.ivars().context.emit("browser", fields);
    }

    fn document(&self, web: &AnyObject) -> Value {
        let tab = self.ivars().tab.borrow();
        // SAFETY: Every caller supplies the retained WKWebView.
        let can_go_back: Bool = unsafe { msg_send![web, canGoBack] };
        json!({"url":tab.url,"generation":tab.generation,"canGoBack":can_go_back.as_bool()})
    }

    fn update_document(&self, web: &AnyObject, force: bool) -> bool {
        let Some(url) = web_url(web) else {
            return false;
        };
        if govue_core::browser_url_policy(url.clone()) != "internal" {
            return false;
        }
        let mut tab = self.ivars().tab.borrow_mut();
        let changed = tab.url != url;
        if changed {
            tab.url = url;
            tab.generation = tab.generation.saturating_add(1);
        }
        let loading = tab.loading;
        drop(tab);
        if changed || force {
            self.emit(
                "navigation",
                json!({"document":self.document(web),"loading":loading}),
            );
        }
        changed
    }

    fn navigation_error(&self, error: &NSError) {
        // WebKit cancels the old load when a new navigation replaces it.
        if error.code() != -999 {
            self.failure("The page could not be loaded.", false);
        }
    }

    fn failure(&self, message: &str, terminated: bool) {
        let mut tab = self.ivars().tab.borrow_mut();
        tab.token = None;
        tab.loading = false;
        drop(tab);
        self.emit("error", json!({"message":message,"terminated":terminated}));
    }

    fn bind(&self, web: &AnyObject) {
        let mut tab = self.ivars().tab.borrow_mut();
        if tab.token.is_some() || page_origin(&tab.url).is_none() {
            return;
        }
        // SAFETY: NSUUID creates a new random document nonce.
        let token = unsafe {
            let uuid: Retained<AnyObject> = msg_send![class(c"NSUUID"), UUID];
            let value: Retained<NSString> = msg_send![&*uuid, UUIDString];
            value.to_string()
        };
        let script = format!(
            "(() => {{ if (location.href !== {}) return;
             const token = {}; const endpoint = window.webkit?.messageHandlers?.{HANDLER};
             if (!endpoint) return;
             window.__govuePageTransport = {{
                 postMessage: raw => endpoint.postMessage(token + ':' + raw),
                 matches: stamp => stamp === token
             }};
             \n{}\n{}\nwindow.__govueBrowser?.setDocumentGeneration({});\n{}
             }})();",
            json!(tab.url),
            json!(token),
            tab.page_bridge,
            tab.configuration.as_deref().unwrap_or_default(),
            tab.generation,
            visibility_script(tab.active),
        );
        tab.token = Some(token);
        drop(tab);
        evaluate(web, &script);
    }

    fn evaluate(&self, web: &AnyObject, script: &str) {
        let tab = self.ivars().tab.borrow();
        if let Some(token) = &tab.token {
            let javascript = format!(
                "if (location.href === {} && window.__govuePageTransport?.matches({})) {{\n{script}\n}}",
                json!(tab.url),
                json!(token)
            );
            evaluate(web, &javascript);
        }
    }
}

struct Browser {
    view: Retained<UIView>,
    content: Retained<AnyObject>,
    delegate: Retained<BrowserDelegate>,
}

impl Browser {
    fn new(context: ComponentContext) -> Self {
        let main = MainThreadMarker::new().expect("WKWebView requires the main thread");
        // SAFETY: Constructs the configured WKWebView and retains its WebKit delegates.
        unsafe {
            let configuration: Retained<AnyObject> =
                msg_send![class(c"WKWebViewConfiguration"), new];
            let _: () = msg_send![&*configuration, setAllowsInlineMediaPlayback: Bool::YES];
            let _: () =
                msg_send![&*configuration, setMediaTypesRequiringUserActionForPlayback: 0_usize];
            let content: Retained<AnyObject> = msg_send![&*configuration, userContentController];
            let allocated: Allocated<UIView> = msg_send![class(c"WKWebView"), alloc];
            let view: Retained<UIView> =
                msg_send![allocated, initWithFrame: CGRect::ZERO, configuration: &*configuration];
            let delegate = BrowserDelegate::alloc(main).set_ivars(DelegateState {
                web: Weak::new(&view),
                context,
                tab: RefCell::new(TabState::default()),
            });
            let delegate: Retained<BrowserDelegate> = msg_send![super(delegate), init];
            let _: () = msg_send![&*content, addScriptMessageHandler: &*delegate, name: &*NSString::from_str(HANDLER)];
            let _: () = msg_send![&*view, setNavigationDelegate: &*delegate];
            let _: () = msg_send![&*view, addObserver: &*delegate, forKeyPath: &*NSString::from_str("URL"),
                options: 0_usize, context: std::ptr::null_mut::<c_void>()];
            Self {
                view,
                content,
                delegate,
            }
        }
    }

    fn start_if_ready(&self) {
        let mut tab = self.delegate.ivars().tab.borrow_mut();
        let Some(configuration) = &tab.configuration else {
            return;
        };
        if tab.started || tab.url.is_empty() {
            return;
        }
        let url = tab.url.clone();
        let source = format!("{}\n{configuration}", tab.bootstrap);
        tab.started = true;
        drop(tab);
        self.install_bootstrap(&source);
        load(&self.view, &url);
    }

    fn install_bootstrap(&self, source: &str) {
        // SAFETY: This controller is private to our view. Only its bootstrap script is replaced.
        unsafe {
            let _: () = msg_send![&*self.content, removeAllUserScripts];
            let script: Allocated<AnyObject> = msg_send![class(c"WKUserScript"), alloc];
            let user_script: Retained<AnyObject> = msg_send![script, initWithSource: &*NSString::from_str(source),
                injectionTime: 0_isize, forMainFrameOnly: Bool::YES];
            let _: () = msg_send![&*self.content, addUserScript: &*user_script];
        }
    }
}

impl NativeComponent for Browser {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        let Some(value) = value else {
            return;
        };
        let mut tab = self.delegate.ivars().tab.borrow_mut();
        match name {
            "source" if !tab.started => {
                let Some(url) = value["url"].as_str() else {
                    return;
                };
                tab.url = url.to_owned();
                tab.generation = value["generation"].as_u64().unwrap_or(0);
                tab.bootstrap = value["bootstrap"].as_str().unwrap_or_default().to_owned();
                tab.page_bridge = value["pageBridge"].as_str().unwrap_or_default().to_owned();
                drop(tab);
                self.start_if_ready();
            }
            "options" => {
                let Some(configuration) = value["script"].as_str() else {
                    return;
                };
                let active = value["active"] == true;
                let changed = tab.configuration.as_deref() != Some(configuration);
                let visibility_changed = tab.active != active;
                if changed {
                    tab.configuration = Some(configuration.to_owned());
                }
                tab.active = active;
                let started = tab.started;
                let bootstrap = changed.then(|| format!("{}\n{configuration}", tab.bootstrap));
                drop(tab);
                self.view.setAlpha(if active { 1.0 } else { 0.0 });
                self.view.setUserInteractionEnabled(active);
                if !started {
                    self.start_if_ready();
                    return;
                }
                if let Some(bootstrap) = bootstrap {
                    self.install_bootstrap(&bootstrap);
                    self.delegate.evaluate(&self.view, configuration);
                }
                if visibility_changed {
                    self.delegate
                        .evaluate(&self.view, &visibility_script(active));
                }
            }
            _ => {}
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        match name {
            "navigate" => {
                if let Some(url) = args.as_str() {
                    load(&self.view, url);
                }
            }
            "inject" => {
                let tab = self.delegate.ivars().tab.borrow();
                if args["generation"] == tab.generation
                    && args["url"] == tab.url
                    && let Some(script) = args["script"].as_str()
                {
                    drop(tab);
                    self.delegate.evaluate(&self.view, script);
                }
            }
            // SAFETY: These navigation operations are sent to the retained WKWebView.
            "reload" => unsafe {
                let _: Option<Retained<AnyObject>> = msg_send![&*self.view, reload];
            },
            "back" => unsafe {
                let _: Option<Retained<AnyObject>> = msg_send![&*self.view, goBack];
            },
            // SAFETY: The main-thread adapter only removes cached web resources.
            "clearCache" => unsafe { govue_ios_clear_youtube_cache() },
            _ => {}
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // SAFETY: Remove the exact observer and handler installed by Browser::new before their views drop.
        unsafe {
            let _: () = msg_send![&*self.view, removeObserver: &*self.delegate, forKeyPath: &*NSString::from_str("URL")];
            let _: () =
                msg_send![&*self.view, setNavigationDelegate: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*self.content, removeScriptMessageHandlerForName: &*NSString::from_str(HANDLER)];
            let _: () = msg_send![&*self.view, stopLoading];
        }
    }
}

fn class(name: &std::ffi::CStr) -> &'static AnyClass {
    AnyClass::get(name).expect("Apple framework class")
}

fn web_url(web: &AnyObject) -> Option<String> {
    // SAFETY: The caller supplies a WKWebView.
    let url: Option<Retained<NSURL>> = unsafe { msg_send![web, URL] };
    url?.absoluteString().map(|value| value.to_string())
}

fn load(web: &AnyObject, address: &str) {
    if govue_core::browser_url_policy(address.to_owned()) != "internal" {
        return;
    }
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(address)) {
        let request = NSURLRequest::requestWithURL(&url);
        // SAFETY: The caller supplies a WKWebView and the request owns its NSURL.
        unsafe {
            let _: Option<Retained<AnyObject>> = msg_send![web, loadRequest: &*request];
        }
    }
}

fn evaluate(web: &AnyObject, script: &str) {
    let ignored: Option<&Block<dyn Fn(*mut AnyObject, *mut NSError)>> = None;
    // SAFETY: WKWebView accepts an optional result callback. The evaluated script is owned for the call.
    unsafe {
        let _: () = msg_send![web, evaluateJavaScript: &*NSString::from_str(script), completionHandler: ignored];
    }
}

fn visibility_script(active: bool) -> String {
    format!(
        "window.dispatchEvent(new CustomEvent('tabVisibilityChanged',{{detail:{{active:{active}}}}}));{}",
        if active {
            ""
        } else {
            "document.querySelectorAll('video,audio').forEach(v=>v.pause());"
        }
    )
}

fn page_origin(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str()?,
            "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
        ))
    .then(|| url.origin().ascii_serialization())
}
