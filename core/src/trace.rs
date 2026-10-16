use std::ffi::CStr;

// Android's NDK atrace API feeds the same Perfetto recording as android.os.Trace.
// Nothing is written unless the recorder has enabled this app's trace category.
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ATrace_isEnabled() -> bool;
    fn ATrace_beginSection(name: *const std::ffi::c_char);
    fn ATrace_endSection();
}

pub struct Section {
    #[cfg(target_os = "android")]
    active: bool,
}

impl Section {
    pub fn begin(name: &'static CStr) -> Self {
        #[cfg(target_os = "android")]
        {
            // Static NUL-terminated labels have no page content or media URLs.
            let active = unsafe { ATrace_isEnabled() };
            if active {
                unsafe { ATrace_beginSection(name.as_ptr()) };
            }
            Self { active }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = name;
            Self {}
        }
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        if self.active {
            unsafe { ATrace_endSection() };
        }
    }
}
