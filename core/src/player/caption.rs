//! Native text layout supplies visual rows; Rust pages them against speech time.
use crate::{Segment, caption_words};
use serde::Deserialize;
use serde_json::{Value, json};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    segment_index: usize,
    source_text: String,
    lines: Vec<String>,
}

fn rows(source: &str, lines: &[String]) -> Option<Vec<Range<usize>>> {
    if lines.is_empty() || lines.len() > 4096 {
        return None;
    }
    // Native line breaking can trim spaces/newlines. Every spoken character
    // must still match, in order, before its offsets can be used for paging.
    let mut chars = source
        .char_indices()
        .filter(|(_, c)| !c.is_whitespace())
        .peekable();
    let mut result = Vec::new();
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

#[allow(
    clippy::cast_precision_loss,
    reason = "Caption limits keep character counts within exact f64 integers."
)]
pub(super) fn frame(
    segment: &Segment,
    index: usize,
    position: f64,
    end: f64,
    raw: &Value,
) -> Value {
    let layout = serde_json::from_value::<Layout>(raw.clone())
        .ok()
        .filter(|layout| layout.segment_index == index && layout.source_text == segment.text);
    let pages = layout
        .and_then(|layout| rows(&segment.text, &layout.lines))
        .map(|rows| {
            rows.chunks(2)
                .map(|pair| pair[0].start..pair[pair.len() - 1].end)
                .collect::<Vec<_>>()
        });
    let mut text = segment.text.as_str();
    if let Some(pages) = pages.filter(|pages| !pages.is_empty()) {
        let words: Vec<_> = segment
            .text
            .split_word_bound_indices()
            .filter(|(_, word)| caption_words::is_word(word))
            .zip(&segment.words)
            .map(|((offset, text), word)| (offset..offset + text.len(), word.start))
            .collect();
        let end = if end.is_finite() {
            end
        } else {
            words
                .last()
                .map_or(segment.start + 5.0, |(_, start)| start + 2.0)
        };
        let offsets: Vec<_> = pages
            .iter()
            .map(|page| segment.text[..page.start].chars().count())
            .collect();
        let total = segment.text.chars().count().max(1);
        let mut starts: Vec<_> = pages
            .iter()
            .zip(&offsets)
            .enumerate()
            .map(|(page_index, (page, offset))| {
                if page_index == 0 {
                    return segment.start;
                }
                words
                    .iter()
                    .find(|(word, _)| word.end > page.start)
                    .map_or_else(
                        || segment.start + (end - segment.start) * *offset as f64 / total as f64,
                        |(_, start)| *start,
                    )
            })
            .collect();
        // Some tracks timestamp a whole phrase at once. Preserve precise page
        // boundaries where available; distribute only pages sharing a timestamp
        // across that phrase's remaining interval, so none are skipped.
        let mut first = 0;
        while first < starts.len() {
            let mut next = first + 1;
            while next < starts.len() && starts[next] <= starts[first] {
                next += 1;
            }
            let until = starts.get(next).copied().unwrap_or(end);
            let last_offset = offsets.get(next).copied().unwrap_or(total);
            let duration = (until - starts[first]).max(0.0);
            for page in first + 1..next {
                starts[page] = starts[first]
                    + duration * (offsets[page] - offsets[first]) as f64
                        / (last_offset - offsets[first]).max(1) as f64;
            }
            first = next;
        }
        let page = starts
            .iter()
            .rposition(|start| *start <= position)
            .unwrap_or(0);
        text = segment.text[pages[page].clone()].trim();
    }
    json!({"text":text,"sourceText":segment.text,"segmentIndex":index})
}
