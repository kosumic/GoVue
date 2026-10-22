//! Device speech preparation, in-memory dictation, and recognition vocabulary.
pub(crate) mod reading;

use crate::{Result, string};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const DRAFT_UNITS: usize = 8000;
static ACTIVE: Mutex<Option<Dictation>> = Mutex::new(None);

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Preparing,
    Listening,
    Finishing,
}

struct Dictation {
    id: String,
    phase: Phase,
    preview: String,
    cancelled: bool,
    confirmed: bool,
}

impl Dictation {
    fn view(&self) -> Value {
        json!({"phase": self.phase, "interimText": self.preview})
    }
}

fn unspaced(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}')
}

fn append(draft: &str, recognized: &str) -> Result<String> {
    if draft.encode_utf16().count() > DRAFT_UNITS {
        return Err("Message is too long.".into());
    }
    let recognized = recognized.trim();
    let space = draft
        .chars()
        .last()
        .zip(recognized.chars().next())
        .is_some_and(|(last, first)| {
            !last.is_whitespace()
                && !unspaced(last)
                && !unspaced(first)
                && !matches!(
                    first,
                    '.' | ','
                        | '!'
                        | '?'
                        | ';'
                        | ':'
                        | ')'
                        | ']'
                        | '}'
                        | '，'
                        | '。'
                        | '！'
                        | '？'
                        | '；'
                        | '：'
                        | '、'
                        | '）'
                        | '」'
                        | '』'
                )
        });
    let mut result = draft.to_owned();
    let mut remaining = DRAFT_UNITS - draft.encode_utf16().count();
    if space && remaining > 0 {
        result.push(' ');
        remaining -= 1;
    }
    for character in recognized.chars() {
        if character.len_utf16() > remaining {
            break;
        }
        result.push(character);
        remaining -= character.len_utf16();
    }
    Ok(result)
}

pub(crate) fn dispatch(request: &str) -> Result<Value> {
    if request.len() > 128_000 {
        return Err("Dictation request is too large.".into());
    }
    let c: Value = serde_json::from_str(request).map_err(|_| "Unreadable dictation request.")?;
    let op = string(&c, "op")?;
    if op == "hints" {
        let draft = string(&c, "draft")?;
        if draft.encode_utf16().count() > DRAFT_UNITS {
            return Err("Message is too long.".into());
        }
        return Ok(json!(hints(std::iter::once(draft), std::iter::empty())));
    }
    let id = string(&c, "sessionId")?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b':' | b'-' | b'_'))
    {
        return Err("Invalid dictation session.".into());
    }
    let mut active = ACTIVE
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    if op == "begin" {
        if string(&c, "draft")?.encode_utf16().count() >= DRAFT_UNITS {
            return Err(
                "The message is full. Send it or shorten it before adding voice input.".into(),
            );
        }
        if active.is_some() {
            return Err("Voice input is already active.".into());
        }
        let session = Dictation {
            id: id.into(),
            phase: Phase::Preparing,
            preview: String::new(),
            cancelled: false,
            confirmed: false,
        };
        let view = session.view();
        *active = Some(session);
        return Ok(view);
    }
    let session = active
        .as_mut()
        .filter(|session| session.id == id)
        .ok_or("This dictation session is no longer active.")?;
    match op {
        "listening" if session.phase == Phase::Preparing && !session.cancelled => {
            session.phase = Phase::Listening;
        }
        "finish" if session.phase == Phase::Listening => session.phase = Phase::Finishing,
        "cancel" => {
            session.cancelled = true;
            session.preview.clear();
            session.phase = Phase::Finishing;
        }
        "result" if session.phase != Phase::Preparing && !session.cancelled => {
            let text = string(&c, "text")?;
            if text.encode_utf16().count() > DRAFT_UNITS * 2 {
                return Err("Recognized speech is too long.".into());
            }
            if c["isFinal"].as_bool().ok_or("Missing final-result flag.")? {
                let draft = append(string(&c, "draft")?, text)?;
                session.preview.clear();
                if !text.trim().is_empty() {
                    session.confirmed = true;
                    let mut view = session.view();
                    view["draft"] = json!(draft);
                    return Ok(view);
                }
            } else {
                session.preview = text.into();
            }
        }
        "end" => {
            let no_speech = !session.cancelled && !session.confirmed;
            *active = None;
            drop(active);
            return Ok(json!({"phase":"idle", "interimText":"", "noSpeech":no_speech}));
        }
        "listening" | "finish" | "result" => {}
        _ => return Err("Unsupported dictation operation.".into()),
    }
    Ok(session.view())
}

fn words<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for text in texts {
        let normalized: String = text.nfkc().collect();
        for word in normalized.unicode_words() {
            let length = word.chars().count();
            if length > 48
                || !word.chars().any(char::is_alphabetic)
                || (length == 1 && word.is_ascii())
            {
                continue;
            }
            let key = word.to_lowercase();
            if let Some((_, count)) = counts.get_mut(&key) {
                *count += 1;
            } else if counts.len() < 4096 {
                counts.insert(key, (word.into(), 1));
            }
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.1
            .cmp(&a.1.1)
            .then(b.0.len().cmp(&a.0.len()))
            .then(a.0.cmp(&b.0))
    });
    ranked
        .into_iter()
        .take(100)
        .map(|(_, (word, _))| word)
        .collect()
}

pub(crate) fn hints<'a, 'b>(
    chat: impl Iterator<Item = &'a str>,
    transcript: impl Iterator<Item = &'b str>,
) -> Vec<String> {
    let chat = words(chat);
    let transcript = words(transcript);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    // Interleave sources so a long transcript cannot crowd out the conversation.
    for index in 0..chat.len().max(transcript.len()) {
        for word in [chat.get(index), transcript.get(index)]
            .into_iter()
            .flatten()
        {
            if seen.insert(word.to_lowercase()) {
                result.push(word.clone());
                if result.len() == 100 {
                    return result;
                }
            }
        }
    }
    result
}
