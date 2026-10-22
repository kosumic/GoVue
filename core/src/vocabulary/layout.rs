//! Map native text rows to existing caption timings without changing the words.
use super::Token;
use crate::vocabulary;
use crate::{Result, Transcript};
use std::ops::Range;

fn rows(source: &str, lines: &[String]) -> Option<Vec<Range<usize>>> {
    let mut chars = source
        .char_indices()
        .filter(|(_, c)| !c.is_whitespace())
        .peekable();
    let mut result = Vec::with_capacity(lines.len());
    let mut end = 0;
    for line in lines {
        let start = chars.peek().map_or(end, |(index, _)| *index);
        for actual in line.chars().filter(|c| !c.is_whitespace()) {
            let (index, expected) = chars.next()?;
            if actual != expected {
                return None;
            }
            end = index + expected.len_utf8();
        }
        result.push(start..end.max(start));
    }
    chars.next().is_none().then_some(result)
}

fn rendered(tokens: &[Token<'_>]) -> (String, Vec<(Range<usize>, f64)>) {
    let mut text = String::new();
    let mut words = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.word && token.speaker_change == Some(true) {
            if index > 0 && !tokens[index - 1].text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("───\n");
        }
        let start = text.len();
        text.push_str(token.text);
        if let Some(time) = token.start {
            words.push((start..text.len(), time));
        }
        if token.word
            && let Some(reading) = token.reading
        {
            text.push('（');
            text.push_str(reading);
            text.push('）');
        }
    }
    (text, words)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "Caption text limits keep character offsets within exact f64 integers."
)]
pub(super) fn starts(data: &Transcript, index: usize, lines: &[String]) -> Result<Vec<f64>> {
    if lines.is_empty() || lines.len() > 4096 {
        return Err("Invalid caption layout.".into());
    }
    let segment = data
        .segments
        .get(index)
        .ok_or("This caption line is no longer available.")?;
    let (text, words) = rendered(&vocabulary::caption_tokens(
        segment,
        data.caption_metadata.as_ref(),
    ));
    let rows = rows(&text, lines).ok_or("Caption layout belongs to another line.")?;
    let end = if segment.duration > 0.0 {
        segment.start + segment.duration
    } else {
        data.segments.get(index + 1).map_or_else(
            || {
                words
                    .last()
                    .map_or(segment.start + 5.0, |(_, start)| start + 2.0)
            },
            |next| next.start,
        )
    };
    let offsets: Vec<_> = rows
        .iter()
        .map(|row| text[..row.start].chars().count())
        .collect();
    let total = text.chars().count().max(1);
    let mut starts: Vec<_> = rows
        .iter()
        .zip(&offsets)
        .enumerate()
        .map(|(index, (row, offset))| {
            if index == 0 {
                return segment.start;
            }
            words
                .iter()
                .find(|(word, _)| word.end > row.start)
                .map_or_else(
                    || segment.start + (end - segment.start) * *offset as f64 / total as f64,
                    |(_, time)| *time,
                )
        })
        .collect();
    // A translated phrase can give several rows the same timestamp. Advance
    // those rows through that phrase, preserving precise boundaries elsewhere.
    let mut first = 0;
    while first < starts.len() {
        let mut next = first + 1;
        while next < starts.len() && starts[next] <= starts[first] {
            next += 1;
        }
        let until = starts.get(next).copied().unwrap_or(end);
        let last_offset = offsets.get(next).copied().unwrap_or(total);
        let duration = (until - starts[first]).max(0.0);
        for row in first + 1..next {
            starts[row] = starts[first]
                + duration * (offsets[row] - offsets[first]) as f64
                    / (last_offset - offsets[first]).max(1) as f64;
        }
        first = next;
    }
    Ok(starts)
}
