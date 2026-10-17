//! LiTube's extension preference groups, expressed as shared validated data.
use crate::{Result, media};
use serde_json::{Value, json};

pub(crate) fn defaults() -> Value {
    let mut value = json!({"adBlock":true,"sponsorBlock":true,"backgroundPlayback":true,"autoPip":true,
        "pauseAfterLine":false,"playbackSpeed":1.0,"qualityHeight":0,"qualityFps":0,
        "rememberPosition":true,"rememberQuality":true,"rememberSpeed":false,"rememberResize":false,
        "useOriginalTitle":false,"showDislikes":true,"showLikes":true,"hideShorts":false,"miniPlayer":true,
        "sponsorCountdown":true,"hapticStrength":30,"resizeMode":"fit","audioKey":"","subtitleKey":"",
        "subtitleStyle":{"scale":1.0,"vertical":1.0,"horizontal":0.0,"background":"semi","bold":false,"color":"#FFFFFF"},
        "categories":["sponsor","selfpromo","interaction","intro","outro","preview","music_offtopic","filler","poi_highlight"]});
    for mode in ["Windowed", "Fullscreen"] {
        for gesture in [
            "Tap",
            "DoubleTap",
            "LongPress",
            "Brightness",
            "Volume",
            "Seek",
            "Fullscreen",
        ] {
            value[format!("gesture{gesture}{mode}")] = json!(true);
        }
    }
    value
}

fn checked(key: &str, value: &Value) -> Result<Value> {
    let valid = match key {
        "playbackSpeed" => value
            .as_f64()
            .is_some_and(|n| super::super::media::playback::SPEEDS.contains(&n)),
        "qualityHeight" => value.as_u64().is_some_and(|n| n <= 8640),
        "qualityFps" => value.as_u64().is_some_and(|n| n <= 240),
        "hapticStrength" => value.as_u64().is_some_and(|n| n <= 100),
        "resizeMode" => matches!(
            value.as_str(),
            Some("fit" | "fill" | "zoom" | "width" | "height")
        ),
        "audioKey" | "subtitleKey" => value.as_str().is_some_and(|s| {
            s.len() <= 512
                && s.chars()
                    .all(|c| c.is_alphanumeric() || "_-.|:".contains(c))
        }),
        "categories" => value.as_array().is_some_and(|items| {
            items.len() <= 9
                && items.iter().all(|v| {
                    matches!(
                        v.as_str(),
                        Some(
                            "sponsor"
                                | "selfpromo"
                                | "interaction"
                                | "intro"
                                | "outro"
                                | "preview"
                                | "music_offtopic"
                                | "filler"
                                | "poi_highlight"
                        )
                    )
                })
        }),
        "subtitleStyle" => {
            let mut style = defaults()["subtitleStyle"].clone();
            let values = value.as_object().ok_or("Invalid subtitle appearance.")?;
            for (name, setting) in values {
                let valid = match name.as_str() {
                    "scale" => setting
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && (0.5..=3.0).contains(&n)),
                    "vertical" => setting
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && (0.0..=1.0).contains(&n)),
                    "horizontal" => setting
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && (-1.0..=1.0).contains(&n)),
                    "background" => matches!(setting.as_str(), Some("none" | "semi" | "opaque")),
                    "bold" => setting.is_boolean(),
                    "color" => setting.as_str().is_some_and(|s| {
                        s.len() == 7
                            && s.starts_with('#')
                            && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
                    }),
                    _ => false,
                };
                if !valid {
                    return Err("Invalid subtitle appearance.".into());
                }
                style[name] = setting.clone();
            }
            return Ok(style);
        }
        _ => value.is_boolean(),
    };
    if !valid {
        return Err(format!("Invalid player setting: {key}."));
    }
    Ok(value.clone())
}

pub(crate) fn settings(command: &Value, root: &str) -> Result<Value> {
    let mut result = defaults();
    let stored = media::read(root, "settings.json", json!({}));
    if let Some(values) = stored.as_object() {
        for (key, value) in values {
            if result.get(key).is_some()
                && let Ok(value) = checked(key, value)
            {
                result[key] = value;
            }
        }
    }
    if command["reset"] == true {
        result = defaults();
    }
    if let Some(values) = command["settings"].as_object() {
        for (key, value) in values {
            if result.get(key).is_some() {
                result[key] = checked(key, value)?;
            }
        }
    }
    if command["settings"].is_object() || command["reset"] == true {
        media::write(root, "settings.json", &result)?;
    }
    Ok(result)
}

pub(crate) fn catalog() -> Value {
    let toggle = |key: &str, title: &str| json!({"key":key,"title":title});
    let gesture = |mode: &str, title: &str| {
        json!({"id":mode,"title":title,"children":[
        toggle(&format!("gestureTap{mode}"),"Single tap"),toggle(&format!("gestureDoubleTap{mode}"),"Double tap"),
        toggle(&format!("gestureLongPress{mode}"),"Hold for 2×"),toggle(&format!("gestureBrightness{mode}"),"Brightness"),
        toggle(&format!("gestureVolume{mode}"),"Volume"),toggle(&format!("gestureSeek{mode}"),"Swipe to seek"),
        toggle(&format!("gestureFullscreen{mode}"),"Enter/exit fullscreen")]})
    };
    json!([
        {"id":"interface","title":"Interface","children":[{"key":"hapticStrength","title":"Haptic strength","kind":"slider"},{"key":"showDislikes","title":"Show dislikes","summary":"Sends video IDs to returnyoutubedislikeapi.com"},{"key":"showLikes","title":"Show likes","summary":"Sends video IDs to returnyoutubedislikeapi.com"},toggle("hideShorts","Hide Shorts")]},
        {"id":"player","title":"Playback","children":[toggle("rememberPosition","Remember position"),toggle("rememberQuality","Remember video quality"),toggle("rememberSpeed","Remember speed"),toggle("rememberResize","Remember resize mode"),toggle("useOriginalTitle","Use original title")]},
        {"id":"gesture","title":"Gestures","children":[gesture("Windowed","In embedded player"),gesture("Fullscreen","In fullscreen")]},
        {"id":"background","title":"Background & mini player","children":[toggle("autoPip","Picture-in-picture"),toggle("miniPlayer","In-app mini player"),toggle("backgroundPlayback","Background playback")]},
        {"id":"sponsorblock","title":"SponsorBlock","children":[{"key":"categories","title":"Skip categories","kind":"categories"},toggle("sponsorCountdown","Sponsor skip countdown")]},
        {"id":"ai","title":"AI","summary":"Providers and models","kind":"nav"},
        {"id":"data","title":"Backup & restore","kind":"nav"},
        {"id":"downloads","title":"Downloads","kind":"nav"}
    ])
}

fn aliases() -> Vec<(String, String)> {
    let mut keys = [
        ("enable_display_dislikes", "showDislikes"),
        ("enable_show_likes", "showLikes"),
        ("enable_hide_shorts", "hideShorts"),
        ("remember_quality", "rememberQuality"),
        ("remember_playback_speed", "rememberSpeed"),
        ("remember_last_position", "rememberPosition"),
        ("remember_resize_mode", "rememberResize"),
        ("use_original_title", "useOriginalTitle"),
        ("enable_background_play", "backgroundPlayback"),
        ("enable_pip", "autoPip"),
        ("enable_in_app_mini_player", "miniPlayer"),
        ("sponsor_countdown", "sponsorCountdown"),
    ]
    .into_iter()
    .map(|(a, b)| (a.into(), b.into()))
    .collect::<Vec<_>>();
    for (source, target) in [
        ("tap", "Tap"),
        ("double_tap", "DoubleTap"),
        ("long_press", "LongPress"),
        ("brightness", "Brightness"),
        ("volume", "Volume"),
        ("seek", "Seek"),
        ("fullscreen", "Fullscreen"),
    ] {
        for (source_mode, mode) in [("windowed", "Windowed"), ("fullscreen", "Fullscreen")] {
            keys.push((
                format!("gesture_{source}_{source_mode}"),
                format!("gesture{target}{mode}"),
            ));
        }
    }
    keys
}
const SKIPS: [(&str, &str); 9] = [
    ("skip_sponsors", "sponsor"),
    ("skip_self_promo", "selfpromo"),
    ("skip_interaction", "interaction"),
    ("skip_intro", "intro"),
    ("skip_outro", "outro"),
    ("skip_preview", "preview"),
    ("skip_music_offtopic", "music_offtopic"),
    ("skip_filler", "filler"),
    ("skip_poi_highlight", "poi_highlight"),
];
pub(crate) fn export(root: &str) -> Result<Value> {
    let current = settings(&json!({}), root)?;
    let download = crate::downloads::dispatch(&json!({"op":"downloads_config"}), root)?;
    let mut preferences = json!({});
    for (source, target) in aliases() {
        preferences[source] = current[target].clone();
    }
    for (source, category) in SKIPS {
        preferences[source] = json!(
            current["categories"]
                .as_array()
                .is_some_and(|v| v.iter().any(|c| c == category))
        );
    }
    Ok(
        json!({"schema":"litube-settings","format":"lingoplay-settings","version":1,"settings":current,"preferences":preferences,"hapticStrength":current["hapticStrength"],"download":{"wifiOnly":download["wifiOnly"],"maxConnections":download["maxConnections"],"defaultQuality":format!("{}p",download["defaultQuality"].as_u64().unwrap_or(1080))}}),
    )
}
pub(crate) fn import(data: &Value, root: &str) -> Result<Value> {
    if data["version"] != 1 {
        return Err("Unsupported settings backup version.".into());
    }
    let mut update = if data["format"] == "lingoplay-settings" && data["settings"].is_object() {
        data["settings"].clone()
    } else if data["schema"] == "litube-settings" && data["preferences"].is_object() {
        let mut update = json!({});
        let prefs = &data["preferences"];
        for (source, target) in aliases() {
            if let Some(value) = prefs.get(&source) {
                update[target] = value.clone();
            }
        }
        let mut categories = settings(&json!({}), root)?["categories"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for (source, category) in SKIPS {
            if let Some(value) = prefs.get(source) {
                let enabled = value
                    .as_bool()
                    .ok_or("Invalid imported SponsorBlock setting.")?;
                categories.retain(|c| c != category);
                if enabled {
                    categories.push(json!(category));
                }
            }
        }
        update["categories"] = json!(categories);
        if !data["hapticStrength"].is_null() {
            update["hapticStrength"] = data["hapticStrength"].clone();
        }
        update
    } else {
        return Err("Unsupported settings backup.".into());
    };
    let known = defaults();
    for (key, value) in update.as_object_mut().ok_or("Invalid settings backup.")? {
        if known.get(key).is_some() {
            *value = checked(key, value)?;
        }
    }
    let mut download = json!({});
    for key in ["wifiOnly", "maxConnections"] {
        if let Some(value) = data["download"].get(key) {
            download[key] = value.clone();
        }
    }
    if let Some(quality) = data["download"]["defaultQuality"].as_str() {
        download["defaultQuality"] = json!(
            quality
                .strip_suffix('p')
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| *n <= 8640)
                .ok_or("Invalid imported download quality.")?
        );
    }
    if !download["wifiOnly"].is_null() && !download["wifiOnly"].is_boolean()
        || !download["maxConnections"].is_null()
            && !download["maxConnections"]
                .as_u64()
                .is_some_and(|n| (1..=16).contains(&n))
    {
        return Err("Invalid imported download preferences.".into());
    }
    crate::downloads::dispatch(&json!({"op":"downloads_config","settings":download}), root)?;
    settings(&json!({"settings":update}), root)
}
