use std::{
    ffi::{CStr, CString, c_char, c_void},
    rc::Rc,
};

use govue_core::ChatNotificationPlatform;
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;

type Opened = unsafe extern "C" fn(*const c_void, *const c_char);

unsafe extern "C" {
    fn govue_reply_notifications_install();
    fn govue_reply_notifications_update(chat: *const c_char, show: bool, revision: u64);
    fn govue_reply_notifications_prepare();
    fn govue_reply_notifications_observe(context: *const c_void, opened: Opened);
    fn govue_reply_notifications_remove(context: *const c_void);
}

pub fn install() {
    // SAFETY: Called on the main thread before the host starts UIApplication.
    unsafe { govue_reply_notifications_install() };
    govue_core::install_chat_notification_platform(Box::new(IosNotifications));
}

pub fn register(components: &Components) {
    components.register(
        "govue/chat-notifications",
        Rc::new(|context| Box::new(NotificationView::new(context))),
    );
}

struct IosNotifications;

impl ChatNotificationPlatform for IosNotifications {
    fn update(&self, chat_id: String, show: bool, revision: u64) {
        if let Ok(chat) = CString::new(chat_id) {
            // SAFETY: The adapter copies the argument before scheduling work on the main queue.
            unsafe { govue_reply_notifications_update(chat.as_ptr(), show, revision) };
        }
    }
}

struct NotificationView {
    view: Retained<UIView>,
    context: Box<ComponentContext>,
    observing: bool,
}

impl NotificationView {
    fn new(context: ComponentContext) -> Self {
        Self {
            view: UIView::new(MainThreadMarker::new().expect("UIKit main thread")),
            context: Box::new(context),
            observing: false,
        }
    }
}

impl NativeComponent for NotificationView {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if value != Some(&Value::Bool(true)) {
            return;
        }
        match name {
            "prepare" => {
                // SAFETY: Calls UserNotifications on the UIKit thread.
                unsafe { govue_reply_notifications_prepare() };
            }
            "observe" => {
                self.observing = true;
                // SAFETY: The context stays boxed until the main-thread observer is removed.
                unsafe {
                    govue_reply_notifications_observe((&raw const *self.context).cast(), opened);
                }
            }
            _ => {}
        }
    }
}

impl Drop for NotificationView {
    fn drop(&mut self) {
        if self.observing {
            // SAFETY: The adapter clears its observer before the context is disposed.
            unsafe { govue_reply_notifications_remove((&raw const *self.context).cast()) };
        }
    }
}

unsafe extern "C" fn opened(context: *const c_void, chat: *const c_char) {
    // SAFETY: The Swift observer supplies its live context and a borrowed C string.
    let context = unsafe { &*context.cast::<ComponentContext>() };
    if let Ok(chat) = unsafe { CStr::from_ptr(chat) }.to_str() {
        context.emit("open", Value::String(chat.into()));
    }
}
