//! App release policy and bundled license notices shared by both platforms.
use crate::Result;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::sync::OnceLock;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Notice {
    id: Cow<'static, str>,
    title: Cow<'static, str>,
    license: Cow<'static, str>,
    category: Cow<'static, str>,
    source_url: Cow<'static, str>,
    attribution: Cow<'static, str>,
    #[serde(skip_serializing)]
    text: Cow<'static, str>,
}

impl Notice {
    const fn featured(
        id: &'static str,
        title: &'static str,
        license: &'static str,
        source_url: &'static str,
        attribution: &'static str,
        text: &'static str,
    ) -> Self {
        Self {
            id: Cow::Borrowed(id),
            title: Cow::Borrowed(title),
            license: Cow::Borrowed(license),
            category: Cow::Borrowed("featured"),
            source_url: Cow::Borrowed(source_url),
            attribution: Cow::Borrowed(attribution),
            text: Cow::Borrowed(text),
        }
    }
}

const FEATURED: &[Notice] = &[
    Notice::featured(
        "govue",
        "GoVue",
        "GPL-3.0",
        "https://github.com/kosumic/GoVue",
        "GoVue is free software under the GNU General Public License version 3.",
        include_str!("../../LICENSE"),
    ),
    Notice::featured(
        "litube",
        "LiTube",
        "GPL-3.0",
        "https://github.com/HydeYYHH/Litube",
        "GoVue is a React Native and Rust port of LiTube. Its browser scripts and player icons are copied or adapted from LiTube.",
        include_str!("../../notices/LiTube-GPL-3.0.txt"),
    ),
    Notice::featured(
        "bgutils",
        "BgUtils",
        "MIT",
        "https://github.com/LuanRT/BgUtils",
        "GoVue bundles BgUtils in its isolated YouTube token renderer.",
        include_str!("../../notices/BgUtils-MIT.txt"),
    ),
    Notice::featured(
        "chromium",
        "Chromium",
        "BSD",
        "https://chromium.googlesource.com/chromium/src",
        "GoVue's Cast adapter includes Chromium trust data adapted through LiTube.",
        include_str!("../../notices/Chromium-BSD.txt"),
    ),
    Notice::featured(
        "dash",
        "dash.js",
        "BSD and third-party notices",
        "https://github.com/Dash-Industry-Forum/dash.js",
        "GoVue's shared Cast browser player bundles dash.js 4.7.4 and its dependency notices.",
        concat!(
            include_str!("../../notices/Dash-js-BSD.txt"),
            "\n\n",
            include_str!("cast/assets/dash.all.min.js.LICENSE.txt")
        ),
    ),
    Notice::featured(
        "markdown",
        "Enriched Markdown & dependencies",
        "MIT and third-party notices",
        "https://github.com/software-mansion/enriched-markdown",
        "GoVue renders AI replies and video summaries with Enriched Markdown, including its parser, syntax, Unicode, and math components.",
        include_str!("../../notices/EnrichedMarkdown-ThirdParty.txt"),
    ),
    Notice::featured(
        "speech",
        "Expo Speech Recognition",
        "MIT",
        "https://github.com/jamsch/expo-speech-recognition",
        "GoVue uses Expo Speech Recognition for chat voice input.",
        include_str!("../../notices/Expo-Speech-Recognition-MIT.txt"),
    ),
    Notice::featured(
        "video",
        "Expo Video",
        "MIT",
        "https://github.com/expo/expo/tree/main/packages/expo-video",
        "GoVue builds Expo Video from source with local playback and picture-in-picture fixes.",
        include_str!("../../notices/Expo-Video-MIT.txt"),
    ),
    Notice::featured(
        "jsonwebtoken",
        "jsonwebtoken",
        "MIT",
        "https://github.com/Keats/jsonwebtoken",
        "GoVue's Rust core uses jsonwebtoken for identity-token verification.",
        include_str!("../../notices/Jsonwebtoken-MIT.txt"),
    ),
    Notice::featured(
        "newtube",
        "NewTube",
        "MIT",
        "https://github.com/aleixrodriala/newtube",
        "GoVue's Cast authentication and framing helpers include NewTube adaptations from LiTube.",
        include_str!("../../notices/NewTube-MIT.txt"),
    ),
    Notice::featured(
        "unicode",
        "Unicode data",
        "Unicode-3.0",
        "https://www.unicode.org/cldr/",
        "GoVue's language picker uses Unicode CLDR display names and language aliases.",
        include_str!("../../notices/Unicode-3.0.txt"),
    ),
];

fn notices() -> Result<impl Iterator<Item = &'static Notice>> {
    #[derive(Deserialize)]
    struct Bundle {
        notices: Vec<Notice>,
    }
    static DEPENDENCIES: OnceLock<Result<Bundle>> = OnceLock::new();
    let bundle = DEPENDENCIES.get_or_init(|| {
        serde_json::from_str(include_str!("../../notices/dependencies.json"))
            .map_err(|_| "Could not read bundled dependency notices.".into())
    });
    bundle
        .as_ref()
        .map(|bundle| FEATURED.iter().chain(bundle.notices.iter()))
        .map_err(Clone::clone)
}

pub fn dispatch(command: &Value) -> Result<Value> {
    match crate::string(command, "op")? {
        "app_licenses" => Ok(json!(notices()?.collect::<Vec<_>>())),
        "app_license" => {
            let id = crate::string(command, "id")?;
            notices()?
                .find(|notice| notice.id == id)
                .map(|notice| json!({"text": notice.text}))
                .ok_or_else(|| "Unknown license notice.".into())
        }
        "app_update" => {
            let installed = Version::parse(crate::string(command, "installedVersion")?)
                .map_err(|_| "Could not read the installed app version.")?;
            let release = &command["release"];
            if release["draft"] != false || release["prerelease"] != false {
                return Err("GitHub did not return a stable release.".into());
            }
            let tag = crate::string(release, "tag_name")?;
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
