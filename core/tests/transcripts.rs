#![cfg(test)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "Keep each temporary filesystem fixture alive through its entire test scenario."
)]

use govue_core::execute;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

const VIDEO: &str = "abcdefghijk";
static NEXT: AtomicUsize = AtomicUsize::new(0);
// A scenario spans multiple calls into the process-wide three-document LRU.
// Other scenarios must not evict an unsaved document between accept and export.
static SCENARIOS: Mutex<()> = Mutex::new(());

struct Store {
    path: PathBuf,
    _scenario: MutexGuard<'static, ()>,
}
impl Store {
    fn new() -> Self {
        let scenario = SCENARIOS.lock().unwrap();
        let path = std::env::temp_dir().join(format!(
            "govue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self {
            path,
            _scenario: scenario,
        }
    }
    fn call(&self, command: Value) -> Value {
        serde_json::from_str(&execute(
            command.to_string(),
            self.path.to_str().unwrap().into(),
        ))
        .unwrap()
    }
    fn value(&self, command: Value) -> Value {
        let reply = self.call(command);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn reply() -> Value {
    json!({"type":"transcript","requestId":"request:1","videoId":VIDEO,"ok":true,"transcript":{
        "videoId":VIDEO,"title":"Example","language":"English","trackId":".en",
        "tracks":[{"id":".en","label":"English","language":"en","automatic":false}],
        "segments":[{"start":12.5,"duration":2,"text":"Hello, world."}]
    }})
}
fn accept(reply: Value) -> Value {
    json!({"op":"accept","videoId":VIDEO,"requestId":"request:1","origin":"https://www.youtube.com","raw":reply.to_string()})
}
fn operation(op: &str) -> Value {
    json!({"op":op,"videoId":VIDEO,"trackId":".en"})
}

fn playback(store: &Store, cursor: Value, position: Value, changes: Value) -> Value {
    let mut command = operation("transcript_playback");
    command["cursor"] = cursor;
    command["position"] = position;
    command["playing"] = json!(true);
    command["rate"] = json!(1.0);
    command["elapsed"] = json!(0.2);
    for (key, value) in changes.as_object().unwrap() {
        command[key] = value.clone();
    }
    store.value(command)
}

#[test]
fn line_pauses_cover_gaps_overlaps_missing_durations_and_resume() {
    let store = Store::new();
    let mut response = reply();
    response["transcript"]["segments"] = json!([
        {"start":0,"duration":1,"text":"First"},
        {"start":2,"duration":0,"text":"Missing duration"},
        {"start":4,"duration":4,"text":"Overlapping"},
        {"start":6,"duration":1,"text":"Final timed line"},
        {"start":8,"duration":0,"text":"Unknown ending"}
    ]);
    store.value(accept(response));
    let mut cursor = Value::Null;
    for (position, expected) in [
        (0.9, false),
        (1.01, true),
        (1.01, false),
        (1.1, false),
        (1.2, false), // The gap does not cause another pause.
        (3.9, false),
        (4.01, true), // Missing duration ends at the next line.
        (5.9, false),
        (6.01, true), // The next line cuts off an overlap.
        (6.9, false),
        (7.01, true), // The final timed line still pauses.
        (8.9, false),
        (9.01, false), // Do not invent a final duration.
    ] {
        let result = playback(&store, cursor, json!(position), json!({}));
        assert_eq!(result["pause"], expected, "position {position}");
        cursor = result["cursor"].clone();
    }
}

#[test]
fn line_pauses_rearm_after_seeks_ads_and_delayed_or_paused_samples() {
    let store = Store::new();
    store.value(accept(reply()));
    let initial = playback(&store, Value::Null, json!(14.4), json!({}));
    assert_eq!(initial["pause"], false);
    let cursor = initial["cursor"].clone();
    assert_eq!(
        playback(&store, cursor.clone(), json!(14.51), json!({}))["pause"],
        true
    );
    for (position, changes) in [
        (json!(30), json!({})),
        (json!(13), json!({})),
        (json!(14.51), json!({"seeking":true})),
        (json!(14.51), json!({"playing":false})),
        (json!(14.51), json!({"elapsed":2})),
        (Value::Null, json!({})),
    ] {
        let result = playback(&store, cursor.clone(), position, changes);
        assert_eq!(result["pause"], false);
    }
    let reset = playback(&store, cursor, Value::Null, json!({}));
    assert!(reset["cursor"].is_null());
    assert_eq!(
        playback(&store, reset["cursor"].clone(), json!(14.6), json!({}))["pause"],
        false
    );
    let fast = playback(&store, Value::Null, json!(13.9), json!({"rate":4}));
    assert_eq!(
        playback(
            &store,
            fast["cursor"].clone(),
            json!(14.6),
            json!({"rate":4})
        )["pause"],
        true
    );
}

#[test]
fn validates_origins_and_correlates_requests_and_video_identity() {
    let store = Store::new();
    for origin in [
        "https://www.youtube.com",
        "https://m.youtube.com/",
        "https://music.youtube.com/",
        "https://www.youtube.com/watch?v=abcdefghijk",
        "https://m.youtube.com/shorts/abcdefghijk",
    ] {
        let mut command = accept(reply());
        command["origin"] = json!(origin);
        assert_eq!(store.value(command)["ok"], true);
    }
    for origin in [
        "https://www.youtube.com.evil.test",
        "http://www.youtube.com",
        "https://www.youtube.com:444",
        "https://user:pass@www.youtube.com",
        "https://www.youtube.com/watch?v=0123456789a",
        "https://www.youtube.com/channel/example",
    ] {
        let mut command = accept(reply());
        command["origin"] = json!(origin);
        assert_eq!(store.call(command)["ok"], false, "{origin}");
    }
    for field in ["requestId", "videoId"] {
        let mut response = reply();
        response[field] = json!("0123456789a");
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    let mut response = reply();
    response["transcript"]["videoId"] = json!("0123456789a");
    assert_eq!(store.call(accept(response))["ok"], false);
}

#[test]
fn rejects_malformed_or_unbounded_data_and_preserves_actionable_errors() {
    let store = Store::new();
    for segments in [
        json!([]),
        json!([{"start":-1,"duration":2,"text":"bad"}]),
        json!([{"start":0,"duration":-1,"text":"bad"}]),
        json!([{"start":0,"duration":1,"text":"x".repeat(50_001)}]),
    ] {
        let mut response = reply();
        response["transcript"]["segments"] = segments;
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    for tracks in [
        json!([null]),
        json!([]),
        json!([{"id":"other","label":"Other","language":"en","automatic":false}]),
    ] {
        let mut response = reply();
        response["transcript"]["tracks"] = tracks;
        assert_eq!(store.call(accept(response))["ok"], false);
    }
    for raw in ["{bad".into(), "x".repeat(8_000_001)] {
        let mut command = accept(reply());
        command["raw"] = json!(raw);
        assert_eq!(store.call(command)["ok"], false);
    }
    let mut response = reply();
    response["ok"] = json!(false);
    response["error"] = json!("No captions");
    let error = store.value(accept(response));
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"], "No captions");
}

#[test]
fn parses_raw_json3_and_keeps_lines_across_the_whole_video() {
    let store = Store::new();
    let mut response = reply();
    response["transcript"]["captionSource"] = json!(json!({"events":[
        {"tStartMs":490_000,"dDurationMs":3000,"segs":[{"utf8":"Later line"}]},
        {"tStartMs":7000,"dDurationMs":2000,"segs":[{"utf8":"a new ＨｏｍｅＰｏｄ "},{"utf8":" mini 2,"}]},
        {"tStartMs":45000,"dDurationMs":2000,"segs":[{"utf8":" Inside\n the gap "}]},
        {"tStartMs":-1,"segs":[{"utf8":"invalid timing"}]},
        {"tStartMs":10,"segs":[{"utf8":"\n"}]}
    ]}).to_string());
    let data = store.value(accept(response));
    assert_eq!(
        data["transcript"]["segments"],
        json!([
            {"start":7.0,"duration":2.0,"text":"a new HomePod mini 2,"},
            {"start":45.0,"duration":2.0,"text":"Inside\nthe gap"},
            {"start":490.0,"duration":3.0,"text":"Later line"}
        ])
    );
    let mut search = operation("search");
    search["query"] = json!("ＨＯＭＥＰＯＤ");
    assert_eq!(store.value(search.clone()), json!([0]));
    search["query"] = json!("");
    assert_eq!(store.value(search), json!([0, 1, 2]));
    assert_eq!(
        store.value(operation("load"))["segments"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn decodes_legacy_and_srv3_xml_with_entity_and_word_boundaries() {
    let store = Store::new();
    for (source, expected) in [
        (
            "<transcript><text start=\"1.5\" dur=\"2\">Tom &amp;amp; Jerry</text></transcript>",
            json!({"start":1.5,"duration":2.0,"text":"Tom & Jerry"}),
        ),
        (
            "<timedtext><body><p t=\"3601000\" d=\"2500\"><s>Hello </s><s>world</s></p></body></timedtext>",
            json!({"start":3601.0,"duration":2.5,"text":"Hello world"}),
        ),
    ] {
        let mut response = reply();
        response["transcript"]["captionSource"] = json!(source);
        assert_eq!(
            store.value(accept(response))["transcript"]["segments"],
            json!([expected])
        );
    }
    assert!(
        store
            .value(operation("export"))
            .as_str()
            .unwrap()
            .contains("1:00:01  Hello world")
    );
}

#[test]
fn restores_normalized_disk_cache_without_raw_sources_or_session_fields() {
    let store = Store::new();
    let mut response = reply();
    response["transcript"]["captionSource"] =
        json!("<transcript><text start=\"12.5\" dur=\"2\">Hello, world.</text></transcript>");
    response["transcript"]["signedUrl"] = json!("private-session-token");
    assert_eq!(store.value(accept(response))["cacheSaved"], true);
    let files: Vec<_> = fs::read_dir(&store.path)
        .unwrap()
        .map(|f| f.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let saved = fs::read_to_string(&files[0]).unwrap();
    assert!(!saved.contains("captionSource"));
    assert!(!saved.contains("private-session-token"));
    let restore = json!({"op":"restore","videoId":VIDEO});
    let restored = store.value(restore.clone());
    assert_eq!(restored["segments"][0]["text"], "Hello, world.");
    assert_eq!(
        store.value(operation("export")),
        json!("Example\nhttps://www.youtube.com/watch?v=abcdefghijk\n\n0:12  Hello, world.")
    );
    fs::write(&files[0], "corrupt cache").unwrap();
    assert_eq!(store.value(restore), Value::Null);
}

fn bilingual(store: &Store, primary: Value, comparison: Value) {
    let mut response = reply();
    response["transcript"]["tracks"] = json!([
        {"id":".en","label":"English","language":"en","automatic":false},
        {"id":".ja","label":"Japanese","language":"ja","automatic":false},
        {"id":".fr","label":"French","language":"fr","automatic":false}
    ]);
    response["transcript"]["segments"] = primary;
    store.value(accept(response.clone()));
    response["transcript"]["trackId"] = json!(".ja");
    response["transcript"]["language"] = json!("Japanese");
    response["transcript"]["segments"] = comparison;
    store.value(accept(response));
}

#[test]
fn bilingual_reader_and_export_never_put_a_newer_caption_before_an_older_one() {
    let store = Store::new();
    bilingual(
        &store,
        json!([
            {"start":2,"duration":1,"text":"A0"},
            {"start":3,"duration":4,"text":"A1"},
            {"start":10,"duration":0,"text":"A2"},
            {"start":12,"duration":2,"text":"A3"},
            {"start":12,"duration":0,"text":"A4"}
        ]),
        json!([
            {"start":0,"duration":4,"text":"B0"},
            {"start":2.5,"duration":3,"text":"B1"},
            {"start":5,"duration":0.5,"text":"B2"},
            {"start":8,"duration":0.5,"text":"B3"},
            {"start":10,"duration":0,"text":"B4"},
            {"start":11,"duration":0.5,"text":"B5"},
            {"start":12,"duration":3,"text":"B6"},
            {"start":16,"duration":0,"text":"B7"}
        ]),
    );
    let mut command = operation("transcript_reader");
    command["comparisonTrackId"] = json!(".ja");
    let result = store.value(command.clone());
    let order: Vec<_> = result["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["lines"].as_array().unwrap())
        .map(|line| {
            (
                line["track"].as_u64().unwrap(),
                line["index"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![
            (1, 0),
            (0, 0),
            (1, 1),
            (0, 1),
            (1, 2),
            (1, 3),
            (0, 2),
            (1, 4),
            (1, 5),
            (0, 3),
            (0, 4),
            (1, 6),
            (1, 7)
        ]
    );
    command["op"] = json!("transcript_export");
    let exported = store.value(command);
    let lines: Vec<_> = exported.as_str().unwrap().lines().skip(3).collect();
    assert_eq!(
        lines.len(),
        order.len(),
        "Every caption appears exactly once"
    );
    for (line, (track, index)) in lines.iter().zip(order) {
        assert!(
            line.ends_with(&format!("{}{}", if track == 0 { "A" } else { "B" }, index)),
            "{line}"
        );
    }
}

#[test]
fn bilingual_search_keeps_chronological_context_and_restores_each_language() {
    let store = Store::new();
    bilingual(
        &store,
        json!([{"start":1,"duration":2,"text":"The matching English line"},{"start":4,"duration":1,"text":"Other"}]),
        json!([{"start":0.5,"duration":2,"text":"翻訳の行"},{"start":4,"duration":1,"text":"別の行"}]),
    );
    let mut command = operation("transcript_reader");
    command["comparisonTrackId"] = json!(".ja");
    command["query"] = json!(" 翻訳 ");
    let matches = store.value(command.clone());
    assert_eq!(
        matches["rows"],
        json!([{"start":0.5,"lines":[{"track":1,"index":0},{"track":0,"index":0}]}])
    );
    command["query"] = json!("MATCHING");
    assert_eq!(store.value(command)["rows"], matches["rows"]);
    for track in [".en", ".ja"] {
        let restored = store.value(json!({"op":"restore","videoId":VIDEO,"trackId":track}));
        assert_eq!(restored["trackId"], track);
    }
}

#[test]
fn bilingual_selection_enforces_two_distinct_tracks_and_keeps_the_last_one() {
    let store = Store::new();
    bilingual(
        &store,
        json!([{"start":0,"duration":1,"text":"Hello"}]),
        json!([{"start":0,"duration":1,"text":"こんにちは"}]),
    );
    let mut command = operation("transcript_select");
    command["trackIds"] = json!([".en"]);
    command["toggleId"] = json!(".ja");
    assert_eq!(store.value(command.clone()), json!([".en", ".ja"]));
    command["trackIds"] = json!([".en", ".ja"]);
    command["toggleId"] = json!(".fr");
    assert_eq!(store.call(command.clone())["ok"], false);
    command["toggleId"] = json!(".en");
    assert_eq!(store.value(command.clone()), json!([".ja"]));
    command["trackIds"] = json!([".en"]);
    assert_eq!(store.value(command.clone()), json!([".en"]));
    for ids in [
        json!([]),
        json!([".en", ".en"]),
        json!([".en", ".ja", ".fr"]),
    ] {
        command["trackIds"] = ids;
        assert_eq!(store.call(command.clone())["ok"], false);
    }
}

#[test]
fn caption_choices_keep_the_source_and_separate_supplied_and_machine_translations() {
    for (original, label, default_audio) in [
        (Some(true), "Spanish audio", "en.4"),
        (None, "Spanish (original)", "en.4"),
        (None, "Spanish audio", "es.4"),
    ] {
        let store = Store::new();
        let mut response = reply();
        response["transcript"]["tracks"] = json!([
            {"id":".en","label":"English","language":"en","automatic":false,"translatable":true},
            {"id":".es","label":"Spanish","language":"es","automatic":false},
            {"id":"a.es","label":"Spanish auto","language":"es","automatic":true},
            {"id":"a.fr","label":"French auto","language":"fr","automatic":true},
            {"id":"translate:es:.en","label":"Spanish translated from English","language":"es","automatic":false,"translatedFrom":".en"}
        ]);
        let mut source_audio = json!({"id":"es.4","label":label,"captionTrackIds":[".es","a.es"],"defaultCaptionTrackId":".es"});
        if let Some(original) = original {
            source_audio["original"] = json!(original);
        }
        response["transcript"]["trackMetadata"] = json!({
            "translationLanguages":[{"code":"es","label":"Spanish"}],
            "audioTracks":[
                {"id":"en.4","label":"English audio","captionTrackIds":[".en"],"defaultCaptionTrackId":".en"},
                source_audio
            ],
            "defaultAudioTrackId":default_audio
        });
        store.value(accept(response.clone()));
        let read = operation("transcript_reader");
        let choices = store.value(read.clone())["trackChoices"].clone();
        let ids = |group: &str| -> Vec<&str> {
            choices[group]
                .as_array()
                .unwrap()
                .iter()
                .map(|track| track["id"].as_str().unwrap())
                .collect()
        };
        assert_eq!(ids("sourceTracks"), [".es", "a.es"]);
        assert_eq!(ids("captionTracks"), [".en"]);
        assert_eq!(ids("translations"), ["a.fr", "translate:es:.en"]);
        let mut search = read;
        search["query"] = json!("no matching caption text");
        assert_eq!(store.value(search)["trackChoices"], choices);
        response["transcript"]["trackId"] = json!("a.fr");
        response["transcript"]["language"] = json!("French auto");
        store.value(accept(response));
        assert_eq!(
            store.value(json!({"op":"transcript_reader","videoId":VIDEO,"trackId":"a.fr"}))["trackChoices"],
            choices,
            "Changing the selected caption must not turn it into the source language"
        );
    }
}

#[test]
fn legacy_captions_without_audio_metadata_keep_a_source_and_group_other_languages() {
    let store = Store::new();
    bilingual(
        &store,
        json!([{"start":0,"duration":1,"text":"Hello"}]),
        json!([{"start":0,"duration":1,"text":"こんにちは"}]),
    );
    let choices = store.value(operation("transcript_reader"))["trackChoices"].clone();
    assert_eq!(choices["sourceTracks"][0]["id"], ".en");
    assert_eq!(choices["sourceTracks"].as_array().unwrap().len(), 1);
    assert_eq!(choices["captionTracks"].as_array().unwrap().len(), 2);
    assert!(choices["translations"].as_array().unwrap().is_empty());
}

#[test]
fn full_storage_does_not_prevent_search_or_export_of_current_transcript() {
    let store = Store::new();
    let blocked = store.path.join("file-not-directory");
    fs::write(&blocked, "occupied").unwrap();
    let call = |command: Value| -> Value {
        serde_json::from_str(&execute(
            command.to_string(),
            blocked.to_str().unwrap().into(),
        ))
        .unwrap()
    };
    let accepted = call(accept(reply()));
    assert_eq!(accepted["value"]["ok"], true);
    assert_eq!(accepted["value"]["cacheSaved"], false);
    assert_eq!(call(operation("export"))["ok"], true);
}
