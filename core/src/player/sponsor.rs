//! Port of LiTube's SponsorSkipController (GPL-3.0).
use crate::{Result, string, video_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Deserialize, Serialize, PartialEq)]
struct Segment {
    start: f64,
    end: f64,
    category: String,
}
#[derive(Default)]
struct Session {
    video: String,
    segments: Vec<Segment>,
    suppressed: Vec<Segment>,
    retired: Vec<Segment>,
    pending: Option<Segment>,
    deadline: f64,
}
static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
pub(crate) fn control(command: &Value, root: &str) -> Result<Value> {
    let video = string(command, "videoId")?;
    if !video_id(video) {
        return Err("Invalid SponsorBlock video.".into());
    }
    let mut sessions = SESSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "SponsorBlock state is busy.")?;
    let session = sessions.entry(root.into()).or_default();
    let action = string(command, "action")?;
    if session.video != video {
        if action != "segments" {
            return Ok(
                json!({"countdown":null,"chip":false,"highlight":false,"target":null,"seek":null}),
            );
        }
        *session = Session {
            video: video.into(),
            ..Default::default()
        };
    }
    if action == "segments" {
        let segments: Vec<Segment> = serde_json::from_value(command["segments"].clone())
            .map_err(|_| "Invalid SponsorBlock segments.")?;
        if segments.len() > 10000
            || segments.iter().any(|s| {
                !s.start.is_finite()
                    || !s.end.is_finite()
                    || s.start < 0.0
                    || s.end < s.start
                    || s.end > 604800.0
            })
        {
            return Err("Invalid SponsorBlock segment range.".into());
        }
        session.segments = segments;
    }
    let position = command["position"].as_f64().unwrap_or(0.0);
    if !position.is_finite() || position < 0.0 {
        return Err("Invalid playback position.".into());
    }
    let inside = session
        .segments
        .iter()
        .find(|s| s.end > s.start && position >= s.start && position < s.end)
        .cloned();
    let highlight = session
        .segments
        .iter()
        .find(|s| {
            s.start == s.end
                && s.start > 0.0
                && !session.retired.contains(s)
                && position >= s.start - 15.0
                && position < s.start + 15.0
        })
        .cloned();
    let mut seek = None;
    let mut countdown = None;
    let mut chip = false;
    let mut is_highlight = false;
    let mut target = None;
    match action {
        "cancel" => {
            if let Some(p) = session.pending.take()
                && !session.suppressed.contains(&p)
            {
                session.suppressed.push(p);
            }
        }
        "seek" => {
            if let Some(s) = inside.clone()
                && !session.suppressed.contains(&s)
            {
                session.suppressed.push(s);
            }
            session.pending = None;
        }
        "skip" => {
            if let Some(s) = session.pending.take().or(inside.clone()) {
                seek = Some(s.end);
                if !session.suppressed.contains(&s) {
                    session.suppressed.push(s);
                }
            } else if let Some(s) = highlight.clone() {
                seek = Some(s.start);
                session.retired.push(s);
            }
        }
        "segments" | "tick" => {}
        _ => return Err("Invalid SponsorBlock action.".into()),
    }
    if seek.is_none() && action != "cancel" && action != "seek" {
        if let Some(inside) = inside {
            target = Some(inside.end);
            if session.suppressed.contains(&inside) {
                chip = true;
                session.pending = None;
            } else {
                if session.pending.as_ref() != Some(&inside) {
                    session.pending = Some(inside.clone());
                    session.deadline = position + 5.0;
                }
                if (command["countdown"] == false || position >= session.deadline)
                    && command["playing"] == true
                {
                    seek = Some(inside.end);
                    session.suppressed.push(inside);
                    session.pending = None;
                } else {
                    countdown = Some((session.deadline - position).ceil().max(0.0) as u32);
                }
            }
        } else {
            let upcoming = session
                .segments
                .iter()
                .filter(|s| s.end > s.start && position < s.start && position >= s.start - 5.0)
                .min_by(|a, b| a.start.total_cmp(&b.start))
                .cloned();
            if command["countdown"] != false
                && upcoming
                    .as_ref()
                    .is_some_and(|s| !session.suppressed.contains(s))
            {
                let s = upcoming.unwrap();
                session.deadline = s.start;
                countdown = Some((s.start - position).ceil().max(0.0) as u32);
                target = Some(s.end);
                session.pending = Some(s);
            } else {
                session.pending = None;
            }
            if let Some(s) = highlight
                && countdown.is_none()
            {
                chip = true;
                is_highlight = true;
                target = Some(s.start);
            }
        }
    }
    Ok(
        json!({"countdown":countdown,"chip":chip,"highlight":is_highlight,"target":target,"seek":seek,"category":session.pending.as_ref().map(|s|&s.category)}),
    )
}
