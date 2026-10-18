use crate::{Result, string};
use pulldown_cmark::{Event, Options, Parser, TagEnd};
use serde_json::{Value, json};

/// Prepare only the visible response text; reasoning and tool activity never enter TTS.
pub(crate) fn plan(command: &Value) -> Result<Value> {
    let markdown = string(command, "text")?;
    if markdown.len() > 2_000_000 || markdown.contains('\0') {
        return Err("This response is too large or contains unsupported speech characters.".into());
    }
    let limit = command["maxLength"]
        .as_u64()
        .filter(|limit| *limit >= 2)
        .ok_or("Invalid speech input limit.")?
        .min(4000) as usize;
    let mut text = String::new();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak | Event::Rule | Event::InlineHtml(_) => {
                text.push(' ');
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::TableCell,
            ) => text.push(' '),
            _ => {}
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err("This response has no text to read aloud.".into());
    }
    // Android counts UTF-16 units. Prefer sentence/word boundaries and never cut a surrogate pair.
    let mut remaining = text.as_str();
    let mut chunks = Vec::new();
    while !remaining.is_empty() {
        let (mut units, mut sentence, mut word) = (0, None, None);
        let mut end = remaining.len();
        for (index, character) in remaining.char_indices() {
            units += character.len_utf16();
            if units > limit {
                end = sentence.or(word).unwrap_or(index);
                break;
            }
            if matches!(character, '.' | '!' | '?' | '。' | '！' | '？' | ';' | '；') {
                sentence = Some(index + character.len_utf8());
            } else if character.is_whitespace() {
                word = Some(index + character.len_utf8());
            }
        }
        let chunk = remaining[..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        remaining = remaining[end..].trim_start();
    }
    Ok(json!({"chunks": chunks}))
}
