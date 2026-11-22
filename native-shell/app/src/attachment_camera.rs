use std::{cell::RefCell, rc::Rc};

use nsh_core::{PointerEvents, element::NativeEvent, prelude::*};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::oneshot;

#[derive(Deserialize)]
struct Capture {
    id: u64,
    file: Option<String>,
    error: Option<String>,
}

type Reply = Result<Option<String>, String>;

#[derive(Clone)]
pub struct AttachmentCamera {
    sequence: Signal<u64>,
    pending: Rc<RefCell<Option<oneshot::Sender<Reply>>>>,
}

impl AttachmentCamera {
    pub fn new() -> Self {
        Self {
            sequence: signal(0),
            pending: Rc::new(RefCell::new(None)),
        }
    }

    pub async fn take_photo(&self) -> Reply {
        let (send, receive) = oneshot::channel();
        *self.pending.borrow_mut() = Some(send);
        self.sequence.update(|id| *id += 1);
        receive
            .await
            .map_err(|_| "The camera was closed.".to_owned())?
    }

    pub fn view(&self) -> NativeView {
        let camera = self.clone();
        let request = self.sequence;
        nsh_core::element::native("govue/chat-camera")
            .absolute()
            .size(px(1.0))
            .opacity(0.0)
            .pointer_events(PointerEvents::None)
            .prop("capture", move || json!(request.get()))
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() == "capture"
                    && let Ok(result) = Capture::deserialize(&event.data)
                    && result.id == request.get_untracked()
                    && let Some(reply) = camera.pending.borrow_mut().take()
                {
                    let _ = reply.send(result.error.map_or(Ok(result.file), Err));
                }
            })
    }
}
