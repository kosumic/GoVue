use lingoplay_core::execute;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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
            "lingoplay-vocabulary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn call(&self, command: Value) -> Value {
        serde_json::from_str(&execute(
            command.to_string(),
            self.0.to_str().unwrap().into(),
        ))
        .unwrap()
    }
    fn value(&self, command: Value) -> Value {
        let reply = self.call(command);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
    fn file(&self) -> PathBuf {
        self.0.join("vocabulary/words.json")
    }
    fn transcript(&self, language: &str) {
        let data = json!({
            "videoId": VIDEO, "title": "Word practice", "language": language, "trackId": language,
            "tracks": [{"id": language,"label": language,"language": language,"automatic":false}],
            "segments": [
                {"start":2.75,"duration":3,"text":"Hello, world! Café can't résumé."},
                {"start":9,"duration":2,"text":"hello again"}
            ]
        });
        self.value(json!({"op":"accept","videoId":VIDEO,"origin":format!("https://www.youtube.com/watch?v={VIDEO}"),"requestId":"words",
            "raw":json!({"type":"transcript","videoId":VIDEO,"requestId":"words","ok":true,"transcript":data}).to_string()}));
    }
    fn selection(language: &str, segment: usize, token: usize, word: &str) -> Value {
        json!({"op":"vocabulary_save","videoId":VIDEO,"trackId":language,"segmentIndex":segment,"tokenIndex":token,"word":word})
    }
    fn save(&self, language: &str, segment: usize, token: usize, word: &str) -> Value {
        self.value(Self::selection(language, segment, token, word))
    }
    fn list(&self, query: &str) -> Value {
        self.value(json!({"op":"vocabulary_list","query":query}))
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unicode_boundaries_preserve_caption_text_and_keep_punctuation_out_of_words() {
    let store = Store::new();
    for text in [
        "Hello, can't can’t!",
        "cafe\u{301} résumé",
        "你好、日本語",
        "नमस्ते दुनिया",
        "👩🏽‍💻 — … !  ",
        "",
    ] {
        let tokens = store.value(json!({"op":"vocabulary_tokenize","text":text}));
        let tokens = tokens.as_array().unwrap();
        assert_eq!(
            tokens
                .iter()
                .map(|token| token["text"].as_str().unwrap())
                .collect::<String>(),
            text
        );
        for token in tokens {
            if token["word"] == true {
                assert!(
                    token["text"]
                        .as_str()
                        .unwrap()
                        .chars()
                        .any(char::is_alphanumeric)
                );
            }
        }
    }
    let tokens = store.value(json!({"op":"vocabulary_tokenize","text":"Hello, can't can’t!"}));
    let words: Vec<_> = tokens
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["word"] == true)
        .map(|t| t["text"].as_str().unwrap())
        .collect();
    assert_eq!(words, ["Hello", "can't", "can’t"]);
    assert_eq!(
        store.call(json!({"op":"vocabulary_tokenize","text":"x".repeat(50_001)}))["ok"],
        false
    );
    assert_eq!(
        store.call(json!({"op":"vocabulary_list","query":"x".repeat(10_001)}))["ok"],
        false
    );
    assert!(!store.file().exists());
}

#[test]
fn translation_uses_all_learning_languages_for_known_words_and_a_native_language_for_learning_words()
 {
    let store = Store::new();
    store.value(json!({"op":"language_settings","settings":{
        "known":[{"language":"French","level":"B1"},{"language":"English","level":"native"}],
        "learning":[{"language":"Spanish","level":"B1"},{"language":"German","level":"A2"},{"language":"es","level":"A1"}]
    }}));
    let plan = |language: &str| {
        let mut command = Store::selection(language, 0, 0, "Hello");
        command["op"] = json!("vocabulary_translation");
        store.value(command)
    };
    store.transcript("en-US");
    let plans = plan("en-US");
    assert_eq!(plans.as_array().unwrap().len(), 2);
    assert_eq!(
        plans[0],
        json!({"input":"Hello","sourceLangCode":"en","targetLangCode":"es","targetLanguage":"Spanish","sameLanguage":false})
    );
    assert_eq!(plans[1]["targetLangCode"], "de");
    assert_eq!(plans[1]["targetLanguage"], "German");
    store.transcript("es-MX");
    let plans = plan("es-MX");
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(plans[0]["sourceLangCode"], "es");
    assert_eq!(plans[0]["targetLangCode"], "en");
    assert!(
        !store.file().exists(),
        "translating does not save vocabulary"
    );
}

#[test]
fn translation_validates_caption_selection_and_requires_learning_languages_for_known_words() {
    let store = Store::new();
    store.transcript("en");
    let mut command = Store::selection("en", 0, 0, "Hello");
    command["op"] = json!("vocabulary_translation");
    let fallback = store.value(command.clone());
    assert_eq!(fallback[0]["sameLanguage"], true);
    for (field, value) in [
        ("word", json!("world")),
        ("tokenIndex", json!(1)),
        ("segmentIndex", json!(99)),
        ("videoId", json!("not-a-video")),
    ] {
        let mut invalid = command.clone();
        invalid[field] = value;
        assert_eq!(store.call(invalid)["ok"], false);
    }
    store.value(json!({"op":"language_settings","settings":{"known":[{"language":"English","level":"native"}],"learning":[]}}));
    let missing = store.call(command.clone());
    assert_eq!(missing["ok"], false);
    assert!(missing["error"].as_str().unwrap().contains("want to learn"));
    store.value(json!({"op":"language_settings","settings":{"known":[{"language":"English","level":"native"}],"learning":[{"language":"en","level":"B2"},{"language":"Japanese","level":"A1"}]}}));
    let plans = store.value(command);
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(plans[0]["targetLangCode"], "ja");
    assert!(!store.file().exists());
}

#[test]
fn saves_source_context_deduplicates_by_language_and_survives_cache_eviction() {
    let store = Store::new();
    assert_eq!(store.list(""), json!([]));
    store.transcript("en");
    let saved = store.save("en", 0, 0, "Hello");
    assert_eq!(saved["saved"], true);
    let entry = saved["entry"].clone();
    assert_eq!(entry["word"], "Hello");
    assert_eq!(entry["context"], "Hello, world! Café can't résumé.");
    assert_eq!(entry["start"], 2.75);
    assert_eq!(entry["videoId"], VIDEO);
    assert_eq!(entry["title"], "Word practice");
    assert_eq!(entry["language"], "en");
    assert!(entry["savedAt"].as_u64().unwrap() > 0);
    assert_eq!(
        store.save("en", 1, 0, "hello"),
        json!({"entry":entry,"saved":false})
    );
    let cafe = store.save("en", 0, 6, "Café")["entry"].clone();
    assert_eq!(store.list("CAFE\u{301}"), json!([cafe, entry]));
    assert_eq!(store.list("practice").as_array().unwrap().len(), 2);
    store.transcript("es");
    assert_eq!(store.save("es", 0, 0, "Hello")["saved"], true);
    assert_eq!(store.list("").as_array().unwrap().len(), 3);
    assert_eq!(store.list("absent"), json!([]));
    let before = store.list("");
    // A fresh execute call reads saved words from disk, even with no caption files.
    for file in fs::read_dir(&store.0).unwrap().flatten() {
        if file.path().extension().is_some_and(|ext| ext == "json") {
            fs::remove_file(file.path()).unwrap();
        }
    }
    assert_eq!(store.list(""), before);
    let disk: Value = serde_json::from_slice(&fs::read(store.file()).unwrap()).unwrap();
    assert_eq!(disk, before);
    store.value(json!({"op":"vocabulary_remove","id":entry["id"]}));
    assert_eq!(store.list("").as_array().unwrap().len(), 2);
    assert!(
        store
            .list("")
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["id"] != entry["id"])
    );
}

#[test]
fn rejects_stale_or_invalid_selections_without_changing_saved_words() {
    let store = Store::new();
    store.transcript("en");
    store.save("en", 0, 0, "Hello");
    let before = fs::read(store.file()).unwrap();
    let valid = Store::selection("en", 0, 3, "world");
    for (field, value) in [
        ("videoId", json!("../bad")),
        ("videoId", json!("zzzzzzzzzzz")),
        ("trackId", json!("missing")),
        ("segmentIndex", json!(-1)),
        ("segmentIndex", json!(999)),
        ("segmentIndex", json!(1.5)),
        ("tokenIndex", json!(-1)),
        ("tokenIndex", json!(999)),
        ("tokenIndex", json!("3")),
        ("tokenIndex", json!(1)),
        ("word", json!("changed")),
    ] {
        let mut command = valid.clone();
        command[field] = value;
        let reply = store.call(command.clone());
        assert_eq!(reply["ok"], false, "Accepted {command}");
        assert_eq!(fs::read(store.file()).unwrap(), before);
    }
}

#[test]
fn storage_errors_and_corrupt_data_do_not_overwrite_the_collection() {
    let store = Store::new();
    store.transcript("en");
    store.save("en", 0, 0, "Hello");
    let before = fs::read(store.file()).unwrap();
    let temporary = store.file().with_extension("tmp");
    fs::create_dir(&temporary).unwrap();
    assert_eq!(
        store.call(Store::selection("en", 0, 3, "world"))["ok"],
        false
    );
    assert_eq!(fs::read(store.file()).unwrap(), before);
    fs::remove_dir(temporary).unwrap();
    for corrupt in [b"{bad".to_vec(), b"{}".to_vec(), vec![b' '; 8_000_001]] {
        fs::write(store.file(), &corrupt).unwrap();
        assert_eq!(store.call(json!({"op":"vocabulary_list"}))["ok"], false);
        assert_eq!(
            store.call(Store::selection("en", 0, 3, "world"))["ok"],
            false
        );
        assert_eq!(fs::read(store.file()).unwrap(), corrupt);
    }
}

#[test]
fn collection_limit_allows_duplicates_and_removal_without_silent_eviction() {
    let store = Store::new();
    store.transcript("en");
    let entry = store.save("en", 0, 0, "Hello")["entry"].clone();
    let mut full = vec![entry.clone()];
    for i in 1..2_000 {
        let mut next = entry.clone();
        let word = format!("word{i}");
        next["id"] = json!(format!("{:x}", Sha256::digest(format!("en\0{word}"))));
        next["word"] = json!(word);
        full.push(next);
    }
    let bytes = serde_json::to_vec(&full).unwrap();
    fs::write(store.file(), &bytes).unwrap();
    assert_eq!(store.save("en", 0, 0, "Hello")["saved"], false);
    assert_eq!(
        store.call(Store::selection("en", 0, 3, "world"))["ok"],
        false
    );
    assert_eq!(fs::read(store.file()).unwrap(), bytes);
    store.value(json!({"op":"vocabulary_remove","id":entry["id"]}));
    assert_eq!(store.save("en", 0, 3, "world")["saved"], true);
    assert_eq!(store.list("").as_array().unwrap().len(), 2_000);
}
