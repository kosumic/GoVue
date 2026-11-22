use std::{fs, path::PathBuf, time::Duration};

use nsh_core::{
    Checked, PointerEvents,
    element::{ElementRef, ImageSrc, NativeEvent, blocking, data_dir, pick_images},
    picker::PickRequest,
    platform::Lifecycle,
    prelude::*,
    share::ShareRequest,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    chat::Chat,
    provider_model,
    provider_qr_artwork::artwork,
    ui::{self, Palette, press},
};

const SCAN: &str = "M4 8V4h4m8 0h4v4m0 8v4h-4m-8 0H4v-4m0-4h16";
const PHOTO: &str = "M4 4h16v16H4zm0 12 5-5 3 3 3-2 5 5M15 8h.01";
const TORCH: &str = "M8 3h8v5l-2 2v10h-4V10L8 8zM8 7h8";
const MAX_PICKED_IMAGE: usize = 20_000_000;

#[derive(Clone, PartialEq)]
enum Mode {
    Share {
        id: String,
        label: String,
        multiple: bool,
    },
    Scan,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Provider {
    label: String,
    base_url: String,
    protocol: String,
    model: String,
    model_count: usize,
    oauth: bool,
}

#[derive(Deserialize)]
struct Code {
    modules: Vec<String>,
    provider: Provider,
}

#[derive(Clone, PartialEq)]
struct Published {
    svg: String,
    provider: Provider,
}

#[derive(Clone, PartialEq)]
struct Candidate {
    code: String,
    provider: Provider,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Permission {
    granted: bool,
    can_ask_again: bool,
}

#[derive(Clone, PartialEq)]
struct ImageRead {
    request: String,
    path: PathBuf,
}

#[derive(Clone)]
pub struct ProviderQr {
    chat: Chat,
    p: Palette,
    owner: Owner,
    mode: Signal<Option<Mode>>,
    selected_only: Signal<bool>,
    published: Signal<Option<Published>>,
    candidate: Signal<Option<Candidate>>,
    permission: Signal<Option<Permission>>,
    torch: Signal<bool>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    revision: Signal<u64>,
    image: Signal<Option<ImageRead>>,
    platform: ElementRef,
}

impl ProviderQr {
    pub fn new(chat: Chat, p: Palette) -> Self {
        Self {
            chat,
            p,
            owner: Owner::current(),
            mode: signal(None),
            selected_only: signal(false),
            published: signal(None),
            candidate: signal(None),
            permission: signal(None),
            torch: signal(false),
            busy: signal(false),
            error: signal(None),
            revision: signal(0),
            image: signal(None),
            platform: element_ref(),
        }
    }

    pub fn visible(&self) -> bool {
        self.mode.with(Option::is_some)
    }

    pub fn show_share(&self, id: String, label: String, multiple: bool) {
        self.reset();
        self.mode.set(Some(Mode::Share {
            id,
            label,
            multiple,
        }));
        self.selected_only.set(false);
        self.generate();
    }

    pub fn show_scanner(&self) {
        self.reset();
        self.permission.set(None);
        self.torch.set(false);
        self.mode.set(Some(Mode::Scan));
    }

    pub fn back(&self) -> bool {
        if !self.visible() {
            return false;
        }
        if !self.busy.get_untracked() {
            self.mode.set(None);
            self.reset();
        }
        true
    }

    fn reset(&self) {
        self.revision.update(|value| *value += 1);
        self.published.set(None);
        self.candidate.set(None);
        self.error.set(None);
        self.busy.set(false);
        self.release_image();
    }

    fn release_image(&self) {
        if let Some(image) = self.image.get_untracked() {
            self.image.set(None);
            let _ = fs::remove_file(image.path);
        }
    }

    fn generate(&self) {
        let Some(Mode::Share { id, .. }) = self.mode.get_untracked() else {
            return;
        };
        self.reset();
        self.busy.set(true);
        let revision = self.revision.get_untracked();
        let selected_only = self.selected_only.get_untracked();
        let panel = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = panel
                    .chat
                    .core()
                    .ai::<Code>(json!({
                        "op":"ai_provider_qr","id":id,"selectedModelOnly":selected_only,
                    }))
                    .await;
                if panel.revision.get_untracked() != revision {
                    return;
                }
                panel.busy.set(false);
                match result {
                    Ok(code) => {
                        if let Some(svg) = artwork(&code.modules) {
                            panel.published.set(Some(Published {
                                svg,
                                provider: code.provider,
                            }));
                        } else {
                            panel
                                .error
                                .set(Some("Could not create a provider QR code.".into()));
                        }
                    }
                    Err(error) => panel.error.set(Some(error.to_string())),
                }
            })
        });
    }

    fn share_image(&self) {
        let Some(code) = self.published.get_untracked() else {
            return;
        };
        let Some(directory) = data_dir().map(|file| file.join("provider-transfer")) else {
            self.error
                .set(Some("The app's sharing directory is unavailable.".into()));
            return;
        };
        self.busy.set(true);
        self.error.set(None);
        let panel = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let created = blocking(move || png(code.svg, directory)).await;
                let result = match created {
                    Ok(Ok(path)) => share(
                        ShareRequest::new()
                            .title("Share provider QR code")
                            .file(path),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|error| error.message),
                    _ => Err("Could not create the QR image. Please try again.".into()),
                };
                panel.busy.set(false);
                if let Err(error) = result {
                    panel.error.set(Some(error));
                }
            })
        });
    }

    fn pick_image(&self) {
        let Some(directory) = data_dir().map(|path| path.join("provider-transfer")) else {
            self.error
                .set(Some("The app's QR image directory is unavailable.".into()));
            return;
        };
        self.reset();
        self.busy.set(true);
        let panel = self.clone();
        let revision = self.revision.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                let mut request = PickRequest::new(false, 1);
                request.max_dimension = Some(DevicePixels(2400));
                let Some(file) = pick_images(request).await.into_iter().next() else {
                    panel.busy.set(false);
                    return;
                };
                let request = revision.to_string();
                let extension = match file.mime.as_str() {
                    "image/jpeg" => "jpg",
                    "image/webp" => "webp",
                    "image/gif" => "gif",
                    _ => "png",
                };
                let path = directory.join(format!("provider-scan-{request}.{extension}"));
                let output = path.clone();
                let wrote = blocking(move || {
                    if file.data.is_empty() || file.data.len() > MAX_PICKED_IMAGE {
                        return false;
                    }
                    fs::create_dir_all(directory)
                        .and_then(|_| fs::write(output, file.data))
                        .is_ok()
                })
                .await
                .unwrap_or(false);
                if !wrote || path.to_str().is_none() {
                    panel.busy.set(false);
                    panel
                        .error
                        .set(Some("Could not read the selected QR image.".into()));
                    return;
                }
                if panel.revision.get_untracked() != revision {
                    let _ = fs::remove_file(path);
                    return;
                }
                panel.image.set(Some(ImageRead {
                    request,
                    path: path.clone(),
                }));
                timer(Duration::from_secs(15)).await;
                if panel
                    .image
                    .with_untracked(|image| image.as_ref().is_some_and(|image| image.path == path))
                {
                    panel.release_image();
                    panel.busy.set(false);
                    panel.error.set(Some(
                        "Could not read the selected QR image. Try again.".into(),
                    ));
                }
            })
        });
    }

    fn codes(&self, data: &Value) {
        let from_image = data["request"].as_str();
        if let Some(request) = from_image {
            if !self.image.with_untracked(|active| {
                active
                    .as_ref()
                    .is_some_and(|image| image.request == request)
            }) {
                return;
            }
            self.release_image();
            if let Some(error) = data["error"].as_str() {
                self.busy.set(false);
                self.error.set(Some(error.into()));
                return;
            }
        } else if self.busy.get_untracked() || lifecycle() != Lifecycle::Active {
            return;
        }
        let values: Vec<_> = data["values"]
            .as_array()
            .into_iter()
            .flatten()
            .take(10)
            .filter_map(Value::as_str)
            .filter(|value| value.len() <= 1000)
            .map(str::to_owned)
            .collect();
        if values.is_empty() {
            if from_image.is_some() {
                self.error.set(Some(
                    "No QR code found. Choose an image with the whole code visible.".into(),
                ));
                self.busy.set(false);
            }
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let panel = self.clone();
        let revision = self.revision.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                for code in values {
                    let result = panel
                        .chat
                        .core()
                        .ai::<Provider>(json!({"op":"ai_preview_provider_qr","payload":code}))
                        .await;
                    if panel.revision.get_untracked() != revision {
                        return;
                    }
                    if let Ok(provider) = result {
                        panel.candidate.set(Some(Candidate { code, provider }));
                        panel.busy.set(false);
                        return;
                    }
                }
                panel.busy.set(false);
                panel.error.set(Some(
                    "This code does not contain a supported GoVue provider.".into(),
                ));
            })
        });
    }

    fn import(&self, imported: impl Fn(Value) + 'static) {
        let Some(candidate) = self.candidate.get_untracked() else {
            return;
        };
        self.busy.set(true);
        self.error.set(None);
        let panel = self.clone();
        self.owner.run(|| {
            spawn(async move {
                let result = panel
                    .chat
                    .update_config(json!({"op":"ai_import_provider_qr","payload":candidate.code}))
                    .await;
                panel.busy.set(false);
                match result {
                    Ok(config) => {
                        let provider = config["providers"]
                            .as_array()
                            .and_then(|list| list.last())
                            .cloned();
                        panel.back();
                        if let Some(provider) = provider {
                            imported(provider);
                        }
                    }
                    Err(error) => panel.error.set(Some(error.to_string())),
                }
            })
        });
    }

    pub fn view(&self, imported: impl Fn(Value) + Clone + 'static) -> If {
        let panel = self.clone();
        let mode = self.mode;
        when(
            move || mode.with(Option::is_some),
            move || panel.page(imported.clone()),
        )
    }

    fn page(&self, imported: impl Fn(Value) + Clone + 'static) -> View {
        let p = self.p;
        let mode = self.mode;
        let candidate = self.candidate;
        let busy = self.busy;
        let error = self.error;
        let close = self.clone();
        let sharing = self.clone();
        let scanner = self.clone();
        let importing = self.clone();
        let retry = self.clone();
        let is_share = move || mode.with(|mode| matches!(mode, Some(Mode::Share { .. })));
        view()
            .absolute()
            .inset(px(0.0))
            .z_index(18)
            .bg(move || p.background())
            .child(
                view()
                    .min_h(px(60.0))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(22.0))
                    .child(
                        text(move || {
                            if is_share() {
                                "Share provider"
                            } else if candidate.with(Option::is_some) {
                                "Import provider"
                            } else {
                                "Scan provider"
                            }
                        })
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || p.ink()),
                    )
                    .child(
                        ui::icon_button(p, ui::CLOSE, "Close provider QR", move || {
                            close.back();
                        })
                        .disabled(busy),
                    ),
            )
            .child(
                scroll().flex_1().child(
                    view()
                        .px(px(22.0))
                        .pb(px(20.0))
                        .gap(px(14.0))
                        .child(when(is_share, move || sharing.share_view()))
                        .child(when(
                            move || !is_share() && candidate.with(Option::is_none),
                            move || scanner.scan_view(),
                        ))
                        .child(when(
                            move || candidate.with(Option::is_some),
                            move || {
                                let importing = importing.clone();
                                let imported = imported.clone();
                                let retry = retry.clone();
                                view()
                                    .gap(px(14.0))
                                    .child(provider_details(
                                        p,
                                        move || {
                                            candidate.with(|item| {
                                                item.as_ref().map(|item| item.provider.clone())
                                            })
                                        },
                                        true,
                                    ))
                                    .child(action(p, "Import provider", busy, move || {
                                        importing.import(imported.clone())
                                    }))
                                    .child(action(p, "Scan another code", busy, move || {
                                        retry.reset()
                                    }))
                            },
                        ))
                        .child(when(busy, move || help(p, || "Working…".into())))
                        .child(when(
                            move || error.with(Option::is_some),
                            move || {
                                help(p, move || error.get().unwrap_or_default())
                                    .text_color(rgb(0xcf2626))
                            },
                        )),
                ),
            )
    }

    fn share_view(&self) -> View {
        let p = self.p;
        let mode = self.mode;
        let code = self.published;
        let selected = self.selected_only;
        let generate = self.clone();
        let share = self.clone();
        let busy = self.busy;
        let width =
            computed(move || (nsh_core::element::window().width.0 - 72.0).clamp(1.0, 360.0));
        view()
            .gap(px(14.0))
            .child(title(p, move || {
                mode.with(|mode| match mode {
                    Some(Mode::Share { label, .. }) => label.clone(),
                    _ => String::new(),
                })
            }))
            .child(help(p, || "Scan with GoVue on another device.".into()))
            .child(view().items_center().child(dynamic(move || {
                match code.get() {
                    Some(code) => {
                        let markup = code.svg.split("<text").next().unwrap_or(&code.svg);
                        view()
                            .items_center()
                            .child(
                                svg(ImageSrc::svg_markup(&format!("{markup}</svg>")))
                                    .label(format!("QR code for {}", code.provider.label))
                                    .w(move || px(width.get()))
                                    .h(move || px(width.get() * 1.1)),
                            )
                            .child(
                                text("GoVue")
                                    .absolute()
                                    .bottom(move || px(width.get() * 0.021))
                                    .text_size(move || px(width.get() * 0.048))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x142c42))
                                    .pointer_events(PointerEvents::None),
                            )
                            .into_element()
                    }
                    None => view()
                        .w(move || px(width.get()))
                        .h(move || px(width.get() * 1.1))
                        .rounded(px(18.0))
                        .items_center()
                        .justify_center()
                        .bg(rgb(0xffffff))
                        .child(ui::icon(SCAN, 44.0, || rgb(0x176568).into()))
                        .into_element(),
                }
            })))
            .child(when(
                move || mode.with(|mode| matches!(mode, Some(Mode::Share { multiple: true, .. }))),
                move || {
                    let generate = generate.clone();
                    provider_model::toggle(
                        p,
                        "Selected model only",
                        move || selected.get(),
                        busy,
                        move |enabled| {
                            selected.set(enabled);
                            generate.generate();
                        },
                    )
                    .label("Share selected model only")
                },
            ))
            .child(when(
                move || code.with(Option::is_some),
                move || {
                    let share = share.clone();
                    view()
                        .gap(px(14.0))
                        .child(action(p, "Share QR image", busy, move || {
                            share.share_image()
                        }))
                        .child(provider_details(
                            p,
                            move || {
                                code.with(|code| code.as_ref().map(|code| code.provider.clone()))
                            },
                            false,
                        ))
                },
            ))
    }

    fn scan_view(&self) -> View {
        let p = self.p;
        let permission = self.permission;
        let torch = self.torch;
        let busy = self.busy;
        let error = self.error;
        let retry = self.clone();
        let choose = self.clone();
        let panel = self.clone();
        let platform = self.platform;
        let image = self.image;
        view()
            .gap(px(14.0))
            .child(
                view()
                    .h(px(276.0))
                    .rounded(px(16.0))
                    .overflow_hidden()
                    .bg(rgb(0x151515))
                    .items_center()
                    .justify_center()
                    .child(ui::icon(SCAN, 48.0, || rgb(0xffffff).into()))
                    .child(
                        native("govue/provider-qr-scanner")
                            .absolute()
                            .inset(px(0.0))
                            .element_ref(&self.platform)
                            .prop("camera", move || {
                                json!(
                                    lifecycle() == Lifecycle::Active
                                        && !busy.get()
                                        && error.with(Option::is_none)
                                )
                            })
                            .prop("torch", move || json!(torch.get()))
                            .prop("readImage", move || {
                                image.with(|image| {
                                    image.as_ref().map_or(
                                        Value::Null,
                                        |image| json!({"request":image.request,"path":image.path}),
                                    )
                                })
                            })
                            .on_native_event(move |event: &NativeEvent| {
                                match event.name.as_ref() {
                                    "permission" => {
                                        if let Ok(value) = Permission::deserialize(&event.data) {
                                            permission.set(Some(value));
                                        }
                                    }
                                    "codes" => panel.codes(&event.data),
                                    "error" => error.set(event.data.as_str().map(str::to_owned)),
                                    _ => {}
                                }
                            }),
                    )
                    .child(
                        view()
                            .absolute()
                            .size(px(180.0))
                            .rounded(px(14.0))
                            .border(px(2.0))
                            .border_color(rgb(0xffffff))
                            .pointer_events(PointerEvents::None),
                    ),
            )
            .child(help(p, || {
                "Point your camera at a GoVue provider QR code.".into()
            }))
            .child(when(
                move || {
                    permission.with(|permission| {
                        permission
                            .as_ref()
                            .is_some_and(|permission| !permission.granted)
                    })
                },
                move || {
                    dynamic(move || {
                        let allowed = permission.with(|permission| {
                            permission
                                .as_ref()
                                .is_some_and(|permission| permission.can_ask_again)
                        });
                        action(
                            p,
                            if allowed {
                                "Allow camera"
                            } else {
                                "Open Settings"
                            },
                            busy,
                            move || platform.command("permission", Value::Null),
                        )
                    })
                },
            ))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(tool(
                        p,
                        PHOTO,
                        "Choose provider QR image",
                        "Choose image",
                        busy,
                        move || choose.pick_image(),
                    ))
                    .child(when(
                        move || {
                            permission.with(|permission| {
                                permission
                                    .as_ref()
                                    .is_some_and(|permission| permission.granted)
                            })
                        },
                        move || {
                            tool(
                                p,
                                TORCH,
                                "Scanner flashlight",
                                "Flashlight",
                                busy,
                                move || torch.update(|on| *on = !*on),
                            )
                            .role(AccessibilityRole::Switch)
                            .accessibility_state(move || {
                                AccessibilityState {
                                    checked: Some(if torch.get() {
                                        Checked::True
                                    } else {
                                        Checked::False
                                    }),
                                    ..Default::default()
                                }
                            })
                        },
                    )),
            )
            .child(when(
                move || error.with(Option::is_some),
                move || {
                    let retry = retry.clone();
                    action(p, "Scan again", busy, move || retry.reset())
                },
            ))
    }
}

fn png(source: String, directory: PathBuf) -> Result<PathBuf, String> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    #[cfg(target_os = "android")]
    {
        options.fontdb_mut().load_fonts_dir("/system/fonts");
        options.fontdb_mut().set_sans_serif_family("Roboto");
    }
    let image =
        resvg::usvg::Tree::from_str(&source, &options).map_err(|error| error.to_string())?;
    let mut pixels =
        resvg::tiny_skia::Pixmap::new(1200, 1320).ok_or("Could not allocate QR image")?;
    resvg::render(
        &image,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("govue-provider-qr.png");
    pixels.save_png(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn title(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(22.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(move || p.ink())
}

fn help(p: Palette, value: impl Fn() -> String + 'static) -> Text {
    text(value)
        .text_size(px(13.0))
        .line_height(px(20.0))
        .text_color(move || p.muted())
}

fn action(p: Palette, label: &'static str, busy: Signal<bool>, work: impl Fn() + 'static) -> View {
    press()
        .label(label)
        .disabled(busy)
        .min_h(px(46.0))
        .rounded(px(10.0))
        .bg(move || p.raised())
        .items_center()
        .justify_center()
        .on_press(work)
        .child(
            text(label)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || p.ink()),
        )
}

fn tool(
    p: Palette,
    icon: &'static str,
    label: &'static str,
    name: &'static str,
    busy: Signal<bool>,
    work: impl Fn() + 'static,
) -> View {
    press()
        .label(label)
        .disabled(busy)
        .min_h(px(44.0))
        .rounded(px(10.0))
        .px(px(12.0))
        .bg(move || p.raised())
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .on_press(work)
        .child(ui::icon(icon, 21.0, move || p.ink()))
        .child(text(name).text_size(px(14.0)).text_color(move || p.ink()))
}

fn provider_details(
    p: Palette,
    data: impl Fn() -> Option<Provider> + Clone + 'static,
    heading: bool,
) -> View {
    let endpoint = data.clone();
    let protocol = data.clone();
    let model = data.clone();
    let name = data.clone();
    view().gap(px(8.0))
        .when(heading, |details| details.child(title(p, move || name().map(|data| data.label).unwrap_or_default())))
        .child(text(move || endpoint().map(|data| data.base_url).unwrap_or_default()).selectable(true)
            .text_size(px(14.0)).text_color(move || p.ink()))
        .child(help(p, move || protocol().map(|data| {
            let plural = if data.model_count == 1 { "model" } else { "models" };
            format!("{} · {} {plural}", data.protocol, data.model_count)
        }).unwrap_or_default()))
        .child(help(p, move || model().map(|data| data.model).unwrap_or_default()))
        .child(help(p, move || data().map(|data| {
            let next = if data.oauth { "Sign in on the receiving device." } else { "Add your API key on the receiving device if required." };
            format!("API keys, account tokens, custom headers and extra parameters are excluded. {next}")
        }).unwrap_or_default()))
}
