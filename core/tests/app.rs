#![cfg(test)]

use serde_json::{Value, json};
use std::collections::HashSet;

fn call(command: Value) -> Value {
    serde_json::from_str(&govue_core::execute(command.to_string(), String::new())).unwrap()
}

fn release(installed: &str, tag: &str) -> Value {
    json!({"op":"app_update", "installedVersion":installed,
        "release":{"tag_name":tag, "draft":false, "prerelease":false}})
}

#[test]
fn updates_compare_numeric_versions_without_downgrades_or_build_metadata_changes() {
    for (installed, latest, expected) in [
        ("0.9.0", "v0.10.0", true),
        ("1.0.9", "1.0.10", true),
        ("1.0.0", "v1.0.0", false),
        ("2.0.0", "v1.9.9", false),
        ("1.0.0+local", "v1.0.0+release", false),
        ("1.0.0-rc.1", "v1.0.0", true),
    ] {
        let reply = call(release(installed, latest));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["value"]["updateAvailable"], expected, "{reply}");
        assert_eq!(
            reply["value"]["latestVersion"],
            latest.trim_start_matches('v')
        );
    }
}

#[test]
fn update_boundary_rejects_unpublished_prerelease_and_unreadable_versions() {
    for field in ["draft", "prerelease"] {
        let mut command = release("1.0.0", "v2.0.0");
        command["release"][field] = json!(true);
        assert_eq!(call(command)["ok"], false);
    }
    for tag in ["", "latest", "v2", "v02.0.0", "v2.0.0-rc.1", "2.0.0\n"] {
        assert_eq!(call(release("1.0.0", tag))["ok"], false, "{tag:?}");
    }
    assert_eq!(call(release("unknown", "v2.0.0"))["ok"], false);
    for release in [
        Value::Null,
        json!({"tag_name":"v2.0.0"}),
        json!({"draft":false,"prerelease":false}),
    ] {
        assert_eq!(
            call(json!({"op":"app_update", "installedVersion":"1.0.0", "release":release}))["ok"],
            false
        );
    }
}

#[test]
fn every_bundled_notice_is_available_through_the_offline_json_boundary() {
    let reply = call(json!({"op":"app_licenses"}));
    assert_eq!(reply["ok"], true, "{reply}");
    let notices = reply["value"].as_array().unwrap();
    let mut ids = HashSet::new();
    let mut categories = HashSet::new();
    for notice in notices {
        let id = notice["id"].as_str().unwrap();
        assert!(ids.insert(id), "Duplicate notice {id}");
        assert!(!notice["title"].as_str().unwrap().is_empty());
        assert!(!notice["license"].as_str().unwrap().is_empty());
        assert!(!notice["attribution"].as_str().unwrap().is_empty());
        let source = url::Url::parse(notice["sourceUrl"].as_str().unwrap()).unwrap();
        assert_eq!(source.scheme(), "https");
        assert!(source.host_str().is_some());
        categories.insert(notice["category"].as_str().unwrap());
        assert!(
            notice.get("text").is_none(),
            "License text must be loaded on demand"
        );
        let content = call(json!({"op":"app_license", "id":id}));
        assert_eq!(content["ok"], true, "{content}");
        assert!(content["value"]["text"].as_str().unwrap().len() > 100);
    }
    assert_eq!(categories, HashSet::from(["featured", "npm", "cargo"]));
    for title in [
        "LiTube",
        "lucide-react-native",
        "react-native",
        "uniffi",
        "serde",
    ] {
        assert!(
            notices.iter().any(|notice| notice["title"] == title),
            "Missing {title}"
        );
    }
    let gpl = call(json!({"op":"app_license", "id":"govue"}));
    assert_eq!(gpl["value"]["text"], include_str!("../../LICENSE"));
    assert_eq!(
        call(json!({"op":"app_license", "id":"../LICENSE"}))["ok"],
        false
    );
}
