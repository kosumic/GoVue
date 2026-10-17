//! Local conversations and provider-independent turn construction.
use super::*;
use std::io::Read;

const MAX_CHAT: usize = 4_000_000;
const MAX_CHATS: usize = 100;
static CHATS: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptRef {
    video_id: String,
    track_id: String,
    title: String,
    language: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    id: String,
    role: String,
    content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    games: Vec<games::Game>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<activity::Block>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    usage: Value,
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider_label: String,
    #[serde(default)]
    reasoning_effort: String,
    #[serde(default)]
    provider_id: String,
    #[serde(default)]
    model_name: String,
    #[serde(default)]
    completed_at: u64,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Conversation {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_id: Option<String>,
    title: String,
    provider_id: String,
    provider_label: String,
    model: String,
    created_at: u64,
    updated_at: u64,
    transcript: Option<TranscriptRef>,
    messages: Vec<Message>,
    active_task: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err("Invalid conversation identity.".into());
    }
    Ok(root.join("ai/chats").join(format!("{id}.json")))
}
fn read(root: &Path, id: &str) -> Result<Conversation> {
    let file = fs::File::open(path(root, id)?).map_err(|_| "This chat is no longer available.")?;
    let mut bytes = Vec::new();
    file.take((MAX_CHAT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read this chat.")?;
    if bytes.len() > MAX_CHAT {
        return Err("Saved chat is too large.".into());
    }
    let chat: Conversation =
        serde_json::from_slice(&bytes).map_err(|_| "This saved chat is unreadable.")?;
    if chat.id != id
        || !(valid_id(&chat.provider_id) || chat.provider_id.is_empty() && chat.messages.is_empty())
        || chat
            .video_id
            .as_ref()
            .is_some_and(|id| !crate::video_id(id))
        || chat
            .video_id
            .as_ref()
            .is_some_and(|id| chat.transcript.as_ref().is_some_and(|t| &t.video_id != id))
        || chat.model.len() > 200
        || chat.title.len() > 1000
        || chat
            .reasoning_effort
            .as_ref()
            .is_some_and(|v| !catalog::valid_reasoning(v))
        || chat.messages.len() > 200
        || !chat.messages.len().is_multiple_of(2)
        || chat.messages.iter().enumerate().any(|(i, m)| {
            !valid_id(&m.id)
                || m.role != if i % 2 == 0 { "user" } else { "assistant" }
                || !matches!(
                    m.status.as_str(),
                    "completed" | "streaming" | "failed" | "stopped"
                )
                || m.content.len() > MAX_OUTPUT
                || m.games.len() > 3
                || m.games.iter().any(|game| !game.validate())
                || !activity::valid(&m.blocks)
                || m.reasoning.len() > MAX_OUTPUT
                || m.model.len() > 200
                || m.provider_label.len() > 100
                || !catalog::valid_reasoning(&m.reasoning_effort)
                || (!m.provider_id.is_empty() && !valid_id(&m.provider_id))
                || m.model_name.chars().count() > 200
                || m.completed_at > usage::MAX_COUNT
                || m.context_window.is_some_and(|n| n > usage::MAX_COUNT)
                || m.max_output_tokens.is_some_and(|n| n > usage::MAX_COUNT)
                || !usage::valid(&m.usage)
        })
    {
        return Err("This saved chat is invalid.".into());
    }
    Ok(chat)
}
fn save(root: &Path, chat: &Conversation) -> Result<()> {
    let bytes = serde_json::to_vec(chat).map_err(|_| "Could not encode this chat.")?;
    if bytes.len() > MAX_CHAT {
        return Err("This chat is full. Start a new chat to continue.".into());
    }
    let target = path(root, &chat.id)?;
    fs::create_dir_all(target.parent().unwrap()).map_err(|_| "Could not save chat history.")?;
    let temp = target.with_extension("tmp");
    fs::write(&temp, bytes)
        .and_then(|_| fs::rename(temp, target))
        .map_err(|_| "Could not save chat history.".into())
}
fn running(id: &str) -> bool {
    jobs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .is_some_and(|j| {
            !j.cancelled.load(Ordering::Relaxed)
                && j.state.lock().unwrap_or_else(|e| e.into_inner())["status"] == "running"
        })
}
fn recover(root: &Path, chat: &mut Conversation) -> Result<()> {
    if chat.active_task.as_deref().is_some_and(|id| !running(id)) {
        let partial = chat.active_task.as_ref().and_then(|id| {
            jobs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .map(|j| {
                    let state = j.state.lock().unwrap_or_else(|e| e.into_inner());
                    (
                        state["text"].as_str().unwrap_or_default().to_owned(),
                        state["reasoning"].as_str().unwrap_or_default().to_owned(),
                        j.activity
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .blocks
                            .clone(),
                    )
                })
        });
        chat.active_task = None;
        if let Some(m) = chat.messages.last_mut().filter(|m| m.status == "streaming") {
            if let Some((text, reasoning, blocks)) = partial {
                m.content = text;
                m.reasoning = reasoning;
                m.blocks = blocks;
            }
            activity::settle(&mut m.blocks, activity::Status::Stopped);
            m.status = "stopped".into();
            m.error = Some("Response interrupted. You can try again.".into());
        }
        save(root, chat)?;
    }
    Ok(())
}
fn list(root: &Path) -> Result<Vec<Conversation>> {
    let dir = root.join("ai/chats");
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Could not open chat history.".into()),
    };
    let mut rows = Vec::new();
    for entry in entries.take(MAX_CHATS * 2).flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(id) = p.file_stem().and_then(|s| s.to_str())
            && let Ok(mut chat) = read(root, id)
        {
            recover(root, &mut chat)?;
            rows.push(chat);
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    Ok(rows)
}
fn visible_chats(root: &Path) -> Result<Vec<Conversation>> {
    let rows = list(root)?;
    let bound: std::collections::HashSet<_> = rows
        .iter()
        .filter_map(|chat| chat.video_id.as_ref())
        .collect();
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .iter()
        .filter(|chat| {
            !(chat.video_id.is_none()
                && chat
                    .transcript
                    .as_ref()
                    .is_some_and(|t| bound.contains(&t.video_id)))
        })
        .filter(|chat| {
            let video = chat
                .video_id
                .as_ref()
                .or_else(|| chat.transcript.as_ref().map(|t| &t.video_id));
            video.is_none_or(|id| seen.insert(id.clone()))
        })
        .cloned()
        .collect())
}
fn caption(root: &Path, value: &Value) -> Result<Option<TranscriptRef>> {
    if value.is_null() {
        return Ok(None);
    }
    let id = string(value, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid transcript identity.".into());
    }
    let _g = crate::REQUEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let transcript = crate::load(
        root.to_str().ok_or("Invalid transcript storage path.")?,
        id,
        string(value, "trackId")?,
    )?;
    Ok(Some(TranscriptRef {
        video_id: transcript.video_id,
        track_id: transcript.track_id,
        title: transcript.title,
        language: transcript.language,
    }))
}

// CHATS serializes lookup and creation, including simultaneous opens on both entry points.
fn open_video(root: &Path, c: &Value) -> Result<Conversation> {
    let id = string(c, "videoId")?;
    if !crate::video_id(id) {
        return Err("Invalid video identity.".into());
    }
    let rows = list(root)?;
    let existing = rows
        .iter()
        .find(|chat| chat.video_id.as_deref() == Some(id))
        .or_else(|| {
            rows.iter().find(|chat| {
                chat.video_id.is_none()
                    && chat.transcript.as_ref().is_some_and(|t| t.video_id == id)
            })
        });
    let mut chat = if let Some(chat) = existing {
        chat.clone()
    } else {
        if rows.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let selected = {
            let _g = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            config(root)?.active_id
        };
        let p = selected.as_deref().and_then(|id| provider(root, id).ok());
        Conversation {
            id: format!("video_{id}"),
            video_id: Some(id.into()),
            title: c["title"]
                .as_str()
                .unwrap_or("Video chat")
                .chars()
                .take(100)
                .collect(),
            provider_id: p.as_ref().map(|p| p.id.clone()).unwrap_or_default(),
            provider_label: p.as_ref().map(|p| p.label.clone()).unwrap_or_default(),
            model: p.map(|p| p.model).unwrap_or_default(),
            created_at: now(),
            updated_at: now(),
            transcript: None,
            messages: Vec::new(),
            active_task: None,
            reasoning_effort: None,
        }
    };
    chat.video_id = Some(id.into());
    if !c["transcript"].is_null() {
        let transcript = caption(root, &c["transcript"])?;
        if transcript.as_ref().is_none_or(|t| t.video_id != id) {
            return Err("This transcript belongs to a different video.".into());
        }
        chat.transcript = transcript;
        if chat.messages.is_empty() {
            chat.title = chat
                .transcript
                .as_ref()
                .unwrap()
                .title
                .chars()
                .take(100)
                .collect();
        }
    }
    save(root, &chat)?;
    Ok(chat)
}
/// Only completed user/assistant pairs become context for the next turn.
pub(super) fn history(value: &Value) -> Result<Vec<Value>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let rows = value.as_array().ok_or("Invalid conversation history.")?;
    if rows.len() > 198 || rows.len() % 2 != 0 {
        return Err("Invalid conversation history.".into());
    }
    let mut total = 0;
    let mut result = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let role = string(row, "role")?;
        let content = string(row, "content")?;
        total += content.len();
        if role != if i % 2 == 0 { "user" } else { "assistant" }
            || content.trim().is_empty()
            || total > 300_000
        {
            return Err(
                "This conversation has reached its context limit. Start a new chat to continue."
                    .into(),
            );
        }
        result.push(json!({"role":role,"content":content}));
    }
    Ok(result)
}
fn finish(root: &Path, id: &str, job: &Arc<Job>, result: &Result<Value>) -> Result<Value> {
    let _g = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    // Deleting a chat or retrying after cancellation must never resurrect an old turn.
    if !path(root, id)?.exists() {
        return Ok(Value::Null);
    }
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Ok(Value::Null);
    }
    let message = chat.messages.last_mut().ok_or("Missing chat reply.")?;
    message.content = job.state.lock().unwrap_or_else(|e| e.into_inner())["text"]
        .as_str()
        .unwrap_or_default()
        .into();
    {
        let state = job.state.lock().unwrap_or_else(|e| e.into_inner());
        message.reasoning = state["reasoning"].as_str().unwrap_or_default().into();
        message.usage = state["usage"].clone();
        message.model = state["model"].as_str().unwrap_or_default().into();
        message.provider_label = state["providerLabel"].as_str().unwrap_or_default().into();
        message.reasoning_effort = state["reasoningEffort"].as_str().unwrap_or_default().into();
        message.provider_id = state["providerId"].as_str().unwrap_or_default().into();
        message.model_name = state["modelName"].as_str().unwrap_or_default().into();
        message.context_window = usage::count(&state["contextWindow"]);
        message.max_output_tokens = usage::count(&state["maxOutputTokens"]);
    }
    message.status = if job.cancelled.load(Ordering::Relaxed) {
        "stopped"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .into();
    message.error = if message.status == "failed" {
        result.as_ref().err().cloned()
    } else {
        None
    };
    message.completed_at = now();
    {
        let mut tape = job.activity.lock().unwrap_or_else(|e| e.into_inner());
        activity::settle(
            &mut tape.blocks,
            match message.status.as_str() {
                "stopped" => activity::Status::Stopped,
                "failed" => activity::Status::Failed,
                _ => activity::Status::Completed,
            },
        );
        message.blocks = tape.blocks.clone();
    }
    chat.active_task = None;
    chat.updated_at = now();
    save(root, &chat)?;
    Ok(json!({"conversation":chat}))
}
pub(super) fn interrupt(job: &Arc<Job>) -> Result<()> {
    if let Some((root, id)) = job.conversation.get() {
        finish(root, id, job, &Err("Response stopped.".into()))?;
    }
    Ok(())
}

/// Commit before continuing the model so a failed final reply cannot lose a game.
pub(super) fn create_game(job: &Arc<Job>, call: &Value) -> Result<Value> {
    let (root, id) = job
        .conversation
        .get()
        .ok_or("Games require a saved conversation.")?;
    let call_id = string(call, "id")?;
    if call_id.is_empty() || call_id.len() > 200 {
        return Err("Invalid tool call identity.".into());
    }
    let definition: games::Definition = serde_json::from_str(string(call, "arguments")?)
        .map_err(|_| "Invalid game arguments. Supply all fields described by the tool schema.")?;
    let _guard = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    job.pending()?;
    let mut chat = read(root, id)?;
    if chat.active_task.as_deref() != Some(&job.id) {
        return Err("This game request is no longer active.".into());
    }
    let transcript = if let Some(reference) = &chat.transcript {
        let _guard = crate::REQUEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Some(crate::load(
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
    let game = games::Game::create(definition, call_id, transcript.as_ref())?;
    let result = json!({"gameId":game.id,"title":game.definition.title,"status":"created","message":"The playable card is in chat. Briefly invite the user to play; do not repeat its HTML or answers."});
    reply.games.push(game);
    reply.blocks = job
        .activity
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .blocks
        .clone();
    let snapshot = json!(reply.games);
    job.pending()?;
    save(root, &chat)?;
    let mut state = job.state.lock().unwrap_or_else(|e| e.into_inner());
    state["gameCommitted"] = json!(true);
    state["games"] = snapshot;
    Ok(result)
}

fn game_action(c: &Value, root: &Path) -> Result<Value> {
    let mut chat = read(root, string(c, "chatId")?)?;
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
    let notify = game.apply(c)?;
    let result = json!({"game":game,"notify":notify});
    chat.updated_at = now();
    save(root, &chat)?;
    Ok(result)
}

fn game_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS.lock().unwrap_or_else(|e| e.into_inner());
        let chat = read(root, string(c, "chatId")?)?;
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
        let video_id = chat
            .video_id
            .or_else(|| chat.transcript.map(|t| t.video_id))
            .ok_or("This game has no source video.")?;
        (video_id, cue)
    };
    games::audio::prepare(root, &video_id, &cue, &c["catalog"])
}

fn transcript_audio(c: &Value, root: &Path) -> Result<Value> {
    let (video_id, cue) = {
        let _guard = CHATS.lock().unwrap_or_else(|e| e.into_inner());
        let chat = read(root, string(c, "chatId")?)?;
        let reference = chat.transcript.ok_or("This chat has no transcript.")?;
        if c["videoId"] != reference.video_id {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let track_id = string(c, "trackId")?;
        let _guard = crate::REQUEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let allowed = crate::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            &reference.track_id,
            |data| data.tracks.iter().any(|track| track.id == track_id),
        )?;
        if !allowed {
            return Err("This transcript is no longer attached to the chat.".into());
        }
        let cue = crate::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &reference.video_id,
            track_id,
            |data| games::Cue::from_transcript(data, string(c, "cueId")?),
        )??;
        (reference.video_id, cue)
    };
    games::audio::prepare(root, &video_id, &cue, &c["catalog"])
}

fn send(c: Value, root: PathBuf) -> Result<Value> {
    let _g = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    let existing = c["chatId"].as_str().filter(|id| !id.is_empty());
    let mut chat = if let Some(id) = existing {
        let mut chat = read(&root, id)?;
        recover(&root, &mut chat)?;
        if chat.active_task.is_some() {
            return Err("This chat is already responding. Stop it first.".into());
        }
        chat
    } else if c["transcript"]["videoId"].is_string() {
        open_video(
            &root,
            &json!({"videoId":c["transcript"]["videoId"],"transcript":c["transcript"]}),
        )?
    } else {
        if list(&root)?.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let p = provider(&root, string(&c, "id")?)?;
        Conversation {
            id: random()?,
            video_id: None,
            title: String::new(),
            provider_id: p.id,
            provider_label: p.label,
            model: p.model,
            created_at: now(),
            updated_at: now(),
            transcript: None,
            messages: Vec::new(),
            active_task: None,
            reasoning_effort: None,
        }
    };
    if chat.active_task.is_some() {
        return Err("This chat is already responding. Stop it first.".into());
    }
    if chat.provider_id.is_empty() {
        let selected = provider(&root, string(&c, "id")?)?;
        chat.provider_id = selected.id;
        chat.provider_label = selected.label;
        chat.model = selected.model;
    }
    let mut p = provider(&root, &chat.provider_id)?;
    p.model = chat.model.clone();
    if p.model.is_empty() {
        return Err("Choose a model in Providers first.".into());
    }
    if let Some(value) = c.get("reasoningEffort") {
        chat.reasoning_effort = parse_reasoning(value)?;
    }
    let retry = c["retry"] == true;
    let prompt = if retry {
        chat.messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .ok_or("There is no message to retry.")?
    } else {
        string(&c, "prompt")?
    }
    .trim()
    .to_owned();
    if prompt.is_empty() || prompt.len() > 16_000 {
        return Err("Enter a message of up to 16,000 bytes.".into());
    }
    if retry {
        chat.messages.pop();
        chat.messages.pop();
    }
    if chat.messages.len() >= 198 {
        return Err("This chat is full. Start a new chat to continue.".into());
    }
    if let Some(value) = c.get("transcript")
        && (!value.is_null() || chat.video_id.is_none())
    {
        let transcript = caption(&root, value)?;
        if chat
            .video_id
            .as_ref()
            .is_some_and(|id| transcript.as_ref().is_none_or(|t| &t.video_id != id))
        {
            return Err("This transcript belongs to a different video.".into());
        }
        if chat.video_id.is_none()
            && let Some(t) = &transcript
        {
            if list(&root)?.iter().any(|other| {
                other.id != chat.id
                    && (other.video_id.as_deref() == Some(t.video_id.as_str())
                        || other
                            .transcript
                            .as_ref()
                            .is_some_and(|r| r.video_id == t.video_id))
            }) {
                return Err("This video already has a chat. Open it from the video.".into());
            }
            chat.video_id = Some(t.video_id.clone());
        }
        chat.transcript = transcript;
    }
    if chat.video_id.is_some() && chat.transcript.is_none() {
        return Err("Load this video's transcript before sending a message.".into());
    }
    let history_rows = chat
        .messages
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| pair[1].status == "completed")
        .flat_map(|pair| {
            pair.iter()
                .map(|m| json!({"role":m.role,"content":if m.games.is_empty() { m.content.clone() } else { format!("{}\n[Interactive games: {}]", m.content, m.games.iter().map(|g| g.definition.title.as_str()).collect::<Vec<_>>().join(", ")) }}))
        })
        .collect::<Vec<_>>();
    history(&json!(history_rows))?;
    let mut wire = json!({"prompt":prompt,"history":history_rows,"intent":c["intent"],"includeTranscript":chat.transcript.is_some()});
    wire["reasoningEffort"] = json!(chat.reasoning_effort);
    let mut recent_games = chat
        .messages
        .iter()
        .flat_map(|m| &m.games)
        .rev()
        .collect::<Vec<_>>();
    recent_games.sort_by_key(|game| {
        std::cmp::Reverse((
            Some(game.id.as_str()) == c["focusGameId"].as_str(),
            game.last_activity(),
        ))
    });
    let mut activity = Vec::new();
    let mut activity_bytes = 0;
    for game in recent_games.into_iter().take(8) {
        let snapshot = game.context();
        let size = snapshot.to_string().len();
        if activity_bytes + size <= 160_000 {
            activity_bytes += size;
            activity.push(snapshot);
        }
    }
    wire["gameActivity"] =
        json!(serde_json::to_string(&activity).map_err(|_| "Could not read game activity.")?);
    if let Some(t) = &chat.transcript {
        wire["videoId"] = json!(t.video_id);
        wire["trackId"] = json!(t.track_id);
    }
    if chat.title.is_empty() {
        chat.title = if c["intent"] == "summary" {
            chat.transcript
                .as_ref()
                .map(|t| t.title.chars().take(100).collect())
                .unwrap_or_else(|| "Video summary".into())
        } else {
            prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect()
        };
    }
    chat.messages.push(Message {
        id: random()?,
        role: "user".into(),
        content: prompt,
        games: Vec::new(),
        blocks: Vec::new(),
        status: "completed".into(),
        error: None,
        reasoning: String::new(),
        usage: Value::Null,
        model: String::new(),
        provider_label: String::new(),
        reasoning_effort: String::new(),
        provider_id: String::new(),
        model_name: String::new(),
        completed_at: 0,
        context_window: None,
        max_output_tokens: None,
    });
    chat.messages.push(Message {
        id: random()?,
        role: "assistant".into(),
        content: String::new(),
        games: Vec::new(),
        blocks: Vec::new(),
        status: "streaming".into(),
        error: None,
        reasoning: String::new(),
        usage: Value::Null,
        model: String::new(),
        provider_label: String::new(),
        reasoning_effort: String::new(),
        provider_id: String::new(),
        model_name: String::new(),
        completed_at: 0,
        context_window: None,
        max_output_tokens: None,
    });
    chat.updated_at = now();
    let job = new_job(&p.id)?;
    let _ = job.conversation.set((root.clone(), chat.id.clone()));
    chat.active_task = Some(job.id.clone());
    if let Err(error) = save(&root, &chat) {
        jobs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.id);
        return Err(error);
    }
    let worker = job.clone();
    let id = chat.id.clone();
    thread::spawn(move || {
        let result = super::chat(&root, &p, &wire, &worker);
        let saved = finish(&root, &id, &worker, &result);
        worker.finish(match result {
            Ok(_) => saved,
            Err(e) => saved.and(Err(e)),
        });
    });
    Ok(json!({"taskId":job.id,"conversation":chat}))
}
fn usage_report(command: &Value, root: &Path) -> Result<Value> {
    let selected = command
        .get("chatId")
        .map(|v| v.as_str().ok_or("Invalid conversation identity."))
        .transpose()?;
    let chats = list(root)?;
    if selected.is_some_and(|id| !chats.iter().any(|chat| chat.id == id)) {
        return Err("This chat is no longer available.".into());
    }
    let cfg = config(root)?;
    let mut turns = Vec::new();
    let mut context = Value::Null;
    for chat in chats
        .iter()
        .filter(|chat| selected.is_none_or(|id| chat.id == id))
    {
        for message in chat
            .messages
            .iter()
            .filter(|m| m.role == "assistant" && m.status != "streaming")
        {
            let model_id = if message.model.is_empty() {
                &chat.model
            } else {
                &message.model
            };
            let label = if message.provider_label.is_empty() {
                &chat.provider_label
            } else {
                &message.provider_label
            };
            if selected.is_some() && usage::count(&message.usage["inputTokens"]).is_some() {
                let model = cfg
                    .providers
                    .iter()
                    .filter(|p| {
                        p.id == message.provider_id
                            || (message.provider_id.is_empty()
                                && (p.id == chat.provider_id || p.label == *label))
                    })
                    .flat_map(|p| &p.models)
                    .find(|m| m.id == *model_id);
                let window = message
                    .context_window
                    .or_else(|| model.and_then(|m| m.context_window));
                let group_limit = cfg
                    .groups
                    .iter()
                    .find(|g| g.id == chat.provider_id)
                    .and_then(|g| g.context_limit);
                let window = match (window, group_limit) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                context = json!({"used":message.usage["latestContextTokens"].as_u64().or_else(|| message.usage["inputTokens"].as_u64()),
                    "window":window,"maxOutputTokens":message.max_output_tokens.or_else(|| model.and_then(|m| m.max_output_tokens)),
                    "reasoningEffort":message.reasoning_effort,"model":model_id,"providerLabel":label});
            }
            turns.push(usage::Turn {
                chat_id: chat.id.clone(),
                provider_id: message.provider_id.clone(),
                provider_label: label.clone(),
                model: model_id.clone(),
                model_name: if message.model_name.is_empty() {
                    model_id.clone()
                } else {
                    message.model_name.clone()
                },
                estimated: message.model.is_empty() || message.provider_label.is_empty(),
                completed_at: message.completed_at,
                usage: message.usage.clone(),
            });
        }
    }
    let mut report = usage::report(&turns, &cfg)?;
    report["chatId"] = json!(selected);
    report["context"] = context;
    report["title"] = json!(
        selected
            .and_then(|id| chats.iter().find(|c| c.id == id))
            .map(|c| c.title.as_str())
    );
    report["chats"] = json!(chats.iter().map(|chat| json!({"id":chat.id,"title":chat.title,"model":chat.model,"providerLabel":chat.provider_label})).collect::<Vec<_>>());
    Ok(report)
}

fn speech_hints(c: &Value, root: &Path) -> Result<Value> {
    let chat = c["chatId"].as_str().map(|id| read(root, id)).transpose()?;
    let draft = c["draft"].as_str().unwrap_or_default();
    if draft.chars().count() > 8000 {
        return Err("Message is too long.".into());
    }
    let texts = std::iter::once(draft)
        .chain(chat.iter().map(|chat| chat.title.as_str()))
        .chain(chat.iter().flat_map(|chat| {
            chat.messages
                .iter()
                .rev()
                .take(20)
                .map(|m| m.content.as_str())
        }));
    let transcript = if c["transcript"].is_null() {
        chat.as_ref().and_then(|chat| chat.transcript.clone())
    } else {
        let reference = &c["transcript"];
        let video_id = string(reference, "videoId")?;
        let track_id = string(reference, "trackId")?;
        if !crate::video_id(video_id) || track_id.len() > 1000 {
            return Err("Invalid transcript identity.".into());
        }
        Some(TranscriptRef {
            video_id: video_id.into(),
            track_id: track_id.into(),
            title: String::new(),
            language: String::new(),
        })
    };
    if let Some(transcript) = transcript {
        if chat
            .as_ref()
            .and_then(|chat| chat.video_id.as_ref())
            .is_some_and(|id| *id != transcript.video_id)
        {
            return Err("This transcript belongs to a different video.".into());
        }
        // Evicted captions should not prevent dictation using the conversation vocabulary.
        let _g = crate::REQUEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let hints = crate::with_transcript(
            root.to_str().ok_or("Invalid transcript storage path.")?,
            &transcript.video_id,
            &transcript.track_id,
            |data| {
                crate::speech::hints(
                    texts.clone(),
                    std::iter::once(data.title.as_str())
                        .chain(data.segments.iter().map(|s| s.text.as_str())),
                )
            },
        );
        return Ok(json!(hints.unwrap_or_else(|_| crate::speech::hints(
            texts,
            std::iter::empty()
        ))));
    }
    Ok(json!(crate::speech::hints(texts, std::iter::empty())))
}

pub(super) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    if c["op"] == "ai_chat_send" {
        return send(c, root);
    }
    if c["op"] == "ai_game_audio" {
        return game_audio(&c, &root);
    }
    if c["op"] == "ai_chat_audio" {
        return transcript_audio(&c, &root);
    }
    let _g = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    match string(&c, "op")? {
        "ai_game_action" => game_action(&c, &root),
        "ai_chat_speech_hints" => speech_hints(&c, &root),
        "ai_usage" => usage_report(&c, &root),
        "ai_chats" => Ok(
            json!({"chats":visible_chats(&root)?.iter().map(|chat| json!({"id":chat.id,"videoId":chat.video_id.as_ref().or_else(|| chat.transcript.as_ref().map(|t| &t.video_id)),"title":chat.title,"providerId":chat.provider_id,"providerLabel":chat.provider_label,"model":chat.model,"updatedAt":chat.updated_at,"videoTitle":chat.transcript.as_ref().map(|t| &t.title)})).collect::<Vec<_>>()}),
        ),
        "ai_chat_video_open" => serde_json::to_value(open_video(&root, &c)?)
            .map_err(|_| "Could not open this video's chat.".into()),
        "ai_chat_model" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            if chat.active_task.is_some() {
                return Err("Stop the response before changing models.".into());
            }
            let selected = provider(&root, string(&c, "id")?)?;
            chat.provider_id = selected.id;
            chat.provider_label = selected.label;
            chat.model = selected.model;
            save(&root, &chat)?;
            serde_json::to_value(chat).map_err(|_| "Could not update this chat.".into())
        }
        "ai_chat_get" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            Ok(serde_json::to_value(chat).map_err(|_| "Could not open this chat.")?)
        }
        "ai_chat_reasoning" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            // Running requests own their wire settings; this preference affects the next turn.
            chat.reasoning_effort = parse_reasoning(&c["reasoningEffort"])?;
            save(&root, &chat)?;
            Ok(serde_json::to_value(chat).map_err(|_| "Could not update this chat.")?)
        }
        "ai_chat_delete" => {
            let id = string(&c, "chatId")?;
            if let Ok(chat) = read(&root, id)
                && let Some(task) = chat.active_task
                && let Some(job) = jobs().lock().unwrap_or_else(|e| e.into_inner()).get(&task)
            {
                job.cancelled.store(true, Ordering::Relaxed);
            }
            match fs::remove_file(path(&root, id)?) {
                Ok(()) => Ok(Value::Null),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
                Err(_) => Err("Could not delete this chat.".into()),
            }
        }
        _ => Err("Unsupported conversation operation.".into()),
    }
}

fn parse_reasoning(value: &Value) -> Result<Option<String>> {
    if value.is_null() || value == "" {
        return Ok(None);
    }
    let level = value
        .as_str()
        .filter(|v| catalog::valid_reasoning(v))
        .ok_or("Invalid reasoning effort.")?;
    Ok(Some(level.into()))
}
