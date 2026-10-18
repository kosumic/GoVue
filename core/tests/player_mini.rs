use govue_core::execute;
use serde_json::{Value, json};
use std::fs;

#[test]
fn large_mini_players_reach_the_close_target_without_storing_drag_or_dismiss_frames() {
    let directory = std::env::temp_dir().join(format!(
        "govue-mini-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let call = |mut command: Value| {
        command["op"] = json!("player_mini");
        command["width"] = json!(430);
        command["height"] = json!(900);
        command["bottomInset"] = json!(24);
        serde_json::from_str::<Value>(&execute(
            command.to_string(),
            directory.to_string_lossy().into_owned(),
        ))
        .unwrap()
    };
    let initial = call(json!({}));
    assert_eq!(initial["ok"], true);
    assert_eq!(initial["value"]["width"], 190.0);
    assert_eq!(initial["value"]["bottom"], 44.0);
    let file = directory.join("media/mini-player.json");
    assert!(!file.exists());
    // At maximum phone width, reaching the target requires partial off-screen
    // movement; clamping to the window bottom would make dismissal unreachable.
    let drag = call(json!({"mode":"drag","frame":{"width":320,"x":-43,"y":82}}));
    assert_eq!(drag["value"]["closeArmed"], true);
    assert_eq!(drag["value"]["y"], 82.0);
    assert!(!file.exists());
    let dismissed = call(json!({"mode":"release","frame":{"width":320,"x":-43,"y":82}}));
    assert_eq!(dismissed["value"]["dismiss"], true);
    assert!(!file.exists());
    let cancelled = call(
        json!({"mode":"settle","frame":{"width":320,"x":-43,"y":82,"url":"https://example.com/private"}}),
    );
    assert_eq!(cancelled["value"]["dismiss"], false);
    assert_eq!(cancelled["value"]["y"], 44.0);
    assert_eq!(call(json!({}))["value"]["width"], 320.0);
    let stored = fs::read_to_string(&file).unwrap();
    assert!(!stored.contains("url"));
    assert!(!stored.contains("close"));
    assert_eq!(
        call(json!({"mode":"drag","frame":{"x":"invalid"}}))["ok"],
        false
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), stored);
    fs::write(&file, "[false]").unwrap();
    assert_eq!(call(json!({}))["value"]["width"], 190.0);
    fs::remove_dir_all(directory).unwrap();
}
