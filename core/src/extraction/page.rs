//! The live page supplies caption bytes; Rust owns selection, parsing and storage.
use super::{Job, json_response, text};
use crate::{MAX_BYTES, Result, Track, string, video_id};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

struct Page {
    adapter: String,
    video: String,
}
static PAGES: OnceLock<Mutex<HashMap<String, Page>>> = OnceLock::new();
pub(super) const STAGE: &str = "page_captions";
pub(super) const CHANNEL: &str = "extraction-page";
pub(super) const TIMEOUT: Duration = Duration::from_secs(12);

pub(super) fn register(command: &Value, root: &str) -> Result<Value> {
    let adapter = string(command, "adapterId")?;
    if adapter.is_empty() || adapter.len() > 150 || adapter.chars().any(char::is_control) {
        return Err("Invalid caption page identity.".into());
    }
    let mut pages = PAGES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Caption page unavailable.")?;
    if let Some(video) = command["videoId"].as_str() {
        if !video_id(video) || pages.len() >= 8 && !pages.contains_key(root) {
            return Err("Invalid caption page registration.".into());
        }
        pages.insert(
            root.into(),
            Page {
                adapter: adapter.into(),
                video: video.into(),
            },
        );
    } else if pages.get(root).is_some_and(|page| page.adapter == adapter) {
        pages.remove(root);
    }
    drop(pages);
    Ok(Value::Null)
}

pub(super) fn captions(
    job: &Job,
    root: &str,
    video: &str,
    track: &Track,
) -> Result<Option<(Vec<crate::Segment>, crate::captions::Metadata)>> {
    let adapter = PAGES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Caption page unavailable.")?
        .get(root)
        .filter(|page| page.video == video)
        .map(|page| page.adapter.clone());
    let Some(adapter) = adapter else {
        return Ok(None);
    };
    let raw = job.evaluate(json!({"stage":STAGE,"adapterId":adapter,"videoId":video,
        "trackId":track.id,"sourceTrackId":track.translated_from.as_deref().unwrap_or(&track.id),
        "language":track.language,"automatic":track.automatic,"label":track.label}))?;
    let reply = json_response(&raw)?;
    let source = string(&reply, "source")?;
    let parsed = match reply["format"].as_str() {
        Some("timedtext") => crate::parse_captions(source)?,
        Some("panel") if track.translated_from.is_none() => panel(source, &track.label)?,
        _ => return Err("Unsupported page caption format.".into()),
    };
    if parsed.0.is_empty() {
        return Err("YouTube's page returned no caption text.".into());
    }
    Ok(Some(parsed))
}

fn panel(source: &str, label: &str) -> Result<(Vec<crate::Segment>, crate::captions::Metadata)> {
    let data = json_response(source)?;
    let mut queue = vec![&data];
    let mut events = Vec::new();
    let mut selected = None;
    let mut visited = 0;
    while let Some(value) = queue.pop() {
        visited += 1;
        if visited > 100_000 {
            return Err("Transcript panel is too large.".into());
        }
        if let Some(segment) = value.get("transcriptSegmentRenderer") {
            let start =
                crate::number(&segment["startMs"]).ok_or("Invalid transcript timestamp.")?;
            let end = crate::number(&segment["endMs"]).ok_or("Invalid transcript timestamp.")?;
            if !start.is_finite() || start < 0.0 || !end.is_finite() || end < start {
                return Err("Invalid transcript timestamp.".into());
            }
            events.push(json!({"tStartMs":start,"dDurationMs":end-start,"segs":[{"utf8":text(&segment["snippet"])}]}));
            if events.len() > crate::MAX_SEGMENTS {
                return Err("Too many transcript segments.".into());
            }
        }
        if let Some(items) = value.get("subMenuItems").and_then(Value::as_array) {
            selected = items
                .iter()
                .find(|item| item["selected"] == true)
                .map(|item| text(&item["title"]))
                .or(selected);
        }
        match value {
            Value::Object(items) => queue.extend(items.values()),
            Value::Array(items) => queue.extend(items),
            _ => {}
        }
    }
    if selected
        .as_deref()
        .is_none_or(|selected| crate::clean(selected) != crate::clean(label))
    {
        return Err("The transcript panel selected another caption language.".into());
    }
    crate::parse_captions(&json!({"events":events}).to_string())
}

pub(super) fn reply(command: &Value, root: &str, job: &Job) -> Result<Value> {
    let raw = string(command, "raw")?;
    if raw.len() > MAX_BYTES {
        return Err("Caption page response is too large.".into());
    }
    let reply = json_response(raw)?;
    let mut runtime = job
        .runtime
        .lock()
        .map_err(|_| "Caption page unavailable.")?;
    let task = runtime
        .task
        .as_ref()
        .filter(|task| task["stage"] == STAGE)
        .ok_or("Caption page request expired.")?;
    let origin = string(command, "origin")?;
    let pages = PAGES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Caption page unavailable.")?;
    // Android WebMessageListener reports the origin, while iOS and older
    // Android bridges report the full document URL. The registered adapter
    // and pending task below correlate either form with the current video.
    if !crate::trusted_origin(origin, string(task, "videoId")?)
        || reply["type"] != "captionPage"
        || reply["requestId"] != command["requestId"]
        || reply["taskId"] != task["taskId"]
        || reply["videoId"] != task["videoId"]
        || reply["trackId"] != task["trackId"]
        || pages
            .get(root)
            .is_none_or(|page| task["adapterId"] != page.adapter || task["videoId"] != page.video)
    {
        return Err("Caption page response belongs to another request or video.".into());
    }
    drop(pages);
    let result = if reply["ok"] == true {
        let source = string(&reply, "source")?;
        if source.is_empty() || source.len() > MAX_BYTES {
            return Err("Invalid page caption source.".into());
        }
        Ok(json!({"source":source,"format":reply["format"]}).to_string())
    } else {
        Err("YouTube's page could not retrieve this caption track.".into())
    };
    runtime.task = None;
    runtime.reply = Some(result);
    drop(runtime);
    job.wake.notify_all();
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::{JOBS, Runtime, Session, UA, dispatch, runtime_tasks};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar};
    use std::thread;
    use std::time::Instant;

    const VIDEO: &str = "abcdefghijk";
    const LABEL: &str = "English (auto-generated)";

    fn panel_data(label: &str) -> String {
        json!({"body":{"transcriptSegmentListRenderer":{"initialSegments":[
            {"transcriptSegmentRenderer":{"startMs":"1500","endMs":"3500","snippet":{"runs":[{"text":"Hello "},{"text":"there"}]}}},
            {"transcriptSegmentRenderer":{"startMs":"4000","endMs":"5000","snippet":{"simpleText":"Next line"}}}
        ]}},"footer":{"sortFilterSubMenuRenderer":{"subMenuItems":[{"selected":true,"title":label}]}}}).to_string()
    }

    fn job(task: Option<Value>) -> Job {
        Job {
            at: Instant::now(),
            prepared_video: None,
            user_agent: UA.into(),
            session: Mutex::new(Session {
                cookies: HashMap::new(),
                since: Instant::now(),
            }),
            cancelled: AtomicBool::new(false),
            result: Mutex::new(None),
            runtime: Mutex::new(Runtime {
                sequence: 1,
                task,
                reply: None,
            }),
            wake: Condvar::new(),
        }
    }

    #[test]
    fn panel_parsing_preserves_timestamps_and_rejects_another_language() {
        let (segments, _) = panel(&panel_data(LABEL), LABEL).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start, 1.5);
        assert_eq!(segments[0].duration, 2.0);
        assert_eq!(segments[0].text, "Hello there");
        assert_eq!(segments[1].start, 4.0);
        assert!(panel(&panel_data("French (auto-generated)"), LABEL).is_err());
        assert!(panel(&panel_data(LABEL).replace("3500", "1000"), LABEL).is_err());
    }

    #[test]
    fn page_replies_require_the_current_origin_video_track_adapter_and_task() {
        let root = format!("caption-page-boundary-{}", std::process::id());
        register(&json!({"adapterId":"page:1","videoId":VIDEO}), &root).unwrap();
        let pending =
            json!({"stage":STAGE,"adapterId":"page:1","videoId":VIDEO,"trackId":"a.en","taskId":7});
        let job = job(Some(pending.clone()));
        let payload = json!({"type":"captionPage","requestId":"worker:page","taskId":7,"videoId":VIDEO,
            "trackId":"a.en","ok":true,"format":"panel","source":panel_data(LABEL)});
        let command = json!({"requestId":"worker:page","origin":format!("https://m.youtube.com/watch?v={VIDEO}"),"raw":payload.to_string()});
        for origin in [
            format!("https://youtube.com.evil.test/watch?v={VIDEO}"),
            format!("http://m.youtube.com/watch?v={VIDEO}"),
            "https://m.youtube.com:8443".into(),
            "https://user@m.youtube.com".into(),
            format!("https://m.youtube.com/?v={VIDEO}"),
            "https://m.youtube.com/watch?v=0123456789a".into(),
        ] {
            let mut wrong = command.clone();
            wrong["origin"] = json!(origin);
            assert!(reply(&wrong, &root, &job).is_err());
        }
        for (key, value) in [
            ("requestId", json!("worker:old")),
            ("taskId", json!(6)),
            ("videoId", json!("0123456789a")),
            ("trackId", json!("a.fr")),
        ] {
            let mut wrong = payload.clone();
            wrong[key] = value;
            let mut request = command.clone();
            request["raw"] = json!(wrong.to_string());
            assert!(reply(&request, &root, &job).is_err());
        }
        register(&json!({"adapterId":"page:2","videoId":VIDEO}), &root).unwrap();
        register(&json!({"adapterId":"page:1","videoId":null}), &root).unwrap();
        assert!(
            reply(&command, &root, &job).is_err(),
            "an old document cannot complete a replacement document's work"
        );
        register(&json!({"adapterId":"page:1","videoId":VIDEO}), &root).unwrap();
        reply(&command, &root, &job).unwrap();
        assert!(job.runtime.lock().unwrap().task.is_none());
        assert!(
            reply(&command, &root, &job).is_err(),
            "a reply is consumed once"
        );
        for origin in ["https://m.youtube.com", "https://m.youtube.com/"] {
            {
                let mut runtime = job.runtime.lock().unwrap();
                runtime.task = Some(pending.clone());
                runtime.reply = None;
            }
            let mut android = command.clone();
            android["origin"] = json!(origin);
            reply(&android, &root, &job).unwrap();
            assert!(job.runtime.lock().unwrap().task.is_none());
            assert!(reply(&android, &root, &job).is_err());
        }
        register(&json!({"adapterId":"page:1","videoId":null}), &root).unwrap();
    }

    #[test]
    fn caption_work_stays_out_of_the_isolated_token_runtime() {
        for (id, language, automatic, translated_from) in [
            ("a.en", "en", true, None),
            (".en", "en", false, None),
            ("translate:fr:a.en", "fr", true, Some("a.en")),
        ] {
            let label = if automatic { LABEL } else { "English" };
            let root = format!("caption-page-workflow-{}-{id}", std::process::id());
            let request = "worker:page-protocol";
            register(&json!({"adapterId":"page:workflow","videoId":VIDEO}), &root).unwrap();
            let worker = Arc::new(job(None));
            JOBS.get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert((root.clone(), request.into()), worker.clone());
            let track: Track = serde_json::from_value(json!({"id":id,"language":language,
                "label":label,"automatic":automatic,"translatedFrom":translated_from}))
            .unwrap();
            let (store, background) = (root.clone(), worker);
            let thread = thread::spawn(move || captions(&background, &store, VIDEO, &track));
            let deadline = Instant::now() + Duration::from_secs(2);
            let task = loop {
                let tasks = runtime_tasks(&root, true).unwrap();
                if let Some(task) = tasks["tasks"].as_array().unwrap().first() {
                    break task["task"].clone();
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            };
            assert_eq!(task["trackId"], id);
            assert_eq!(task["sourceTrackId"], translated_from.unwrap_or(id));
            assert_eq!(task["language"], language);
            assert!(
                runtime_tasks(&root, false).unwrap()["tasks"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert!(
                dispatch(&json!({"op":"extract_poll","requestId":request}), &root).unwrap()["task"]
                    .is_null()
            );
            let (format, source) = if translated_from.is_some() {
                (
                    "timedtext",
                    json!({"events":[
                        {"tStartMs":1500,"dDurationMs":2000,"segs":[{"utf8":"Hello there"}]},
                        {"tStartMs":4000,"dDurationMs":1000,"segs":[{"utf8":"Next line"}]}
                    ]})
                    .to_string(),
                )
            } else {
                ("panel", panel_data(label))
            };
            let payload = json!({"type":"captionPage","requestId":request,"taskId":task["taskId"],"videoId":VIDEO,
            "trackId":task["trackId"],"ok":true,"format":format,"source":source});
            dispatch(&json!({"op":"extract_page","requestId":request,"origin":format!("https://m.youtube.com/watch?v={VIDEO}"),"raw":payload.to_string()}),&root).unwrap();
            let (segments, _) = thread.join().unwrap().unwrap().unwrap();
            assert_eq!(segments.len(), 2);
            assert_eq!(segments[0].text, "Hello there");
            JOBS.get()
                .unwrap()
                .lock()
                .unwrap()
                .remove(&(root.clone(), request.into()));
            register(&json!({"adapterId":"page:workflow","videoId":null}), &root).unwrap();
        }
    }
}
