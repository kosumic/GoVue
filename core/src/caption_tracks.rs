//! Caption/audio associations and explicit, source-scoped translation choices.
use crate::{Result, Track, Transcript, clean};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Language {
    pub code: String,
    pub label: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Audio {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub original: bool,
    pub caption_track_ids: Vec<String>,
    pub default_caption_track_id: Option<String>,
    pub captions_initial_state: Option<String>,
    pub has_default_track: Option<bool>,
    pub visibility: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Metadata {
    pub translation_languages: Vec<Language>,
    pub audio_tracks: Vec<Audio>,
    pub default_audio_track_id: Option<String>,
}

fn text(value: &Value) -> String {
    if let Some(text) = value.as_str().or_else(|| value["simpleText"].as_str()) {
        return clean(text);
    }
    clean(
        &value["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|run| run["text"].as_str())
            .collect::<String>(),
    )
}
fn short(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| s.len() <= 512 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}
pub(crate) fn language(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub(crate) fn extract(data: &Value, audio_key: &str) -> (Vec<Track>, Metadata) {
    extract_for_source(data, audio_key, None)
}

pub(crate) fn extract_for_source(
    data: &Value,
    audio_key: &str,
    source_id: Option<&str>,
) -> (Vec<Track>, Metadata) {
    let list = &data["captions"]["playerCaptionsTracklistRenderer"];
    let mut tracks: Vec<_> = list["captionTracks"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .enumerate()
        .map(|(index, raw)| Track {
            id: short(&raw["vssId"]).unwrap_or_else(|| {
                format!("{}:{index}", raw["languageCode"].as_str().unwrap_or(""))
            }),
            label: text(&raw["name"]),
            language: raw["languageCode"].as_str().unwrap_or("").into(),
            automatic: raw["kind"] == "asr",
            translatable: raw["isTranslatable"] == true,
            audio_track_ids: Vec::new(),
            translated_from: None,
        })
        .collect();
    let mut audio_tracks: Vec<_> = list["audioTracks"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .enumerate()
        .map(|(index, raw)| {
            let id = short(&raw["audioTrackId"]).unwrap_or_else(|| format!("index:{index}"));
            let audio = data["streamingData"]["adaptiveFormats"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|format| format["audioTrack"]["id"] == id)
                .map(|format| &format["audioTrack"]);
            let label = audio
                .map(|audio| text(&audio["displayName"]))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| id.clone());
            let original = audio.is_some_and(|audio| audio["isOriginal"] == true)
                || label.to_lowercase().contains("original");
            Audio {
                id,
                label,
                original,
                caption_track_ids: raw["captionTrackIndices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| tracks.get(usize::try_from(n.as_u64()?).ok()?))
                    .map(|t| t.id.clone())
                    .collect(),
                default_caption_track_id: raw["defaultCaptionTrackIndex"]
                    .as_u64()
                    .and_then(|n| tracks.get(usize::try_from(n).ok()?))
                    .map(|t| t.id.clone()),
                captions_initial_state: short(&raw["captionsInitialState"]),
                has_default_track: raw["hasDefaultTrack"].as_bool(),
                visibility: short(&raw["visibility"]),
            }
        })
        .collect();
    for audio in &mut audio_tracks {
        if let Some(default) = &audio.default_caption_track_id
            && !audio.caption_track_ids.contains(default)
        {
            audio.caption_track_ids.push(default.clone());
        }
        audio.caption_track_ids.sort();
        audio.caption_track_ids.dedup();
        for track in &mut tracks {
            if audio.caption_track_ids.contains(&track.id) {
                track.audio_track_ids.push(audio.id.clone());
            }
        }
    }
    let default_index =
        usize::try_from(list["defaultAudioTrackIndex"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    let default_audio = audio_tracks
        .iter()
        .find(|t| !audio_key.is_empty() && t.id == audio_key)
        .or_else(|| audio_tracks.get(default_index));
    let default_audio_track_id = default_audio.map(|t| t.id.clone());
    let default_source = default_audio
        .and_then(|audio| audio.default_caption_track_id.as_ref())
        .and_then(|id| tracks.iter().find(|t| t.id == *id))
        .or_else(|| tracks.first())
        .cloned();
    let source = source_id
        .and_then(|id| tracks.iter().find(|track| track.id == id))
        .cloned()
        .or(default_source);
    let mut translation_languages: Vec<Language> = Vec::new();
    for raw in list["translationLanguages"]
        .as_array()
        .into_iter()
        .flatten()
        .take(300)
    {
        let Some(code) = raw["languageCode"].as_str().filter(|code| language(code)) else {
            continue;
        };
        if translation_languages.iter().any(|item| item.code == code) {
            continue;
        }
        translation_languages.push(Language {
            code: code.into(),
            label: text(&raw["languageName"]),
        });
    }
    if let Some(source) = source.filter(|source| source.translatable) {
        for target in &translation_languages {
            if tracks.len() >= 300 {
                break;
            }
            if target.code == source.language {
                continue;
            }
            let id = format!("translate:{}:{}", target.code, source.id);
            if id.len() > 512 || tracks.iter().any(|track| track.id == id) {
                continue;
            }
            tracks.push(Track {
                id,
                label: format!("{} · Translated from {}", target.label, source.label),
                language: target.code.clone(),
                automatic: source.automatic,
                translatable: false,
                audio_track_ids: source.audio_track_ids.clone(),
                translated_from: Some(source.id.clone()),
            });
        }
    }
    (
        tracks,
        Metadata {
            translation_languages,
            audio_tracks,
            default_audio_track_id,
        },
    )
}

pub(crate) fn default_track<'a>(tracks: &'a [Track], metadata: &Metadata) -> Option<&'a Track> {
    metadata
        .default_audio_track_id
        .as_ref()
        .and_then(|id| metadata.audio_tracks.iter().find(|audio| audio.id == *id))
        .and_then(|audio| audio.default_caption_track_id.as_ref())
        .and_then(|id| tracks.iter().find(|track| track.id == *id))
        .or_else(|| tracks.iter().find(|track| track.translated_from.is_none()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Choices<'a> {
    source_tracks: Vec<&'a Track>,
    caption_tracks: Vec<&'a Track>,
    translations: Vec<&'a Track>,
}

pub(crate) fn choices(data: &Transcript) -> Choices<'_> {
    let native = |track: &&Track| track.translated_from.is_none();
    let audio_source = |audio: &Audio| {
        audio
            .default_caption_track_id
            .as_ref()
            .and_then(|id| {
                data.tracks
                    .iter()
                    .filter(native)
                    .find(|track| track.id == *id)
            })
            .or_else(|| {
                data.tracks
                    .iter()
                    .filter(native)
                    .find(|track| audio.caption_track_ids.contains(&track.id))
            })
    };
    let source = data.track_metadata.as_ref().and_then(|metadata| {
        metadata
            .audio_tracks
            .iter()
            // Older cached metadata still has the audio display name.
            .filter(|audio| audio.original || audio.label.to_lowercase().contains("original"))
            .find_map(audio_source)
            .or_else(|| {
                metadata
                    .audio_tracks
                    .iter()
                    .find(|audio| Some(&audio.id) == metadata.default_audio_track_id.as_ref())
                    .and_then(audio_source)
            })
    });
    let source = source.or_else(|| data.tracks.iter().find(native));
    let mut choices = Choices {
        source_tracks: Vec::new(),
        caption_tracks: Vec::new(),
        translations: Vec::new(),
    };
    for track in &data.tracks {
        let source_language = source.is_some_and(|source| {
            source.id == track.id
                || (!source.language.is_empty()
                    && source.language.eq_ignore_ascii_case(&track.language))
        });
        if track.translated_from.is_some() {
            choices.translations.push(track);
        } else if source_language {
            choices.source_tracks.push(track);
        } else if track.automatic {
            choices.translations.push(track);
        } else {
            choices.caption_tracks.push(track);
        }
    }
    choices
}

pub(crate) fn matches_audio(data: &Transcript, audio: &str) -> bool {
    audio.is_empty()
        || data
            .tracks
            .iter()
            .find(|t| t.id == data.track_id)
            .is_some_and(|track| track.audio_track_ids.iter().any(|id| id == audio))
}

pub(crate) fn description(data: &Transcript) -> String {
    let Some(track) = data.tracks.iter().find(|track| track.id == data.track_id) else {
        return String::new();
    };
    let mut lines = Vec::new();
    if let Some(source) = track
        .translated_from
        .as_ref()
        .and_then(|id| data.tracks.iter().find(|track| track.id == *id))
    {
        lines.push(format!(
            "YouTube machine translation from {}.",
            source.label
        ));
    }
    if let Some(metadata) = &data.track_metadata {
        let audio: Vec<_> = metadata
            .audio_tracks
            .iter()
            .filter(|audio| track.audio_track_ids.contains(&audio.id))
            .map(|audio| audio.label.as_str())
            .collect();
        if !audio.is_empty() {
            lines.push(format!("Associated audio: {}.", audio.join(", ")));
        }
        lines.push(format!(
            "{} available translation languages.",
            metadata.translation_languages.len()
        ));
    }
    lines.join("\n")
}

pub(crate) fn validate(data: &Transcript) -> Result<()> {
    for track in &data.tracks {
        if track.audio_track_ids.len() > 200
            || track.audio_track_ids.iter().any(|id| id.len() > 512)
            || track.translated_from.as_ref().is_some_and(|source| {
                !language(&track.language)
                    || !data
                        .tracks
                        .iter()
                        .any(|t| t.id == *source && t.translatable)
            })
        {
            return Err("Invalid caption association.".into());
        }
    }
    if let Some(metadata) = &data.track_metadata
        && (metadata.translation_languages.len() > 300
            || metadata.audio_tracks.len() > 200
            || metadata
                .translation_languages
                .iter()
                .any(|l| !language(&l.code) || l.label.len() > 1000)
            || metadata.audio_tracks.iter().any(|audio| {
                audio.id.len() > 512
                    || audio.label.len() > 1000
                    || audio.caption_track_ids.len() > 200
                    || audio
                        .caption_track_ids
                        .iter()
                        .any(|id| !data.tracks.iter().any(|t| t.id == *id))
                    || [&audio.visibility, &audio.captions_initial_state]
                        .iter()
                        .any(|v| v.as_ref().is_some_and(|s| s.len() > 512))
            }))
    {
        return Err("Invalid caption track metadata.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audio_selection_scopes_translations_and_keeps_native_alternatives() {
        let player = json!({"captions":{"playerCaptionsTracklistRenderer":{
            "captionTracks":[
                {"vssId":".en","languageCode":"en","name":{"simpleText":"English"},"isTranslatable":true,"baseUrl":"never-persist"},
                {"vssId":".ja","languageCode":"ja","name":{"simpleText":"Japanese"},"isTranslatable":true},
                {"vssId":".es","languageCode":"es","name":{"simpleText":"Spanish"}}
            ],
            "audioTracks":[{"audioTrackId":"en.4","captionTrackIndices":[0,2],"defaultCaptionTrackIndex":0},
                {"audioTrackId":"ja.4","captionTrackIndices":[1],"defaultCaptionTrackIndex":1,"hasDefaultTrack":true,"captionsInitialState":"ON","visibility":"ON"}],
            "defaultAudioTrackIndex":0,"translationLanguages":[
                {"languageCode":"es","languageName":{"simpleText":"Spanish"}},
                {"languageCode":"fr&pot=bad","languageName":{"simpleText":"Invalid"}},
                {"languageCode":"ja","languageName":{"simpleText":"Japanese"}}]
        }},"streamingData":{"adaptiveFormats":[
            {"audioTrack":{"id":"en.4","displayName":"English audio","isOriginal":true}},
            {"audioTrack":{"id":"ja.4","displayName":"Japanese audio"}}
        ]}});
        let (tracks, metadata) = extract(&player, "ja.4");
        assert_eq!(default_track(&tracks, &metadata).unwrap().id, ".ja");
        let translated = tracks.iter().find(|t| t.id == "translate:es:.ja").unwrap();
        assert_eq!(translated.translated_from.as_deref(), Some(".ja"));
        assert_eq!(translated.audio_track_ids, ["ja.4"]);
        assert!(
            tracks.iter().any(|t| t.id == ".es"),
            "native Spanish remains independently selectable"
        );
        assert_eq!(metadata.translation_languages.len(), 2);
        assert!(metadata.audio_tracks[0].original);
        assert!(!metadata.audio_tracks[1].original);
        assert_eq!(metadata.audio_tracks[1].label, "Japanese audio");
        let serialized = json!({"tracks":tracks,"metadata":metadata}).to_string();
        assert!(!serialized.contains("never-persist"));
        let (tracks, metadata) = extract(&player, "");
        assert_eq!(default_track(&tracks, &metadata).unwrap().id, ".en");
        assert!(tracks.iter().any(|t| t.id == "translate:es:.en"));
        let (tracks, _) = extract_for_source(&player, "en.4", Some(".ja"));
        assert!(
            tracks.iter().any(|t| t.id == "translate:es:.ja"),
            "an explicit caption keeps its translation source when audio changes"
        );
    }
}
