use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    rc::Rc,
};

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::{Value, json};

type Callback = unsafe extern "C" fn(*const c_void, *const c_char, u64, *const c_char, f64);

unsafe extern "C" {
    fn govue_now_playing_create(context: *const c_void, control: Callback) -> *mut c_void;
    fn govue_now_playing_update(handle: *mut c_void, bytes: *const u8, count: isize);
    fn govue_now_playing_close(handle: *mut c_void);
}

pub fn register(components: &Components) {
    components.register(
        "govue/now-playing",
        Rc::new(|context| Box::new(NowPlaying::new(context))),
    );
}

struct NowPlaying {
    view: Retained<UIView>,
    session: Option<NonNull<c_void>>,
    _context: Box<ComponentContext>,
}

impl NowPlaying {
    fn new(context: ComponentContext) -> Self {
        let thread = MainThreadMarker::new().expect("UIKit main thread");
        let context = Box::new(context);
        // SAFETY: UIKit hosts this component on the main thread. The boxed
        // context remains at the same address until Drop closes its session.
        let session = NonNull::new(unsafe {
            govue_now_playing_create((&raw const *context).cast(), control)
        });
        Self {
            view: UIView::new(thread),
            session,
            _context: context,
        }
    }
}

impl NativeComponent for NowPlaying {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != "snapshot" {
            return;
        }
        if let Some(session) = self.session {
            let snapshot = value.unwrap_or(&Value::Null).to_string();
            // SAFETY: The main-thread Swift session copies this bounded JSON.
            unsafe {
                govue_now_playing_update(
                    session.as_ptr(),
                    snapshot.as_ptr(),
                    snapshot.len() as isize,
                );
            }
        }
    }
}

impl Drop for NowPlaying {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            // SAFETY: Unregisters all callbacks before the boxed context drops.
            unsafe { govue_now_playing_close(session.as_ptr()) };
        }
    }
}

unsafe extern "C" fn control(
    context: *const c_void,
    action: *const c_char,
    generation: u64,
    video: *const c_char,
    position: f64,
) {
    // SAFETY: The Swift adapter calls this on the main thread while the
    // component and the callback's two borrowed C strings are alive.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    let action = unsafe { CStr::from_ptr(action) }.to_string_lossy();
    let video = unsafe { CStr::from_ptr(video) }.to_string_lossy();
    context.emit(
        "control",
        json!({"action":action,"video":video,"generation":generation,"position":position}),
    );
}
