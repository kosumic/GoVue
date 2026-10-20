//! App release policy and bundled license notices shared by both platforms.
use crate::{Result, string};
use semver::Version;
use serde_json::{Value, json};

struct Notice {
    id: &'static str,
    title: &'static str,
    license: &'static str,
    text: &'static str,
}

const NOTICES: &[Notice] = &[
    Notice {
        id: "govue",
        title: "GoVue / LiTube",
        license: "GPL-3.0",
        text: include_str!("../../LICENSE"),
    },
    Notice {
        id: "bgutils",
        title: "BgUtils",
        license: "MIT",
        text: include_str!("../../notices/BgUtils-MIT.txt"),
    },
    Notice {
        id: "chromium",
        title: "Chromium",
        license: "BSD",
        text: include_str!("../../notices/Chromium-BSD.txt"),
    },
    Notice {
        id: "dash",
        title: "dash.js",
        license: "BSD",
        text: include_str!("../../notices/Dash-js-BSD.txt"),
    },
    Notice {
        id: "markdown",
        title: "Enriched Markdown & dependencies",
        license: "Third-party notices",
        text: include_str!("../../notices/EnrichedMarkdown-ThirdParty.txt"),
    },
    Notice {
        id: "speech",
        title: "Expo Speech Recognition",
        license: "MIT",
        text: include_str!("../../notices/Expo-Speech-Recognition-MIT.txt"),
    },
    Notice {
        id: "video",
        title: "Expo Video",
        license: "MIT",
        text: include_str!("../../notices/Expo-Video-MIT.txt"),
    },
    Notice {
        id: "jsonwebtoken",
        title: "jsonwebtoken",
        license: "MIT",
        text: include_str!("../../notices/Jsonwebtoken-MIT.txt"),
    },
    Notice {
        id: "newtube",
        title: "NewTube",
        license: "MIT",
        text: include_str!("../../notices/NewTube-MIT.txt"),
    },
    Notice {
        id: "unicode",
        title: "Unicode data",
        license: "Unicode-3.0",
        text: include_str!("../../notices/Unicode-3.0.txt"),
    },
];

pub fn dispatch(command: &Value) -> Result<Value> {
    match string(command, "op")? {
        "app_licenses" => Ok(NOTICES
            .iter()
            .map(|notice| {
                json!({
                    "id": notice.id, "title": notice.title, "license": notice.license,
                })
            })
            .collect()),
        "app_license" => {
            let id = string(command, "id")?;
            NOTICES
                .iter()
                .find(|notice| notice.id == id)
                .map(|notice| json!({"text": notice.text}))
                .ok_or_else(|| "Unknown license notice.".into())
        }
        "app_update" => {
            let installed = Version::parse(string(command, "installedVersion")?)
                .map_err(|_| "Could not read the installed app version.")?;
            let release = &command["release"];
            if release["draft"] != false || release["prerelease"] != false {
                return Err("GitHub did not return a stable release.".into());
            }
            let tag = string(release, "tag_name")?;
            let latest = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
                .map_err(|_| "Could not read the latest release version.")?;
            if !latest.pre.is_empty() {
                return Err("GitHub did not return a stable release.".into());
            }
            Ok(json!({"latestVersion": latest.to_string(),
                "updateAvailable": latest.cmp_precedence(&installed).is_gt()}))
        }
        _ => Err("Unknown app request.".into()),
    }
}
