use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    rc::Rc,
    slice,
};

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;

type Callback = unsafe extern "C" fn(*const c_void, *const c_char, *const u8, isize);

unsafe extern "C" {
    fn govue_renderer_create(
        view: *const c_void,
        context: *const c_void,
        tokens: bool,
        callback: Callback,
    ) -> *mut c_void;
    fn govue_renderer_send(
        renderer: *mut c_void,
        method: *const c_char,
        bytes: *const u8,
        length: isize,
    );
    fn govue_renderer_close(renderer: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/player-runtime",
        Rc::new(|context| Box::new(Renderer::new(context, false))),
    );
    components.register(
        "govue/token-runtime",
        Rc::new(|context| Box::new(Renderer::new(context, true))),
    );
}

struct Renderer {
    view: Retained<UIView>,
    handle: Option<NonNull<c_void>>,
    _context: Box<ComponentContext>,
}

impl Renderer {
    fn new(context: ComponentContext, tokens: bool) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: Swift uses a weak reference to this main-thread view and
        // calls the receiver only until the component closes its renderer.
        let handle = NonNull::new(unsafe {
            govue_renderer_create(
                Retained::as_ptr(&view).cast(),
                (&raw const *context).cast(),
                tokens,
                receive,
            )
        });
        Self {
            view,
            handle,
            _context: context,
        }
    }

    fn send(&self, command: &CStr, args: &Value) {
        if let Some(handle) = self.handle {
            let json = args.to_string();
            // SAFETY: Swift copies the command and JSON on the main thread.
            unsafe {
                govue_renderer_send(
                    handle.as_ptr(),
                    command.as_ptr(),
                    json.as_ptr(),
                    json.len() as isize,
                )
            };
        }
    }
}

impl NativeComponent for Renderer {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "script"
            && let Some(value) = value
        {
            self.send(c"script", value);
        }
    }

    fn command(&mut self, action: &str, args: &Value) {
        let command = match action {
            "evaluate" => c"evaluate",
            "release" => c"release",
            "cancel" => c"cancel",
            _ => return,
        };
        self.send(command, args);
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: Releases the Swift owner before dropping the callback context.
            unsafe { govue_renderer_close(handle.as_ptr()) };
        }
    }
}

unsafe extern "C" fn receive(
    context: *const c_void,
    event: *const c_char,
    bytes: *const u8,
    length: isize,
) {
    if !(1..=2_010_000).contains(&length) {
        return;
    }
    // SAFETY: The native adapter supplies a live boxed context and keeps both
    // borrowed buffers valid for this call on the main thread.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let Ok(event) = unsafe { CStr::from_ptr(event) }.to_str() else {
        return;
    };
    let bytes = unsafe { slice::from_raw_parts(bytes, length as usize) };
    if let Ok(payload) = serde_json::from_slice(bytes) {
        context.emit(event, payload);
    }
}
