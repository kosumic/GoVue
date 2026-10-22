//! Native-lifetime download orchestration. JS observes snapshots and requests actions.
use super::{history, preferences, store, transfer};
use crate::{DownloadPlatform, Result, json, media, request, string};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

static PLATFORM: OnceLock<Box<dyn DownloadPlatform>> = OnceLock::new();
static RUNNERS: OnceLock<Mutex<HashMap<String, Arc<Runner>>>> = OnceLock::new();
struct Work {
    cancelled: AtomicBool,
    delete_media: AtomicBool,
    transfer: AtomicBool,
}
#[derive(Default)]
struct Runner {
    ready: AtomicBool,
    jobs: Mutex<HashMap<String, Arc<Work>>>,
    removals: Mutex<HashMap<String, bool>>,
    sleep: Mutex<()>,
    wake: Condvar,
}
pub(crate) fn install(platform: Box<dyn DownloadPlatform>) {
    let _ = PLATFORM.set(platform);
}
fn platform() -> Result<&'static dyn DownloadPlatform> {
    PLATFORM
        .get()
        .map(std::convert::AsRef::as_ref)
        .ok_or("Install an updated GoVue build to download media.".into())
}
fn entries(root: &str) -> Result<Value> {
    history(&json!({"entries":media::read(root,"downloads.json",json!([]))}))
}
fn entry(root: &str, id: &str) -> Result<Value> {
    entries(root)?
        .as_array()
        .and_then(|e| e.iter().find(|e| e["id"] == id))
        .cloned()
        .ok_or("This download no longer exists.".into())
}
fn directory() -> Result<PathBuf> {
    let path = PathBuf::from(platform()?.directory());
    if !path.is_absolute() {
        return Err("Download storage is unavailable.".into());
    }
    fs::create_dir_all(&path).map_err(|_| "Could not create download storage.")?;
    Ok(path)
}
fn file(name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.contains("..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err("Invalid saved file.".into());
    }
    let directory = directory()?
        .canonicalize()
        .map_err(|_| "Download storage is unavailable.")?;
    let path = directory.join(name);
    if path.is_symlink() {
        return Err("Invalid saved file.".into());
    }
    Ok(path)
}
fn uri(path: &Path) -> Result<String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| "Invalid saved file.".into())
}
fn local(raw: &Value) -> Result<PathBuf> {
    raw.as_str()
        .and_then(|s| url::Url::parse(s).ok())
        .and_then(|u| u.to_file_path().ok())
        .ok_or("The media download is incomplete.".into())
}
fn event(root: &str, id: &str, name: &str, values: Value) -> Result<Value> {
    let mut command = json!({"op":"downloads_event","id":id,"event":name});
    if let Some(values) = values.as_object() {
        for (key, value) in values {
            command[key] = value.clone();
        }
    }
    request(&command, root)
}
fn snapshot(root: &str, runner: &Runner) -> Result<Value> {
    let mut waiting = Vec::new();
    let jobs = runner.jobs.lock().map_err(|_| "Downloads are busy.")?;
    for (id, work) in jobs.iter() {
        if work.transfer.load(Ordering::Acquire)
            && transfer::dispatch(&json!({"op":"downloads_transfer_poll","id":id}), root)
                .is_ok_and(|s| s["networkWaiting"] == true)
        {
            waiting.push(id.clone());
        }
    }
    drop(jobs);
    Ok(
        json!({"entries":entries(root)?,"ready":runner.ready.load(Ordering::Acquire),
        "waitingForWifi":waiting,"preferences":preferences(root)}),
    )
}
fn notify(root: &str) {
    if let (Ok(entries), Ok(platform)) = (entries(root), platform()) {
        let wifi = transfer::wifi_connected();
        let wifi_only = preferences(root)["wifiOnly"] == true;
        let active: Vec<_> = entries
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                matches!(
                    e["state"].as_str(),
                    Some("resolving" | "downloading" | "combining")
                ) || e["state"] == "queued" && (!wifi_only || wifi)
            })
            .collect();
        platform.activity(
            u32::try_from(active.len()).unwrap_or(2000),
            active
                .first()
                .and_then(|e| e["title"].as_str())
                .unwrap_or("")
                .into(),
            active.iter().filter_map(|e| e["received"].as_u64()).sum(),
            active.iter().filter_map(|e| e["total"].as_u64()).sum(),
        );
    }
    crate::changed("downloads");
}
fn delete_files(value: &Value) -> Result<()> {
    for artifact in value["artifacts"].as_array().into_iter().flatten() {
        if let Some(uri) = artifact["publishedUri"].as_str()
            && !platform()?.delete_published(uri.into())
        {
            return Err("Could not delete the published file.".into());
        }
    }
    for name in std::iter::once(&value["file"]).chain(
        value["artifacts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| &a["file"]),
    ) {
        let path = file(name.as_str().ok_or("Invalid saved file.")?)?;
        if path.exists() {
            fs::remove_file(path).map_err(|_| "Could not delete the saved file.")?;
        }
    }
    Ok(())
}
fn restore(root: &str) -> Result<()> {
    request(&json!({"op":"downloads_restore"}), root)?;
    for value in entries(root)?
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["state"] == "complete")
    {
        let id = string(value, "id")?;
        let published = value["artifacts"]
            .as_array()
            .is_some_and(|a| a.iter().any(|a| a["publishedUri"].is_string()));
        let files: Vec<_> = value["artifacts"]
            .as_array()
            .filter(|a| !a.is_empty())
            .map_or_else(
                || vec![&value["file"]],
                |a| a.iter().map(|a| &a["file"]).collect(),
            );
        let missing = published && platform()?.publish(id.into()) < 0
            || files
                .iter()
                .any(|name| file(name.as_str().unwrap_or("")).is_ok_and(|p| !p.is_file()));
        if missing {
            let _ = event(root, id, "missing", Value::Null);
        }
    }
    Ok(())
}
fn ensure(root: &str) -> Result<Arc<Runner>> {
    platform()?;
    let mut runners = RUNNERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Downloads are busy.")?;
    if let Some(runner) = runners.get(root) {
        return Ok(runner.clone());
    }
    if runners.len() >= 4 {
        return Err("Too many download stores.".into());
    }
    let runner = Arc::new(Runner::default());
    let (worker, root_owned) = (runner.clone(), root.to_owned());
    thread::Builder::new()
        .name("download-runner".into())
        .spawn(move || {
            let _ = restore(&root_owned);
            worker.ready.store(true, Ordering::Release);
            notify(&root_owned);
            loop {
                let _ = schedule(&root_owned, &worker);
                let Ok(guard) = worker.sleep.lock() else {
                    break;
                };
                if worker
                    .wake
                    .wait_timeout(guard, Duration::from_millis(500))
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|_| "Could not start downloads.")?;
    runners.insert(root.into(), runner.clone());
    drop(runners);
    Ok(runner)
}
fn schedule(root: &str, runner: &Arc<Runner>) -> Result<()> {
    let excluded: Vec<_> = runner
        .jobs
        .lock()
        .map_err(|_| "Downloads are busy.")?
        .keys()
        .cloned()
        .collect();
    let result = if entries(root)?.as_array().is_some_and(|e| {
        e.iter()
            .any(|e| e["state"] == "queued" && !excluded.iter().any(|id| e["id"] == *id))
    }) {
        request(
            &json!({"op":"downloads_claim","wifi":transfer::wifi_connected(),"excludeIds":excluded}),
            root,
        )?
    } else {
        json!({"claimed":[]})
    };
    for id in result["claimed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let value = result["entries"]
            .as_array()
            .and_then(|e| e.iter().find(|e| e["id"] == id))
            .cloned()
            .ok_or("Missing download.")?;
        let work = Arc::new(Work {
            cancelled: AtomicBool::new(false),
            delete_media: AtomicBool::new(true),
            transfer: AtomicBool::new(false),
        });
        runner
            .jobs
            .lock()
            .map_err(|_| "Downloads are busy.")?
            .insert(id.into(), work.clone());
        let (root, runner, id) = (root.to_owned(), runner.clone(), id.to_owned());
        thread::spawn(move || {
            if let Err(error) = run(&root, &value, &work)
                && !work.cancelled.load(Ordering::Acquire)
                && entry(&root, &id).is_ok_and(|e| {
                    matches!(
                        e["state"].as_str(),
                        Some("resolving" | "downloading" | "combining")
                    )
                })
            {
                let _ = event(&root, &id, "failed", json!({"error":error}));
            }
            if work.transfer.load(Ordering::Acquire) {
                let _ = request(&json!({"op":"downloads_transfer_release","id":id}), &root);
            }
            if work.cancelled.load(Ordering::Acquire)
                && work.delete_media.load(Ordering::Acquire)
                && let Ok(value) = entry(&root, &id)
            {
                let _ = delete_files(&value);
            }
            if let Ok(mut jobs) = runner.jobs.lock() {
                jobs.remove(&id);
            }
            runner.wake.notify_all();
            notify(&root);
        });
    }
    let pending = runner
        .removals
        .lock()
        .map_err(|_| "Downloads are busy.")?
        .clone();
    for (id, delete) in pending {
        if runner
            .jobs
            .lock()
            .map_err(|_| "Downloads are busy.")?
            .contains_key(&id)
        {
            continue;
        }
        if let Ok(value) = entry(root, &id) {
            if delete {
                delete_files(&value)?;
            }
            request(&json!({"op":"downloads_remove","id":id}), root)?;
        }
        runner
            .removals
            .lock()
            .map_err(|_| "Downloads are busy.")?
            .remove(&id);
        notify(root);
    }
    if !result["claimed"].as_array().is_none_or(Vec::is_empty) {
        notify(root);
    }
    Ok(())
}
fn run(root: &str, value: &Value, work: &Work) -> Result<()> {
    let id = string(value, "id")?;
    let check = || {
        if work.cancelled.load(Ordering::Acquire) {
            return Err("Download cancelled.".into());
        }
        let current = entry(root, id)?;
        if matches!(
            current["state"].as_str(),
            Some("paused" | "cancelled" | "queued")
        ) && !work.transfer.load(Ordering::Acquire)
        {
            return Err("Download paused or cancelled.".into());
        }
        Ok(())
    };
    let mut catalog = crate::extraction::resolve(
        root,
        "media",
        string(value, "videoId")?,
        "default",
        "",
        false,
        &check,
    )?;
    let formats: Vec<_> = catalog["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            json!({
        "key":s["key"].as_str().map(str::to_owned).unwrap_or_else(||format!("{}:",s["itag"])),
        "mimeType":s["mimeType"],"width":s["width"],"height":s["height"],"fps":s["fps"]})
        })
        .collect();
    let keys: Value =
        serde_json::from_str(&platform()?.supported_formats(json!(formats).to_string()))
            .map_err(|_| "Media capabilities are unavailable.")?;
    catalog = request(
        &json!({"op":"media_capabilities","catalog":catalog,"keys":keys}),
        root,
    )?;
    check()?;
    let prepared = request(
        &json!({"op":"downloads_prepared","id":id,"catalog":catalog}),
        root,
    )?;
    let plan = &prepared["plan"];
    let subtitle = if plan["subtitle"].is_object() {
        let transcript = crate::extraction::resolve(
            root,
            "transcript",
            string(value, "videoId")?,
            string(&plan["subtitle"], "id")?,
            "",
            false,
            &check,
        )?;
        Some(request(
            &json!({"op":"downloads_subtitles","videoId":value["videoId"],"trackId":transcript["trackId"]}),
            root,
        )?)
    } else {
        None
    };
    check()?;
    let destination = file(string(value, "file")?)?;
    if destination.exists() {
        fs::remove_file(&destination).map_err(|_| "Could not replace saved media.")?;
    }
    let mut status = request(
        &json!({"op":"downloads_transfer_start","id":id,"catalog":catalog,"userAgent":""}),
        root,
    )?;
    work.transfer.store(true, Ordering::Release);
    loop {
        if work.cancelled.load(Ordering::Acquire) {
            let _ = request(&json!({"op":"downloads_transfer_cancel","id":id}), root);
            return Err("Download cancelled.".into());
        }
        let current = entry(root, id)?;
        if status["done"] == true && current["state"] != "paused" {
            break;
        }
        request(
            &json!({"op":"downloads_transfer_pause","id":id,"paused":current["state"]=="paused"}),
            root,
        )?;
        if current["state"] == "downloading" && status["received"] != current["received"] {
            event(root, id, "progress", json!({"received":status["received"]}))?;
        }
        notify(root);
        thread::sleep(Duration::from_millis(500));
        status = request(&json!({"op":"downloads_transfer_poll","id":id}), root)?;
    }
    if let Some(error) = status["error"].as_str() {
        return Err(error.into());
    }
    check()?;
    if plan["attachmentsOnly"] != true {
        let parts = status["files"]
            .as_array()
            .ok_or("The media download is incomplete.")?;
        match parts.as_slice() {
            [source] => {
                fs::copy(local(source)?, &destination).map_err(|_| "Could not save media.")?;
            }
            [video, audio] => {
                event(root, id, "combining", Value::Null)?;
                if !platform()?.mux(string_uri(video)?, string_uri(audio)?, uri(&destination)?) {
                    return Err("Could not combine downloaded media.".into());
                }
            }
            _ => return Err("The media download is incomplete.".into()),
        }
    }
    if let Some(subtitle) = subtitle {
        fs::write(
            file(&format!("{id}.srt"))?,
            subtitle.as_str().ok_or("Subtitles could not be loaded.")?,
        )
        .map_err(|_| "Could not save subtitles.")?;
    }
    if plan["coverUrl"].is_string() {
        fs::copy(local(&status["coverFile"])?, file(&format!("{id}.jpg"))?)
            .map_err(|_| "Could not save the cover image.")?;
    }
    check()?;
    if entry(root, id)?["state"] == "downloading" {
        event(root, id, "combining", Value::Null)?;
    }
    let published = platform()?.publish(id.into());
    if published < 0 {
        return Err(
            "The file could not be saved in Downloads. Check storage space and retry.".into(),
        );
    }
    check()?;
    event(
        root,
        id,
        "complete",
        json!({"publicationRequired":published==1}),
    )?;
    Ok(())
}
fn string_uri(value: &Value) -> Result<String> {
    uri(&local(value)?)
}

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let runner = ensure(root)?;
    match string(command, "op")? {
        "downloads_runner_start" | "downloads_runner_snapshot" => return snapshot(root, &runner),
        "downloads_runner_enqueue" => {
            let mut command = command.clone();
            command["op"] = json!("downloads_enqueue");
            let result = store::dispatch(&command, root)?;
            runner.wake.notify_all();
            crate::changed("downloads");
            return Ok(result);
        }
        "downloads_runner_pause" | "downloads_runner_cancel" | "downloads_runner_retry" => {
            let id = string(command, "id")?;
            let value = entry(root, id)?;
            let jobs = runner.jobs.lock().map_err(|_| "Downloads are busy.")?;
            let work = jobs.get(id);
            let mut event = json!({"op":"downloads_event","id":id});
            if command["op"] == "downloads_runner_pause" {
                let paused = value["state"] == "paused";
                if command["paused"]
                    .as_bool()
                    .is_some_and(|desired| desired == paused)
                {
                    drop(jobs);
                    return snapshot(root, &runner);
                }
                if paused && preferences(root)["wifiOnly"] == true && !transfer::wifi_connected() {
                    return Err("This download is waiting for Wi-Fi.".into());
                }
                event["event"] = json!(if paused { "resume" } else { "pause" });
                event["retained"] =
                    json!(paused && work.is_some_and(|w| w.transfer.load(Ordering::Acquire)));
                store::dispatch(&event, root)?;
                if work.is_some_and(|w| w.transfer.load(Ordering::Acquire)) {
                    let _ = transfer::dispatch(
                        &json!({"op":"downloads_transfer_pause","id":id,"paused":!paused}),
                        root,
                    );
                }
            } else {
                event["event"] = json!(if command["op"] == "downloads_runner_retry" {
                    "retry"
                } else {
                    "cancel"
                });
                store::dispatch(&event, root)?;
                if command["op"] == "downloads_runner_cancel"
                    && let Some(work) = work
                {
                    work.cancelled.store(true, Ordering::Release);
                    let _ = transfer::dispatch(
                        &json!({"op":"downloads_transfer_cancel","id":id}),
                        root,
                    );
                }
            }
        }
        "downloads_runner_remove" => {
            let ids = command["ids"]
                .as_array()
                .filter(|v| v.len() <= 2000)
                .ok_or("Invalid download selection.")?;
            for id in ids {
                let id = id.as_str().ok_or("Invalid download identity.")?;
                let value = entry(root, id)?;
                if !matches!(
                    value["state"].as_str(),
                    Some("complete" | "cancelled" | "failed" | "skipped")
                ) {
                    store::dispatch(
                        &json!({"op":"downloads_event","id":id,"event":"cancel"}),
                        root,
                    )?;
                }
                if let Some(work) = runner
                    .jobs
                    .lock()
                    .map_err(|_| "Downloads are busy.")?
                    .get(id)
                {
                    work.delete_media
                        .store(command["deleteMedia"] != false, Ordering::Release);
                    work.cancelled.store(true, Ordering::Release);
                    let _ = transfer::dispatch(
                        &json!({"op":"downloads_transfer_cancel","id":id}),
                        root,
                    );
                }
                runner
                    .removals
                    .lock()
                    .map_err(|_| "Downloads are busy.")?
                    .insert(id.into(), command["deleteMedia"] != false);
            }
        }
        "downloads_runner_uri" | "downloads_runner_export" => {
            let value = entry(root, string(command, "id")?)?;
            let name = command["file"].as_str().unwrap_or(string(&value, "file")?);
            if name != value["file"].as_str().unwrap_or("")
                && !value["artifacts"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|a| a["file"] == name))
            {
                return Err("This artifact is no longer available.".into());
            }
            if command["op"] == "downloads_runner_uri" {
                return Ok(json!(uri(&file(name)?)?));
            }
            let export_name = super::publication::name(&value, name)?;
            let folder = PathBuf::from(platform()?.export_directory());
            if !folder.is_absolute() {
                return Err("Export storage is unavailable.".into());
            }
            fs::create_dir_all(&folder).map_err(|_| "Could not prepare file sharing.")?;
            let destination = folder.join(&export_name);
            if destination.is_symlink() {
                return Err("Invalid export file.".into());
            }
            fs::copy(file(name)?, &destination).map_err(|_| "The saved file is missing.")?;
            return Ok(json!({"uri":uri(&destination)?,"name":export_name}));
        }
        _ => return Err("Unknown download action.".into()),
    }
    runner.wake.notify_all();
    crate::changed("downloads");
    snapshot(root, &runner)
}
