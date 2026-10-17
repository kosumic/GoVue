//! Vocabulary biasing stays local and bounded before crossing into the recognizer.
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

fn words<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for text in texts {
        let normalized: String = text.nfkc().collect();
        for word in normalized.unicode_words() {
            let length = word.chars().count();
            if length > 48
                || !word.chars().any(char::is_alphabetic)
                || (length == 1 && word.is_ascii())
            {
                continue;
            }
            let key = word.to_lowercase();
            if let Some((_, count)) = counts.get_mut(&key) {
                *count += 1;
            } else if counts.len() < 4096 {
                counts.insert(key, (word.into(), 1));
            }
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.1
            .cmp(&a.1.1)
            .then(b.0.len().cmp(&a.0.len()))
            .then(a.0.cmp(&b.0))
    });
    ranked
        .into_iter()
        .take(100)
        .map(|(_, (word, _))| word)
        .collect()
}

pub(crate) fn hints<'a, 'b>(
    chat: impl Iterator<Item = &'a str>,
    transcript: impl Iterator<Item = &'b str>,
) -> Vec<String> {
    let chat = words(chat);
    let transcript = words(transcript);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    // Interleave sources so a long transcript cannot crowd out the conversation.
    for index in 0..chat.len().max(transcript.len()) {
        for word in [chat.get(index), transcript.get(index)]
            .into_iter()
            .flatten()
        {
            if seen.insert(word.to_lowercase()) {
                result.push(word.clone());
                if result.len() == 100 {
                    return result;
                }
            }
        }
    }
    result
}
