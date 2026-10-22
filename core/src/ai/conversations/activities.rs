//! Conversation game creation, progress, export and bounded audio.
use super::storage::CHATS;
use super::{commands, library, storage};
use crate::Result;
use crate::ai::jobs::Job;
use crate::ai::{chat_sessions, games, identity, practice};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

/// Commit before continuing the model so a failed final reply cannot lose a game.
pub(in crate::ai) fn create_game(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let (root, id) = job
        .conversation
        .get()
        .ok_or("Games require a saved conversation.")?;
    let call_id = crate::string(call, "id")?;
    if call_id.is_empty() || call_id.len() > 200 {
        return Err("Invalid tool call identity.".into());
    }
    let definition: games::Definition = serde_json::from_str(crate::string(call, "arguments")?)
        .map_err(|_| "Invalid game arguments. Supply all fields described by the tool schema.")?;
    let _guard = CHATS
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    job.pending()?;
    let mut chat = storage::read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This game request is no longer active.".into());
    }
    let transcript = if let Some(reference) = &chat.transcript {
        let _guard = crate::REQUEST_LOCK
            .lock(c"GoVue core request: chat.create_game")
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        Some(crate::transcript::load(
            root.to_str().ok_or("Invalid storage path.")?,
            &reference.video_id,
            &reference.track_id,
        )?)
    } else {
        None
    };
    let reply = chat
        .messages
        .last_mut()
        .ok_or("Missing assistant message.")?;
    if let Some(game) = reply.games.iter().find(|game| game.call_id == call_id) {
        return Ok(json!({"gameId":game.id,"title":game.definition.title,"status":"created"}));
    }
    if reply.games.len() >= 3 {
        return Err("At most three games can be created in one reply.".into());
    }
    let mut game = games::Game::create(definition, call_id, transcript.as_deref())?;
    if game.source.is_none() {
        game.source = chat.video_id.as_ref().map(|id| games::Source {
            video_id: id.clone(),
            title: chat.title.clone(),
            start: None,
            track_id: None,
            language: None,
        });
    }
    job.pending()?;
    let game = practice::capture(root, &game)?;
    let result = json!({"gameId":game.id,"title":game.definition.title,"status":"created","savedToPractice":true,"message":"The playable activity is in chat and saved in Practice in the sidebar. Briefly invite the user to play; do not repeat its HTML or answers."});
    reply.games.push(game);
    reply.blocks.clone_from(
        &job.activity
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?
            .blocks,
    );
    let snapshot = json!(reply.games);
    job.pending()?;
    storage::save(root, &chat)?;
    let mut state = job
        .state
        .lock()
        .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
    state["gameCommitted"] = json!(true);
    state["games"] = snapshot;
    drop(state);
    job.notifications.changed();
    Ok(result)
}

pub(in crate::ai) fn read_game_library(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let (root, _) = job
        .conversation
        .get()
        .ok_or("Games require a saved conversation.")?;
    job.pending()?;
    let mut command: Value = serde_json::from_str(crate::string(call, "arguments")?)
        .map_err(|_| "Game library arguments must be a JSON object.")?;
    let fields = command
        .as_object_mut()
        .ok_or("Game library arguments must be an object.")?;
    let op = match crate::string(call, "name")? {
        "list_interactive_games" => "ai_practice_catalog",
        "read_interactive_game" => "ai_practice_get",
        _ => return Err("Unknown game library tool.".into()),
    };
    fields.insert("op".into(), json!(op));
    commands::dispatch(command, root.clone())
}

pub(super) fn game_export(c: &Value, root: &Path) -> Result<Value> {
    let chat = storage::read(root, crate::string(c, "chatId")?)?;
    let game = chat
        .messages
        .iter()
        .filter(|m| m.id == c["messageId"])
        .flat_map(|m| &m.games)
        .find(|g| g.id == c["gameId"])
        .ok_or("This game is no longer available.")?;
    let source = game
        .source
        .as_ref()
        .map(|source| json!(source))
        .or_else(|| {
            chat.transcript
                .as_ref()
                .map(|reference| json!(reference))
                .or_else(|| chat.video_id.as_ref().map(|id| json!({"videoId":id})))
        });
    // Export the original recipe and frozen captions, never play history or
    // conversation/provider identities. A reused game starts from its rules.
    let text = serde_json::to_string_pretty(&json!({
        "format":"lingoplay-game", "version":1,
        "definition":game.definition, "source":source, "cues":game.cues,
    }))
    .map_err(|_| "Could not copy this game.")?;
    Ok(json!({"text":text}))
}

pub(super) fn game_action(c: &Value, root: &Path) -> Result<Value> {
    let mut chat = storage::read(root, crate::string(c, "chatId")?)?;
    let message = chat
        .messages
        .iter_mut()
        .find(|m| m.id == c["messageId"])
        .ok_or("This game message is no longer available.")?;
    let game = message
        .games
        .iter_mut()
        .find(|g| g.id == c["gameId"])
        .ok_or("This game is no longer available.")?;
    practice::capture(root, game)?;
    let result = practice::apply(root, c)?;
    *game = serde_json::from_value(result["game"].clone())
        .map_err(|_| "Could not restore game progress.")?;
    chat.updated_at = identity::now();
    storage::save(root, &chat)?;
    library::refresh_games(&chat)?;
    if result["notify"] == true {
        chat_sessions::feedback(
            root,
            &chat.id,
            crate::string(&result["game"], "id")?,
            result["game"]["revision"].as_u64().unwrap_or(0),
        )?;
    }
    Ok(result)
}

pub(super) fn game_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let chat = storage::read(root, crate::string(c, "chatId")?)?;
        let game = chat
            .messages
            .iter()
            .filter(|m| m.id == c["messageId"])
            .flat_map(|m| &m.games)
            .find(|g| g.id == c["gameId"])
            .ok_or("This game is no longer available.")?;
        let cue = game
            .cues
            .iter()
            .find(|cue| cue.id == c["cueId"])
            .ok_or("This game cannot play that caption.")?
            .clone();
        let video_id = game
            .source
            .as_ref()
            .map(|s| s.video_id.clone())
            .or_else(|| {
                chat.video_id
                    .or_else(|| chat.transcript.map(|t| t.video_id))
            })
            .ok_or("This game has no source video.")?;
        (video_id, cue)
    };
    if c["prefetch"] == true {
        games::audio::prefetch(root, &video_id, &cue, &c["catalog"])
    } else {
        games::audio::prepare(root, &video_id, &cue, &c["catalog"])
    }
}

pub(super) fn transcript_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS
            .lock()
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let chat = storage::read(root, crate::string(c, "chatId")?)?;
        let reference = chat.transcript.ok_or("This chat has no transcript.")?;
        if c["videoId"] != reference.video_id {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let track_id = crate::string(c, "trackId")?;
        let _guard = crate::REQUEST_LOCK
            .lock(c"GoVue core request: chat.transcript_audio")
            .map_err(|_| "AI state is unavailable. Restart GoVue.")?;
        let allowed = crate::transcript::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            &reference.track_id,
            |data| data.tracks.iter().any(|track| track.id == track_id),
        )?;
        if !allowed {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let cue = crate::transcript::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            track_id,
            |data| games::Cue::from_transcript(data, crate::string(c, "cueId")?),
        )??;
        (reference.video_id, cue)
    };
    if c["prefetch"] == true {
        games::audio::prefetch(root, &video_id, &cue, &c["catalog"])
    } else {
        games::audio::prepare(root, &video_id, &cue, &c["catalog"])
    }
}
