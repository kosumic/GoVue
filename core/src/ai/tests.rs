use super::*;
use crate::poison;

pub(super) fn job() -> Arc<Job> {
    Arc::new(Job {
        id: random().unwrap(),
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
    poison(&response.state);
    response.start(200);
    response.failed();
    assert!(response.is_cancelled());
    assert!(!response.push(b"provider output".to_vec()));
    assert!(response.state.is_poisoned());
}

#[test]
fn poisoned_job_state_is_rejected_by_polling_and_finalization() {
    let job = job();
    jobs().lock().unwrap().insert(job.id.clone(), job.clone());
    poison(&job.state);
    assert!(job.pending().is_err());
    assert!(job.publish_activity().is_err());
    let replies: Vec<Value> = ["ai_poll", "ai_chat_poll", "ai_cancel", "ai_forget"]
        .into_iter()
        .map(|op| {
            serde_json::from_str(&ai_execute(
                json!({"op":op,"taskId":job.id}).to_string(),
                String::new(),
            ))
            .unwrap()
        })
        .collect();
    jobs().lock().unwrap().remove(&job.id);
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
    poison(&job.activity);
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
