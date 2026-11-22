use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr::NonNull,
    rc::Rc,
};

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;

type Reply = unsafe extern "C" fn(*const c_void, *const c_char);

unsafe extern "C" {
    fn govue_chat_speech_create(context: *const c_void, reply: Reply) -> *mut c_void;
    fn govue_chat_speech_command(speech: *mut c_void, name: *const c_char, json: *const c_char);
    fn govue_chat_speech_close(speech: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/chat-speech",
        Rc::new(|context| {
            let context = Box::new(context);
            // SAFETY: The component keeps the callback context alive until the main-thread adapter closes.
            let handle = unsafe { govue_chat_speech_create((&raw const *context).cast(), receive) };
            Box::new(Speech {
                view: UIView::new(MainThreadMarker::new().expect("UIKit main thread")),
                handle: NonNull::new(handle).expect("GoVue speech session"),
                _context: context,
            })
        }),
    );
}

struct Speech {
    view: Retained<UIView>,
    handle: NonNull<c_void>,
    _context: Box<ComponentContext>,
}

impl NativeComponent for Speech {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, _: &str, _: Option<&Value>) {}

    fn command(&mut self, name: &str, arguments: &Value) {
        if let (Ok(name), Ok(arguments)) = (CString::new(name), CString::new(arguments.to_string()))
        {
            // SAFETY: The main-thread controller borrows the C strings only during this call.
            unsafe {
                govue_chat_speech_command(self.handle.as_ptr(), name.as_ptr(), arguments.as_ptr())
            };
        }
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        // SAFETY: Ending the native session disables all callbacks before the boxed context drops.
        unsafe { govue_chat_speech_close(self.handle.as_ptr()) };
    }
}

unsafe extern "C" fn receive(context: *const c_void, json: *const c_char) {
    // SAFETY: Swift calls on the main thread while the context and encoded event are valid.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
    if bytes.len() <= 128_000
        && let Ok(value) = serde_json::from_slice(bytes)
    {
        context.emit("voice", value);
    }
}
