//! Reader choices use languages, never video-specific track IDs or caption URLs.
use crate::{Result, Track, Transcript, caption_tracks};
use serde::{Deserialize, Serialize};

const FILE: &str = "transcript-preferences.json";

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Captions,
    Automatic,
    Translated,
}

impl Kind {
    fn of(track: &Track) -> Self {
        if track.translated_from.is_some() {
            Self::Translated
        } else if track.automatic {
            Self::Automatic
        } else {
            Self::Captions
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Choice {
    Source,
    Language { language: String, kind: Kind },
}

pub(crate) fn save(root: &str, data: &Transcript, ids: &[&str]) -> Result<()> {
    let choices = caption_tracks::choices(data);
    let tracks = ids
        .iter()
        .filter_map(|id| data.tracks.iter().find(|track| track.id == *id))
        .map(|track| {
            if choices
                .source_tracks
                .iter()
                .any(|source| source.id == track.id)
            {
                Choice::Source
            } else {
                Choice::Language {
                    language: track.language.to_ascii_lowercase(),
                    kind: Kind::of(track),
                }
            }
        })
        .collect::<Vec<_>>();
    crate::media::write(root, FILE, &serde_json::json!(tracks))
}

pub(crate) fn resolve(root: &str, data: &Transcript) -> Vec<String> {
    let preferences = serde_json::from_value::<Vec<Choice>>(crate::media::read(
        root,
        FILE,
        serde_json::Value::Null,
    ))
    .ok()
    .filter(|tracks| {
        !tracks.is_empty()
            && tracks.len() <= 2
            && tracks.iter().all(|track| match track {
                Choice::Source => true,
                Choice::Language { language, .. } => caption_tracks::language(language),
            })
    })
    .unwrap_or_else(|| vec![Choice::Source]);
    let choices = caption_tracks::choices(data);
    let source = choices.source_tracks.first().copied();
    let mut selected: Vec<&Track> = Vec::new();
    for preference in preferences {
        let track = match preference {
            Choice::Source => source,
            Choice::Language { language, kind } => data
                .tracks
                .iter()
                .filter(|track| track.language.eq_ignore_ascii_case(&language))
                .min_by_key(|track| Kind::of(track) != kind),
        };
        if let Some(track) = track
            && !selected
                .iter()
                .any(|other| other.language.eq_ignore_ascii_case(&track.language))
        {
            selected.push(track);
        }
    }
    // Unavailable languages do not overwrite the saved choices. They can return
    // on the next video, while this one always has a usable source transcript.
    if selected.is_empty()
        && let Some(source) = source
    {
        selected.push(source);
    }
    if selected.is_empty() {
        return vec![data.track_id.clone()];
    }
    selected.iter().map(|track| track.id.clone()).collect()
}
