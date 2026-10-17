use crate::{Result, Transcript, folded, load, string, timestamp};
use serde::Serialize;
use serde_json::{Value, json};

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
        let track = if indices[1] == lengths[1]
            || (indices[0] < lengths[0]
                && sources[0].segments[indices[0]].start <= sources[1].segments[indices[1]].start)
        {
            0
        } else {
            1
        };
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
        if !primary.tracks.iter().any(|t| t.id == toggle) {
            return Err("This caption language is unavailable.".into());
        }
        if selected.contains(&toggle) {
            if selected.len() > 1 {
                selected.retain(|id| *id != toggle);
            }
        } else if selected.len() < 2 {
            selected.push(toggle);
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
    let sources = [Some(primary), comparison.as_ref()];
    if op == "transcript_export" {
        let mut lines = Vec::new();
        for row in rows {
            for line in row.lines {
                let source = sources[line.track].unwrap();
                let segment = &source.segments[line.index];
                lines.push(format!(
                    "{}  [{}] {}",
                    timestamp(segment.start),
                    source.language,
                    segment.text
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
                folded(&sources[line.track].unwrap().segments[line.index].text).contains(&query)
            })
        });
    }
    Ok(json!({ "rows": rows, "total": total }))
}
