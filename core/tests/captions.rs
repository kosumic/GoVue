use lingoplay_core::execute;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const VIDEO: &str = "abcdefghijk";
struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "lingoplay-captions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn value(&self, command: Value) -> Value {
        let reply: Value = serde_json::from_str(&execute(
            command.to_string(),
            self.0.to_string_lossy().into(),
        ))
        .unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
    fn ingest(&self, video: &str, source: &str) -> Value {
        let transcript = json!({"videoId":video,"title":"Example","language":"English","trackId":".en",
            "tracks":[{"id":".en","label":"English","language":"en","automatic":true}],"segments":[],"captionSource":source});
        self.value(json!({"op":"accept","videoId":video,"origin":"https://www.youtube.com","requestId":"captions",
            "raw":json!({"type":"transcript","ok":true,"videoId":video,"requestId":"captions","transcript":transcript}).to_string()}))["transcript"].clone()
    }
    fn command(&self, op: &str) -> Value {
        self.value(json!({"op":op,"videoId":VIDEO,"trackId":".en","segmentIndex":0}))
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preserves_timing_layout_formatting_confidence_and_boundaries_across_disk_restore() {
    let store = Store::new();
    let source = json!({"baseUrl":"never-persist-signed-url","pens":[{"bAttr":1,"fcForeColor":1122867,"unknownSecret":"never-persist"},{"iAttr":1,"uAttr":1}],
        "wsWinStyles":[{"mhModeHint":2,"pdPrintDir":1}],"wpWinPositions":[{"apPoint":7,"avVerPos":100}],
        "events":[{"id":7,"tStartMs":0,"dDurationMs":10000,"wsWinStyleId":0,"wpWinPosId":0},
        {"wWinId":7,"tStartMs":1000,"dDurationMs":3000,"segs":[
            {"utf8":" Ｈello,\n","pPenId":0,"acAsrConf":0},
            {"utf8":" world! ","tOffsetMs":750,"pPenId":1,"acAsrConf":180,"isSpeakerChange":true,"sessionToken":"never-persist"}]}]}).to_string();
    let data = store.ingest(VIDEO, &source);
    let line = &data["segments"][0];
    assert_eq!(line["text"], "Hello,\nworld!");
    assert_eq!(
        line["words"],
        json!([{"text":"Hello","start":1.0},{"text":"world","start":1.75}])
    );
    assert_eq!(
        data["captionMetadata"]["events"][1]["parts"][1]["confidence"],
        180
    );
    assert_eq!(
        data["captionMetadata"]["events"][1]["parts"][1]["speakerChange"],
        true
    );
    assert_eq!(data["captionMetadata"]["windowStyles"][0]["pdPrintDir"], 1);
    let prepared = store.command("vocabulary_caption");
    let tokens = prepared["tokens"].as_array().unwrap();
    assert_eq!(
        tokens
            .iter()
            .map(|t| t["text"].as_str().unwrap())
            .collect::<String>(),
        line["text"]
    );
    assert_eq!(tokens[0]["bold"], true);
    assert_eq!(tokens[0]["end"], 1.75);
    assert_eq!(tokens[3]["start"], 1.75);
    assert_eq!(tokens[3]["italic"], true);
    assert_eq!(tokens[3]["underline"], true);
    assert_eq!(tokens[3]["end"], 4.0);
    assert_eq!(tokens[3]["speakerChange"], true);
    assert!(tokens[0]["speakerChange"].is_null());
    let saved = store.value(json!({"op":"vocabulary_save","videoId":VIDEO,"trackId":".en","segmentIndex":0,"tokenIndex":3,"word":"world"}));
    assert_eq!(saved["entry"]["start"], 1.75);
    assert!(
        store
            .command("vocabulary_details")
            .as_str()
            .unwrap()
            .contains("uncalibrated")
    );
    for i in 0..4 {
        store.ingest(&format!("video{i:06}"), &source);
    }
    assert_eq!(
        store.command("restore"),
        data,
        "restore must reload the evicted document from disk"
    );
    assert_eq!(store.command("vocabulary_caption"), prepared);
    for file in fs::read_dir(&store.0)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
    {
        let text = fs::read_to_string(file.path()).unwrap();
        for secret in ["never-persist", "baseUrl", "sessionToken", "captionSource"] {
            assert!(!text.contains(secret), "Persisted {secret}");
        }
    }
}

#[test]
fn speaker_flags_mark_boundaries_once_and_preserve_original_word_indices() {
    let store = Store::new();
    let source = json!({"events":[{"tStartMs":0,"dDurationMs":5000,"segs":[
        {"utf8":"Hello there. ","isSpeakerChange":true},
        {"utf8":"— ","isSpeakerChange":true},
        {"utf8":"Next speaker stays. ","isSpeakerChange":false},
        {"utf8":"漢字です。un","isSpeakerChange":true},
        {"utf8":"finished","isSpeakerChange":true},
        {"utf8":" ","isSpeakerChange":true}
    ]}]})
    .to_string();
    let data = store.ingest(VIDEO, &source);
    let prepared = store.command("vocabulary_caption");
    let plain = store.value(json!({"op":"vocabulary_tokenize","text":data["segments"][0]["text"]}));
    let tokens = prepared["tokens"].as_array().unwrap();
    assert_eq!(tokens.len(), plain.as_array().unwrap().len());
    for (token, plain) in tokens.iter().zip(plain.as_array().unwrap()) {
        assert_eq!(token["text"], plain["text"]);
        assert_eq!(token["word"], plain["word"]);
    }
    assert_eq!(
        tokens
            .iter()
            .filter(|token| token["speakerChange"] == true)
            .map(|token| token["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Hello", "Next", "漢", "unfinished"]
    );
    let (index, token) = tokens
        .iter()
        .enumerate()
        .find(|(_, token)| token["text"] == "Next")
        .unwrap();
    let saved = store.value(json!({"op":"vocabulary_save","videoId":VIDEO,"trackId":".en","segmentIndex":0,"tokenIndex":index,"word":token["text"]}));
    assert_eq!(saved["entry"]["word"], "Next");
    let unflagged =
        json!({"events":[{"tStartMs":0,"segs":[{"utf8":"No divider.","isSpeakerChange":false}]}]})
            .to_string();
    store.ingest(VIDEO, &unflagged);
    assert!(
        store.command("vocabulary_caption")["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .all(|token| token["speakerChange"].is_null())
    );
}

#[test]
fn continuation_markers_join_only_their_caption_window_and_keep_the_source_events() {
    let store = Store::new();
    let source = json!({"events":[
        {"wWinId":1,"tStartMs":0,"dDurationMs":2000,"segs":[{"utf8":"First","tOffsetMs":0}]},
        {"wWinId":2,"tStartMs":500,"dDurationMs":1000,"segs":[{"utf8":"Other window"}]},
        {"wWinId":1,"tStartMs":1000,"dDurationMs":2000,"aAppend":1,"segs":[{"utf8":" more","tOffsetMs":0}]},
        {"wWinId":1,"tStartMs":3000,"aAppend":1,"segs":[{"utf8":"\n"}]},
        {"wWinId":1,"tStartMs":3000,"dDurationMs":2000,"segs":[{"utf8":"next.","tOffsetMs":0}]},
        {"wWinId":1,"tStartMs":6000,"dDurationMs":1000,"segs":[{"utf8":"Fresh sentence."}]}]}).to_string();
    let data = store.ingest(VIDEO, &source);
    assert_eq!(data["segments"].as_array().unwrap().len(), 3);
    assert_eq!(data["segments"][0]["text"], "First more\nnext.");
    assert_eq!(data["segments"][0]["duration"], 5.0);
    assert_eq!(data["segments"][0]["words"][2]["start"], 3.0);
    assert_eq!(data["segments"][1]["text"], "Other window");
    assert_eq!(data["segments"][2]["text"], "Fresh sentence.");
    assert_eq!(
        data["captionMetadata"]["events"].as_array().unwrap().len(),
        6
    );
}

#[test]
fn ruby_is_not_spoken_text_but_is_rendered_searchable_and_exportable_in_json3_and_srv3() {
    let store = Store::new();
    let json = json!({"pens":[{"bAttr":1},{"rbRuby":1},{"rbRuby":2},{"rbRuby":4}],"events":[
        {"tStartMs":5000,"dDurationMs":4000,"segs":[{"utf8":"漢字","pPenId":1,"tOffsetMs":0},
        {"utf8":"（","pPenId":2},{"utf8":"かんじ","pPenId":3},{"utf8":"）","pPenId":2},{"utf8":"です","pPenId":0,"tOffsetMs":700}]}]}).to_string();
    let xml = "<timedtext><head><pen id=\"0\" b=\"1\"/><pen id=\"1\" rb=\"1\"/><pen id=\"2\" rb=\"2\"/><pen id=\"3\" rb=\"4\"/></head><body><p t=\"5000\" d=\"4000\"><s p=\"1\" t=\"0\">漢字</s><s p=\"2\">（</s><s p=\"3\">かんじ</s><s p=\"2\">）</s><s p=\"0\" t=\"700\">です</s></p></body></timedtext>";
    for source in [json.as_str(), xml] {
        let data = store.ingest(VIDEO, source);
        assert_eq!(data["segments"][0]["text"], "漢字です");
        let prepared = store.command("vocabulary_caption");
        assert_eq!(prepared["tokens"][1]["reading"], "かんじ");
        assert_eq!(prepared["tokens"][2]["start"], 5.7);
        assert!(
            store
                .command("export")
                .as_str()
                .unwrap()
                .contains("漢字(かんじ)です")
        );
        for query in ["かんじ", "漢字です", "かんじです"] {
            assert_eq!(
                store.value(json!({"op":"search","videoId":VIDEO,"trackId":".en","query":query})),
                json!([0])
            );
            assert_eq!(
                store.value(
                    json!({"op":"transcript_reader","videoId":VIDEO,"trackId":".en","query":query})
                )["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
    }
}

#[test]
fn absent_invalid_or_unalignable_offsets_fall_back_without_losing_caption_text() {
    let store = Store::new();
    for parts in [
        json!([{"utf8":"a "},{"utf8":"b"}]),
        json!([{"utf8":"a ","tOffsetMs":0},{"utf8":"b","tOffsetMs":-1}]),
        json!([{"utf8":"a ","tOffsetMs":500},{"utf8":"b","tOffsetMs":100}]),
        json!([{"utf8":"a ","tOffsetMs":0},{"utf8":"b","tOffsetMs":1000}]),
        json!([{"utf8":"a ","tOffsetMs":"NaN"},{"utf8":"b","tOffsetMs":500}]),
    ] {
        let data = store.ingest(
            VIDEO,
            &json!({"events":[{"tStartMs":2000,"dDurationMs":1000,"segs":parts}]}).to_string(),
        );
        assert_eq!(data["segments"][0]["text"], "a b");
        assert!(data["segments"][0]["words"].is_null());
        assert!(store.command("vocabulary_caption")["tokens"][0]["start"].is_null());
    }
    let data = store.ingest(VIDEO,&json!({"events":[{"tStartMs":0,"dDurationMs":1000,"segs":[
        {"utf8":"cafe","tOffsetMs":0},{"utf8":"\u{301} ","tOffsetMs":100},{"utf8":"日本語","tOffsetMs":500}]}]}).to_string());
    assert_eq!(data["segments"][0]["text"], "café 日本語");
    assert!(data["segments"][0]["words"].is_null());
}
