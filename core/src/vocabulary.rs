use super::{folded, load, string, video_id, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;

const MAX_WORDS: usize = 2_000;
const MAX_BYTES: u64 = 8_000_000;

#[derive(Serialize)]
struct Token<'a> {
    text: &'a str,
    word: bool,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    text.split_word_bounds()
        .map(|text| Token {
            text,
            word: text.chars().any(char::is_alphanumeric),
        })
        .collect()
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
        .ok_or_else(|| format!("Invalid {key}."))
}

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match string(command, "op")? {
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
            Ok(json!(words
                .into_iter()
                .filter(|entry| {
                    query.is_empty()
                        || [&entry.word, &entry.context, &entry.title, &entry.language]
                            .iter()
                            .any(|text| folded(text).contains(&query))
                })
                .collect::<Vec<_>>()))
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
            let tokens = tokens(&segment.text);
            let token = tokens
                .get(index(command, "tokenIndex")?)
                .filter(|token| token.word)
                .ok_or("Choose a word in the caption.")?;
            if token.text != string(command, "word")? {
                return Err("This caption changed. Hold the word again to save it.".into());
            }
            let id = identity(&data.language, token.text);
            let mut words = read(root)?;
            if let Some(entry) = words.iter().find(|entry| entry.id == id) {
                return Ok(json!({"entry":entry,"saved":false}));
            }
            let entry = SavedWord {
                id,
                word: token.text.to_owned(),
                language: data.language,
                video_id: data.video_id,
                title: data.title,
                start: segment.start,
                context: segment.text.clone(),
                saved_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "Could not read the device clock.")?
                    .as_millis() as u64,
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
