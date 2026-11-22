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
    fn govue_provider_qr_create(
        view: *const UIView,
        context: *const c_void,
        emit: Event,
    ) -> *mut c_void;
    fn govue_provider_qr_command(
        handle: *mut c_void,
        operation: i32,
        bytes: *const u8,
        length: isize,
    );
    fn govue_provider_qr_close(handle: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/provider-qr-scanner",
        Rc::new(|context| Box::new(ScannerHost::new(context))),
    );
}

struct ScannerHost {
    view: Retained<UIView>,
    scanner: NonNull<c_void>,
    _context: Box<ComponentContext>,
}

impl ScannerHost {
    fn new(context: ComponentContext) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: The view and its boxed context outlive the main-thread scanner host.
        let scanner = unsafe {
            govue_provider_qr_create(Retained::as_ptr(&view), (&raw const *context).cast(), emit)
        };
        Self {
            view,
            scanner: NonNull::new(scanner).expect("QR scanner"),
            _context: context,
        }
    }

    fn send(&self, operation: i32, args: &Value) {
        let json = args.to_string();
        // SAFETY: Swift copies these bytes before returning to the calling thread.
        unsafe {
            govue_provider_qr_command(
                self.scanner.as_ptr(),
                operation,
                json.as_ptr(),
                json.len() as isize,
            );
        }
    }
}

impl NativeComponent for ScannerHost {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        match name {
            "camera" => self.send(0, value.unwrap_or(&Value::Bool(false))),
            "torch" => self.send(1, value.unwrap_or(&Value::Bool(false))),
            "readImage" => {
                if let Some(value) = value.filter(|value| value.is_object()) {
                    self.send(3, value);
                }
            }
            _ => {}
        }
    }

    fn command(&mut self, name: &str, _args: &Value) {
        if name == "permission" {
            self.send(2, &Value::Null);
        }
    }
}

impl Drop for ScannerHost {
    fn drop(&mut self) {
        // SAFETY: The scanner clears its event callback before disposing the context.
        unsafe { govue_provider_qr_close(self.scanner.as_ptr()) };
    }
}

unsafe extern "C" fn emit(context: *const c_void, name: *const c_char, data: *const c_char) {
    // SAFETY: Swift invokes the callback on the main thread with its live context and borrowed strings.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let raw = unsafe { CStr::from_ptr(data) }.to_bytes();
    if let Ok(data) = serde_json::from_slice(raw) {
        context.emit(&name, data);
    }
}
