#![cfg(test)]

use serde_json::{Value, json};
use std::fs;

#[test]
fn mini_players_stay_in_bounds_and_dock_on_release_without_storing_drag_frames() {
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
        serde_json::from_str::<Value>(&govue_core::execute(
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
    // Dragging through the former close target stays within the workspace and
    // releasing docks the player instead of dismissing it.
    let drag = call(json!({"mode":"drag","frame":{"width":320,"x":-43,"y":82}}));
    assert_eq!(drag["ok"], true);
    assert_eq!(drag["value"]["x"], -43.0);
    assert_eq!(drag["value"]["y"], 44.0);
    assert!(!file.exists());
    let released = call(
        json!({"mode":"release","frame":{"width":320,"x":-43,"y":82,"url":"https://example.com/private"}}),
    );
    assert_eq!(released["ok"], true);
    assert_eq!(released["value"]["x"], -98.0);
    assert_eq!(released["value"]["y"], 44.0);
    assert_eq!(
        call(json!({}))["value"],
        released["value"],
        "The released placement survives reopening the mini-player"
    );
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
