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
    fn govue_ios_word_tools_create(
        view: *const UIView,
        selection: bool,
        context: *const c_void,
        emit: Event,
    ) -> *mut c_void;
    fn govue_ios_word_tools_request(
        handle: *mut c_void,
        action: i32,
        data: *const u8,
        length: isize,
    );
    fn govue_ios_word_tools_close(handle: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/caption-words",
        Rc::new(|context| Box::new(WordHost::new(context, true))),
    );
    components.register(
        "govue/word-translation",
        Rc::new(|context| Box::new(WordHost::new(context, false))),
    );
}

struct WordHost {
    view: Retained<UIView>,
    host: NonNull<c_void>,
    _context: Box<ComponentContext>,
}

impl WordHost {
    fn new(context: ComponentContext, selection: bool) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: The view and boxed component context remain valid for the main-thread host.
        let host = unsafe {
            govue_ios_word_tools_create(
                Retained::as_ptr(&view),
                selection,
                (&raw const *context).cast(),
                event,
            )
        };
        Self {
            view,
            host: NonNull::new(host).expect("word tools"),
            _context: context,
        }
    }

    fn request(&self, action: i32, value: &Value) {
        let value = value.to_string();
        // SAFETY: Swift parses the borrowed bytes within the synchronous platform call.
        unsafe {
            govue_ios_word_tools_request(
                self.host.as_ptr(),
                action,
                value.as_ptr(),
                value.len() as isize,
            )
        };
    }
}

impl NativeComponent for WordHost {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "selection"
            && let Some(value) = value
        {
            self.request(0, value);
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        match name {
            "translate" => self.request(1, args),
            "cancel" => self.request(2, args),
            _ => {}
        }
    }
}

impl Drop for WordHost {
    fn drop(&mut self) {
        // SAFETY: The host clears its main-thread callbacks before the boxed context is dropped.
        unsafe { govue_ios_word_tools_close(self.host.as_ptr()) };
    }
}

unsafe extern "C" fn event(context: *const c_void, name: *const c_char, value: *const c_char) {
    // SAFETY: Swift passes the live main-thread context and borrowed, null-terminated strings.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let value = unsafe { CStr::from_ptr(value) }.to_bytes();
    if let Ok(data) = serde_json::from_slice(value) {
        context.emit(&name, data);
    }
}
