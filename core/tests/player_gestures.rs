use lingoplay_core::execute;
use serde_json::{Value, json};

fn call(command: Value) -> Value {
    serde_json::from_str(&execute(command.to_string(), String::new())).unwrap()
}

#[test]
fn double_tap_accumulation_expires_changes_direction_and_stays_with_its_player() {
    let mut command = json!({"op":"player_double_tap","owner":"tap-test","videoId":"abcdefghijk","width":400,"x":390,"at":1000,"position":5,"duration":19});
    let first = call(command.clone());
    assert_eq!(first["ok"], true);
    assert_eq!(first["value"]["target"], 15.0);
    assert_eq!(first["value"]["accumulated"], 10);
    command["at"] = json!(1400);
    command["position"] = json!(15);
    let second = call(command.clone());
    assert_eq!(second["value"]["target"], 19.0);
    assert_eq!(second["value"]["accumulated"], 20);
    command["x"] = json!(0);
    command["at"] = json!(1600);
    assert_eq!(call(command.clone())["value"]["accumulated"], -10);
    command["at"] = json!(2000);
    assert_eq!(call(command.clone())["value"]["accumulated"], -20);
    command["at"] = json!(2801);
    assert_eq!(call(command.clone())["value"]["accumulated"], -10);
    command["owner"] = json!("new-player");
    command["at"] = json!(2900);
    assert_eq!(call(command.clone())["value"]["accumulated"], -10);
    command["videoId"] = json!("0123456789a");
    assert_eq!(call(command.clone())["value"]["accumulated"], -10);
    command["owner"] = json!("https://example.com/private");
    assert_eq!(call(command)["ok"], false);
}

#[test]
fn drag_policy_keeps_its_initial_mode_and_uses_source_sensitivity_and_thresholds() {
    let mut command = json!({"op":"player_gesture","width":400,"height":200,"x":138,"dx":0,"dy":-40,"position":5,"duration":19,"volume":0.5,"brightness":0.5});
    let brightness = call(command.clone());
    assert_eq!(brightness["value"]["kind"], "brightness");
    assert_eq!(brightness["value"]["target"], 0.8);
    command["x"] = json!(300);
    let volume = call(command.clone());
    assert_eq!(volume["value"]["kind"], "volume");
    assert_eq!(volume["value"]["target"], 0.74);
    command["x"] = json!(200);
    command["dy"] = json!(-16);
    assert_eq!(call(command.clone())["value"]["commit"], false);
    command["dy"] = json!(-17);
    assert_eq!(call(command.clone())["value"]["commit"], true);
    command["mode"] = json!("seek");
    command["dx"] = json!(20);
    command["dy"] = json!(100);
    let seek = call(command.clone());
    assert_eq!(seek["value"]["kind"], "seek");
    assert_eq!(seek["value"]["target"], 11.0);
    command["width"] = json!(0);
    assert_eq!(call(command)["ok"], false);
}
