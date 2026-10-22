use super::{Result, Segment, Transcript, caption_words::is_word, folded, string, video_id};
use crate::transcript::{load, with_transcript};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;

mod layout;

const MAX_WORDS: usize = 2_000;
const MAX_BYTES: u64 = 8_000_000;

#[derive(Serialize)]
struct Token<'a> {
    text: &'a str,
    word: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reading: Option<&'a str>,
    #[serde(rename = "speakerChange", skip_serializing_if = "Option::is_none")]
    speaker_change: Option<bool>,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    text.split_word_bounds()
        .map(|text| Token {
            text,
            word: is_word(text),
            start: None,
            end: None,
            bold: None,
            italic: None,
            underline: None,
            reading: None,
            speaker_change: None,
        })
        .collect()
}

fn caption_tokens<'a>(
    segment: &'a Segment,
    metadata: Option<&crate::captions::Metadata>,
) -> Vec<Token<'a>> {
    let mut result = tokens(&segment.text);
    let mut end = (segment.duration > 0.0).then_some(segment.start + segment.duration);
    let mut next_start = None;
    let mut words = segment.words.iter().rev();
    for token in result.iter_mut().rev().filter(|token| token.word) {
        let Some(word) = words.next() else { break };
        if next_start.is_some_and(|next| next > word.start) {
            end = next_start;
        }
        token.start = Some(word.start);
        token.end = end;
        next_start = Some(word.start);
    }
    let raw = segment
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    let mut ranges = Vec::new();
    let mut offset = 0;
    for run in &segment.runs {
        ranges.push((offset..offset + run.text.len(), run));
        offset += run.text.len();
    }
    let mut boundaries = ranges
        .iter()
        .filter(|(_, run)| run.speaker_change == Some(true))
        .map(|(range, _)| range.start)
        .peekable();
    let mut ranges = ranges.iter().peekable();
    for (token, (offset, text)) in result.iter_mut().filter(|t| t.word).zip(
        raw.split_word_bound_indices()
            .filter(|(_, word)| is_word(word)),
    ) {
        if token.text != text {
            break;
        }
        // A flag belongs to a boundary, not every word in its source run. Carry
        // whitespace-only flags forward and mark a split word just once.
        while boundaries
            .peek()
            .is_some_and(|start| *start < offset + text.len())
        {
            token.speaker_change = Some(true);
            boundaries.next();
        }
        while ranges.peek().is_some_and(|(range, _)| range.end <= offset) {
            ranges.next();
        }
        let Some((range, run)) = ranges.peek() else {
            break;
        };
        if let Some(pen) = run.pen_id.and_then(|id| metadata?.pens.get(id)) {
            token.bold = pen.get("bAttr").map(|n| *n == 1);
            token.italic = pen.get("iAttr").map(|n| *n == 1);
            token.underline = pen.get("uAttr").map(|n| *n == 1);
        }
        if !run
            .text
            .get(offset + text.len() - range.start..)
            .unwrap_or("")
            .chars()
            .any(char::is_alphanumeric)
        {
            token.reading = run.reading.as_deref();
        }
    }
    result
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedWord {
    id: String,
    word: String,
    language: String,
    video_id: String,
    title: String,
    start: f64,
    context: String,
    saved_at: u64,
}

fn identity(language: &str, word: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}\0{}", folded(language), folded(word)))
    )
}

fn path(root: &str) -> PathBuf {
    // Separate from the bounded transcript cache so words outlive cache eviction.
    Path::new(root).join("vocabulary").join("words.json")
}

fn validate(words: &[SavedWord]) -> Result<()> {
    if words.len() > MAX_WORDS {
        return Err("You can save up to 2,000 words. Remove a word before saving another.".into());
    }
    let mut seen = HashSet::new();
    for entry in words {
        if entry.word.is_empty()
            || entry.word.chars().count() > 256
            || entry.word.chars().any(char::is_control)
            || !entry.word.chars().any(char::is_alphanumeric)
            || entry.language.len() > 500
            || entry.title.len() > 10_000
            || entry.context.len() > 50_000
            || !video_id(&entry.video_id)
            || !entry.start.is_finite()
            || entry.start < 0.0
            || entry.id != identity(&entry.language, &entry.word)
            || !seen.insert(&entry.id)
        {
            return Err("Invalid saved word data.".into());
        }
    }
    Ok(())
}

fn read(root: &str) -> Result<Vec<SavedWord>> {
    let path = path(root);
    let words: Vec<SavedWord> = match fs::metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_BYTES => {
            let bytes = fs::read(&path).map_err(|_| "Could not read your saved words.")?;
            serde_json::from_slice(&bytes).map_err(|_| "Could not read your saved words.")?
        }
        _ => return Err("Could not read your saved words.".into()),
    };
    validate(&words)?;
    Ok(words)
}

fn write(root: &str, words: &[SavedWord]) -> Result<()> {
    validate(words)?;
    let bytes = serde_json::to_vec(words).map_err(|_| "Could not encode your saved words.")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Saved words storage is full. Remove some words and try again.".into());
    }
    let path = path(root);
    fs::create_dir_all(path.parent().ok_or("Invalid saved words path.")?)
        .map_err(|_| "Could not create saved words storage.")?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)
        .and_then(|_| fs::rename(&temporary, &path))
        .map_err(|_| "Could not save your words. Please try again.".into())
}

fn index(command: &Value, key: &str) -> Result<usize> {
    command[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("Invalid {key}.").into())
}

struct Selection {
    text: String,
    token_index: usize,
    end_token_index: usize,
    utf16_start: usize,
    utf16_end: usize,
    start: Option<f64>,
    end: Option<f64>,
}

fn selection(data: &Transcript, command: &Value, validate_text: bool) -> Result<Selection> {
    let segment = data
        .segments
        .get(index(command, "segmentIndex")?)
        .ok_or("This caption line is no longer available.")?;
    let tokens = caption_tokens(segment, data.caption_metadata.as_ref());
    let mut offset = 0;
    let offsets: Vec<_> = tokens
        .iter()
        .map(|token| {
            let start = offset;
            offset += token.text.encode_utf16().count();
            (start, offset)
        })
        .collect();
    let (first, last) = if !validate_text && command.get("selectionStart").is_some() {
        let start = index(command, "selectionStart")?;
        let end = index(command, "selectionEnd")?;
        if start >= end || end > offset {
            return Err("Choose words in this caption.".into());
        }
        let mut matches = tokens
            .iter()
            .zip(&offsets)
            .enumerate()
            .filter_map(|(i, (token, (a, b)))| (token.word && *a < end && *b > start).then_some(i));
        let first = matches.next().ok_or("Choose words in this caption.")?;
        (first, matches.next_back().unwrap_or(first))
    } else {
        let first = index(command, "tokenIndex")?;
        let last = if command.get("endTokenIndex").is_some() {
            index(command, "endTokenIndex")?
        } else {
            first
        };
        (first, last)
    };
    if first > last
        || !tokens.get(first).is_some_and(|token| token.word)
        || !tokens.get(last).is_some_and(|token| token.word)
    {
        return Err("Choose words in this caption.".into());
    }
    let text = tokens[first..=last]
        .iter()
        .map(|token| token.text)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > 256 || text.chars().any(char::is_control) {
        return Err("Select a shorter phrase (up to 256 characters).".into());
    }
    if validate_text && text != string(command, "word")? {
        return Err("This caption changed. Select the words again.".into());
    }
    Ok(Selection {
        text,
        token_index: first,
        end_token_index: last,
        utf16_start: offsets[first].0,
        utf16_end: offsets[last].1,
        start: tokens[first].start,
        end: tokens[last].end,
    })
}

fn source_language(data: &Transcript) -> Result<(&'static str, &'static str)> {
    let source = data
        .tracks
        .iter()
        .find(|track| track.id == data.track_id)
        .map_or(data.language.as_str(), |track| track.language.as_str());
    crate::language_catalog::translation_language(source)
        .ok_or("The caption language is unavailable for translation.".into())
}

pub(super) fn word_context(command: &Value, root: &str) -> Result<Value> {
    let data = load(
        root,
        string(command, "videoId")?,
        string(command, "trackId")?,
    )?;
    let token = selection(&data, command, true)?;
    let (code, name) = source_language(&data)?;
    Ok(
        json!({"word":token.text,"sourceLanguage":name,"sourceLangCode":code,
        "sentence":data.segments[index(command,"segmentIndex")?].text.chars().take(1000).collect::<String>()}),
    )
}

#[derive(Serialize)]
pub(super) struct WordAudioTiming {
    pub start: f64,
    pub end: f64,
    kind: &'static str,
}

pub(super) fn word_audio(command: &Value, root: &str) -> Result<Option<WordAudioTiming>> {
    let data = load(
        root,
        string(command, "videoId")?,
        string(command, "trackId")?,
    )?;
    let token = selection(&data, command, true)?;
    let valid = |(start, end): &(f64, f64)| {
        start.is_finite() && end.is_finite() && *start >= 0.0 && end > start && end - start <= 30.0
    };
    if let Some((start, end)) = token.start.zip(token.end).filter(valid) {
        return Ok(Some(WordAudioTiming {
            start,
            end,
            kind: "word",
        }));
    }
    let segment_index = index(command, "segmentIndex")?;
    let segment = &data.segments[segment_index];
    let end = if segment.duration > 0.0 {
        Some(segment.start + segment.duration)
    } else {
        data.segments.get(segment_index + 1).map(|next| next.start)
    };
    Ok(end
        .map(|end| (segment.start, end))
        .filter(valid)
        .map(|(start, end)| WordAudioTiming {
            start,
            end,
            kind: "caption",
        }))
}

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match string(command, "op")? {
        "vocabulary_selection" => {
            let data = load(
                root,
                string(command, "videoId")?,
                string(command, "trackId")?,
            )?;
            let selected = selection(&data, command, false)?;
            Ok(
                json!({"word":selected.text,"tokenIndex":selected.token_index,
                "endTokenIndex":selected.end_token_index,"selectionStart":selected.utf16_start,
                "selectionEnd":selected.utf16_end}),
            )
        }
        "vocabulary_word_audio" => Ok(json!(word_audio(command, root)?)),
        "vocabulary_details" => with_transcript(
            root,
            string(command, "videoId")?,
            string(command, "trackId")?,
            |data| {
                json!(
                    format!(
                        "{}\n{}",
                        crate::caption_tracks::description(data),
                        crate::captions::details(data.caption_metadata.as_ref())
                    )
                    .trim()
                )
            },
        ),
        "vocabulary_caption_layout" => with_transcript(
            root,
            string(command, "videoId")?,
            string(command, "trackId")?,
            |data| {
                let lines: Vec<String> = serde_json::from_value(command["lines"].clone())
                    .map_err(|_| "Invalid caption layout.")?;
                Ok(json!(layout::starts(
                    data,
                    index(command, "segmentIndex")?,
                    &lines
                )?))
            },
        )?,
        "vocabulary_caption" => with_transcript(
            root,
            string(command, "videoId")?,
            string(command, "trackId")?,
            |data| {
                let segment = data
                    .segments
                    .get(index(command, "segmentIndex")?)
                    .ok_or("This caption line is no longer available.")?;
                Ok(
                    json!({"text":segment.text,"tokens":caption_tokens(segment,data.caption_metadata.as_ref())}),
                )
            },
        )?,
        "vocabulary_tokenize" => {
            let text = string(command, "text")?;
            if text.len() > 50_000 {
                return Err("Caption line is too long.".into());
            }
            Ok(json!(tokens(text)))
        }
        "vocabulary_list" => {
            let query = command["query"].as_str().unwrap_or("");
            if query.len() > 10_000 {
                return Err("Search query is too long.".into());
            }
            let query = folded(query);
            let words = read(root)?;
            Ok(json!(
                words
                    .into_iter()
                    .filter(|entry| {
                        query.is_empty()
                            || [&entry.word, &entry.context, &entry.title, &entry.language]
                                .iter()
                                .any(|text| folded(text).contains(&query))
                    })
                    .collect::<Vec<_>>()
            ))
        }
        "vocabulary_translation" => {
            let data = load(
                root,
                string(command, "videoId")?,
                string(command, "trackId")?,
            )?;
            let token = selection(&data, command, true)?;
            let (source_code, _) = source_language(&data)?;
            let mut plans = Vec::new();
            let mut seen = HashSet::new();
            for target in crate::languages::translation_targets(root, source_code)? {
                let (target_code, target_name) =
                    crate::language_catalog::translation_language(&target)
                        .ok_or("Choose your known and learning languages in Language settings.")?;
                if !seen.insert(target_code) {
                    continue;
                }
                plans.push(json!({"input":token.text, "sourceLangCode":source_code,
                "targetLangCode":target_code, "targetLanguage":target_name,
                "sameLanguage":source_code == target_code}));
            }
            Ok(json!(plans))
        }
        "vocabulary_save" => {
            let video = string(command, "videoId")?;
            if !video_id(video) {
                return Err("Invalid video identity.".into());
            }
            let data = load(root, video, string(command, "trackId")?)?;
            let segment = data
                .segments
                .get(index(command, "segmentIndex")?)
                .ok_or("This caption line is no longer available.")?;
            let token = selection(&data, command, true)?;
            let id = identity(&data.language, &token.text);
            let start = token.start.unwrap_or(segment.start);
            let mut words = read(root)?;
            if let Some(entry) = words.iter().find(|entry| entry.id == id) {
                return Ok(json!({"entry":entry,"saved":false}));
            }
            let entry = SavedWord {
                id,
                word: token.text,
                language: data.language,
                video_id: data.video_id,
                title: data.title,
                start,
                context: segment.text.clone(),
                saved_at: u64::try_from(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| "Could not read the device clock.")?
                        .as_millis(),
                )
                .map_err(|_| "Device clock is out of range.")?,
            };
            words.insert(0, entry);
            write(root, &words)?;
            Ok(json!({"entry":words[0],"saved":true}))
        }
        "vocabulary_remove" => {
            let id = string(command, "id")?;
            let mut words = read(root)?;
            words.retain(|entry| entry.id != id);
            write(root, &words)?;
            Ok(Value::Null)
        }
        _ => Err("Unsupported vocabulary operation.".into()),
    }
}
