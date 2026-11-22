use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use nsh_core::{
    Bounds, Points,
    element::{ElementRef, ScrollEvent},
    prelude::*,
};

use crate::ui::{self, Palette, press};

const FOLLOW_DISTANCE: f32 = 72.0;

#[derive(Clone, Copy)]
struct Position {
    offset: f32,
    follows_end: bool,
}

#[derive(Clone, Default)]
pub struct ChatViewports(Rc<RefCell<HashMap<String, Position>>>);

impl ChatViewports {
    pub fn thread(&self, id: String, find: Memo<bool>, jump: Signal<u64>) -> ChatViewport {
        let previous = self.0.borrow().get(&id).copied();
        let viewport = ChatViewport {
            id: Rc::new(id),
            history: self.clone(),
            scroll: element_ref(),
            ready: signal(false),
            previous: signal(previous.map(|position| position.offset)),
            follow: signal(previous.is_none_or(|position| position.follows_end)),
            away: signal(false),
            dragging: signal(false),
            offset: signal(0.0),
            content_height: signal(0.0),
            viewport_height: signal(0.0),
            finding: find,
        };
        effect({
            let viewport = viewport.clone();
            let first = Cell::new(true);
            move || {
                jump.get();
                if !first.replace(false) {
                    viewport.jump(false);
                }
            }
        });
        effect({
            let viewport = viewport.clone();
            move || {
                if find.get() {
                    viewport.follow.set(false);
                }
            }
        });
        viewport
    }
}

#[derive(Clone)]
pub struct ChatViewport {
    id: Rc<String>,
    history: ChatViewports,
    pub scroll: ElementRef,
    ready: Signal<bool>,
    previous: Signal<Option<f32>>,
    follow: Signal<bool>,
    away: Signal<bool>,
    dragging: Signal<bool>,
    offset: Signal<f32>,
    content_height: Signal<f32>,
    viewport_height: Signal<f32>,
    finding: Memo<bool>,
}

impl ChatViewport {
    pub fn measured(&self, bounds: &Bounds<Points>, content: bool) {
        if content {
            self.content_height.set(bounds.size.height.0);
        } else {
            self.viewport_height.set(bounds.size.height.0);
        }
        if self.content_height.get_untracked() <= 0.0 || self.viewport_height.get_untracked() <= 0.0
        {
            return;
        }
        if !self.ready.get_untracked() {
            self.ready.set(true);
            if !self.follow.get_untracked()
                && let Some(offset) = self.previous.get_untracked()
            {
                self.scroll.scroll_to_offset(px(offset), false);
                return;
            }
        }
        if self.follow.get_untracked()
            && !self.dragging.get_untracked()
            && !self.finding.get_untracked()
        {
            self.scroll.scroll_to_end(false);
        }
    }

    pub fn moved(&self, event: &ScrollEvent) {
        let offset = event.offset.y.0.max(0.0);
        let away = event.content_size.height.0 - event.size.height.0 - offset >= FOLLOW_DISTANCE;
        let movement = offset - self.offset.get_untracked();
        self.offset.set(offset);
        if self.dragging.get_untracked() && !self.finding.get_untracked() {
            if movement < -1.0 {
                self.follow.set(false);
            } else if movement > 1.0 && !away {
                self.follow.set(true);
            }
        }
        self.away.set(away);
        if self.ready.get_untracked() {
            self.history.0.borrow_mut().insert(
                self.id.as_ref().clone(),
                Position {
                    offset,
                    follows_end: self.follow.get_untracked(),
                },
            );
        }
    }

    pub fn drag(&self, active: bool) {
        self.dragging.set(active);
    }

    fn jump(&self, animated: bool) {
        self.follow.set(true);
        self.away.set(false);
        self.scroll.scroll_to_end(animated);
    }

    pub fn jump_button(&self, palette: Palette) -> If {
        let shown = self.clone();
        let viewport = self.clone();
        when(
            move || {
                shown.ready.get()
                    && shown.away.get()
                    && !shown.follow.get()
                    && !shown.dragging.get()
                    && !shown.finding.get()
            },
            move || {
                let viewport = viewport.clone();
                press()
                    .absolute()
                    .bottom(px(12.0))
                    .self_center()
                    .size(px(44.0))
                    .rounded(px(22.0))
                    .justify_center()
                    .items_center()
                    .bg(move || palette.raised())
                    .label("Jump to latest message")
                    .on_press(move || viewport.jump(true))
                    .child(ui::icon("M12 5v14m-7-7 7 7 7-7", 20.0, move || {
                        palette.ink()
                    }))
            },
        )
    }
}
