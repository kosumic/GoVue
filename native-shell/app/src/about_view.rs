use govue_shell_runtime::Core;
use nsh_core::{
    Dialog, DialogButton, DialogButtonStyle,
    element::{dialog, share},
    platform::clipboard::data_uri,
    prelude::*,
    share::ShareRequest,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    browser::Browser,
    downloads::{Downloads, alert},
    settings_view::SettingsScreen,
    ui::{self, Palette, press},
};

const REPO: &str = "https://github.com/kosumic/GoVue";
const RELEASES: &str = "https://github.com/kosumic/GoVue/releases/latest";
const SHARE: &str = "M8 10l8-4m-8 8 8 4M8 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0m14-8a3 3 0 1 1-6 0 3 3 0 0 1 6 0m0 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0";
const CODE: &str = "m8 6-6 6 6 6m8-12 6 6-6 6m-2-15-4 18";
const EXTERNAL: &str = "M13 4h7v7m0-7-9 9m8 1v6H4V5h6";
const DOWNLOAD: &str = "M12 3v12m-4-4 4 4 4-4M4 16v4h16v-4";
const FILE: &str = "M5 3h9l5 5v13H5zM14 3v5h5M8 13h8m-8 4h8";
const REFRESH: &str = "M20 7v5h-5M4 17v-5h5m-4 1a7 7 0 0 1 12-6l3 4m-16 2 3 4a7 7 0 0 0 12-6";
const TRASH: &str = "M4 6h16M9 6V3h6v3M6 6l1 14h10l1-14M10 10v6m4-6v6";
const AT: &str = "M16 8v5a2 2 0 0 0 4 0v-1a8 8 0 1 0-3 6m-1-6a4 4 0 1 1-8 0 4 4 0 0 1 8 0";
const LICENSE: &str = "M12 3v17M5 20h14M4 7h16M5 7l-3 6h6zm14 0-3 6h6z";

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Notice {
    id: String,
    title: String,
    license: String,
    category: String,
    source_url: String,
    attribution: String,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Update {
    latest_version: String,
    update_available: bool,
}

pub fn about(
    core: Core,
    p: Palette,
    open: Signal<Option<SettingsScreen>>,
    browser: Browser,
    downloads: Downloads,
) -> View {
    let version = serde_json::from_str::<Value>(include_str!("../../../package.json"))
        .ok()
        .and_then(|manifest| manifest["version"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let build = signal(None::<String>);
    let checking = signal(false);
    let status = signal(None::<String>);
    let update = signal(None::<Update>);
    let cache = signal(false);
    spawn({
        let core = core.clone();
        async move {
            if let Ok(info) = core.execute::<Value>(json!({"op":"info"})).await
                && let Some(commit) = info["commit"].as_str().filter(|commit| {
                    (40..=64).contains(&commit.len())
                        && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                build.set(Some(commit.into()));
            }
        }
    });
    let icon = data_uri("image/png", include_bytes!("../../../assets/icon.png"));
    page(p, "About", "Navigate back", move || open.set(None)).child(scroll().flex_1().child(
        view().w_full().max_w(px(720.0)).self_center().pb(px(32.0))
            .child(view().items_center().pt(px(28.0)).pb(px(24.0)).px(px(24.0))
                .child(image(icon).size(px(80.0)).rounded(px(18.0)).mb(px(16.0)))
                .child(text("GoVue").text_size(px(28.0)).line_height(px(36.0)).font_weight(FontWeight::SEMIBOLD).text_color(move || p.ink()))
                .child(text("Watch YouTube. Learn from every line.").mt(px(6.0)).text_center().text_size(px(14.0)).text_color(move || p.muted()))
                .child(text(format!("Version {version}")).mt(px(10.0)).text_size(px(12.0)).text_color(move || p.muted()))
                .child(dynamic(move || {
                    let commit = build.get()?;
                    let short = &commit[..12];
                    let target = format!("{REPO}/commit/{commit}");
                    Some(press().h(px(44.0)).flex_row().items_center().gap(px(6.0))
                        .label(format!("Open commit {short}")).on_press(move || open_external(target.clone()))
                        .child(text(format!("Commit {short}")).text_size(px(12.0)).text_color(move || p.muted()))
                        .child(ui::icon(EXTERNAL, 12.0, move || p.muted())))
                })))
            .child(section(p, "Source & community")
                .child(view().flex_row().items_center().gap(px(8.0))
                    .child(link(p, "GitHub repository", "kosumic/GoVue", CODE, REPO).flex_1())
                    .child(ui::icon_button(p, SHARE, "Share repository", || share_text("GoVue source code", REPO.into()))))
                .child(link(p, "Follow the developer", "@Kosumi1989 on X", AT, "https://x.com/Kosumi1989")))
            .child(section(p, "Updates & sharing")
                .child(action(p, move || if checking.get() { "Checking for updates…" } else { "Check for updates" },
                    "Check the latest stable release on GitHub", REFRESH, false, {
                        let core = core.clone();
                        let version = version.clone();
                        move || {
                            if checking.get_untracked() { return; }
                            checking.set(true); update.set(None); status.set(None);
                            let core = core.clone();
                            let version = version.clone();
                            spawn(async move {
                                match core.execute::<Update>(json!({"op":"app_check_update","installedVersion":version})).await {
                                    Ok(result) => {
                                        status.set(Some(if result.update_available { format!("Version {} is available.", result.latest_version) } else { "You’re up to date.".into() }));
                                        update.set(Some(result));
                                    }
                                    Err(error) => status.set(Some(error.to_string())),
                                }
                                checking.set(false);
                            });
                        }
                    }).disabled(checking))
                .child(when(move || status.with(Option::is_some), move || detail(p, move || status.get().unwrap_or_default()).py(px(10.0))))
                .child(dynamic(move || {
                    let update = update.get().filter(|update| update.update_available)?;
                    Some(link(p, "View update", &format!("GoVue {}", update.latest_version), DOWNLOAD, RELEASES))
                }))
                .child(action(p, || "Share GoVue", "Send someone the latest release", SHARE, false,
                    || share_text("GoVue", format!("GoVue — Watch YouTube. Learn from every line.\n{RELEASES}")))))
            .child(section(p, "Check out my other apps")
                .child(link(p, "Whip", "Download Whip for Android and iOS", DOWNLOAD, "https://kosumic.github.io/whip/")))
            .child(section(p, "Credits & licenses")
                .child(link(p, "LiTube", "Based on HydeYYHH/Litube · GPL-3.0", CODE, "https://github.com/HydeYYHH/Litube"))
                .child(action(p, || "Open-source licenses", "GoVue’s license and bundled third-party notices", LICENSE, false,
                    move || open.set(Some(SettingsScreen::Licenses)))))
            .child(section(p, "Troubleshooting")
                .child(action(p, || "Clear cache", "Remove cached YouTube web resources", TRASH, false, move || {
                    let browser = browser.clone();
                    spawn(async move {
                        let response = dialog(Dialog {
                            id: 0,
                            title: Some("Clear cache?".into()),
                            message: Some("Remove cached web resources from YouTube.".into()),
                            buttons: vec![DialogButton{ text:"Cancel".into(),style:DialogButtonStyle::Cancel }, DialogButton{ text:"Clear".into(),style:DialogButtonStyle::Destructive }],
                            cancelable: true,
                            sheet: false,
                        }).await;
                        if response == 1 { browser.clear_cache(); cache.set(true); }
                    });
                }))
                .child(diagnostics(p, core.clone(), downloads.clone(), version.clone(), false))
                .child(diagnostics(p, core, downloads, version, true))
                .child(when(cache, move || detail(p, || "Cache cleared.".into()).py(px(10.0)))))
    ))
}

fn diagnostics(p: Palette, core: Core, downloads: Downloads, version: String, save: bool) -> View {
    let (title, description, icon) = if save {
        (
            "Save diagnostics",
            "Save a diagnostic JSON file to your device",
            DOWNLOAD,
        )
    } else {
        (
            "Share diagnostics",
            "App, core, and media transport information",
            FILE,
        )
    };
    action(
        p,
        move || title,
        description,
        icon,
        false,
        move || {
            let core = core.clone();
            let version = version.clone();
            let downloads = downloads.clone();
            spawn(async move {
                match core
                    .execute::<Value>(
                        json!({"op":"app_diagnostics_file","installedVersion":version}),
                    )
                    .await
                {
                    Ok(file) if save => downloads.save_export(file, "application/json"),
                    Ok(file) => {
                        let path = file["uri"]
                            .as_str()
                            .and_then(|uri| url::Url::parse(uri).ok())
                            .and_then(|uri| uri.to_file_path().ok());
                        if let Some(path) = path
                            && let Err(error) = share(
                                ShareRequest::new()
                                    .title("govue-diagnostics.json")
                                    .file(path),
                            )
                            .await
                        {
                            alert(error.message).await;
                        }
                    }
                    Err(error) => alert(error.to_string()).await,
                }
            });
        },
    )
}

pub fn licenses(core: Core, p: Palette, open: Signal<Option<SettingsScreen>>) -> View {
    let notices = signal(Vec::<Notice>::new());
    let failed = signal(false);
    let load = {
        let core = core.clone();
        move || {
            let core = core.clone();
            failed.set(false);
            spawn(async move {
                match core
                    .execute::<Vec<Notice>>(json!({"op":"app_licenses"}))
                    .await
                {
                    Ok(list) => notices.set(list),
                    Err(_) => failed.set(true),
                }
            });
        }
    };
    load();
    page(p, "Open-source licenses", "Back to About", move || open.set(Some(SettingsScreen::About)))
        .child(scroll().flex_1().child(view().w_full().max_w(px(720.0)).self_center().px(px(20.0)).pb(px(40.0))
            .child(when(move || notices.with(|notices| !notices.is_empty()), move || {
                detail(p, move || format!("{} notices", notices.with(Vec::len))).mb(px(10.0))
            }))
            .child(detail(p, || "GoVue is free software under the GNU General Public License v3.0. These project credits and direct dependency notices are bundled with the app and available offline.".into()))
            .child(when(failed, move || {
                let load = load.clone();
                action(p, || "Try again", "Could not load license notices. Please try again.", REFRESH, false, load)
            }))
            .children([("featured", "Project & credits"), ("npm", "npm dependencies"), ("cargo", "Rust dependencies")].into_iter().map(|(category, title)| {
                let core = core.clone();
                when(move || notices.with(|notices| notices.iter().any(|notice| notice.category == category)), move || {
                    let core = core.clone();
                    view().pt(px(24.0)).gap(px(4.0)).child(detail(p, move || title.into()).font_weight(FontWeight::SEMIBOLD))
                        .child(each(move || notices.with(|list| list.iter().filter(|item| item.category == category).cloned().collect::<Vec<_>>()),
                            |notice| notice.id.clone(), move |item, _| license(p, core.clone(), item.get_untracked())))
                })
            }).collect::<Vec<_>>())))
}

fn license(p: Palette, core: Core, notice: Notice) -> View {
    let opened = signal(false);
    let body = signal(None::<String>);
    let error = signal(false);
    let source = notice.source_url;
    let title = notice.title;
    let load = move || {
        let core = core.clone();
        let id = notice.id.clone();
        error.set(false);
        spawn(async move {
            match core
                .execute::<Value>(json!({"op":"app_license","id":id}))
                .await
            {
                Ok(result) => body.set(result["text"].as_str().map(str::to_owned)),
                Err(_) => error.set(true),
            }
        });
    };
    view()
        .gap(px(3.0))
        .pt(px(20.0))
        .pb(px(12.0))
        .border_b(px(0.5))
        .border_color(move || p.raised())
        .child(
            text(title.clone())
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
        .child(detail(p, move || notice.license.clone()))
        .child(detail(p, move || notice.attribution.clone()).mt(px(8.0)))
        .child(
            press()
                .label(format!("View source for {title}"))
                .h(px(48.0))
                .self_start()
                .items_center()
                .justify_center()
                .on_press(move || open_external(source.clone()))
                .child(text("View source").text_color(move || p.ink())),
        )
        .child(
            press()
                .label(move || {
                    format!(
                        "{} license for {title}",
                        if opened.get() { "Hide" } else { "View" }
                    )
                })
                .h(px(48.0))
                .self_start()
                .justify_center()
                .on_press({
                    let load = load.clone();
                    move || {
                        opened.update(|opened| *opened = !*opened);
                        if opened.get_untracked() && body.with_untracked(Option::is_none) {
                            load();
                        }
                    }
                })
                .child(
                    text(move || {
                        if opened.get() {
                            "Hide license"
                        } else {
                            "View license"
                        }
                    })
                    .text_color(move || p.ink()),
                ),
        )
        .child(when(opened, move || {
            let load = load.clone();
            view()
                .py(px(16.0))
                .child(when(error, move || {
                    action(
                        p,
                        || "Try again",
                        "Could not load this license. Please try again.",
                        REFRESH,
                        false,
                        load.clone(),
                    )
                }))
                .child(
                    text(move || body.get().unwrap_or_default())
                        .selectable(true)
                        .font_family("monospace")
                        .text_size(px(12.0))
                        .line_height(px(20.0))
                        .text_color(move || p.ink()),
                )
        }))
}

fn page(p: Palette, title: &'static str, back: &'static str, close: impl Fn() + 'static) -> View {
    view()
        .absolute()
        .inset(px(0.0))
        .bg(move || p.background())
        .flex_col()
        .child(
            view()
                .flex_row()
                .items_center()
                .h(px(48.0))
                .px(px(4.0))
                .child(ui::icon_button(p, ui::BACK, back, close))
                .child(
                    text(title)
                        .pl(px(4.0))
                        .text_size(px(20.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || p.ink()),
                ),
        )
}

fn section(p: Palette, heading: &'static str) -> View {
    view()
        .mx(px(16.0))
        .pt(px(20.0))
        .pb(px(16.0))
        .border_t(px(0.5))
        .border_color(move || p.raised())
        .child(detail(p, move || heading.into()).font_weight(FontWeight::SEMIBOLD))
}

fn detail(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}

fn action(
    p: Palette,
    title: impl Fn() -> &'static str + Clone + 'static,
    description: &str,
    icon: &'static str,
    external: bool,
    handler: impl Fn() + 'static,
) -> View {
    press()
        .label(title.clone())
        .on_press(handler)
        .flex_row()
        .items_center()
        .min_h(px(72.0))
        .px(px(4.0))
        .py(px(14.0))
        .gap(px(16.0))
        .child(ui::icon(icon, 23.0, move || p.ink()))
        .child(
            view()
                .flex_1()
                .gap(px(3.0))
                .child(
                    text(title)
                        .text_size(px(15.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || p.ink()),
                )
                .child(
                    text(description.to_owned())
                        .text_size(px(12.0))
                        .line_height(px(18.0))
                        .text_color(move || p.muted()),
                ),
        )
        .child(ui::icon(
            if external { EXTERNAL } else { ui::NEXT },
            18.0,
            move || p.muted(),
        ))
}

fn link(
    p: Palette,
    title: &'static str,
    description: &str,
    path: &'static str,
    url: &'static str,
) -> View {
    action(
        p,
        move || title,
        description,
        path,
        true,
        move || open_external(url.into()),
    )
}

fn open_external(target: String) {
    if url::Url::parse(&target).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
        spawn(async move {
            if !open_url(target).await {
                alert("Could not open this link.".into()).await;
            }
        });
    }
}

fn share_text(title: &'static str, message: String) {
    spawn(async move {
        if let Err(error) = share(ShareRequest::new().title(title).text(message)).await {
            alert(error.message).await;
        }
    });
}
