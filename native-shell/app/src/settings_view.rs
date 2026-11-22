use std::rc::Rc;

use govue_shell_runtime::{CaptionStyle, Core};
use nsh_core::element::{dialog, pick_files};
use nsh_core::{
    Checked, Dialog, DialogButton, DialogButtonStyle, PressEvent,
    file_dialog::FileFilter,
    picker::{FilePickRequest, PickedStatus},
    prelude::*,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::downloads::{Downloads, alert};
use crate::ui::{self, Palette, press};

const RESET: &str = "M3 12a9 9 0 1 0 2.6-6.4L3 8m0-5v5h5";
const SETTINGS: &str = "M12 2v3m0 14v3M2 12h3m14 0h3M19 12a7 7 0 1 1-14 0 7 7 0 0 1 14 0zm-4 0a3 3 0 1 1-6 0 3 3 0 0 1 6 0z";
const BACKUP: &str = "M3 9h18v11H3zM2 4h20v5H2zm8 9h4";
const IMPORT: &str = "M12 3v12m-5-5 5 5 5-5M4 16v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4";
const EXPORT: &str = "M12 15V3m-5 5 5-5 5 5M4 16v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4";
const BACKUP_LIMIT: usize = 1_000_000;

#[derive(Clone)]
pub struct Preferences {
    core: Core,
    pub state: Signal<Option<Map<String, Value>>>,
    commands: mpsc::UnboundedSender<Value>,
    error: Signal<Option<String>>,
    imported: Signal<bool>,
}

impl Preferences {
    pub fn new(core: Core) -> Self {
        let (send, mut receive) = mpsc::unbounded_channel();
        let preferences = Self {
            core,
            state: signal(None),
            commands: send,
            error: signal(None),
            imported: signal(false),
        };
        let worker = preferences.clone();
        spawn(async move {
            while let Some(command) = receive.recv().await {
                let importing = command["op"] == "media_settings_import";
                match worker.core.execute(command).await {
                    Ok(state) => {
                        worker.error.set(None);
                        worker.state.set(Some(state));
                        if importing {
                            worker.imported.set(true);
                        }
                    }
                    Err(error) if importing => {
                        spawn(alert(error.to_string()));
                    }
                    Err(error) => worker.error.set(Some(error.to_string())),
                }
            }
        });
        preferences.request(json!({}));
        preferences
    }

    pub fn update(&self, changes: Value) {
        self.request(json!({"settings":changes}));
    }

    pub fn caption_style(&self) -> Option<CaptionStyle> {
        self.state
            .with_untracked(|settings| settings.as_ref()?.get("subtitleStyle").cloned())
            .and_then(|style| serde_json::from_value(style).ok())
            .or_else(|| self.default_caption_style())
    }

    pub fn default_caption_style(&self) -> Option<CaptionStyle> {
        self.core
            .state(json!({"op":"player_caption_style_default"}))
            .ok()
    }

    pub fn caption_color<T: DeserializeOwned>(&self, mut values: Value) -> Option<T> {
        values["op"] = json!("player_caption_color");
        self.core.state(values).ok()
    }

    fn request(&self, mut values: Value) {
        values["op"] = json!("media_settings");
        let _ = self.commands.send(values);
    }

    fn read(&self, name: &str) -> Value {
        self.state.with(|state| {
            state
                .as_ref()
                .and_then(|settings| settings.get(name))
                .cloned()
                .unwrap_or(Value::Null)
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SettingsScreen {
    Extensions,
    Backup,
}

impl SettingsScreen {
    pub fn back(self) -> Option<Self> {
        match self {
            Self::Extensions => None,
            Self::Backup => Some(Self::Extensions),
        }
    }
}

#[derive(Clone, Deserialize, PartialEq)]
struct Node {
    id: Option<String>,
    key: Option<String>,
    title: String,
    summary: Option<String>,
    kind: Option<String>,
    category: Option<String>,
    children: Option<Vec<Node>>,
}

impl Node {
    fn identity(&self) -> String {
        self.id
            .as_ref()
            .or(self.key.as_ref())
            .unwrap_or(&self.title)
            .clone()
    }
}

#[derive(Clone)]
pub struct SettingsView {
    preferences: Preferences,
    palette: Palette,
    nodes: Signal<Vec<Node>>,
    open: Signal<Option<SettingsScreen>>,
    downloads: Downloads,
}

impl SettingsView {
    pub fn new(
        preferences: Preferences,
        palette: Palette,
        open: Signal<Option<SettingsScreen>>,
        downloads: Downloads,
    ) -> Self {
        let nodes = signal(Vec::new());
        let core = preferences.core.clone();
        spawn(async move {
            if let Ok(catalog) = core
                .execute::<Vec<Node>>(json!({"op":"media_settings_catalog"}))
                .await
            {
                nodes.set(catalog);
            }
        });
        Self {
            preferences,
            palette,
            nodes,
            open,
            downloads,
        }
    }

    pub fn view(&self) -> If {
        let open = self.open;
        let panel = self.clone();
        when(
            move || open.with(Option::is_some),
            move || {
                let extensions = panel.clone();
                let backup = panel.clone();
                view()
                    .absolute()
                    .inset(px(0.0))
                    .child(when(
                        move || open.get() == Some(SettingsScreen::Extensions),
                        move || extensions.extensions(),
                    ))
                    .child(when(
                        move || open.get() == Some(SettingsScreen::Backup),
                        move || backup.backup(),
                    ))
            },
        )
    }

    fn backup(&self) -> View {
        let p = self.palette;
        let open = self.open;
        let imported = self.preferences.imported;
        let core = self.preferences.core.clone();
        let downloads = self.downloads.clone();
        let restore = self.clone();
        let pending = signal(false);
        view()
            .absolute()
            .inset(px(0.0))
            .flex_col()
            .bg(move || p.background())
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .h(px(48.0))
                    .px(px(4.0))
                    .child(ui::icon_button(p, ui::BACK, "Navigate back", move || {
                        open.set(Some(SettingsScreen::Extensions))
                    }))
                    .child(
                        text("Backup & restore")
                            .flex_1()
                            .pl(px(4.0))
                            .text_size(px(20.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || p.ink()),
                    ),
            )
            .child(
                scroll().flex_1().child(
                    view()
                        .w_full()
                        .max_w(px(720.0))
                        .self_center()
                        .child(action_row(
                            p,
                            "Export settings",
                            Some("Save current settings to a JSON file"),
                            EXPORT,
                            move || {
                                if pending.get_untracked() {
                                    return;
                                }
                                pending.set(true);
                                let core = core.clone();
                                let downloads = downloads.clone();
                                spawn(async move {
                                    let result = core
                                        .execute(json!({"op":"media_settings_export_file"}))
                                        .await;
                                    if open.get_untracked() == Some(SettingsScreen::Backup) {
                                        pending.set(false);
                                        match result {
                                            Ok(file) => {
                                                downloads.save_export(file, "application/json")
                                            }
                                            Err(error) => alert(error.to_string()).await,
                                        }
                                    }
                                });
                            },
                        ))
                        .child(action_row(
                            p,
                            "Import settings",
                            Some("Restore settings from a backup file"),
                            IMPORT,
                            move || {
                                if pending.get_untracked() {
                                    return;
                                }
                                pending.set(true);
                                let restore = restore.clone();
                                spawn(async move {
                                    restore.import_backup().await;
                                    if open.get_untracked() == Some(SettingsScreen::Backup) {
                                        pending.set(false);
                                    }
                                });
                            },
                        ))
                        .child(when(imported, move || {
                            text("Settings imported.")
                                .p(px(16.0))
                                .text_size(px(13.0))
                                .text_color(move || p.muted())
                        })),
                ),
            )
    }

    async fn import_backup(&self) {
        let request = FilePickRequest::new(false, 1)
            .max_size(BACKUP_LIMIT as u64)
            .filter(FileFilter::new("JSON", ["json"]));
        let Some(file) = pick_files(request).await.into_iter().next() else {
            return;
        };
        if self.open.get_untracked() != Some(SettingsScreen::Backup) {
            return;
        }
        if file.status == PickedStatus::TooLarge || file.data.len() > BACKUP_LIMIT {
            alert("Settings backup is too large.".into()).await;
            return;
        }
        let Ok(data) = serde_json::from_slice::<Value>(&file.data) else {
            alert("Could not read this settings backup.".into()).await;
            return;
        };
        let action = dialog(Dialog {
            id: 0,
            title: Some("Import settings?".into()),
            message: Some("This replaces matching extension and player preferences.".into()),
            buttons: vec![
                DialogButton {
                    text: "Cancel".into(),
                    style: DialogButtonStyle::Cancel,
                },
                DialogButton {
                    text: "Import".into(),
                    style: DialogButtonStyle::Default,
                },
            ],
            cancelable: true,
            sheet: false,
        })
        .await;
        if action == 1 && self.open.get_untracked() == Some(SettingsScreen::Backup) {
            self.preferences.imported.set(false);
            let _ = self
                .preferences
                .commands
                .send(json!({"op":"media_settings_import","data":data}));
        }
    }

    fn extensions(&self) -> View {
        let p = self.palette;
        let open = self.open;
        let nodes = self.nodes;
        let panel = self.clone();
        let reset = self.preferences.clone();
        let error = self.preferences.error;
        let languages = signal(false);
        let language_core = self.preferences.core.clone();
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
                    .child(ui::icon_button(p, ui::BACK, "Navigate back", move || {
                        open.set(None)
                    }))
                    .child(
                        text("Extensions")
                            .flex_1()
                            .pl(px(4.0))
                            .text_size(px(20.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || p.ink()),
                    )
                    .child(ui::icon_button(p, RESET, "Reset extensions", move || {
                        let reset = reset.clone();
                        spawn(async move {
                            let action = dialog(Dialog {
                                id: 0,
                                title: Some("Reset extensions?".into()),
                                message: Some(
                                    "Restore all extension preferences to their defaults.".into(),
                                ),
                                buttons: vec![
                                    DialogButton {
                                        text: "Cancel".into(),
                                        style: DialogButtonStyle::Cancel,
                                    },
                                    DialogButton {
                                        text: "Reset".into(),
                                        style: DialogButtonStyle::Destructive,
                                    },
                                ],
                                cancelable: true,
                                sheet: false,
                            })
                            .await;
                            if action == 1 {
                                reset.request(json!({"reset":true}));
                            }
                        });
                    })),
            )
            .child(scroll().flex_1().child(view()
                .child(row(p, "Languages", None, 0, move || languages.update(|value| *value = !*value))
                    .child(ui::icon("M3 5h12M9 3v2m1 2c0 5-2 8-6 10m2-8c1 3 4 6 7 7m-1 5 5-11 5 11m-8-4h6", 24.0, move || p.ink()))
                    .child(dynamic(move || ui::icon(if languages.get() { ui::DOWN } else { ui::NEXT }, 24.0, move || p.muted()))))
                .child(when(languages, move || crate::language_view::Languages::new(language_core.clone(), p).view()))
                .child(each(
                move || {
                    nodes.with(|nodes| {
                        nodes
                            .iter()
                            .filter(|node| node.id.as_deref() != Some("ai"))
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                },
                Node::identity,
                move |node, _| panel.row(node.get_untracked(), 0),
            ))))
            .child(when(
                move || error.with(Option::is_some),
                move || {
                    text(move || error.get().unwrap_or_default())
                        .px(px(16.0))
                        .py(px(8.0))
                        .text_size(px(13.0))
                        .text_color(rgb(0xff0000))
                },
            ))
    }

    fn row(&self, node: Node, depth: usize) -> View {
        let p = self.palette;
        if node.kind.as_deref() == Some("nav") {
            let open = self.open;
            let downloads = self.downloads.clone();
            let imported = self.preferences.imported;
            let library = node.id.as_deref() == Some("downloads");
            return action_row(
                p,
                &node.title,
                node.summary.as_deref(),
                if library { IMPORT } else { BACKUP },
                move || {
                    if library {
                        downloads.open();
                    } else {
                        imported.set(false);
                        open.set(Some(SettingsScreen::Backup));
                    }
                },
            );
        }
        if let Some(children) = node.children {
            let open = signal(false);
            let panel = self.clone();
            let children = Rc::new(children);
            return view()
                .child(
                    row(p, &node.title, node.summary.as_deref(), depth, move || {
                        open.update(|open| *open = !*open)
                    })
                    .children((depth == 0).then(|| ui::icon(SETTINGS, 24.0, move || p.ink())))
                    .child(dynamic(move || {
                        ui::icon(
                            if open.get() { ui::DOWN } else { ui::NEXT },
                            24.0,
                            move || p.muted(),
                        )
                    })),
                )
                .child(when(open, move || {
                    view().children(
                        children
                            .iter()
                            .cloned()
                            .map(|child| panel.row(child, depth + 1))
                            .collect::<Vec<_>>(),
                    )
                }));
        }
        if node.kind.as_deref() == Some("slider") {
            return self.haptic_strength(node.title);
        }
        let preferences = self.preferences.clone();
        let enabled = computed({
            let preferences = preferences.clone();
            let target = node.key.clone();
            let category = node.category.clone();
            move || {
                if let Some(category) = &category {
                    preferences.read("sponsorBlock") == true
                        && preferences
                            .read("categories")
                            .as_array()
                            .is_some_and(|categories| categories.iter().any(|id| id == category))
                } else {
                    target
                        .as_ref()
                        .is_some_and(|key| preferences.read(key) == true)
                }
            }
        });
        let category = node.category;
        let target = node.key;
        row(p, &node.title, node.summary.as_deref(), depth, move || {
            let active = !enabled.get_untracked();
            if let Some(category) = &category {
                preferences.request(json!({"category":{"id":category,"enabled":active}}));
            } else if let Some(key) = &target {
                preferences.update(json!({key:active}));
            }
        })
        .role(AccessibilityRole::Switch)
        .accessibility_state(move || AccessibilityState {
            checked: Some(if enabled.get() {
                Checked::True
            } else {
                Checked::False
            }),
            ..Default::default()
        })
        .child(
            view()
                .relative()
                .w(px(42.0))
                .h(px(24.0))
                .rounded(px(12.0))
                .bg(move || if enabled.get() { p.ink() } else { p.muted() })
                .child(
                    view()
                        .absolute()
                        .top(px(3.0))
                        .left(move || px(if enabled.get() { 21.0 } else { 3.0 }))
                        .size(px(18.0))
                        .rounded(px(9.0))
                        .bg(move || p.background()),
                ),
        )
    }

    fn haptic_strength(&self, title: String) -> View {
        let p = self.palette;
        let settings = self.preferences.clone();
        let read = self.preferences.clone();
        let value = computed(move || read.read("hapticStrength").as_u64().unwrap_or(0));
        let width = signal(1.0_f32);
        view()
            .px(px(20.0))
            .py(px(10.0))
            .gap(px(8.0))
            .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
            .child(
                text(move || {
                    if value.get() == 0 {
                        "Off".into()
                    } else {
                        format!("{}%", value.get())
                    }
                })
                .text_size(px(12.0))
                .text_color(move || p.muted()),
            )
            .child(
                press()
                    .label("Haptic strength")
                    .h(px(32.0))
                    .justify_center()
                    .on_layout(move |bounds: &Bounds<Points>| {
                        width.set(bounds.size.width.0.max(1.0))
                    })
                    .on_press(move |event: &PressEvent| {
                        let percent = (event.position.x.0 / width.get_untracked() * 100.0)
                            .clamp(0.0, 100.0)
                            .round() as u32;
                        settings.update(json!({"hapticStrength":percent}));
                    })
                    .child(
                        view().h(px(3.0)).bg(move || p.muted()).child(
                            view()
                                .h_full()
                                .w(move || percentage(value.get() as f32 / 100.0))
                                .bg(rgb(0x3ea6ff)),
                        ),
                    ),
            )
    }
}

fn action_row(
    p: Palette,
    label: &str,
    detail: Option<&str>,
    icon: &'static str,
    action: impl Fn() + 'static,
) -> View {
    let title = label.to_owned();
    let detail = detail.map(str::to_owned);
    press()
        .label(title.clone())
        .on_press(action)
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .min_h(px(48.0))
        .py(px(8.0))
        .child(ui::icon(icon, 24.0, move || p.ink()))
        .child(
            view()
                .flex_1()
                .gap(px(3.0))
                .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
                .children(detail.map(|detail| {
                    text(detail)
                        .text_size(px(12.0))
                        .text_color(move || p.muted())
                })),
        )
        .child(ui::icon(ui::NEXT, 24.0, move || p.muted()))
}

fn row(
    p: Palette,
    label: &str,
    detail: Option<&str>,
    depth: usize,
    action: impl Fn() + 'static,
) -> View {
    let title = label.to_owned();
    let detail = detail.map(str::to_owned);
    press()
        .label(title.clone())
        .on_press(action)
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .pl(px(16.0 + depth.saturating_sub(1) as f32 * 12.0))
        .pr(px(16.0))
        .min_h(px(48.0))
        .py(px(8.0))
        .child(
            view()
                .flex_1()
                .gap(px(3.0))
                .child(text(title).text_size(px(14.0)).text_color(move || p.ink()))
                .children(detail.map(|detail| {
                    text(detail)
                        .text_size(px(12.0))
                        .text_color(move || p.muted())
                })),
        )
}
