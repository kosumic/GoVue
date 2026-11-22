use std::{
    cell::RefCell,
    ffi::{CStr, c_char},
    sync::Mutex,
};

use nsh_core::element::Root;
use nsh_platform_ios::{AppView, AssetBase, SurfaceOptions};
use objc2::MainThreadMarker;
use objc2_ui_kit::UIView;

static ROOT: Mutex<Option<Root>> = Mutex::new(None);
thread_local! {
    static APP: RefCell<Option<AppView>> = const { RefCell::new(None) };
}

unsafe extern "C" {
    fn govue_ios_run(
        attach: unsafe extern "C" fn(*const UIView),
        open_url: unsafe extern "C" fn(*const c_char),
        style: extern "C" fn() -> isize,
        hidden: extern "C" fn() -> bool,
        animation: extern "C" fn() -> isize,
    ) -> i32;
}

pub fn run(root: Root) {
    let mut logging = nsh_platform_apple::unified_log::Options::default();
    logging.development = std::env::var_os("NSH_DEVELOPMENT").is_some();
    nsh_platform_apple::unified_log::install(logging);
    *ROOT.lock().expect("GoVue root") = Some(root);
    // SAFETY: The application starts UIKit once on the main thread.
    std::process::exit(unsafe {
        govue_ios_run(
            attach,
            open_url,
            status_style,
            status_hidden,
            status_animation,
        )
    });
}

extern "C" fn status_style() -> isize {
    nsh_platform_ios::preferred_status_bar_style().0
}
extern "C" fn status_hidden() -> bool {
    nsh_platform_ios::prefers_status_bar_hidden()
}
extern "C" fn status_animation() -> isize {
    nsh_platform_ios::status_bar_animation().0
}

unsafe extern "C" fn attach(parent: *const UIView) {
    let Some(root) = ROOT.lock().expect("GoVue root").take() else {
        return;
    };
    let thread = MainThreadMarker::new().expect("UIKit main thread");
    // SAFETY: The scene delegate lends its live root controller's view on the main thread.
    let parent = unsafe { &*parent };
    let asset_base = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .map(|directory| AssetBase::Directory(directory.join("nsh/assets")))
    });
    let mut options = SurfaceOptions::default();
    options.limits = nsh_core::js::Limits::standalone();
    options.asset_base = asset_base;
    options.handoff = nsh_platform_apple::handoff::declared();
    let shell = AppView::new(thread, parent.bounds(), options);
    parent.addSubview(shell.view());
    shell.run(root);
    APP.with(|app| app.replace(Some(shell)));
}

unsafe extern "C" fn open_url(url: *const c_char) {
    // SAFETY: The scene delegate calls synchronously with a borrowed C string.
    let Ok(url) = unsafe { CStr::from_ptr(url) }.to_str() else {
        return;
    };
    if url.len() <= 100_000 {
        APP.with(|app| {
            if let Some(shell) = app.borrow().as_ref() {
                shell.open_url(url);
            }
        });
    }
}
