use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::PathBuf,
    slice,
};

use govue_core::{DownloadHttp, DownloadPlatform, DownloadRequest, MediaRangeReply};
use objc2::rc::autoreleasepool;
use objc2_foundation::NSString;
use serde_json::Value;

const MAX_REPLY: usize = 1_048_576;
type Reply = unsafe extern "C" fn(*mut c_void, i32, *const u8, isize, *const c_char);

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn NSHomeDirectory() -> *const NSString;
}

unsafe extern "C" {
    fn govue_download_wifi_connected() -> bool;
    fn govue_download_mux(
        video: *const c_char,
        audio: *const c_char,
        output: *const c_char,
    ) -> bool;
    fn govue_download_fetch(
        context: *mut c_void,
        url: *const c_char,
        user_agent: *const c_char,
        origin: *const c_char,
        method: *const c_char,
        start: u64,
        end: u64,
        has_range: bool,
        wifi_only: bool,
        respond: Reply,
    );
}

pub fn install() {
    let home = autoreleasepool(|_| {
        // SAFETY: Foundation supplies the current process's sandbox path.
        unsafe { NSHomeDirectory().as_ref() }.map(|path| PathBuf::from(path.to_string()))
    });
    if let Some(home) = home.filter(|path| path.is_absolute()) {
        govue_core::install_download_platform(Box::new(IosFiles(home)));
        govue_core::install_download_http(Box::new(IosHttp));
    }
}

struct IosFiles(PathBuf);

impl DownloadPlatform for IosFiles {
    fn directory(&self) -> String {
        self.0.join("Documents/downloads").to_string_lossy().into()
    }

    fn export_directory(&self) -> String {
        self.0
            .join("Library/Caches/download-exports")
            .to_string_lossy()
            .into()
    }

    fn supported_formats(&self, formats: String) -> String {
        let items = serde_json::from_str::<Vec<Value>>(&formats).unwrap_or_default();
        let selected = items
            .iter()
            .take(500)
            .filter_map(|item| {
                let mime = item["mimeType"].as_str()?;
                let audio = mime.starts_with("audio/mp4") && mime.contains("mp4a");
                let video = mime.starts_with("video/mp4")
                    && ["avc1", "hvc1", "hev1"]
                        .iter()
                        .any(|codec| mime.contains(codec));
                let key = item["key"].as_str()?;
                (audio || video).then_some(key)
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&selected).unwrap_or_else(|_| "[]".into())
    }

    fn mux(&self, video: String, audio: String, output: String) -> bool {
        let (Ok(video), Ok(audio), Ok(output)) = (
            CString::new(video),
            CString::new(audio),
            CString::new(output),
        ) else {
            return false;
        };
        // SAFETY: The Swift adapter uses these three strings within the call.
        unsafe { govue_download_mux(video.as_ptr(), audio.as_ptr(), output.as_ptr()) }
    }

    fn publish(&self, _: String) -> i32 {
        0
    }

    fn delete_published(&self, _: String) -> bool {
        false
    }

    fn activity(&self, _: u32, _: String, _: u64, _: u64) {}
}

struct IosHttp;

impl DownloadHttp for IosHttp {
    fn wifi_connected(&self) -> bool {
        // SAFETY: The Swift network adapter owns a process-wide path monitor.
        unsafe { govue_download_wifi_connected() }
    }

    fn fetch(&self, request: DownloadRequest) -> MediaRangeReply {
        let mut reply = MediaRangeReply {
            status: 0,
            body: Vec::new(),
            location: None,
        };
        let (Ok(url), Ok(agent), Ok(origin), Ok(method)) = (
            CString::new(request.url),
            CString::new(request.user_agent),
            CString::new(request.origin),
            CString::new(request.method),
        ) else {
            return reply;
        };
        let (start, end, has_range) = request
            .range
            .map_or((0, 0, false), |bounds| (bounds.start, bounds.end, true));
        // SAFETY: The synchronous Swift adapter borrows the four C strings and
        // calls the callback once while this output remains live.
        unsafe {
            govue_download_fetch(
                (&raw mut reply).cast(),
                url.as_ptr(),
                agent.as_ptr(),
                origin.as_ptr(),
                method.as_ptr(),
                start,
                end,
                has_range,
                request.wifi_only,
                receive,
            );
        }
        reply
    }
}

unsafe extern "C" fn receive(
    context: *mut c_void,
    status: i32,
    bytes: *const u8,
    length: isize,
    location: *const c_char,
) {
    if !(0..=MAX_REPLY as isize).contains(&length) || (bytes.is_null() && length != 0) {
        return;
    }
    // SAFETY: The context points to the requesting Rust worker's live output.
    // Swift retains the Data and optional C string until this callback returns.
    let reply = unsafe { &mut *context.cast::<MediaRangeReply>() };
    reply.status = status;
    if length != 0 {
        reply
            .body
            .extend_from_slice(unsafe { slice::from_raw_parts(bytes, length as usize) });
    }
    if !location.is_null() {
        reply.location = unsafe { CStr::from_ptr(location) }
            .to_str()
            .ok()
            .map(str::to_owned);
    }
}
