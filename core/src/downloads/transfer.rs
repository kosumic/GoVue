//! Bounded parallel downloads. HTTPS remains the platform MediaHttp adapter;
//! range planning, retries, pause/cancel and file assembly remain in Rust.
use crate::{Result, media, string, transport::DownloadSource};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

const CHUNK: u64 = 1_048_576;
struct Transfer {
    paused: Mutex<bool>,
    wifi_only: AtomicBool,
    wake: Condvar,
    cancelled: AtomicBool,
    done: AtomicBool,
    received: AtomicU64,
    total: u64,
    error: Mutex<Option<String>>,
    files: Vec<String>,
    cover: Option<String>,
}
static HTTP: OnceLock<Box<dyn crate::DownloadHttp>> = OnceLock::new();
pub(crate) fn install_http(backend: Box<dyn crate::DownloadHttp>) {
    let _ = HTTP.set(backend);
}
pub(crate) fn fetch(request: crate::DownloadRequest) -> io::Result<crate::MediaRangeReply> {
    HTTP.get()
        .ok_or_else(|| io::Error::other("Download networking is unavailable."))
        .map(|backend| backend.fetch(request))
}
impl Transfer {
    fn network_waiting(&self) -> bool {
        self.wifi_only.load(Ordering::Acquire)
            && !HTTP.get().is_some_and(|backend| backend.wifi_connected())
    }

    // A native check on every attempt also runs while JS is suspended. Timed
    // waits let connectivity and setting changes wake work without a JS event.
    fn permit(&self, failed: &AtomicBool) -> io::Result<bool> {
        let mut paused = self
            .paused
            .lock()
            .map_err(|_| io::Error::other("Download stopped."))?;
        loop {
            if self.cancelled.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Download cancelled.",
                ));
            }
            if !*paused && !self.network_waiting() {
                return Ok(self.wifi_only.load(Ordering::Acquire));
            }
            paused = self
                .wake
                .wait_timeout(paused, Duration::from_millis(500))
                .map_err(|_| io::Error::other("Download stopped."))?
                .0;
        }
    }
}
static TRANSFERS: OnceLock<Mutex<HashMap<String, Arc<Transfer>>>> = OnceLock::new();
pub(super) fn configure(root: &str, wifi_only: bool) {
    let transfers = TRANSFERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (key, transfer) in transfers.iter() {
        if key.starts_with(&format!("{root}/")) {
            transfer.wifi_only.store(wifi_only, Ordering::Release);
            transfer.wake.notify_all();
        }
    }
}
fn cleanup(transfer: Arc<Transfer>) {
    thread::spawn(move || {
        while !transfer.done.load(Ordering::Acquire) {
            thread::sleep(std::time::Duration::from_millis(100));
        }
        for path in transfer.files.iter().chain(transfer.cover.iter()) {
            let _ = fs::remove_file(path);
        }
    });
}
pub(super) fn reset(root: &str) {
    let mut transfers = TRANSFERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    transfers.retain(|key, transfer| {
        if !key.starts_with(&format!("{root}/")) {
            return true;
        }
        transfer.cancelled.store(true, Ordering::Relaxed);
        transfer.wake.notify_all();
        cleanup(transfer.clone());
        false
    });
    drop(transfers);
    let directory = Path::new(root).join("media").join("transfers");
    if let Ok(files) = fs::read_dir(directory) {
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|v| v == "tmp") && path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
    }
}
pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let id = string(command, "id")?;
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("Invalid transfer identity.".into());
    }
    let key = format!("{root}/{id}");
    let mut transfers = TRANSFERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Transfers are busy.")?;
    if string(command, "op")? == "downloads_transfer_start" {
        if transfers.contains_key(&key) || transfers.len() >= 16 {
            return Err(
                "This transfer is already running or the download limit was reached.".into(),
            );
        }
        let entries = media::read(root, "downloads.json", json!([]));
        let entry = entries
            .as_array()
            .and_then(|items| items.iter().find(|e| e["id"] == id))
            .ok_or("Download no longer exists.")?;
        if entry["state"] != "downloading" || entry["videoId"] != command["catalog"]["videoId"] {
            return Err("Download was paused or cancelled.".into());
        }
        let plan = super::plan(
            &command["catalog"],
            &super::Config::parse(&entry["config"])?,
        )?;
        let parts = plan["parts"].as_array().ok_or("Missing download parts.")?;
        let sources = parts
            .iter()
            .map(|part| {
                DownloadSource::new(
                    part,
                    command["userAgent"].as_str().unwrap_or(""),
                    string(&command["catalog"], "origin")?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let directory = Path::new(root).join("media").join("transfers");
        fs::create_dir_all(&directory).map_err(|_| "Could not create transfer storage.")?;
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).map_err(|_| "Could not create transfer identity.")?;
        let nonce = crate::hex(&bytes);
        let files = (0..parts.len())
            .map(|i| {
                directory
                    .join(format!("{id}-{nonce}-{i}.tmp"))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        let transfer = Arc::new(Transfer {
            paused: Mutex::new(false),
            wifi_only: AtomicBool::new(super::preferences(root)["wifiOnly"] == true),
            wake: Condvar::new(),
            cancelled: AtomicBool::new(false),
            done: AtomicBool::new(false),
            received: AtomicU64::new(0),
            total: plan["bytes"].as_u64().unwrap_or(0),
            error: Mutex::new(None),
            files,
            cover: plan["coverUrl"].as_str().map(|_| {
                directory
                    .join(format!("{id}-{nonce}-cover.tmp"))
                    .to_string_lossy()
                    .into_owned()
            }),
        });
        let cover_url = plan["coverUrl"].as_str().map(str::to_owned);
        let connections = super::preferences(root)["maxConnections"]
            .as_u64()
            .unwrap_or(4)
            .clamp(1, 16) as usize;
        let lengths = parts
            .iter()
            .map(|p| p["bytes"].as_u64().unwrap_or(0))
            .collect::<Vec<_>>();
        transfers.insert(key.clone(), transfer.clone());
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                for (index, source) in sources.into_iter().enumerate() {
                    copy_part(
                        &transfer,
                        &source,
                        &transfer.files[index],
                        lengths[index],
                        connections,
                    )?;
                }
                if let (Some(url), Some(path)) = (cover_url, &transfer.cover) {
                    copy_cover(&transfer, url, path)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                let mut stored = transfer
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if stored.is_none() {
                    *stored = Some(error);
                }
            }
            transfer.done.store(true, Ordering::Release);
        });
    }
    let transfer = transfers
        .get(&key)
        .cloned()
        .ok_or("This transfer is no longer running.")?;
    match string(command, "op")? {
        "downloads_transfer_pause" => {
            *transfer.paused.lock().map_err(|_| "Download stopped.")? = command["paused"] == true;
            transfer.wake.notify_all();
        }
        "downloads_transfer_cancel" | "downloads_transfer_release" => {
            transfer.cancelled.store(true, Ordering::Relaxed);
            transfer.wake.notify_all();
        }
        _ => {}
    }
    let done = transfer.done.load(Ordering::Acquire);
    let error = transfer
        .error
        .lock()
        .map_err(|_| "Download stopped.")?
        .clone();
    let value = json!({"done":done,"error":error,"networkWaiting":!done && transfer.network_waiting(),"received":transfer.received.load(Ordering::Relaxed),"total":transfer.total,"files":if done&&error.is_none(){transfer.files.iter().map(|p|format!("file://{p}")).collect::<Vec<_>>()}else{vec![]},"coverFile":if done&&error.is_none(){transfer.cover.as_ref().map(|p|format!("file://{p}"))}else{None}});
    if string(command, "op")? == "downloads_transfer_release" {
        transfers.remove(&key);
        drop(transfers);
        cleanup(transfer);
    }
    Ok(value)
}

fn copy_chunks(
    transfer: &Transfer,
    source: &DownloadSource,
    file: &Mutex<File>,
    offset: &AtomicU64,
    failed: &AtomicBool,
    length: u64,
) -> Result<()> {
    loop {
        transfer.permit(failed).map_err(|e| e.to_string())?;
        let start = offset.fetch_add(CHUNK, Ordering::Relaxed);
        if start >= length {
            return Ok(());
        }
        let end = (start + CHUNK).min(length) - 1;
        let bytes = source
            .download_bytes(start, end, &|| transfer.permit(failed))
            .map_err(|_| "Media request failed. Resume to refresh its address.")?;
        let mut output = file.lock().map_err(|_| "Could not assemble media file.")?;
        output
            .seek(SeekFrom::Start(start))
            .and_then(|_| output.write_all(&bytes))
            .map_err(|_| "Could not write media. Check available storage.")?;
        drop(output);
        transfer
            .received
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
    }
}

fn copy_cover(transfer: &Transfer, url: String, path: &str) -> Result<()> {
    let failed = AtomicBool::new(false);
    let mut attempts = 0;
    while attempts < 3 {
        let wifi_only = transfer.permit(&failed).map_err(|e| e.to_string())?;
        let reply = fetch(crate::DownloadRequest {
            url: url.clone(),
            user_agent: String::new(),
            origin: "https://m.youtube.com".into(),
            method: "GET".into(),
            range: None,
            wifi_only,
        })
        .map_err(|e| e.to_string())?;
        if reply.status == 200 && !reply.body.is_empty() && reply.body.len() as u64 <= CHUNK {
            return fs::write(path, reply.body)
                .map_err(|_| "Could not save the cover image.".into());
        }
        if reply.status != -1 {
            attempts += 1;
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err("The cover image could not be downloaded.".into())
}

fn copy_part(
    transfer: &Transfer,
    source: &DownloadSource,
    path: &str,
    length: u64,
    connections: usize,
) -> Result<()> {
    let file = File::create(path).map_err(|_| "Could not create media file.")?;
    file.set_len(length)
        .map_err(|_| "Not enough storage for this download.")?;
    let file = Mutex::new(file);
    let offset = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    thread::scope(|scope| {
        for _ in 0..connections {
            let t = transfer;
            let file = &file;
            let offset = &offset;
            let failed = &failed;
            scope.spawn(move || {
                let Err(error) = copy_chunks(t, source, file, offset, failed, length) else {
                    return;
                };
                failed.store(true, Ordering::Relaxed);
                t.error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_or_insert(error);
            });
        }
    });
    if failed.load(Ordering::Relaxed) {
        return Err(transfer
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| "Download failed.".into()));
    }
    Ok(())
}
