//! AI worker ownership, cancellation, capacity and terminal activity.
use super::{activity, random, replay, updates};
use crate::{Result, json};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

static JOBS: OnceLock<Mutex<HashMap<String, Arc<Job>>>> = OnceLock::new();

pub(super) struct Job {
    pub(super) id: String,
    pub(super) owner: String,
    pub(super) cancelled: AtomicBool,
    pub(super) state: Mutex<Value>,
    pub(super) activity: Mutex<activity::Tape>,
    pub(super) updates: Mutex<updates::Cursor>,
    pub(super) notifications: updates::Notifier,
    pub(super) conversation: OnceLock<(PathBuf, String)>,
    pub(super) last_status: AtomicI32,
}
impl Job {
    pub(super) fn publish_activity(&self) -> Result<()> {
        let blocks = json!(
            self.activity
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                .blocks
        );
        self.state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?["blocks"] = blocks;
        self.notifications.changed();
        Ok(())
    }
    pub(super) fn pending(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err("Request cancelled.".into())
        } else if self.state.is_poisoned()
            || self.activity.is_poisoned()
            || self.updates.is_poisoned()
        {
            Err("AI state is unavailable. Restart GoVue.".into())
        } else {
            Ok(())
        }
    }
    pub(super) fn finish(&self, result: Result<Value>) {
        let settled = (|| -> Result<()> {
            activity::settle(
                &mut self
                    .activity
                    .lock()
                    .map_err(|_| "AI state is unavailable. Restart GoVue.")?
                    .blocks,
                if self.cancelled.load(Ordering::Relaxed) {
                    activity::Status::Stopped
                } else if result.is_ok() {
                    activity::Status::Completed
                } else {
                    activity::Status::Failed
                },
            );
            self.publish_activity()
        })();
        let result = settled.and(result);
        // Polling reports a poisoned state directly; a worker must not recover it.
        let Ok(mut s) = self.state.lock() else {
            self.notifications.finish();
            return;
        };
        if self.cancelled.load(Ordering::Relaxed) {
            s["status"] = json!("cancelled");
            drop(s);
            self.notifications.finish();
            return;
        }
        match result {
            Ok(v) => {
                s["status"] = json!("completed");
                s["result"] = v;
            }
            Err(e) => {
                s["status"] = json!("failed");
                s["error"] = json!(e);
            }
        }
        drop(s);
        self.notifications.finish();
    }
}
pub(super) fn jobs() -> &'static Mutex<HashMap<String, Arc<Job>>> {
    JOBS.get_or_init(Default::default)
}
pub(super) fn new_job(owner: &str) -> Result<Arc<Job>> {
    let mut all = jobs()
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    // Finished tasks are disposable; active tasks are never silently evicted.
    let mut running = 0;
    let mut finished = Vec::new();
    for (id, job) in all.iter() {
        if job
            .state
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?["status"]
            == "running"
        {
            running += 1;
        } else if all.len() >= 16 {
            finished.push(id.clone());
        }
    }
    if running >= 8 {
        return Err("Too many AI requests. Stop an existing request first.".into());
    }
    for id in finished {
        all.remove(&id);
    }
    let id = random()?;
    let job = Arc::new(Job {
        notifications: updates::Notifier::new(&id),
        id,
        owner: owner.into(),
        cancelled: AtomicBool::new(false),
        state: Mutex::new(json!({"status":"running","text":""})),
        activity: Mutex::new(activity::Tape::default()),
        updates: Mutex::new(updates::Cursor::default()),
        conversation: OnceLock::new(),
        last_status: AtomicI32::new(0),
    });
    all.insert(job.id.clone(), job.clone());
    drop(all);
    Ok(job)
}
pub(super) fn cancel_owner(id: &str) -> Result<()> {
    for job in jobs()
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .values()
    {
        if job.owner == id
            || job
                .state
                .lock()
                .map_err(|_| "AI state is unavailable. Restart GoVue.")?["providerId"]
                == id
        {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }
    replay::forget_provider(id)
}
