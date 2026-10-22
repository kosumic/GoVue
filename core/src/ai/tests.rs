use super::{activity, updates};
use crate::ai::http::{AiResponse, ResponseState};
use crate::ai::jobs::Job;
use crate::ai::sse::Sse;
use crate::ai::{commands, identity, jobs};

use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicI32};
use std::sync::{Arc, Mutex, OnceLock};

pub(super) fn job() -> Arc<Job> {
    let id = identity::random().unwrap();
    Arc::new(Job {
        notifications: updates::Notifier::new(&id),
        id,
        owner: "fixture".into(),
        cancelled: AtomicBool::new(false),
        state: Mutex::new(json!({"status":"running","text":""})),
        activity: Mutex::new(activity::Tape::default()),
        updates: Mutex::new(updates::Cursor::default()),
        conversation: OnceLock::new(),
        last_status: AtomicI32::new(0),
    })
}

fn response(job: Option<Arc<Job>>) -> AiResponse {
    AiResponse {
        state: Mutex::new(ResponseState {
            status: 200,
            bytes: Vec::new(),
            stream: Some(Sse::new("responses", job.clone())),
            error: None,
            total: 0,
        }),
        job,
    }
}

#[test]
fn poisoned_response_stops_native_callbacks() {
    let response = response(None);
    crate::poison(&response.state);
    response.start(200);
    response.failed();
    assert!(response.is_cancelled());
    assert!(!response.push(b"provider output".to_vec()));
    assert!(response.state.is_poisoned());
}

#[test]
fn poisoned_job_state_is_rejected_by_polling_and_finalization() {
    let job = job();
    jobs::jobs()
        .lock()
        .unwrap()
        .insert(job.id.clone(), job.clone());
    crate::poison(&job.state);
    assert!(job.pending().is_err());
    assert!(job.publish_activity().is_err());
    let replies: Vec<Value> = ["ai_poll", "ai_chat_poll", "ai_cancel", "ai_forget"]
        .into_iter()
        .map(|op| {
            serde_json::from_str(&commands::ai_execute(
                json!({"op":op,"taskId":job.id}).to_string(),
                String::new(),
            ))
            .unwrap()
        })
        .collect();
    jobs::jobs().lock().unwrap().remove(&job.id);
    for reply in replies {
        assert_eq!(
            reply,
            json!({"ok":false,"error":"AI state is unavailable. Restart GoVue."})
        );
    }
    job.finish(Ok(json!({"text":"must not publish"})));
    assert!(job.state.is_poisoned());
}

#[test]
fn poisoned_activity_stops_output_and_finishes_with_an_error() {
    let job = job();
    crate::poison(&job.activity);
    let response = response(Some(job.clone()));
    assert!(response.is_cancelled());
    assert!(!response.push(b"provider output".to_vec()));
    let mut parser = Sse::new("responses", Some(job.clone()));
    assert!(
        parser
            .event(r#"{"type":"response.output_text.delta","delta":"must not publish"}"#)
            .is_err()
    );
    assert!(parser.text.is_empty());
    job.finish(Ok(json!({"text":"must not publish"})));
    let state = job.state.lock().unwrap().clone();
    assert_eq!(state["status"], "failed");
    assert_eq!(state["error"], "AI state is unavailable. Restart GoVue.");
    assert!(state["result"].is_null());
    assert!(job.activity.is_poisoned());
}

#[test]
fn progress_notifications_coalesce_and_terminal_states_publish_after_unlocking() {
    use std::sync::mpsc;
    use std::time::Duration;

    struct Events {
        jobs: Vec<Arc<Job>>,
        sender: mpsc::Sender<(String, Option<Value>)>,
    }
    impl crate::CoreEvents for Events {
        fn changed(&self, channel: String) {
            if let Some(job) = self
                .jobs
                .iter()
                .find(|job| channel == format!("ai:{}", job.id))
            {
                // Native consumers must be able to read the new state immediately.
                let state = job.state.try_lock().ok().map(|state| state.clone());
                let _ = self.sender.send((channel, state));
            }
        }
    }

    let running = job();
    let failed = job();
    let cancelled = job();
    let (sender, received) = mpsc::channel();
    crate::install_core_events(Box::new(Events {
        jobs: vec![running.clone(), failed.clone(), cancelled.clone()],
        sender,
    }));
    let response = response(Some(running.clone()));
    for _ in 0..20 {
        assert!(
            response.push(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n".to_vec()
            )
        );
    }
    let (channel, state) = received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(channel, format!("ai:{}", running.id));
    assert_eq!(state.unwrap()["text"], "x".repeat(20));
    assert!(
        received.try_recv().is_err(),
        "a burst emits one notification"
    );

    // A queued progress notification must not delay or duplicate completion.
    running.publish_activity().unwrap();
    running.finish(Ok(json!({})));
    assert_eq!(
        received.try_recv().unwrap().1.unwrap()["status"],
        "completed"
    );
    failed.finish(Err("Fixture failure".into()));
    let (_, state) = received.try_recv().unwrap();
    assert_eq!(state.as_ref().unwrap()["status"], "failed");
    assert_eq!(state.unwrap()["error"], "Fixture failure");
    cancelled
        .cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
    cancelled.finish(Ok(json!({})));
    assert_eq!(
        received.try_recv().unwrap().1.unwrap()["status"],
        "cancelled"
    );
    for job in [&running, &failed, &cancelled] {
        job.notifications.changed();
        job.notifications.finish();
    }
    assert!(received.recv_timeout(Duration::from_millis(250)).is_err());
}
