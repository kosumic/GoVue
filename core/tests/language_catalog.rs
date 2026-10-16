use lingoplay_core::execute;
use serde_json::{json, Value};
use std::collections::HashSet;

fn call(mut command: Value) -> Value {
    command["op"] = json!("language_choices");
    // Catalog requests are independent of transcript identity and disk storage.
    serde_json::from_str(&execute(command.to_string(), String::new())).unwrap()
}

fn choices(command: Value) -> Value {
    let reply = call(command);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

fn codes(value: &Value) -> Vec<&str> {
    value["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|section| section["data"].as_array().unwrap())
        .map(|option| option["code"].as_str().unwrap())
        .collect()
}

#[test]
fn catalog_returns_unique_choices_with_common_languages_first() {
    let result = choices(json!({}));
    let all = codes(&result);
    assert_eq!(all.len(), 196);
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), 196);
    assert_eq!(result["sections"][0]["title"], "Common languages");
    assert_eq!(result["sections"][1]["title"], "More languages");
    assert_eq!(
        &all[..12],
        ["en", "es", "fr", "de", "zh", "ja", "ko", "it", "pt", "ar", "hi", "ru"]
    );
    assert_eq!(result["selected"], Value::Null);
    assert_eq!(choices(json!({"query":"  "})), result);
}

#[test]
fn search_accepts_native_names_accents_aliases_and_codes() {
    for (query, code) in [
        ("  ESPANOL  ", "es"),
        ("日本語", "ja"),
        ("Farsi", "fa"),
        ("Mandarin", "zh"),
        ("yue", "yue"),
    ] {
        let result = choices(json!({"query":query}));
        assert!(codes(&result).contains(&code), "{query}: {result}");
        assert_eq!(result["sections"][0]["title"], "Languages");
    }
    let japanese = choices(json!({"query":"日本語"}));
    assert_eq!(japanese["sections"][0]["data"][0]["nativeName"], "日本語");
    assert!(codes(&choices(json!({"query":"not-a-real-language"}))).is_empty());
}

#[test]
fn matching_and_exclusions_use_enum_identity_for_legacy_saved_names() {
    for (name, code) in [
        ("Chinese", "zh"),
        ("Cantonese", "yue"),
        ("Filipino", "fil"),
        ("Tagalog", "tl"),
        ("Akan", "ak"),
        ("Twi", "tw"),
        (" Ｅｎｇｌｉｓｈ ", "en"),
        ("日本語", "ja"),
        ("Farsi", "fa"),
        ("es", "es"),
    ] {
        let result = choices(json!({"selected":name}));
        assert_eq!(result["selected"]["code"], code, "{name}");
    }
    let result = choices(
        json!({"excluded":[" Ｅｎｇｌｉｓｈ ","日本語","Chinese","Filipino","A custom language"]}),
    );
    let available = codes(&result);
    for code in ["en", "ja", "zh", "fil"] {
        assert!(!available.contains(&code), "{code}");
    }
    for code in ["es", "fr", "yue", "tl"] {
        assert!(available.contains(&code), "{code}");
    }
    assert!(codes(&choices(json!({"query":"Japanese","excluded":["日本語"]}))).is_empty());
    assert_eq!(
        choices(json!({"selected":"A custom language"}))["selected"],
        Value::Null
    );
}

#[test]
fn catalog_boundary_rejects_invalid_and_unbounded_input() {
    for request in [
        json!({"query":12}),
        json!({"query":null}),
        json!({"excluded":"English"}),
        json!({"excluded":[12]}),
        json!({"selected":false}),
        json!({"query":"x".repeat(201)}),
        json!({"selected":"x".repeat(81)}),
        json!({"excluded":["x".repeat(81)]}),
        json!({"excluded":vec!["en";51]}),
    ] {
        let reply = call(request.clone());
        assert_eq!(reply["ok"], false, "{request}");
        assert!(!reply["error"].as_str().unwrap().is_empty());
    }
    assert!(call(json!({"query":"x".repeat(200),"selected":"x".repeat(80),"excluded":vec!["x".repeat(80);50]}))["ok"].as_bool().unwrap());
}
