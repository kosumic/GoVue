use crate::{Result, Transcript, folded, load, string, timestamp};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const LANGUAGE_USAGE: &str = "caption-language-usage.json";

fn language_usage(root: &str) -> BTreeMap<String, u64> {
    crate::media::read(root, LANGUAGE_USAGE, json!({}))
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(language, count)| {
            let count = count.as_u64().filter(|count| *count > 0)?;
            crate::caption_tracks::language(language)
                .then(|| (language.to_ascii_lowercase(), count))
        })
        .take(512)
        .collect()
}

fn remember_language(root: &str, language: &str) {
    if !crate::caption_tracks::language(language) {
        return;
    }
    let mut usage = language_usage(root);
    let language = language.to_ascii_lowercase();
    if usage.len() >= 512 && !usage.contains_key(&language) {
        return;
    }
    let count = usage.entry(language).or_default();
    *count = count.saturating_add(1);
    // Ranking preferences must not prevent a valid caption selection.
    let _ = crate::media::write(root, LANGUAGE_USAGE, &json!(usage));
}

#[derive(Serialize)]
struct Line {
    track: usize,
    index: usize,
}

#[derive(Serialize)]
struct Row {
    start: f64,
    lines: Vec<Line>,
}

fn end(data: &Transcript, index: usize) -> f64 {
    let segment = &data.segments[index];
    let next = data
        .segments
        .get(index + 1)
        .map_or(f64::INFINITY, |s| s.start);
    if segment.duration > 0.0 {
        (segment.start + segment.duration).min(next)
    } else {
        next
    }
}

// Merge the two already sorted tracks without moving a later cue ahead of an
// earlier one. Neighboring cues may share a row when their time ranges overlap;
// the first selected language wins ties only when timestamps are identical.
#[allow(
    clippy::float_cmp,
    reason = "Cue timestamps are compared exactly to preserve identical-start tie ordering."
)]
fn rows(primary: &Transcript, comparison: Option<&Transcript>) -> Vec<Row> {
    let sources = [primary, comparison.unwrap_or(primary)];
    let lengths = [
        primary.segments.len(),
        comparison.map_or(0, |data| data.segments.len()),
    ];
    let mut indices = [0, 0];
    let mut rows: Vec<Row> = Vec::with_capacity(lengths[0].max(lengths[1]));
    let mut previous_end = 0.0;
    while indices[0] < lengths[0] || indices[1] < lengths[1] {
        let track = usize::from(
            !(indices[1] == lengths[1]
                || (indices[0] < lengths[0]
                    && sources[0].segments[indices[0]].start
                        <= sources[1].segments[indices[1]].start)),
        );
        let index = indices[track];
        let start = sources[track].segments[index].start;
        let line = Line { track, index };
        if let Some(row) = rows.last_mut().filter(|row| {
            row.lines.len() == 1
                && row.lines[0].track != track
                && (start == row.start || start < previous_end)
        }) {
            row.lines.push(line);
        } else {
            rows.push(Row {
                start,
                lines: vec![line],
            });
        }
        previous_end = end(sources[track], index);
        indices[track] += 1;
    }
    rows
}

pub(super) fn dispatch(
    op: &str,
    primary: &Transcript,
    command: &Value,
    root: &str,
) -> Result<Value> {
    if op == "transcript_select" {
        let ids = command["trackIds"]
            .as_array()
            .ok_or("Invalid caption selection.")?;
        if ids.is_empty() || ids.len() > 2 {
            return Err("Select one or two transcript languages.".into());
        }
        let mut selected: Vec<&str> = Vec::new();
        for id in ids {
            let id = id.as_str().ok_or("Invalid caption selection.")?;
            if !primary.tracks.iter().any(|t| t.id == id) || selected.contains(&id) {
                return Err("Invalid caption selection.".into());
            }
            selected.push(id);
        }
        let toggle = string(command, "toggleId")?;
        let track = primary
            .tracks
            .iter()
            .find(|t| t.id == toggle)
            .ok_or("This caption language is unavailable.")?;
        if selected.contains(&toggle) {
            if selected.len() > 1 {
                selected.retain(|id| *id != toggle);
            }
        } else if selected.len() < 2 {
            selected.push(toggle);
            remember_language(root, &track.language);
        } else {
            return Err("Select up to two languages. Remove a language first.".into());
        }
        return Ok(json!(selected));
    }
    let comparison = match command.get("comparisonTrackId").filter(|v| !v.is_null()) {
        Some(id) => {
            let id = id.as_str().ok_or("Invalid comparison language.")?;
            if id == primary.track_id || !primary.tracks.iter().any(|t| t.id == id) {
                return Err("Invalid comparison language.".into());
            }
            Some(load(root, &primary.video_id, id)?)
        }
        None => None,
    };
    let mut rows = rows(primary, comparison.as_ref());
    let sources = [primary, comparison.as_ref().unwrap_or(primary)];
    if op == "transcript_export" {
        let mut lines = Vec::new();
        for row in rows {
            for line in row.lines {
                let source = sources[line.track];
                let segment = &source.segments[line.index];
                lines.push(format!(
                    "{}  [{}] {}",
                    timestamp(segment.start),
                    source.language,
                    crate::captions::readable_text(segment)
                ));
            }
        }
        return Ok(json!(format!(
            "{}\nhttps://www.youtube.com/watch?v={}\n\n{}",
            primary.title,
            primary.video_id,
            lines.join("\n")
        )));
    }
    let query = command["query"].as_str().unwrap_or("");
    if query.len() > 10_000 {
        return Err("Search query is too long.".into());
    }
    let query = folded(query);
    let total = rows.len();
    if !query.is_empty() {
        rows.retain(|row| {
            row.lines.iter().any(|line| {
                crate::captions::matches(&sources[line.track].segments[line.index], &query)
            })
        });
    }
    let usage = language_usage(root);
    let mut choices = crate::caption_tracks::choices(primary);
    for tracks in [&mut choices.caption_tracks, &mut choices.translations] {
        tracks.sort_by_cached_key(|track| {
            std::cmp::Reverse(
                usage
                    .get(&track.language.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
        });
    }
    Ok(json!({ "rows": rows, "total": total, "trackChoices": choices }))
}
