use lingoplay_core::execute;
use serde_json::{Value, json};
use std::{fs, time::SystemTime};

#[test]
fn pip_actions_follow_preferences_queue_and_audio_handoff_without_persisting_window_state() {
    let directory = std::env::temp_dir().join(format!(
        "lingoplay-pip-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let call = |command: Value| {
        serde_json::from_str::<Value>(&execute(
            command.to_string(),
            directory.to_string_lossy().into_owned(),
        ))
        .unwrap()
    };
    assert_eq!(
        call(json!({"op":"media_settings","settings":{"autoPip":true,"backgroundPlayback":true}}))
            ["ok"],
        true
    );
    assert_eq!(
        call(
            json!({"op":"player_add_batch","items":[{"videoId":"abcdefghijk","title":"First"},{"videoId":"lmnopqrstuv","title":"Second"}]})
        )["ok"],
        true
    );
    assert_eq!(
        call(json!({"op":"player_queue_enabled","enabled":true}))["ok"],
        true
    );
    let mut player =
        json!({"op":"player_pip","videoId":"abcdefghijk","active":true,"playing":true,"pip":true});
    let first = call(player.clone())["value"].clone();
    assert_eq!(first["available"], true);
    assert_eq!(first["autoEnter"], true);
    assert_eq!(first["actions"][0]["id"], "audio");
    assert_eq!(first["actions"][1]["label"], "Pause");
    assert_eq!(first["actions"][2]["enabled"], true);
    player["action"] = json!("toggle");
    assert_eq!(call(player.clone())["value"]["perform"], "pause");
    player["playing"] = json!(false);
    player["ended"] = json!(true);
    assert_eq!(call(player.clone())["value"]["perform"], "replay");
    player["playing"] = json!(true);
    player["pip"] = json!(false);
    player["action"] = json!("next");
    assert_eq!(call(player.clone())["ok"], false);
    player["pip"] = json!(true);
    assert_eq!(call(player.clone())["value"]["perform"], "next");
    player["action"] = json!("audio");
    let audio = call(player.clone())["value"].clone();
    assert_eq!(audio["perform"], "backgroundAudio");
    assert_eq!(audio["audioOnly"], true);
    assert_eq!(audio["autoEnter"], false);

    // The existing activity briefly resumes to leave PiP. That lifecycle event
    // must not undo the user's audio choice; a later deliberate return does.
    player["action"] = json!("foreground");
    player["handoff"] = json!(true);
    assert_eq!(call(player.clone())["value"]["audioOnly"], true);
    player["videoId"] = json!("lmnopqrstuv");
    player["action"] = json!("snapshot");
    assert_eq!(call(player.clone())["value"]["audioOnly"], true);
    player["action"] = json!("foreground");
    player["handoff"] = json!(false);
    assert_eq!(call(player.clone())["value"]["audioOnly"], false);
    assert_eq!(call(player.clone())["value"]["autoEnter"], true);

    call(json!({"op":"media_settings","settings":{"backgroundPlayback":false}}));
    player["action"] = json!("snapshot");
    let without_audio = call(player.clone())["value"].clone();
    assert_eq!(without_audio["actions"][0]["id"], "previous");
    assert_eq!(without_audio["actions"][0]["enabled"], true);
    assert_eq!(without_audio["actions"][2]["enabled"], false);
    player["action"] = json!("previous");
    assert_eq!(call(player.clone())["value"]["perform"], "previous");
    player["action"] = json!("audio");
    assert_eq!(call(player.clone())["ok"], false);
    call(json!({"op":"media_settings","settings":{"autoPip":false}}));
    player["action"] = json!("snapshot");
    let disabled = call(player.clone())["value"].clone();
    assert_eq!(disabled["available"], false);
    assert_eq!(disabled["autoEnter"], false);
    assert!(disabled["actions"].as_array().unwrap().is_empty());
    player["action"] = json!("toggle");
    assert_eq!(call(player.clone())["ok"], false);
    call(json!({"op":"media_settings","settings":{"autoPip":true,"backgroundPlayback":true}}));
    player["action"] = json!("audio");
    assert_eq!(call(player.clone())["value"]["audioOnly"], true);
    player["action"] = json!("audio_failed");
    assert_eq!(call(player.clone())["value"]["audioOnly"], false);
    player["active"] = json!(false);
    player["action"] = json!("snapshot");
    assert_eq!(call(player)["value"]["available"], false);

    for entry in fs::read_dir(directory.join("media")).unwrap() {
        let content = fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!content.contains("audioOnly"));
        assert!(!content.contains("backgroundAudio"));
        assert!(!content.contains("perform"));
    }
    fs::remove_dir_all(directory).unwrap();
}
