//! Local literal search. Offsets identify matches in UTF-8, never JS string indices.
use super::*;

const MAX_MATCHES: usize = 500;
const CONTEXT: usize = 48;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Hit {
    pub message_id: Option<String>,
    pub role: Option<String>,
    pub offset: usize,
    pub before: String,
    pub matched: String,
    pub after: String,
    pub leading: bool,
    pub trailing: bool,
}

pub(super) fn query(c: &Value) -> Result<String> {
    let query = c
        .get("query")
        .map_or(Ok(""), |v| v.as_str().ok_or("Invalid search query."))?;
    if query.chars().count() > 200 {
        return Err("Search query is too long.".into());
    }
    Ok(query.trim().to_owned())
}

fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn occurrences(text: &str, needle: &str, limit: usize) -> Vec<Hit> {
    if needle.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let mut folded = String::with_capacity(text.len());
    let mut boundaries = Vec::new();
    if text.is_ascii() {
        folded = text.to_ascii_lowercase();
    } else {
        for (offset, character) in text.char_indices() {
            boundaries.push((folded.len(), offset));
            folded.extend(character.to_lowercase());
        }
        boundaries.push((folded.len(), text.len()));
    }
    let mut hits = Vec::new();
    let mut previous = None;
    for (offset, matched) in folded.match_indices(needle) {
        let (start, end) = if boundaries.is_empty() {
            (offset, offset + matched.len())
        } else {
            let first = boundaries.partition_point(|&(p, _)| p <= offset) - 1;
            let last = boundaries.partition_point(|&(p, _)| p < offset + matched.len());
            (boundaries[first].1, boundaries[last].1)
        };
        if previous == Some((start, end)) {
            continue;
        }
        previous = Some((start, end));
        let before = text[..start]
            .char_indices()
            .rev()
            .nth(CONTEXT - 1)
            .map_or(0, |(p, _)| p);
        let after = text[end..]
            .char_indices()
            .nth(CONTEXT)
            .map_or(text.len(), |(p, _)| end + p);
        hits.push(Hit {
            message_id: None,
            role: None,
            offset: start,
            before: text[before..start].into(),
            matched: text[start..end].into(),
            after: text[end..after].into(),
            leading: before > 0,
            trailing: after < text.len(),
        });
        if hits.len() == limit {
            break;
        }
    }
    hits
}

pub(super) fn first(text: &str, query: &str) -> Option<Hit> {
    occurrences(text, &fold(query), 1).into_iter().next()
}

pub(super) fn find<'a>(
    c: &Value,
    messages: impl Iterator<Item = (&'a str, &'a str, &'a str)>,
) -> Result<Value> {
    let query = query(c)?;
    let needle = fold(&query);
    let mut matches = Vec::new();
    for (id, role, text) in messages {
        for mut hit in occurrences(text, &needle, MAX_MATCHES + 1 - matches.len()) {
            hit.message_id = Some(id.into());
            hit.role = Some(role.into());
            matches.push(hit);
        }
        if matches.len() > MAX_MATCHES {
            break;
        }
    }
    let truncated = matches.len() > MAX_MATCHES;
    matches.truncate(MAX_MATCHES);
    let previous = &c["previous"];
    let mut selected = (previous["query"] == query)
        .then(|| {
            matches.iter().position(|hit| {
                hit.message_id.as_deref() == previous["messageId"].as_str()
                    && Some(hit.offset as u64) == previous["offset"].as_u64()
            })
        })
        .flatten()
        .unwrap_or(0);
    if !matches.is_empty() {
        selected = match c["direction"].as_str() {
            Some("next") => (selected + 1) % matches.len(),
            Some("previous") => (selected + matches.len() - 1) % matches.len(),
            None => selected,
            _ => return Err("Invalid search direction.".into()),
        };
    }
    Ok(
        json!({"query":query,"matches":matches,"selected":if matches.is_empty() { None } else { Some(selected) },"truncated":truncated}),
    )
}
