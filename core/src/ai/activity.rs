//! Ordered, display-safe chat activity. Provider arguments and continuations stay private.
use super::{MAX_OUTPUT, plan, usage, valid_id};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BLOCKS: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Commentary,
    FinalAnswer,
    PartialAnswer,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Status {
    Preparing,
    Running,
    Completed,
    Failed,
    Stopped,
}
impl Status {
    fn active(self) -> bool {
        matches!(self, Self::Preparing | Self::Running)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Block {
    Text {
        id: String,
        content: String,
        status: Status,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<Phase>,
    },
    Reasoning {
        id: String,
        content: String,
        status: Status,
    },
    Plan {
        id: String,
        steps: Vec<plan::Step>,
        #[serde(skip_serializing_if = "Option::is_none")]
        explanation: Option<String>,
        status: Status,
    },
    Tool {
        id: String,
        call_id: String,
        name: String,
        status: Status,
        started_at: u64,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        game_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}
impl Block {
    fn status_mut(&mut self) -> &mut Status {
        match self {
            Self::Text { status, .. }
            | Self::Reasoning { status, .. }
            | Self::Plan { status, .. }
            | Self::Tool { status, .. } => status,
        }
    }
    fn close(&mut self, next: Status) {
        // Ending a response cannot claim unfinished model work is complete.
        let next = if next == Status::Completed
            && matches!(self, Self::Plan { steps, .. } if steps.iter().any(|s| s.status != plan::StepStatus::Completed))
        {
            Status::Stopped
        } else {
            next
        };
        let status = self.status_mut();
        if status.active() {
            *status = next;
            if let Self::Tool {
                started_at,
                duration_ms,
                ..
            } = self
            {
                *duration_ms = milliseconds().saturating_sub(*started_at);
            }
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Text {
                id,
                content,
                status,
                ..
            }
            | Self::Reasoning {
                id,
                content,
                status,
            } => valid_id(id) && content.len() <= MAX_OUTPUT && *status != Status::Preparing,
            Self::Plan {
                id,
                steps,
                explanation,
                status,
            } => {
                valid_id(id)
                    && plan::valid(steps, explanation.as_deref())
                    && *status != Status::Preparing
                    && (*status != Status::Completed
                        || steps
                            .iter()
                            .all(|s| s.status == plan::StepStatus::Completed))
            }
            Self::Tool {
                id,
                call_id,
                name,
                started_at,
                duration_ms,
                game_id,
                title,
                error,
                ..
            } => {
                valid_id(id)
                    && !call_id.is_empty()
                    && call_id.len() <= 200
                    && name.len() <= 100
                    && *started_at <= usage::MAX_COUNT
                    && *duration_ms <= usage::MAX_COUNT
                    && game_id.as_ref().is_none_or(|id| valid_id(id))
                    && title.as_ref().is_none_or(|s| s.len() <= 1000)
                    && error.as_ref().is_none_or(|s| s.len() <= 2000)
            }
        }
    }
}

pub(super) fn valid(blocks: &[Block]) -> bool {
    blocks.len() <= MAX_BLOCKS
        && blocks.iter().all(Block::valid)
        && blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { content, .. } | Block::Reasoning { content, .. } => {
                    Some(content.len())
                }
                Block::Tool { .. } | Block::Plan { .. } => None,
            })
            .sum::<usize>()
            <= MAX_OUTPUT * 2
}
pub(super) fn settle(blocks: &mut [Block], status: Status) {
    for block in blocks {
        block.close(status);
    }
}
fn milliseconds() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

#[derive(Default)]
pub(super) struct Tape {
    pub blocks: Vec<Block>,
}
impl Tape {
    pub fn plan(&mut self, steps: Vec<plan::Step>, explanation: Option<String>) -> Result<()> {
        let status = if steps
            .iter()
            .all(|s| s.status == plan::StepStatus::Completed)
        {
            Status::Completed
        } else {
            Status::Running
        };
        if let Some(Block::Plan {
            steps: saved,
            explanation: note,
            status: current,
            ..
        }) = self
            .blocks
            .iter_mut()
            .find(|b| matches!(b, Block::Plan { .. }))
        {
            *saved = steps;
            *note = explanation;
            *current = status;
        } else {
            self.close_output();
            self.room()?;
            self.blocks.push(Block::Plan {
                id: format!("activity-{}", self.blocks.len()),
                steps,
                explanation,
                status,
            });
        }
        Ok(())
    }
    pub fn close_output(&mut self) {
        if let Some(block @ (Block::Text { .. } | Block::Reasoning { .. })) = self.blocks.last_mut()
        {
            block.close(Status::Completed);
        }
    }
    pub fn phase(&mut self, phase: Option<Phase>) {
        if phase.is_some()
            && let Some(Block::Text { phase: saved, .. }) = self.blocks.last_mut()
        {
            *saved = phase;
        }
    }
    pub fn output(&mut self, reasoning: bool, delta: &str, phase: Option<Phase>) -> Result<()> {
        if delta.is_empty() {
            return Ok(());
        }
        let same = match (self.blocks.last(), reasoning) {
            (
                Some(Block::Text {
                    status: Status::Running,
                    phase: saved,
                    ..
                }),
                false,
            ) => *saved == phase,
            (
                Some(Block::Reasoning {
                    status: Status::Running,
                    ..
                }),
                true,
            ) => true,
            _ => false,
        };
        if !same {
            self.close_output();
            self.room()?;
            let id = format!("activity-{}", self.blocks.len());
            self.blocks.push(if reasoning {
                Block::Reasoning {
                    id,
                    content: String::new(),
                    status: Status::Running,
                }
            } else {
                Block::Text {
                    id,
                    content: String::new(),
                    status: Status::Running,
                    phase,
                }
            });
        }
        if let Some(Block::Text { content, .. } | Block::Reasoning { content, .. }) =
            self.blocks.last_mut()
        {
            if content.len() + delta.len() > MAX_OUTPUT {
                return Err("Provider response is too large.".into());
            }
            content.push_str(delta);
        }
        Ok(())
    }
    pub fn prepare(&mut self, call_id: &str, name: &str) -> Result<()> {
        if call_id.is_empty() {
            return Ok(());
        } // Some providers send identity after the first delta.
        if call_id.len() > 200 || name.len() > 100 {
            return Err("Invalid tool identity.".into());
        }
        if let Some(Block::Tool { name: saved, .. }) = self.blocks.iter_mut().rev().find(|b|
            matches!(b, Block::Tool { call_id: id, status: Status::Preparing | Status::Running, .. } if id == call_id)) {
            *saved = name.into();
            return Ok(());
        }
        self.close_output();
        self.room()?;
        self.blocks.push(Block::Tool {
            id: format!("activity-{}", self.blocks.len()),
            call_id: call_id.into(),
            name: name.into(),
            status: Status::Preparing,
            started_at: milliseconds(),
            duration_ms: 0,
            game_id: None,
            title: None,
            error: None,
        });
        Ok(())
    }
    pub fn running(&mut self, call_id: &str, name: &str) -> Result<()> {
        self.prepare(call_id, name)?;
        if let Some(Block::Tool { status, .. }) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, Block::Tool { call_id: id, .. } if id == call_id))
        {
            *status = Status::Running;
        }
        Ok(())
    }
    pub fn result(&mut self, call_id: &str, result: &Result<Value>) {
        let created = self.blocks.iter().rev().find(|block| matches!(block,
            Block::Tool { call_id: id, .. } if id == call_id))
            .is_some_and(|block| matches!(block, Block::Tool { name, .. } if name == "create_interactive_game"));
        let game = result
            .as_ref()
            .ok()
            .filter(|_| created)
            .and_then(|v| v["gameId"].as_str());
        let linked = game.is_some_and(|id| {
            self.blocks
                .iter()
                .any(|b| matches!(b, Block::Tool { game_id: Some(saved), .. } if saved == id))
        });
        if let Some(block) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, Block::Tool { call_id: id, .. } if id == call_id))
        {
            block.close(if result.is_ok() {
                Status::Completed
            } else {
                Status::Failed
            });
            if let Block::Tool {
                game_id,
                title,
                error,
                ..
            } = block
            {
                *game_id = game.filter(|_| !linked).map(str::to_owned);
                *title = result
                    .as_ref()
                    .ok()
                    .and_then(|v| v["title"].as_str())
                    .map(str::to_owned);
                *error = result.as_ref().err().map(ToString::to_string);
            }
        }
    }
    fn room(&self) -> Result<()> {
        if self.blocks.len() >= MAX_BLOCKS {
            Err("This response has too many activity steps.".into())
        } else {
            Ok(())
        }
    }
}
