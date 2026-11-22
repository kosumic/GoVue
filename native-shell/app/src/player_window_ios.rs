use std::rc::Rc;

use nsh_platform_ios::{Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::Value;

unsafe extern "C" {
    fn govue_ios_player_fullscreen(view: *const UIView, enabled: bool);
}

pub fn register(components: &Components) {
    components.register(
        "govue/player-window",
        Rc::new(|_| {
            Box::new(Window(UIView::new(
                MainThreadMarker::new().expect("UIKit main thread"),
            )))
        }),
    );
}

struct Window(Retained<UIView>);

impl NativeComponent for Window {
    fn view(&self) -> &UIView {
        &self.0
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "fullscreen" {
            // SAFETY: The platform owns the view and applies this prop on the main thread.
            unsafe {
                govue_ios_player_fullscreen(
                    Retained::as_ptr(&self.0),
                    value == Some(&Value::Bool(true)),
                )
            };
        }
    }
}
