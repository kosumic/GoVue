use crate::trace;
use std::ffi::CStr;
use std::sync::{Mutex, MutexGuard};

/// Keep the existing storage serialization while measuring every acquisition,
/// including Rust workers that do not enter through execute().
pub(crate) struct RequestLock(Mutex<()>);

pub(crate) struct Guard<'a> {
    // Field drop order ends the hold slice before unlocking, then the context.
    _hold: trace::Section,
    _mutex: MutexGuard<'a, ()>,
    _context: trace::Section,
}

impl RequestLock {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(()))
    }

    // Only static labels are accepted: request data must never enter a trace.
    pub(crate) fn lock(&self, context: &'static CStr) -> Result<Guard<'_>, ()> {
        let context = trace::Section::begin(context);
        let guard = {
            let _wait = trace::Section::begin(c"GoVue core: lock_wait");
            self.0.lock().map_err(|_| ())?
        };
        Ok(Guard {
            _hold: trace::Section::begin(c"GoVue core: lock_hold"),
            _mutex: guard,
            _context: context,
        })
    }
}
