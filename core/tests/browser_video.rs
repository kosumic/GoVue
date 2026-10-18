use govue_core::execute;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

fn call(root: &str, command: Value) -> Value {
    serde_json::from_str(&execute(command.to_string(), root.into())).unwrap()
}
fn ok(root: &str, command: Value) -> Value {
    let result = call(root, command);
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}

#[test]
fn chat_video_navigation_reuses_the_document_and_opens_missing_videos() {
    let root = format!(
        "browser-video-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let original = "https://m.youtube.com/watch?v=abcdefghijk&t=12s&list=PLexample";
    let opened = ok(&root, json!({"op":"browser_open","url":original}));
    let id = opened["activeId"].clone();
    let document = opened["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tab| tab["id"] == id)
        .unwrap()
        .clone();
    ok(
        &root,
        json!({"op":"browser_open","url":"https://m.youtube.com/","suspendWatch":true}),
    );
    let restored = ok(&root, json!({"op":"browser_open","videoId":"abcdefghijk"}));
    assert_eq!(restored["activeId"], id);
    assert_eq!(restored["target"]["url"], original);
    assert_eq!(restored["target"]["load"], false);
    assert_eq!(restored["tabs"].as_array().unwrap().len(), 2);
    assert_eq!(
        restored["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tab| tab["id"] == id)
            .unwrap(),
        &document
    );

    let missing = ok(&root, json!({"op":"browser_open","videoId":"0123456789a"}));
    assert_eq!(
        missing["target"]["url"],
        "https://m.youtube.com/watch?v=0123456789a"
    );
    assert_eq!(missing["target"]["load"], true);
    assert_eq!(missing["tabs"].as_array().unwrap().len(), 2);
    let external = ok(
        &root,
        json!({"op":"browser_open","url":"https://youtube.com.evil.test/watch?v=abcdefghijk"}),
    );
    let safe = ok(&root, json!({"op":"browser_open","videoId":"abcdefghijk"}));
    assert_ne!(safe["activeId"], external["activeId"]);
    assert_eq!(
        safe["target"]["url"],
        "https://m.youtube.com/watch?v=abcdefghijk"
    );
    assert_eq!(safe["target"]["load"], true);
    let before = ok(&root, json!({"op":"browser_state"}));
    for invalid in [
        json!("../unsafe"),
        json!("abcdefghijk&other=value"),
        Value::Null,
        json!(123),
    ] {
        assert_eq!(
            call(&root, json!({"op":"browser_open","videoId":invalid}))["ok"],
            false
        );
        assert_eq!(ok(&root, json!({"op":"browser_state"})), before);
    }
}
