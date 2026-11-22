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
    fn govue_ios_game_create(
        view: *const UIView,
        context: *const c_void,
        event: Event,
    ) -> *mut c_void;
    fn govue_ios_game_load(handle: *mut c_void, data: *const u8, length: isize);
    fn govue_ios_game_reply(handle: *mut c_void, data: *const u8, length: isize);
    fn govue_ios_game_close(handle: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/game",
        Rc::new(|context| Box::new(GameHost::new(context))),
    );
}

struct GameHost {
    view: Retained<UIView>,
    host: NonNull<c_void>,
    _context: Box<ComponentContext>,
}

impl GameHost {
    fn new(context: ComponentContext) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: The Native Shell component owns the view and callback context on the main thread.
        let host = unsafe {
            govue_ios_game_create(Retained::as_ptr(&view), (&raw const *context).cast(), event)
        };
        Self {
            view,
            host: NonNull::new(host).expect("game renderer"),
            _context: context,
        }
    }
}

impl NativeComponent for GameHost {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "document"
            && let Some(text) = value.and_then(Value::as_str)
        {
            // SAFETY: Swift copies the document synchronously and keeps no pointer to the bytes.
            unsafe { govue_ios_game_load(self.host.as_ptr(), text.as_ptr(), text.len() as isize) };
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        if name == "reply"
            && let Some(reply) = args.as_str()
        {
            // SAFETY: Swift copies the JSON synchronously on the main thread.
            unsafe {
                govue_ios_game_reply(self.host.as_ptr(), reply.as_ptr(), reply.len() as isize)
            };
        }
    }
}

impl Drop for GameHost {
    fn drop(&mut self) {
        // SAFETY: Swift removes its observations and message handler before the callback context drops.
        unsafe { govue_ios_game_close(self.host.as_ptr()) };
    }
}

unsafe extern "C" fn event(context: *const c_void, name: *const c_char, value: *const c_char) {
    // SAFETY: GameRenderer calls with its live main-thread context and borrowed C strings.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let value = unsafe { CStr::from_ptr(value) }.to_bytes();
    if let Ok(data) = serde_json::from_slice(value) {
        context.emit(&name, data);
    }
}
