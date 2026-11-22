use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    rc::Rc,
};

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::{Value, json};

type Reply = unsafe extern "C" fn(*const c_void, u64, *const c_char, *const c_char);

unsafe extern "C" {
    fn govue_chat_camera_create(
        view: *const UIView,
        context: *const c_void,
        reply: Reply,
    ) -> *mut c_void;
    fn govue_chat_camera_take(session: *mut c_void, request: u64);
    fn govue_chat_camera_close(session: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/chat-camera",
        Rc::new(|context| Box::new(Camera::new(context))),
    );
}

struct Camera {
    view: Retained<UIView>,
    session: NonNull<c_void>,
    _context: Box<ComponentContext>,
    last: u64,
}

impl Camera {
    fn new(context: ComponentContext) -> Self {
        let view = UIView::new(MainThreadMarker::new().expect("UIKit main thread"));
        let context = Box::new(context);
        // SAFETY: The native component owns the view and the pinned callback context.
        let session = unsafe {
            govue_chat_camera_create(Retained::as_ptr(&view), (&raw const *context).cast(), reply)
        };
        Self {
            view,
            session: NonNull::new(session).expect("GoVue camera session"),
            _context: context,
            last: 0,
        }
    }
}

impl NativeComponent for Camera {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "capture"
            && let Some(request) = value.and_then(Value::as_u64)
            && request > self.last
        {
            self.last = request;
            // SAFETY: The live camera adapter is updated on the UIKit main thread.
            unsafe { govue_chat_camera_take(self.session.as_ptr(), request) };
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        // SAFETY: Closes the adapter and disables callbacks before the context drops.
        unsafe { govue_chat_camera_close(self.session.as_ptr()) };
    }
}

unsafe extern "C" fn reply(
    context: *const c_void,
    id: u64,
    file: *const c_char,
    error: *const c_char,
) {
    // SAFETY: Swift calls on the main thread while the context and non-null C strings are alive.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let file = (!file.is_null()).then(|| unsafe { CStr::from_ptr(file).to_string_lossy() });
    let error = (!error.is_null()).then(|| unsafe { CStr::from_ptr(error).to_string_lossy() });
    context.emit("capture", json!({"id":id,"file":file,"error":error}));
}
