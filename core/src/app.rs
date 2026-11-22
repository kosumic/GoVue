//! App release policy and bundled license notices shared by both platforms.
use crate::Result;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

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

const NATIVE: &[Notice] = &[
    Notice::featured(
        "nativeshell",
        "Native Shell",
        "Apache-2.0",
        "https://github.com/longbridge/nsh",
        "GoVue uses Native Shell for its native UI, system integration, and rich text rendering.",
        include_str!("../../notices/NativeShell-Apache-2.0.txt"),
    ),
    Notice::featured(
        "litube",
        "LiTube",
        "GPL-3.0",
        "https://github.com/HydeYYHH/Litube",
        "GoVue is based on LiTube. Its browser scripts and player icons are copied or adapted from LiTube.",
        include_str!("../../notices/LiTube-GPL-3.0.txt"),
    ),
];

fn notices(native: bool) -> Result<impl Iterator<Item = &'static Notice>> {
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
        .map(move |bundle| {
            let featured = FEATURED.iter().filter(move |notice| {
                !native || !matches!(&*notice.id, "litube" | "markdown" | "speech" | "video")
            });
            let format = if native { NATIVE } else { &[] };
            let dependencies = bundle
                .notices
                .iter()
                .filter(move |notice| !native || notice.category == "cargo");
            featured.chain(format).chain(dependencies)
        })
        .map_err(Clone::clone)
}

pub fn dispatch(command: &Value) -> Result<Value> {
    let native = command["runtime"] == "nativeShell";
    match crate::string(command, "op")? {
        "app_licenses" => Ok(json!(notices(native)?.collect::<Vec<_>>())),
        "app_license" => {
            let id = crate::string(command, "id")?;
            notices(native)?
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
        "app_diagnostics_file" => crate::downloads::runner::export_json(
            "govue-diagnostics.json",
            &json!({
                "app":"GoVue",
                "version":crate::string(command, "installedVersion")?,
                "core":crate::info(),
                "transport":crate::transport::status()?,
            }),
        ),
        _ => Err("Unknown app request.".into()),
    }
}

pub(crate) fn check_update(command: &Value) -> Result<Value> {
    const MAX_RELEASE: u64 = 256_000;
    let installed = crate::string(command, "installedVersion")?;
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let response = agent
        .get("https://api.github.com/repos/kosumic/GoVue/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .header("Cache-Control", "no-cache")
        .header("User-Agent", "GoVue")
        .call();
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Timeout(_)) => {
            return Err("Update check timed out. Please try again.".into());
        }
        Err(_) => return Err("Could not check for updates. Please try again.".into()),
    };
    match response.status().as_u16() {
        200 => {}
        404 => return Err("No stable release has been published yet.".into()),
        403 | 429 => return Err("GitHub is limiting update checks. Please try again later.".into()),
        _ => return Err("Could not check for updates. Please try again.".into()),
    }
    let mut raw = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_RELEASE + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "GitHub returned an unreadable release.")?;
    if raw.len() as u64 > MAX_RELEASE {
        return Err("GitHub returned an unreadable release.".into());
    }
    let release: Value =
        serde_json::from_slice(&raw).map_err(|_| "GitHub returned an unreadable release.")?;
    dispatch(&json!({"op":"app_update","installedVersion":installed,"release":release}))
}
