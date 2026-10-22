#![cfg(test)]

use serde_json::{Value, json};

fn reply(command: Value) -> Value {
    serde_json::from_str(&govue_core::state_execute(command.to_string())).unwrap()
}
fn call(command: Value) -> Value {
    let response = reply(command);
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}

#[test]
fn browser_input_and_identity_reject_unsafe_addresses() {
    for (input, expected) in [
        ("example.com", "https://example.com/"),
        ("http://localhost:8080/test", "http://localhost:8080/test"),
        ("localhost:8080", "https://localhost:8080/"),
        (
            "learn Japanese",
            "https://www.google.com/search?q=learn%20Japanese",
        ),
        (
            "https://youtu.be/abcdefghijk?t=12&si=tracking",
            "https://www.youtube.com/watch?v=abcdefghijk&t=12",
        ),
    ] {
        assert_eq!(call(json!({"op":"browser_input","input":input})), expected);
    }
    for input in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://user:pass@example.com",
        "",
        "https://example.com/\nwatch",
    ] {
        assert_eq!(
            reply(json!({"op":"browser_input","input":input}))["ok"],
            false
        );
    }
    for url in [
        "https://www.youtube.com/watch?v=abcdefghijk",
        "https://m.youtube.com/shorts/abcdefghijk",
        "https://www.youtube.com/live/abcdefghijk",
    ] {
        assert_eq!(
            call(json!({"op":"browser_identity","url":url}))["videoId"],
            "abcdefghijk"
        );
    }
    for url in [
        "https://www.youtube.com/",
        "https://www.youtube.com.evil.test/watch?v=abcdefghijk",
        "http://www.youtube.com/watch?v=abcdefghijk",
        "https://example.com/watch?v=abcdefghijk",
        "https://youtube.com/watch?v=bad",
        "https://user@youtube.com/watch?v=abcdefghijk",
        "https://youtube.com:8443/watch?v=abcdefghijk",
    ] {
        assert!(call(json!({"op":"browser_identity","url":url}))["videoId"].is_null());
    }
    assert_eq!(
        serde_json::from_str::<Value>(&govue_core::state_execute("x".repeat(100_001))).unwrap()["ok"],
        false
    );
}

#[test]
fn untrusted_layout_and_position_messages_are_bounded_and_correlated() {
    let layout = json!({"left":10,"top":48,"width":380,"height":214,"viewportWidth":400,"viewportHeight":700,"visible":true});
    let mut extra = layout.clone();
    extra["extra"] = json!("untrusted");
    let accepted = call(json!({"op":"browser_layout","layout":extra}));
    assert!(accepted["extra"].is_null());
    assert_eq!(accepted["width"], 380.0);
    for (key, value) in [
        ("left", json!(-1601)),
        ("top", json!(2801)),
        ("width", json!(-1)),
        ("height", json!(2801)),
        ("viewportWidth", json!(0)),
        ("viewportHeight", json!(10001)),
        ("visible", json!("true")),
        ("width", json!("380")),
    ] {
        let mut invalid = layout.clone();
        invalid[key] = value;
        assert!(
            call(json!({"op":"browser_layout","layout":invalid})).is_null(),
            "{key}"
        );
    }
    let position = json!({"op":"browser_position","videoId":"abcdefghijk","requestId":"active","origin":"https://www.youtube.com/watch?v=abcdefghijk","playback":true,"message":{"videoId":"abcdefghijk","requestId":"active","seconds":3.5,"playing":true,"seeking":false,"rate":1.5}});
    assert_eq!(
        call(position.clone()),
        json!({"position":3.5,"playing":true,"seeking":false,"rate":1.5})
    );
    for (key, value) in [
        ("videoId", json!("0123456789a")),
        ("requestId", json!("old")),
        ("seconds", json!(-1)),
        ("rate", json!(17)),
        ("playing", json!("true")),
    ] {
        let mut invalid = position.clone();
        invalid["message"][key] = value;
        assert!(call(invalid).is_null(), "{key}");
    }
    let mut invalid = position;
    invalid["origin"] = json!("https://youtube.com.evil.test");
    assert!(call(invalid).is_null());
}

#[test]
fn playback_policy_owns_startup_manual_selection_and_async_generations() {
    let id = call(json!({"op":"player_session_create"}));
    let action = |op: &str, mut fields: Value| {
        fields["op"] = json!(format!("player_session_{op}"));
        fields["sessionId"] = id.clone();
        call(fields)
    };
    action("select", json!({"videoId":"abcdefghijk"}));
    let auto = json!({"owner":"abcdefghijk","ready":true,"foreground":true,"pending":false,"settings":{"adBlock":true}});
    for (key, value) in [
        ("ready", json!(false)),
        ("foreground", json!(false)),
        ("pending", json!(true)),
        ("owner", json!("0123456789a")),
        ("settings", json!({})),
    ] {
        let mut blocked = auto.clone();
        blocked[key] = value;
        assert_eq!(action("auto", blocked), false, "{key}");
    }
    assert_eq!(action("auto", auto.clone()), true);
    assert_eq!(
        action("auto", auto.clone()),
        false,
        "A failed startup must cool down"
    );
    action("page_dead", json!({}));
    let first = action(
        "begin",
        json!({"settings":{"rememberSpeed":false,"playbackSpeed":2,"rememberResize":false,"resizeMode":"fill","rememberQuality":false,"qualityHeight":1080}}),
    );
    assert_eq!(
        first["settings"],
        json!({"playbackSpeed":1,"resizeMode":"fit"})
    );
    assert_eq!(first["height"], 0);
    assert!(action("begin", json!({"settings":{}})).is_null());
    action("select", json!({"videoId":"0123456789a"}));
    assert_eq!(action("current", json!({"token":first["token"]})), false);
    assert_eq!(action("finish", json!({"token":first["token"]})), false);
    let next = action(
        "begin",
        json!({"settings":{"rememberSpeed":true,"playbackSpeed":1.5,"rememberResize":true,"resizeMode":"fill","rememberQuality":true,"qualityHeight":720,"qualityFps":60}}),
    );
    assert_eq!(
        next["settings"],
        json!({"playbackSpeed":1.5,"resizeMode":"fill"})
    );
    assert_eq!(next["height"], 720);
    action(
        "commit",
        json!({"playback":{"videoId":"0123456789a","qualityHeight":720,"qualityFps":60}}),
    );
    action("finish", json!({"token":next["token"]}));
    let quality = action("quality_begin", json!({}));
    assert_eq!(action("quality_current", json!({"token":quality})), true);
    let replacement = action("replace", json!({}));
    action("close", json!({"manual":true}));
    assert_eq!(action("quality_current", json!({"token":quality})), false);
    assert_eq!(
        action("replace_current", json!({"token":replacement})),
        false
    );
    let mut selected = auto;
    selected["owner"] = json!("0123456789a");
    assert_eq!(
        action("auto", selected),
        false,
        "Choosing the web player persists until navigation"
    );
    action("dispose", json!({}));
}
