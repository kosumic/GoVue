use std::rc::Rc;
use std::time::Duration;

use govue_shell_runtime::Core;
use nsh_core::{
    PasteEvent, ResizeMode,
    element::{
        ImageSrc, Owner, clipboard_has_image, clipboard_image, open_path, pick_files, pick_images,
    },
    picker::{FilePickRequest, PickRequest, PickedFile},
    prelude::*,
};
use serde_json::{Value, json};
use url::Url;

use crate::chat::Attachment;
use crate::ui::{self, Palette, press};

pub const PAPERCLIP: &str =
    "M21.4 11.6 12 21a6 6 0 0 1-8.5-8.5l10-10a4 4 0 0 1 5.7 5.7l-10 10a2 2 0 0 1-2.8-2.8l9-9";
const FILE: &str =
    "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zm0 0v6h6M8 13h8m-8 4h8";
const PHOTO: &str = "M3 3h18v18H3zm5 4a2 2 0 1 0 0 4 2 2 0 0 0 0-4zm13 9-4-4L7 21";
const CLIPBOARD: &str = "M9 3h6v4H9zm0 2H5v17h14V5h-4";

#[derive(Clone, Copy)]
enum Source {
    Photos,
    Files,
    Clipboard,
}

#[derive(Clone)]
pub struct AttachmentsView {
    core: Core,
    owner: Owner,
    p: Palette,
    files: Signal<Vec<Attachment>>,
    limit: Signal<usize>,
    busy: Signal<bool>,
    actions: Signal<bool>,
    clipboard: Signal<bool>,
    preview: Signal<Option<Attachment>>,
    error: Signal<Option<String>>,
    revision: Signal<u64>,
}

impl AttachmentsView {
    pub fn new(core: Core, p: Palette) -> Self {
        Self {
            core,
            owner: Owner::current(),
            p,
            files: signal(Vec::new()),
            limit: signal(0),
            busy: signal(false),
            actions: signal(false),
            clipboard: signal(false),
            preview: signal(None),
            error: signal(None),
            revision: signal(0),
        }
    }

    pub fn busy(&self) -> bool {
        self.busy.get()
    }

    pub fn empty(&self) -> bool {
        self.files.with(Vec::is_empty)
    }

    pub fn ids(&self) -> Vec<String> {
        self.files
            .with_untracked(|files| files.iter().map(|file| file.id.clone()).collect())
    }

    pub fn sent(&self) {
        self.files.set(Vec::new());
        self.error.set(None);
    }

    pub fn reset(&self) {
        let ids = self.ids();
        self.revision.update(|revision| *revision += 1);
        self.actions.set(false);
        self.preview.set(None);
        self.sent();
        self.release(ids);
    }

    fn release(&self, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        let core = self.core.clone();
        self.owner.run(|| {
            spawn(async move {
                let _ = core
                    .ai::<Value>(json!({"op":"ai_chat_attachment_release","attachments":ids}))
                    .await;
            });
        });
    }

    pub fn back(&self) -> bool {
        if self.preview.get_untracked().is_some() {
            self.preview.set(None);
        } else if self.actions.get_untracked() {
            self.actions.set(false);
        } else {
            return false;
        }
        true
    }

    pub fn show_actions(&self) {
        if self.busy() {
            return;
        }
        self.error.set(None);
        self.clipboard.set(false);
        self.actions.set(true);
        let clipboard = self.clipboard;
        spawn(async move {
            clipboard.set(clipboard_has_image().await);
        });
    }

    fn prepare(&self, run: impl AsyncFnOnce(Self, u64) + 'static) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let files = self.clone();
        let revision = self.revision.get_untracked();
        self.owner.run(|| {
            spawn(async move {
                let result = files.ensure_limit().await;
                if files.revision.get_untracked() == revision {
                    match result {
                        Ok(()) => run(files.clone(), revision).await,
                        Err(error) => files.error.set(Some(error)),
                    }
                }
                files.busy.set(false);
            });
        });
    }

    async fn ensure_limit(&self) -> Result<(), String> {
        if self.limit.get_untracked() == 0 {
            let caps: Value = self
                .core
                .ai(json!({"op":"ai_chat_attachment_limits"}))
                .await
                .map_err(|error| error.to_string())?;
            let cap = caps["maxAttachments"]
                .as_u64()
                .filter(|cap| *cap > 0)
                .ok_or("Attachment limits are unavailable.")?;
            self.limit.set(cap as usize);
        }
        if self.remaining() == 0 {
            return Err(format!(
                "Attach up to {} files per message.",
                self.limit.get_untracked()
            ));
        }
        Ok(())
    }

    fn remaining(&self) -> u32 {
        self.limit
            .get_untracked()
            .saturating_sub(self.files.with_untracked(Vec::len)) as u32
    }

    fn pick(&self, source: Source) {
        self.actions.set(false);
        self.prepare(async move |files, revision| {
            timer(Duration::from_millis(250)).await;
            if files.revision.get_untracked() != revision {
                return;
            }
            let count = files.remaining();
            match source {
                Source::Photos => {
                    let mut request = PickRequest::new(count > 1, count);
                    request.max_dimension = Some(DevicePixels(1600));
                    files
                        .import_picks(revision, pick_images(request).await)
                        .await;
                }
                Source::Files => {
                    let request = FilePickRequest::new(count > 1, count)
                        .max_size(govue_core::ai_attachment_source_limit());
                    files
                        .import_picks(revision, pick_files(request).await)
                        .await;
                }
                Source::Clipboard => {
                    let Some(image) = clipboard_image().await else {
                        files
                            .error
                            .set(Some("No image to paste from the clipboard.".into()));
                        return;
                    };
                    let Ok((mime, bytes)) = nsh_core::image::decode_data_uri(&image.url) else {
                        files
                            .error
                            .set(Some("Could not read the clipboard image.".into()));
                        return;
                    };
                    let name = match mime {
                        "image/jpeg" => "clipboard.jpg",
                        "image/webp" => "clipboard.webp",
                        _ => "clipboard.png",
                    };
                    files
                        .import(revision, name.into(), mime.into(), bytes)
                        .await;
                }
            }
        });
    }

    pub fn paste(&self, event: &PasteEvent) {
        if event.files.is_empty() {
            return;
        }
        let pasted = event.files.clone();
        self.prepare(async move |files, revision| {
            for file in pasted.into_iter().take(files.remaining() as usize) {
                files
                    .import(revision, file.name, file.mime, file.data)
                    .await;
            }
        });
    }

    async fn import_picks(&self, revision: u64, picked: Vec<PickedFile>) {
        for file in picked.into_iter().take(self.remaining() as usize) {
            if file.data.is_empty() {
                self.error.set(Some(format!(
                    "Could not read {} or the file is too large.",
                    file.name
                )));
                continue;
            }
            self.import(revision, file.name, file.mime, file.data).await;
        }
    }

    async fn import(
        &self,
        revision: u64,
        name: String,
        mime: String,
        bytes: impl AsRef<[u8]> + Send + 'static,
    ) {
        if self.revision.get_untracked() != revision {
            return;
        }
        match self
            .core
            .import_attachment::<Attachment>(name, mime, bytes)
            .await
        {
            Ok(file) if self.revision.get_untracked() != revision => self.release(vec![file.id]),
            Ok(file) => self.files.update(|files| {
                if files.iter().all(|draft| draft.id != file.id) {
                    files.push(file);
                }
            }),
            Err(error) if self.revision.get_untracked() == revision => {
                self.error.set(Some(error.to_string()))
            }
            Err(_) => {}
        }
    }

    pub fn notice(&self) -> impl IntoElement {
        let error = self.error;
        when(
            move || error.with(Option::is_some),
            move || {
                text(move || error.get().unwrap_or_default())
                    .px(px(12.0))
                    .text_color(rgb(0xdf3434))
                    .text_size(px(13.0))
                    .role(AccessibilityRole::Alert)
            },
        )
    }

    pub fn strip(
        &self,
        files: impl Fn() -> Vec<Attachment> + 'static,
        removable: Option<Rc<dyn Fn() -> bool>>,
    ) -> impl IntoElement {
        let list = computed(files);
        let panel = self.clone();
        when(
            move || list.with(|files| !files.is_empty()),
            move || {
                let panel = panel.clone();
                let removable = removable.clone();
                scroll()
                    .horizontal()
                    .scrollbar(false)
                    .w_full()
                    .h(px(116.0))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(10.0))
                            .px(px(8.0))
                            .py(px(8.0))
                            .child(each(
                                list,
                                |file| file.id.clone(),
                                move |file, _| panel.tile(file.get_untracked(), removable.clone()),
                            )),
                    )
            },
        )
    }

    pub fn draft_strip(&self, blocked: impl Fn() -> bool + 'static) -> impl IntoElement {
        let files = self.files;
        self.strip(move || files.get(), Some(Rc::new(blocked)))
    }

    fn tile(&self, file: Attachment, blocked: Option<Rc<dyn Fn() -> bool>>) -> View {
        let p = self.p;
        let is_image = file.kind == "image";
        let open = self.clone();
        let preview = file.clone();
        let label = format!("Preview {}", file.name);
        let content = if is_image {
            image(ImageSrc::from(file.uri.clone()))
                .w_full()
                .h_full()
                .object_fit(ResizeMode::Cover)
                .into_element()
        } else {
            view()
                .flex_1()
                .p(px(10.0))
                .gap(px(3.0))
                .child(ui::icon(FILE, 25.0, move || p.ink()))
                .child(
                    text(file.name.clone())
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(move || p.ink()),
                )
                .child(
                    text(format!(
                        "{} · {} KB",
                        file.kind.to_uppercase(),
                        file.bytes.div_ceil(1024).max(1)
                    ))
                    .text_size(px(10.0))
                    .text_color(move || p.muted()),
                )
                .into_element()
        };
        let mut tile = view()
            .relative()
            .w(px(if is_image { 88.0 } else { 176.0 }))
            .h(px(100.0))
            .child(
                press()
                    .flex_1()
                    .overflow_hidden()
                    .rounded(px(12.0))
                    .bg(move || p.raised())
                    .label(label)
                    .on_press(move || open.open(preview.clone()))
                    .child(content),
            );
        if let Some(blocked) = blocked {
            let panel = self.clone();
            let label = format!("Remove {}", file.name);
            tile = tile.child(
                press()
                    .absolute()
                    .top(px(0.0))
                    .right(px(0.0))
                    .size(px(36.0))
                    .rounded(px(18.0))
                    .items_center()
                    .justify_center()
                    .bg(move || p.background())
                    .disabled(move || blocked())
                    .label(label)
                    .on_press(move || {
                        panel
                            .files
                            .update(|files| files.retain(|draft| draft.id != file.id));
                        panel.release(vec![file.id.clone()]);
                    })
                    .child(ui::icon(ui::CLOSE, 16.0, move || p.ink())),
            );
        }
        tile
    }

    fn open(&self, file: Attachment) {
        if file.kind != "pdf" {
            self.preview.set(Some(file));
            return;
        }
        let error = self.error;
        spawn(async move {
            let path = Url::parse(&file.uri)
                .ok()
                .and_then(|uri| uri.to_file_path().ok());
            if let Some(path) = path
                && open_path(path).await.is_ok()
            {
                return;
            }
            error.set(Some(
                "Could not preview PDF. Install a PDF viewer to open this file.".into(),
            ));
        });
    }

    pub fn overlays(&self) -> impl IntoElement {
        let p = self.p;
        let actions = self.actions;
        let preview = self.preview;
        let panel = self.clone();
        let core = self.core.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .pointer_events(nsh_core::PointerEvents::BoxNone)
            .child(when(move || actions.get(), move || panel.action_sheet()))
            .child(when(
                move || preview.with(Option::is_some),
                move || {
                    let core = core.clone();
                    dynamic(move || {
                        let Some(file) = preview.get() else {
                            return view();
                        };
                        preview_view(p, core.clone(), file, move || preview.set(None))
                    })
                },
            ))
    }

    fn action_sheet(&self) -> View {
        let p = self.p;
        let actions = self.actions;
        let clipboard = self.clipboard;
        let paste = self.clone();
        view()
            .absolute()
            .inset(px(0.0))
            .items_center()
            .justify_center()
            .px(px(24.0))
            .child(
                press()
                    .absolute()
                    .inset(px(0.0))
                    .bg(rgba(0x00000080))
                    .label("Dismiss attachment options")
                    .on_press(move || actions.set(false)),
            )
            .child(
                view()
                    .w_full()
                    .max_w(px(420.0))
                    .rounded(px(24.0))
                    .p(px(12.0))
                    .bg(move || p.background())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .px(px(8.0))
                            .child(
                                text("Attach to message")
                                    .flex_1()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || p.ink()),
                            )
                            .child(ui::icon_button(
                                p,
                                ui::CLOSE,
                                "Close attachment options",
                                move || actions.set(false),
                            )),
                    )
                    .child(self.action(Source::Photos, PHOTO, "Photos", None))
                    .child(self.action(
                        Source::Files,
                        FILE,
                        "Files",
                        Some("Images, PDFs, and text/code files"),
                    ))
                    .child(when(
                        move || clipboard.get(),
                        move || {
                            paste.action(Source::Clipboard, CLIPBOARD, "Paste from clipboard", None)
                        },
                    )),
            )
    }

    fn action(
        &self,
        source: Source,
        icon: &'static str,
        label: &'static str,
        detail: Option<&'static str>,
    ) -> View {
        let panel = self.clone();
        let p = self.p;
        press()
            .min_h(px(52.0))
            .flex_row()
            .items_center()
            .gap(px(14.0))
            .px(px(12.0))
            .rounded(px(12.0))
            .label(label)
            .on_press(move || panel.pick(source))
            .child(ui::icon(icon, 22.0, move || p.ink()))
            .child(
                view()
                    .gap(px(3.0))
                    .child(text(label).text_size(px(15.0)).text_color(move || p.ink()))
                    .children(detail.map(|detail| {
                        text(detail)
                            .text_size(px(12.0))
                            .text_color(move || p.muted())
                    })),
            )
    }
}

fn preview_view(p: Palette, core: Core, file: Attachment, close: impl Fn() + 'static) -> View {
    let content = if file.kind == "image" {
        scroll()
            .flex_1()
            .scrollbar(false)
            .zoom_range(1.0, 4.0)
            .child(
                image(ImageSrc::from(file.uri))
                    .label(file.name.clone())
                    .w_full()
                    .h(move || px((nsh_core::element::window().height.0 - 100.0).max(1.0)))
                    .object_fit(ResizeMode::Contain),
            )
            .into_element()
    } else {
        let data = signal("Loading…".to_owned());
        spawn(async move {
            let result = core
                .ai::<Value>(json!({"op":"ai_chat_attachment_preview","id":file.id}))
                .await;
            data.set(match result {
                Ok(value) => value["text"].as_str().unwrap_or_default().to_owned(),
                Err(error) => error.to_string(),
            });
        });
        scroll()
            .flex_1()
            .child(
                text(move || data.get())
                    .selectable(true)
                    .p(px(20.0))
                    .text_size(px(14.0))
                    .line_height(px(21.0))
                    .font_family("monospace")
                    .text_color(move || p.ink()),
            )
            .into_element()
    };
    view()
        .absolute()
        .inset(px(0.0))
        .bg(move || p.background())
        .child(
            view()
                .flex_row()
                .items_center()
                .px(px(12.0))
                .child(
                    text(file.name)
                        .flex_1()
                        .truncate()
                        .text_size(px(14.0))
                        .text_color(move || p.ink()),
                )
                .child(ui::icon_button(
                    p,
                    ui::CLOSE,
                    "Close attachment preview",
                    close,
                )),
        )
        .child(content)
}
