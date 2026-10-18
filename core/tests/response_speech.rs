use lingoplay_core::execute;
use serde_json::{Value, json};

fn call(text: &str, limit: Value) -> Value {
    serde_json::from_str(&execute(
        json!({"op":"response_speech", "text":text, "maxLength":limit}).to_string(),
        String::new(),
    ))
    .unwrap()
}

fn chunks(text: &str, limit: u64) -> Vec<String> {
    let reply = call(text, json!(limit));
    assert_eq!(reply["ok"], true, "{reply}");
    serde_json::from_value(reply["value"]["chunks"].clone()).unwrap()
}

#[test]
fn reads_markdown_content_without_formatting_or_link_destinations() {
    let markdown = "# Greeting\n\n**Hello**, [world](https://example.com/private).\n\n- `bonjour`\n- 日本語😀\n\n```rust\nfn main() {}\n```\n\n<!-- hidden -->";
    assert_eq!(
        chunks(markdown, 4000),
        ["Greeting Hello, world. bonjour 日本語😀 fn main() {}"]
    );
    assert_eq!(
        chunks(
            "| Word | Meaning |\n| --- | --- |\n| bonjour | hello |",
            4000
        ),
        ["Word Meaning bonjour hello"]
    );
    assert_eq!(
        chunks("hello<br>world &amp; `*literal*`", 4000),
        ["hello world & *literal*"]
    );
}

#[test]
fn long_responses_keep_every_character_within_the_native_utf16_limit() {
    let text = "你好。😀".repeat(2500);
    let result = chunks(&text, 4000);
    assert!(result.len() > 1);
    assert_eq!(result.concat(), text);
    assert!(
        result
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= 4000)
    );
    assert_eq!(chunks("😀😀😀", 2), ["😀", "😀", "😀"]);
    let word = "a".repeat(4001);
    assert_eq!(chunks(&word, u64::MAX), ["a".repeat(4000), "a".into()]);
}

#[test]
fn prefers_sentence_and_word_boundaries_without_dropping_text() {
    assert_eq!(
        chunks("First. Second sentence here. Last.", 24),
        ["First.", "Second sentence here.", "Last."]
    );
    assert_eq!(
        chunks("one two three four", 8),
        ["one two", "three", "four"]
    );
}

#[test]
fn rejects_empty_text_invalid_limits_and_oversized_input() {
    for limit in [json!(0), json!(1), json!(-1), json!(2.5), Value::Null] {
        assert_eq!(call("Hello", limit)["ok"], false);
    }
    for text in ["", "  \n\t", "***\n\n<!-- hidden -->", "hello\0world"] {
        assert_eq!(call(text, json!(4000))["ok"], false);
    }
    assert_eq!(call(&"a".repeat(2_000_001), json!(4000))["ok"], false);
}
