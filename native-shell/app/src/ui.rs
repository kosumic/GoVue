use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: Memo<bool>,
}

impl Palette {
    pub fn background(self) -> Hsla {
        rgb(if self.dark.get() { 0x0f0f0f } else { 0xffffff }).into()
    }

    pub fn ink(self) -> Hsla {
        rgb(if self.dark.get() { 0xffffff } else { 0x0f0f0f }).into()
    }

    pub fn muted(self) -> Hsla {
        rgb(if self.dark.get() { 0xaaaaaa } else { 0x606060 }).into()
    }

    pub fn raised(self) -> Hsla {
        rgb(if self.dark.get() { 0x272727 } else { 0xf2f2f2 }).into()
    }
}

pub const MENU: &str = "M4 6h16M4 12h16M4 18h16";
pub const CLOSE: &str = "m18 6-12 12M6 6l12 12";
pub const BACK: &str = "m15 18-6-6 6-6";
pub const NEXT: &str = "m9 18 6-6-6-6";
pub const SEARCH: &str = "M20 11a9 9 0 1 1-18 0 9 9 0 0 1 18 0zm-2.6 6.4L22 22";
pub const EDIT: &str = "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7m-1.7-9.3 2 2a1 1 0 0 1 0 1.4L12 15l-4 1 1-4 9-9.3a1 1 0 0 1 1.3 0z";
pub const DOWN: &str = "m6 9 6 6 6-6";
pub const UP: &str = "M12 19V5m-7 7 7-7 7 7";
pub const PLAY: &str = "M5 4.5 20 12 5 19.5z";
pub const STOP: &str = "M6 5h12a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z";
pub const CAPTIONS: &str = "M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm4 6h3m3 0h4M7 15h4m3 0h3";
pub const COPY: &str =
    "M8 5H5a2 2 0 0 0-2 2v12m5-8a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2z";

pub fn icon(path: &'static str, size: f32, color: impl Fn() -> Hsla + 'static) -> Svg {
    svg(move || {
        let color = color().to_rgb();
        let parts = [color.r, color.g, color.b].map(|part| (part * 255.0).round() as u8);
        let [r, g, b] = parts;
        ImageSrc::svg_markup(&format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"
            fill="none" stroke="#{r:02x}{g:02x}{b:02x}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="{path}"/></svg>"##))
    }).size(px(size))
}

pub fn icon_button(
    palette: Palette,
    path: &'static str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> View {
    press()
        .size(px(44.0))
        .items_center()
        .justify_center()
        .label(label)
        .on_press(on_press)
        .child(icon(path, 22.0, move || palette.ink()))
}

pub fn press() -> View {
    view().role(AccessibilityRole::Button)
}
