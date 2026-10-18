#![cfg(test)]

use govue_core::execute;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "govue-languages-{}-{}",
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
    fn read(&self) -> Value {
        let reply = self.call(json!({"op":"language_settings"}));
        assert_eq!(reply["ok"], true, "{reply}");
        reply["value"].clone()
    }
    fn save(&self, settings: Value) -> Value {
        self.call(json!({"op":"language_settings","settings":settings}))
    }
    fn file(&self) -> PathBuf {
        self.0.join("settings/languages.json")
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn language_lists_persist_normalized_names_current_levels_and_separate_goals() {
    let store = Store::new();
    assert_eq!(store.read(), json!({"known":[],"learning":[]}));
    assert!(!store.file().exists());
    let settings = json!({
        "known":[{"language":"  English  ","level":"native"},{"language":"Ｓｐａｎｉｓｈ","level":"A1"}],
        "learning":[{"language":"Spanish","level":"B1"},{"language":"日本語","level":"A2"}]
    });
    let saved = store.save(settings);
    assert_eq!(saved["ok"], true, "{saved}");
    let mut expected = json!({
        "known":[{"language":"English","level":"native"},{"language":"Spanish","level":"A1"}],
        "learning":[{"language":"Spanish","level":"B1"},{"language":"日本語","level":"A2"}]
    });
    assert_eq!(saved["value"], expected);
    assert_eq!(store.read(), expected);
    let disk: Value = serde_json::from_slice(&fs::read(store.file()).unwrap()).unwrap();
    assert_eq!(disk, expected);

    // Reading uses disk too, so a fresh native runtime restores the same profile.
    expected["known"] = json!([{"language":"English","level":"C2"}]);
    expected["learning"][0]["level"] = json!("B2");
    assert_eq!(store.save(expected.clone())["ok"], true);
    assert_eq!(store.read(), expected);
    fs::write(store.file(), b"{\"known\":[],\"learning\":[]}").unwrap();
    assert_eq!(store.read(), json!({"known":[],"learning":[]}));

    for level in ["A1", "A2", "B1", "B2", "C1", "C2"] {
        let profile = json!({"known":[{"language":"French","level":level}],"learning":[{"language":"French","level":level}]});
        assert_eq!(store.save(profile.clone())["ok"], true, "{level}");
        assert_eq!(store.read(), profile);
    }
}

#[test]
fn invalid_language_updates_never_replace_the_saved_lists() {
    let store = Store::new();
    let initial = json!({"known":[{"language":"English","level":"native"}],"learning":[]});
    assert_eq!(store.save(initial.clone())["ok"], true);
    let bytes = fs::read(store.file()).unwrap();
    let mut invalid = vec![
        json!({"known":[],"learning":[{"language":"Spanish","level":"native"}]}),
        json!({"known":[],"learning":[{"language":"Spanish","level":"A0"}]}),
        json!({"known":[{"language":"French","level":"B3"}],"learning":[]}),
        json!({"known":[{"language":"French","level":null}],"learning":[]}),
        json!({"known":[{"language":"French","level":"B1","extra":true}],"learning":[]}),
        json!({"known":[{"language":"English","level":"B1"},{"language":" ｅｎｇｌｉｓｈ ","level":"C1"}],"learning":[]}),
        json!({"known":[],"learning":[{"language":"Japanese","level":"A1"},{"language":"JAPANESE","level":"A2"}]}),
        json!({"known":[],"learning":"Spanish"}),
        json!({"known":[]}),
        json!({"known":[],"learning":[],"extra":true}),
        json!([]),
    ];
    for name in [
        String::new(),
        " \t\n ".into(),
        "x".repeat(81),
        "bad\0name".into(),
    ] {
        invalid.push(json!({"known":[{"language":name,"level":"B1"}],"learning":[]}));
    }
    let too_many: Vec<_> = (0..51)
        .map(|i| json!({"language":format!("Language {i}"),"level":"A1"}))
        .collect();
    invalid.push(json!({"known":too_many,"learning":[]}));
    for update in invalid {
        let reply = store.save(update.clone());
        assert_eq!(reply["ok"], false, "Accepted {update}");
        assert!(!reply["error"].as_str().unwrap().is_empty());
        assert_eq!(fs::read(store.file()).unwrap(), bytes);
        assert_eq!(store.read(), initial);
    }
}

#[test]
fn storage_failures_are_reported_without_silently_clearing_languages() {
    let store = Store::new();
    let initial = json!({"known":[{"language":"French","level":"C1"}],"learning":[]});
    assert_eq!(store.save(initial.clone())["ok"], true);
    let bytes = fs::read(store.file()).unwrap();
    fs::create_dir(store.file().with_extension("tmp")).unwrap();
    assert_eq!(store.save(json!({"known":[],"learning":[]}))["ok"], false);
    assert_eq!(fs::read(store.file()).unwrap(), bytes);
    assert_eq!(store.read(), initial);

    for corrupt in [b"{bad".to_vec(), b"{}".to_vec(), vec![b' '; 64_001]] {
        fs::write(store.file(), &corrupt).unwrap();
        let reply = store.call(json!({"op":"language_settings"}));
        assert_eq!(reply["ok"], false);
        assert_eq!(fs::read(store.file()).unwrap(), corrupt);
    }
}
