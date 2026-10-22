#![cfg(test)]

use serde_json::{Value, json};

#[test]
fn webview_policy_and_tab_state_accept_the_same_addresses() {
    for (url, expected) in [
        ("https://example.com/", "internal"),
        ("https://example.com:443/path?q=1", "internal"),
        ("https://m.youtube.com/watch?v=abcdefghijk", "internal"),
        ("https://youtu.be/abcdefghijk", "internal"),
        ("http://example.com/", "external"),
        ("http://localhost:8080/", "external"),
        ("https://example.com:8443/", "external"),
        ("https://youtu.be/invalid", "external"),
        ("https://user:pass@example.com/", "blocked"),
        ("https://example.com/\nwatch", "blocked"),
        ("javascript:alert(1)", "blocked"),
        ("data:text/html,hello", "blocked"),
        ("file:///etc/passwd", "blocked"),
        ("about:blank", "blocked"),
        ("not a url", "blocked"),
    ] {
        assert_eq!(
            govue_core::browser_url_policy(url.into()),
            expected,
            "{url}"
        );
        let reply: Value = serde_json::from_str(&govue_core::execute(
            json!({"op":"browser_open","url":url}).to_string(),
            format!("browser-policy-{url}"),
        ))
        .unwrap();
        assert_eq!(reply["ok"], expected == "internal", "{url}: {reply}");
    }
    assert_eq!(
        govue_core::browser_url_policy(format!("https://example.com/{}", "a".repeat(8192))),
        "blocked"
    );
}
