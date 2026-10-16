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
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Conversation {
    id: String,
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
        || !valid_id(&chat.provider_id)
        || chat.model.len() > 200
        || chat.title.len() > 1000
        || chat
            .reasoning_effort
            .as_ref()
            .is_some_and(|v| !catalog::valid_reasoning(v))
        || chat.messages.len() > 200
        || chat.messages.len() % 2 != 0
        || chat.messages.iter().enumerate().any(|(i, m)| {
            !valid_id(&m.id)
                || m.role != if i % 2 == 0 { "user" } else { "assistant" }
                || !matches!(
                    m.status.as_str(),
                    "completed" | "streaming" | "failed" | "stopped"
                )
                || m.content.len() > MAX_OUTPUT
                || m.reasoning.len() > MAX_OUTPUT
                || m.model.len() > 200
                || m.provider_label.len() > 100
                || !catalog::valid_reasoning(&m.reasoning_effort)
                || (!m.usage.is_null()
                    && !m.usage.as_object().is_some_and(|usage| {
                        usage.iter().all(|(key, value)| {
                            matches!(
                                key.as_str(),
                                "inputTokens" | "outputTokens" | "reasoningTokens"
                            ) && value.as_u64().is_some()
                        })
                    }))
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
                    j.state.lock().unwrap_or_else(|e| e.into_inner())["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
        });
        chat.active_task = None;
        if let Some(m) = chat.messages.last_mut().filter(|m| m.status == "streaming") {
            if let Some(text) = partial {
                m.content = text;
            }
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
        if let Some(id) = p.file_stem().and_then(|s| s.to_str()) {
            if let Ok(mut chat) = read(root, id) {
                recover(root, &mut chat)?;
                rows.push(chat);
            }
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    Ok(rows)
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
    } else {
        if list(&root)?.len() >= MAX_CHATS {
            return Err("Chat history is full. Delete an older chat first.".into());
        }
        let p = provider(&root, string(&c, "id")?)?;
        Conversation {
            id: random()?,
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
    if let Some(value) = c.get("transcript") {
        chat.transcript = caption(&root, value)?;
    }
    let history_rows = chat
        .messages
        .chunks_exact(2)
        .filter(|pair| pair[1].status == "completed")
        .flat_map(|pair| {
            pair.iter()
                .map(|m| json!({"role":m.role,"content":m.content}))
        })
        .collect::<Vec<_>>();
    history(&json!(history_rows))?;
    let mut wire = json!({"prompt":prompt,"history":history_rows,"intent":c["intent"],"includeTranscript":chat.transcript.is_some()});
    wire["reasoningEffort"] = json!(chat.reasoning_effort);
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
        status: "completed".into(),
        error: None,
        reasoning: String::new(),
        usage: Value::Null,
        model: String::new(),
        provider_label: String::new(),
        reasoning_effort: String::new(),
    });
    chat.messages.push(Message {
        id: random()?,
        role: "assistant".into(),
        content: String::new(),
        status: "streaming".into(),
        error: None,
        reasoning: String::new(),
        usage: Value::Null,
        model: String::new(),
        provider_label: String::new(),
        reasoning_effort: String::new(),
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
pub(super) fn dispatch(c: Value, root: PathBuf) -> Result<Value> {
    if c["op"] == "ai_chat_send" {
        return send(c, root);
    }
    let _g = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    match string(&c, "op")? {
        "ai_chats" => Ok(
            json!({"chats":list(&root)?.iter().map(|chat| json!({"id":chat.id,"title":chat.title,"providerId":chat.provider_id,"providerLabel":chat.provider_label,"model":chat.model,"updatedAt":chat.updated_at,"videoTitle":chat.transcript.as_ref().map(|t| &t.title)})).collect::<Vec<_>>()}),
        ),
        "ai_chat_get" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            Ok(serde_json::to_value(chat).map_err(|_| "Could not open this chat.")?)
        }
        "ai_chat_reasoning" => {
            let mut chat = read(&root, string(&c, "chatId")?)?;
            recover(&root, &mut chat)?;
            if chat.active_task.is_some() {
                return Err("Stop the response before changing reasoning.".into());
            }
            chat.reasoning_effort = parse_reasoning(&c["reasoningEffort"])?;
            save(&root, &chat)?;
            Ok(serde_json::to_value(chat).map_err(|_| "Could not update this chat.")?)
        }
        "ai_chat_delete" => {
            let id = string(&c, "chatId")?;
            if let Ok(chat) = read(&root, id) {
                if let Some(task) = chat.active_task {
                    if let Some(job) = jobs().lock().unwrap_or_else(|e| e.into_inner()).get(&task) {
                        job.cancelled.store(true, Ordering::Relaxed);
                    }
                }
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
