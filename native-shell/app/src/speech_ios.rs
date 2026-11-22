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
type Create = unsafe extern "C" fn(*const c_void, Reply) -> *mut c_void;
type Command = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char);
type Close = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" {
    fn govue_chat_speech_create(context: *const c_void, reply: Reply) -> *mut c_void;
    fn govue_chat_speech_command(speech: *mut c_void, name: *const c_char, json: *const c_char);
    fn govue_chat_speech_close(speech: *mut c_void);
    fn govue_chat_reader_create(context: *const c_void, reply: Reply) -> *mut c_void;
    fn govue_chat_reader_command(reader: *mut c_void, name: *const c_char, json: *const c_char);
    fn govue_chat_reader_close(reader: *mut c_void);
}

pub fn register(components: &Components) {
    register_service(
        components,
        "govue/chat-speech",
        "voice",
        govue_chat_speech_create,
        govue_chat_speech_command,
        govue_chat_speech_close,
    );
    register_service(
        components,
        "govue/chat-read-aloud",
        "readAloud",
        govue_chat_reader_create,
        govue_chat_reader_command,
        govue_chat_reader_close,
    );
}

struct Destination {
    context: ComponentContext,
    event: &'static str,
}

fn register_service(
    components: &Components,
    name: &'static str,
    event: &'static str,
    create: Create,
    command: Command,
    close: Close,
) {
    components.register(
        name,
        Rc::new(move |context| {
            let context = Box::new(Destination { context, event });
            // SAFETY: The component keeps the callback context alive until the main-thread adapter closes.
            let handle = unsafe { create((&raw const *context).cast(), receive) };
            Box::new(Speech {
                view: UIView::new(MainThreadMarker::new().expect("UIKit main thread")),
                handle: NonNull::new(handle).expect("GoVue speech session"),
                _context: context,
                command,
                close,
            })
        }),
    );
}

struct Speech {
    view: Retained<UIView>,
    handle: NonNull<c_void>,
    _context: Box<Destination>,
    command: Command,
    close: Close,
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
            unsafe { (self.command)(self.handle.as_ptr(), name.as_ptr(), arguments.as_ptr()) };
        }
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        // SAFETY: Ending the native session disables all callbacks before the boxed context drops.
        unsafe { (self.close)(self.handle.as_ptr()) };
    }
}

unsafe extern "C" fn receive(context: *const c_void, json: *const c_char) {
    // SAFETY: Swift calls on the main thread while the context and encoded event are valid.
    let target = unsafe { &*context.cast::<Destination>() };
    let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
    if bytes.len() <= 128_000
        && let Ok(value) = serde_json::from_slice(bytes)
    {
        target.context.emit(target.event, value);
    }
}
