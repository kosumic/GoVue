//! Optional source timings aligned to the same Unicode words used for saving.
use crate::Segment;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Word {
    pub text: String,
    pub start: f64,
}

pub(crate) fn is_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub(crate) fn parse(parts: &[Value], start: f64, duration: f64) -> Vec<Word> {
    if !parts.iter().any(|part| part.get("tOffsetMs").is_some()) {
        return Vec::new();
    }
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut offset = 0.0;
    for part in parts {
        let Some(content) = part["utf8"].as_str() else {
            continue;
        };
        if let Some(raw) = part.get("tOffsetMs") {
            let Some(next) = crate::number(raw).filter(|n| n.is_finite() && *n >= offset) else {
                return Vec::new();
            };
            offset = next;
        }
        let from = text.len();
        text.extend(content.nfkc());
        spans.push((from..text.len(), start + offset / 1000.0));
    }
    let mut spans = spans.into_iter().peekable();
    let words = text
        .split_word_bound_indices()
        .filter(|(_, word)| is_word(word))
        .filter_map(|(index, word)| {
            while spans.peek().is_some_and(|(range, _)| range.end <= index) {
                spans.next();
            }
            Some(Word {
                text: word.to_owned(),
                start: spans.peek()?.1,
            })
        })
        .collect();
    // Normalizing separate source fragments can differ at a combining-character
    // boundary. Keep the full caption and fall back rather than misalign a word.
    let segment = Segment {
        start,
        duration,
        text: crate::clean(
            &parts
                .iter()
                .filter_map(|part| part["utf8"].as_str())
                .collect::<String>(),
        ),
        words,
        runs: Vec::new(),
        window_id: None,
    };
    if valid(&segment) {
        segment.words
    } else {
        Vec::new()
    }
}

pub(crate) fn valid(segment: &Segment) -> bool {
    let mut words = segment.words.iter();
    let mut previous = segment.start;
    for text in segment
        .text
        .split_word_bounds()
        .filter(|word| is_word(word))
    {
        let Some(word) = words.next() else {
            return false;
        };
        if word.text != text
            || !word.start.is_finite()
            || word.start < previous
            || (segment.duration > 0.0 && word.start >= segment.start + segment.duration)
        {
            return false;
        }
        previous = word.start;
    }
    words.next().is_none()
}
