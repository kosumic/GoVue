//! Normalized caption structure. No upstream URLs, tokens, or response bodies.
use crate::{MAX_BYTES, MAX_SEGMENTS, Result, Segment, caption_words, clean, folded, number};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use unicode_normalization::UnicodeNormalization;

type Attributes = BTreeMap<String, u32>;
const PEN: &[(&str, &str, u32)] = &[
    ("b", "bAttr", 1),
    ("i", "iAttr", 1),
    ("u", "uAttr", 1),
    ("of", "ofOffset", 2),
    ("fc", "fcForeColor", 0xffffff),
    ("fo", "foForeAlpha", 255),
    ("bc", "bcBackColor", 0xffffff),
    ("bo", "boBackAlpha", 255),
    ("et", "etEdgeType", 4),
    ("ec", "ecEdgeColor", 0xffffff),
    ("fs", "fsFontStyle", 7),
    ("sz", "szPenSize", 1000),
    ("rb", "rbRuby", 15),
    ("hg", "hgHorizGroup", 1),
];
const WINDOW_STYLE: &[(&str, &str, u32)] = &[
    ("mh", "mhModeHint", 2),
    ("ju", "juJustifCode", 3),
    ("pd", "pdPrintDir", 3),
    ("sd", "sdScrollDir", 3),
    ("wfc", "wfcWinFillColor", 0xffffff),
    ("wfo", "wfoWinFillAlpha", 255),
];
const WINDOW_POSITION: &[(&str, &str, u32)] = &[
    ("ap", "apPoint", 8),
    ("cc", "ccCols", 1000),
    ("rc", "rcRows", 1000),
    ("ah", "ahHorPos", 100),
    ("av", "avVerPos", 100),
];

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Part {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_change: Option<bool>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_change: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Event {
    start: f64,
    duration: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_id: Option<u32>,
    #[serde(default)]
    append: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pen_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_style_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_position_id: Option<usize>,
    parts: Vec<Part>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Metadata {
    pub pens: Vec<Attributes>,
    window_styles: Vec<Attributes>,
    window_positions: Vec<Attributes>,
    events: Vec<Event>,
}

pub(crate) fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(clean)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub(crate) fn readable_text(segment: &Segment) -> String {
    if !segment.runs.iter().any(|run| run.reading.is_some()) {
        return segment.text.clone();
    }
    clean_text(
        &segment
            .runs
            .iter()
            .map(|run| match &run.reading {
                Some(reading) => format!("{}（{reading}）", run.text),
                None => run.text.clone(),
            })
            .collect::<String>(),
    )
}

pub(crate) fn matches(segment: &Segment, query: &str) -> bool {
    folded(&segment.text).contains(query)
        || segment.runs.iter().any(|run| run.reading.is_some())
            && (folded(&readable_text(segment)).contains(query)
                || folded(
                    &segment
                        .runs
                        .iter()
                        .map(|run| run.reading.as_deref().unwrap_or(&run.text))
                        .collect::<String>(),
                )
                .contains(query))
}

fn integer(value: &Value, max: u32) -> Option<u32> {
    number(value)
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= f64::from(max) && n.fract() == 0.0)
        .map(|n| n as u32)
}
fn reference(value: &Value) -> Option<usize> {
    integer(value, 1023).map(|n| n as usize)
}

fn attributes(raw: &Value, fields: &[(&str, &str, u32)]) -> Attributes {
    fields
        .iter()
        .filter_map(|(_, key, max)| integer(&raw[*key], *max).map(|v| ((*key).into(), v)))
        .collect()
}
fn table(data: &Value, key: &str, fields: &[(&str, &str, u32)]) -> Vec<Attributes> {
    data[key]
        .as_array()
        .into_iter()
        .flatten()
        .take(1024)
        .map(|raw| attributes(raw, fields))
        .collect()
}

fn runs(event: &Event, metadata: &Metadata) -> Vec<Run> {
    let ruby = |part: &Part| {
        part.pen_id
            .or(event.pen_id)
            .and_then(|id| metadata.pens.get(id))
            .and_then(|pen| pen.get("rbRuby"))
            .copied()
            .unwrap_or(0)
    };
    let timed = event.parts.iter().any(|part| part.offset.is_some());
    let mut result = Vec::new();
    let mut index = 0;
    let mut offset = 0.0;
    while let Some(part) = event.parts.get(index) {
        offset = part.offset.unwrap_or(offset);
        let mut run = Run {
            text: part.text.clone(),
            start: timed.then_some(event.start + offset),
            pen_id: part.pen_id.or(event.pen_id),
            reading: None,
            confidence: part.confidence,
            speaker_change: part.speaker_change,
        };
        if ruby(part) == 1
            && let Some(group) = event.parts.get(index + 1..index + 4)
            && ruby(&group[0]) == 2
            && (3..=8).contains(&ruby(&group[1]))
            && ruby(&group[2]) == 2
        {
            let reading = clean(&group[1].text);
            if !reading.is_empty() {
                run.reading = Some(reading);
                index += 3;
            }
        }
        result.push(run);
        index += 1;
    }
    result
}

pub(crate) fn refresh(segment: &mut Segment) {
    if segment.runs.is_empty() {
        return;
    }
    segment.text = clean_text(
        &segment
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
    );
    let parts: Vec<_> = segment
        .runs
        .iter()
        .map(|run| {
            let mut part = json!({"utf8":run.text});
            if let Some(start) = run.start {
                part["tOffsetMs"] = json!((start - segment.start) * 1000.0);
            }
            part
        })
        .collect();
    segment.words = if segment
        .runs
        .iter()
        .any(|run| run.start.is_none() && caption_words::is_word(&run.text))
    {
        Vec::new()
    } else {
        caption_words::parse(&parts, segment.start, segment.duration)
    };
}

pub(crate) fn parse(source: &str) -> Result<(Vec<Segment>, Metadata)> {
    if source.len() > MAX_BYTES {
        return Err("Caption response is too large.".into());
    }
    let data = if source.trim_start().starts_with('{') {
        serde_json::from_str::<Value>(source).map_err(|_| "Unreadable JSON captions.")?
    } else {
        xml(source)?
    };
    let events = data["events"].as_array().ok_or("Missing caption events.")?;
    if events.len() > MAX_SEGMENTS * 4 {
        return Err("Too many caption events.".into());
    }
    let mut metadata = Metadata {
        pens: table(&data, "pens", PEN),
        window_styles: table(&data, "wsWinStyles", WINDOW_STYLE),
        window_positions: table(&data, "wpWinPositions", WINDOW_POSITION),
        events: Vec::new(),
    };
    for raw in events {
        let Some(start) = number(&raw["tStartMs"]).filter(|n| n.is_finite() && *n >= 0.0) else {
            continue;
        };
        let duration = number(&raw["dDurationMs"])
            .filter(|n| n.is_finite() && *n >= 0.0)
            .unwrap_or(0.0);
        let mut parts: Vec<Part> = raw["segs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| {
                let text: String = part["utf8"].as_str()?.nfkc().collect();
                Some(Part {
                    text,
                    offset: number(&part["tOffsetMs"])
                        .filter(|n| n.is_finite() && *n >= 0.0)
                        .map(|n| n / 1000.0),
                    pen_id: reference(&part["pPenId"]),
                    confidence: integer(&part["acAsrConf"], 255).map(|n| n as u8),
                    speaker_change: part["isSpeakerChange"].as_bool(),
                })
            })
            .collect();
        let mut previous = 0.0;
        let invalid_timing = raw["segs"].as_array().into_iter().flatten().any(|part| {
            let Some(value) = part.get("tOffsetMs") else {
                return false;
            };
            let Some(offset) = number(value).filter(|n| n.is_finite() && *n >= previous) else {
                return true;
            };
            previous = offset;
            duration > 0.0 && offset >= duration
        });
        if invalid_timing {
            for part in &mut parts {
                part.offset = None;
            }
        }
        metadata.events.push(Event {
            start: start / 1000.0,
            duration: duration / 1000.0,
            id: integer(&raw["id"], 1_000_000),
            window_id: integer(&raw["wWinId"], 1_000_000),
            append: raw["aAppend"] == true || integer(&raw["aAppend"], 1) == Some(1),
            pen_id: reference(&raw["pPenId"]),
            window_style_id: reference(&raw["wsWinStyleId"]),
            window_position_id: reference(&raw["wpWinPosId"]),
            parts,
        });
    }
    let mut segments: Vec<Segment> = Vec::new();
    let mut windows: HashMap<Option<u32>, usize> = HashMap::new();
    let mut pending_break = HashMap::new();
    let mut ordered: Vec<_> = metadata.events.iter().collect();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    for event in ordered {
        if event.parts.is_empty() {
            windows.remove(&event.id.or(event.window_id));
            pending_break.remove(&event.id.or(event.window_id));
            continue;
        }
        let next_runs = runs(event, &metadata);
        let content = next_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let prior = windows.get(&event.window_id).copied().filter(|index| {
            let line = &segments[*index];
            event.start >= line.start
                && (line.duration == 0.0 || event.start <= line.start + line.duration + 0.25)
        });
        let after_break = pending_break.remove(&event.window_id).unwrap_or(false);
        let continuation = event.append
            || after_break
                && prior.is_some_and(|index| {
                    let line = &segments[index];
                    event.start - line.start < 12.0
                        && line.text.chars().count() < 300
                        && !line.text.ends_with(['.', '?', '!', '。', '？', '！'])
                });
        if continuation && let Some(index) = prior {
            let line = &mut segments[index];
            if line.text.len() + content.len() <= 50_000 {
                line.duration = if event.duration == 0.0 {
                    0.0
                } else {
                    (line.start + line.duration).max(event.start + event.duration) - line.start
                };
                line.runs.extend(next_runs);
                refresh(line);
                if event.append && content.trim().is_empty() && content.contains('\n') {
                    pending_break.insert(event.window_id, true);
                }
                continue;
            }
        }
        if content.trim().is_empty() {
            continue;
        }
        let mut line = Segment {
            start: event.start,
            duration: event.duration,
            text: String::new(),
            words: Vec::new(),
            runs: next_runs,
            window_id: event.window_id,
        };
        refresh(&mut line);
        windows.insert(event.window_id, segments.len());
        segments.push(line);
        if segments.len() > MAX_SEGMENTS {
            return Err("Too many caption lines.".into());
        }
    }
    for line in &mut segments {
        if !line.runs.iter().any(|run| {
            run.start.is_some()
                || run.pen_id.is_some()
                || run.reading.is_some()
                || run.confidence.is_some()
                || run.speaker_change.is_some()
        }) {
            line.runs.clear();
        }
    }
    Ok((segments, metadata))
}

fn xml(source: &str) -> Result<Value> {
    let document = roxmltree::Document::parse(source).map_err(|_| "Unreadable XML captions.")?;
    let mut result = json!({"events":[],"pens":[],"wsWinStyles":[],"wpWinPositions":[]});
    for (tag, key, fields) in [
        ("pen", "pens", PEN),
        ("ws", "wsWinStyles", WINDOW_STYLE),
        ("wp", "wpWinPositions", WINDOW_POSITION),
    ] {
        for node in document.descendants().filter(|n| n.has_tag_name(tag)) {
            let Some(id) = node
                .attribute("id")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n < 1024)
            else {
                continue;
            };
            let mut item = json!({});
            for (xml, json, max) in fields {
                if let Some(value) = node.attribute(*xml) {
                    let value = if value.starts_with('#') {
                        u32::from_str_radix(value.trim_start_matches('#'), 16).ok()
                    } else {
                        value.parse::<u32>().ok()
                    };
                    if let Some(value) = value.filter(|value| value <= max) {
                        item[*json] = json!(value);
                    }
                }
            }
            let items = result[key].as_array_mut().unwrap();
            items.resize(items.len().max(id + 1), json!({}));
            items[id] = item;
        }
    }
    for node in document
        .descendants()
        .filter(|n| n.has_tag_name("text") || n.has_tag_name("p") || n.has_tag_name("w"))
    {
        let legacy = node.has_tag_name("text");
        let factor = if legacy { 1000.0 } else { 1.0 };
        let numeric = |key| node.attribute(key).and_then(|s| s.parse::<f64>().ok());
        let Some(start) = numeric(if legacy { "start" } else { "t" }) else {
            continue;
        };
        let mut event = json!({"tStartMs":start * factor,"dDurationMs":numeric(if legacy {"dur"} else {"d"}).unwrap_or(0.0) * factor,"segs":[]});
        for (xml, json) in [
            ("id", "id"),
            ("w", "wWinId"),
            ("a", "aAppend"),
            ("p", "pPenId"),
            ("ws", "wsWinStyleId"),
            ("wp", "wpWinPosId"),
        ] {
            if let Some(value) = numeric(xml) {
                event[json] = json!(value);
            }
        }
        if !node.has_tag_name("w") {
            for leaf in node
                .descendants()
                .filter(|n| n.is_text() || n.has_tag_name("br"))
            {
                let text = if leaf.has_tag_name("br") {
                    "\n"
                } else {
                    leaf.text().unwrap_or("")
                };
                let mut part = json!({"utf8":html_escape::decode_html_entities(text)});
                if let Some(span) = leaf.ancestors().find(|n| n.has_tag_name("s")) {
                    for (xml, json) in [("t", "tOffsetMs"), ("p", "pPenId"), ("ac", "acAsrConf")] {
                        if let Some(value) = span.attribute(xml).and_then(|s| s.parse::<f64>().ok())
                        {
                            part[json] = json!(value);
                        }
                    }
                }
                event["segs"].as_array_mut().unwrap().push(part);
            }
        }
        result["events"].as_array_mut().unwrap().push(event);
    }
    Ok(result)
}

pub(crate) fn validate(metadata: &Metadata) -> bool {
    let valid_table = |table: &[Attributes], fields: &[(&str, &str, u32)]| {
        table.len() <= 1024
            && table.iter().all(|item| {
                item.iter().all(|(key, value)| {
                    fields
                        .iter()
                        .any(|(_, name, max)| key == name && value <= max)
                })
            })
    };
    valid_table(&metadata.pens, PEN)
        && valid_table(&metadata.window_styles, WINDOW_STYLE)
        && valid_table(&metadata.window_positions, WINDOW_POSITION)
        && metadata.events.len() <= MAX_SEGMENTS * 4
        && metadata.events.iter().all(|event| {
            event.start.is_finite()
                && event.start >= 0.0
                && event.duration.is_finite()
                && event.duration >= 0.0
                && event.parts.len() <= 50_000
                && event.parts.iter().all(|part| {
                    part.text.len() <= 50_000
                        && part.offset.is_none_or(|n| n.is_finite() && n >= 0.0)
                        && part.pen_id.is_none_or(|n| n < 1024)
                })
        })
}

pub(crate) fn details(metadata: Option<&Metadata>) -> String {
    let Some(data) = metadata else {
        return "Refresh this saved transcript to load caption details.".into();
    };
    let parts: Vec<_> = data.events.iter().flat_map(|event| &event.parts).collect();
    let confidence: Vec<_> = parts.iter().filter_map(|part| part.confidence).collect();
    let scores = match (confidence.iter().min(), confidence.iter().max()) {
        (Some(min), Some(max)) if min == max => format!(
            "Recognition scores: constant {min}; not useful for identifying uncertain words."
        ),
        (Some(min), Some(max)) => format!(
            "Recognition scores: {min}–{max} across {} fragments; uncalibrated, not an accuracy percentage.",
            confidence.len()
        ),
        _ => "Recognition scores: unavailable.".into(),
    };
    format!(
        "{} source cues · {} continuation markers · {} caption windows\n{} line breaks · {} pronunciation styles\n{scores}\n{} possible speech-boundary flags. Dividers mark possible speaker changes, without identifying speakers.",
        data.events.iter().filter(|e| !e.parts.is_empty()).count(),
        data.events.iter().filter(|e| e.append).count(),
        data.events.iter().filter(|e| e.id.is_some()).count(),
        parts
            .iter()
            .map(|p| p.text.matches('\n').count())
            .sum::<usize>(),
        data.pens
            .iter()
            .filter(|pen| pen.get("rbRuby").is_some_and(|n| (3..=8).contains(n)))
            .count(),
        parts
            .iter()
            .filter(|p| p.speaker_change == Some(true))
            .count()
    )
}
