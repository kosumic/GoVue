use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    rc::Rc,
};

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;

type Event = unsafe extern "C" fn(*const c_void, *const c_char, *const c_char);

unsafe extern "C" {
    fn govue_ios_pip_create(
        view: *const UIView,
        context: *const c_void,
        event: Event,
    ) -> *mut c_void;
    fn govue_ios_pip_update(handle: *mut c_void, data: *const u8, length: isize);
    fn govue_ios_pip_ready(handle: *mut c_void);
    fn govue_ios_pip_start(handle: *mut c_void, generation: u64);
    fn govue_ios_pip_close(handle: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/player-pip",
        Rc::new(|context| Box::new(PipHost::new(context))),
    );
}

struct PipHost {
    view: Retained<UIView>,
    host: NonNull<c_void>,
    _context: Box<ComponentContext>,
}

impl PipHost {
    fn new(context: ComponentContext) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: The NSH main-thread component keeps the view and boxed callback context alive.
        let host = unsafe {
            govue_ios_pip_create(Retained::as_ptr(&view), (&raw const *context).cast(), event)
        };
        Self {
            view,
            host: NonNull::new(host).expect("iOS PiP control"),
            _context: context,
        }
    }
}

impl NativeComponent for PipHost {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        // SAFETY: Native Shell applies props on the main thread. Swift copies the JSON synchronously.
        unsafe {
            match name {
                "ready" if value == Some(&Value::Bool(true)) => {
                    govue_ios_pip_ready(self.host.as_ptr())
                }
                "config" => {
                    let payload = value.unwrap_or(&Value::Null).to_string();
                    govue_ios_pip_update(
                        self.host.as_ptr(),
                        payload.as_ptr(),
                        payload.len() as isize,
                    );
                }
                _ => {}
            }
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        if name == "enter"
            && let Some(generation) = args.as_u64()
        {
            // SAFETY: The live AVKit control receives commands on the main thread.
            unsafe { govue_ios_pip_start(self.host.as_ptr(), generation) };
        }
    }
}

impl Drop for PipHost {
    fn drop(&mut self) {
        // SAFETY: Swift cancels its delegates and observations before the callback context drops.
        unsafe { govue_ios_pip_close(self.host.as_ptr()) };
    }
}

unsafe extern "C" fn event(context: *const c_void, name: *const c_char, value: *const c_char) {
    // SAFETY: Swift invokes this with the boxed main-thread context and valid borrowed strings.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let payload = unsafe { CStr::from_ptr(value) }.to_bytes();
    if let Ok(payload) = serde_json::from_slice(payload) {
        context.emit(&name, payload);
    }
}
