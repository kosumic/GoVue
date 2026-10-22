//! Playback policy and asynchronous ownership, independent of React and Expo Video.
use crate::{Result, json};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Session {
    video: String,
    playback: Value,
    preparation: Option<u64>,
    quality: Option<u64>,
    replacement: u64,
    sequence: u64,
    manual: bool,
    retry: Option<Instant>,
}
impl Session {
    fn next(&mut self) -> Result<u64> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or("Player session exhausted.")?;
        Ok(self.sequence)
    }
    fn cooling(&self) -> bool {
        self.retry.is_some_and(|until| Instant::now() < until)
    }
    fn invalidate(&mut self) -> Result<()> {
        self.quality = None;
        self.replacement = self.next()?;
        Ok(())
    }
}
static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub(crate) fn dispatch(command: &Value) -> Result<Value> {
    let op = crate::string(command, "op")?;
    let mut sessions = SESSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Player is busy.")?;
    if op == "player_session_create" {
        if sessions.len() >= 8 {
            return Err("Too many player sessions.".into());
        }
        let id = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
        return Ok(json!(id));
    }
    let id = command["sessionId"]
        .as_u64()
        .ok_or("Missing player session.")?;
    if op == "player_session_dispose" {
        sessions.remove(&id);
        return Ok(Value::Null);
    }
    if id == 0 || id > SEQUENCE.load(Ordering::Relaxed) {
        return Err("Invalid player session.".into());
    }
    let session = sessions.entry(id).or_default();
    let token = command["token"].as_u64();
    let result = match op {
        "player_session_select" => {
            let video = command["videoId"].as_str().unwrap_or("");
            if !video.is_empty() && !crate::video_id(video) {
                return Err("Invalid video identity.".into());
            }
            session.video = video.into();
            session.preparation = None;
            session.manual = false;
            session.retry = None;
            session.invalidate()?;
            Ok(json!(
                !session.playback.is_null() && session.playback["videoId"] != video
            ))
        }
        "player_session_preparing" => Ok(json!(session.preparation.is_some())),
        "player_session_current" => Ok(json!(token.is_some() && token == session.preparation)),
        "player_session_begin" => {
            if session.video.is_empty() || session.preparation.is_some() {
                return Ok(Value::Null);
            }
            let token = session.next()?;
            session.preparation = Some(token);
            session.manual = false;
            let settings = &command["settings"];
            let previous = session.playback["videoId"] == session.video;
            Ok(json!({"token":token,"videoId":session.video,
                "settings":if !previous {json!({
                    "playbackSpeed":if settings["rememberSpeed"]==true {settings["playbackSpeed"].clone()}else{json!(1)},
                    "resizeMode":if settings["rememberResize"]==true {settings["resizeMode"].clone()}else{json!("fit")}
                })}else{json!({})},
                "height":if previous {session.playback["qualityHeight"].clone()}else if settings["rememberQuality"]==true {settings["qualityHeight"].clone()}else{json!(0)},
                "fps":session.playback["qualityFps"].as_u64().filter(|n|previous && *n>0).or(settings["qualityFps"].as_u64()).unwrap_or(0)}))
        }
        "player_session_finish" => {
            let current = token.is_some() && token == session.preparation;
            if current {
                session.preparation = None;
            }
            Ok(json!(current))
        }
        "player_session_auto" => {
            let settings = &command["settings"];
            let allowed = !session.video.is_empty()
                && command["owner"] == session.video
                && command["ready"] == true
                && command["foreground"] == true
                && command["pending"] != true
                && !session.manual
                && session.playback.is_null()
                && session.preparation.is_none()
                && !session.cooling()
                && (settings["backgroundPlayback"] == true
                    || settings["adBlock"] == true
                    || settings["autoPip"] == true);
            if allowed {
                session.retry = Some(Instant::now() + Duration::from_secs(30));
            }
            Ok(json!(allowed))
        }
        "player_session_recover" => {
            let allowed = !session.playback.is_null()
                && session.playback["offline"] != true
                && session.playback["videoId"] == session.video
                && command["remote"] != true
                && session.preparation.is_none()
                && !session.cooling();
            if allowed {
                session.retry = Some(Instant::now() + Duration::from_secs(30));
            }
            Ok(json!(allowed))
        }
        "player_session_page_dead" => {
            session.retry = None;
            session.quality = None;
            Ok(Value::Null)
        }
        "player_session_quality_begin" => {
            if session.playback.is_null()
                || session.playback["offline"] == true
                || session.playback["videoId"] != session.video
            {
                return Ok(Value::Null);
            }
            let token = session.next()?;
            session.quality = Some(token);
            Ok(json!(token))
        }
        "player_session_quality_current" => Ok(json!(
            token.is_some()
                && token == session.quality
                && session.playback["videoId"] == session.video
        )),
        "player_session_quality_finish" => {
            let current = token.is_some() && token == session.quality;
            if current {
                session.quality = None;
            }
            Ok(json!(current))
        }
        "player_session_replace" => {
            session.replacement = session.next()?;
            Ok(json!(session.replacement))
        }
        "player_session_replace_current" => Ok(json!(token == Some(session.replacement))),
        "player_session_commit" => {
            let playback = &command["playback"];
            let video = crate::string(playback, "videoId")?;
            if !crate::video_id(video) {
                return Err("Invalid video identity.".into());
            }
            session.playback = json!({"videoId":video,"offline":playback["offline"]==true,
                "qualityHeight":playback["qualityHeight"],"qualityFps":playback["qualityFps"]});
            Ok(Value::Null)
        }
        "player_session_offline" => {
            session.preparation = None;
            session.manual = true;
            session.invalidate()?;
            Ok(Value::Null)
        }
        "player_session_close" => {
            session.preparation = None;
            session.playback = Value::Null;
            session.manual = command["manual"] == true;
            session.invalidate()?;
            Ok(Value::Null)
        }
        _ => Err("Unknown player session action.".into()),
    };
    drop(sessions);
    result
}
