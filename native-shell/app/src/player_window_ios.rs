use std::rc::Rc;

use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_ui_kit::UIView;
use serde_json::{Value, json};

unsafe extern "C" {
    fn govue_ios_player_fullscreen(view: *const UIView, enabled: bool);
    fn govue_ios_player_levels(view: *const UIView, volume: &mut f64, brightness: &mut f64);
    fn govue_ios_player_level(view: *const UIView, volume: bool, value: f64);
}

pub fn register(components: &Components) {
    components.register(
        "govue/player-window",
        Rc::new(|context| {
            Box::new(Window {
                view: UIView::new(MainThreadMarker::new().expect("UIKit main thread")),
                context,
            })
        }),
    );
}

struct Window {
    view: Retained<UIView>,
    context: ComponentContext,
}

impl NativeComponent for Window {
    fn view(&self) -> &UIView {
        &self.view
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name == "fullscreen" {
            // SAFETY: The platform owns the view and applies this prop on the main thread.
            unsafe {
                govue_ios_player_fullscreen(
                    Retained::as_ptr(&self.view),
                    value == Some(&Value::Bool(true)),
                )
            };
        }
    }

    fn command(&mut self, name: &str, args: &Value) {
        let view = Retained::as_ptr(&self.view);
        if name == "levels" {
            let (mut volume, mut brightness) = (1.0, 0.5);
            // SAFETY: The main-thread Swift adapter writes both values synchronously.
            unsafe { govue_ios_player_levels(view, &mut volume, &mut brightness) };
            self.context
                .emit("levels", json!({"volume":volume,"brightness":brightness}));
        } else if name == "level"
            && let Some(level) = args["value"].as_f64().filter(|value| value.is_finite())
        {
            // SAFETY: The component owns this view, and UIKit runs on the main thread.
            unsafe { govue_ios_player_level(view, args["volume"] == true, level) };
        }
    }
}
