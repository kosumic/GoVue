//! A bounded, model-maintained checklist, independent of provider framing.
use super::Job;
use crate::{Result, string};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    explanation: Option<String>,
    plan: Vec<Step>,
}

pub(super) fn valid(steps: &[Step], explanation: Option<&str>) -> bool {
    (1..=8).contains(&steps.len())
        && steps
            .iter()
            .all(|s| !s.step.trim().is_empty() && s.step.len() <= 240)
        && steps
            .iter()
            .filter(|s| s.status == StepStatus::InProgress)
            .count()
            <= 1
        && explanation.is_none_or(|s| s.len() <= 1000)
}

pub(super) fn update(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let raw = string(call, "arguments")?;
    if raw.len() > 6000 {
        return Err("The plan is too large. Use at most eight short steps.".into());
    }
    let update: Update = serde_json::from_str(raw).map_err(|_| "Invalid plan update.")?;
    if !valid(&update.plan, update.explanation.as_deref()) {
        return Err("Use one to eight short steps and at most one in_progress step.".into());
    }
    job.pending()?;
    job.activity
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?
        .plan(update.plan, update.explanation)?;
    Ok(json!({"message":"Plan updated."}))
}

pub(super) fn tool() -> Value {
    json!({"name":"update_plan","description":"Maintain a short checklist for substantial work. Send the full plan on each update. Mark steps completed only when the work has actually finished. This updates the displayed plan; it does not perform the work.",
        "parameters":{"type":"object","properties":{
            "explanation":{"type":"string","description":"Optional brief explanation of a change in approach."},
            "plan":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","properties":{
                "step":{"type":"string","description":"A short, concrete step."},
                "status":{"type":"string","enum":["pending","in_progress","completed"]}
            },"required":["step","status"],"additionalProperties":false}}
        },"required":["plan"],"additionalProperties":false}})
}

pub(super) const INSTRUCTIONS: &str = r"
Keep the user informed while you work. Before substantial work such as generating a game, send a short assistant message explaining what you will make and your first step, then continue working in the same turn. Give concise updates at meaningful milestones or when your approach changes. Say what you have learned or what you are about to do. Do not narrate private reasoning, repeat generic thinking messages, invent progress, or stop after announcing work.
For a new game or other work with several meaningful steps, call update_plan early with a short checklist, before generating the full game. Keep at most one step in_progress. Update the plan when real work finishes or the approach changes, and finish it before the final response. The host displays the checklist, so do not repeat it in chat. Skip the plan for simple questions and small follow-ups. Do not add unnecessary tool calls just to report progress. A plan update does not execute or validate anything. Only report a game as created after create_interactive_game succeeds. If creation fails, correct the problem before marking that step completed.
";
