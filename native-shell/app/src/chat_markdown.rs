use nsh_base::{
    MarkdownOptions, TextViewCodeAction, TextViewLinkPressEvent, TextViewPart, TextViewParts,
    text_view,
};
use nsh_core::{element::StyleRefinement, prelude::*};
use url::Url;

use crate::ui::Palette;

pub fn chat_markdown(
    p: Palette,
    content: impl Fn() -> String + 'static,
    streaming: impl Fn() -> bool + 'static,
) -> impl IntoElement {
    use TextViewPart as Part;
    let heading = |size, line| {
        move |style: StyleRefinement| {
            style
                .text_size(px(size))
                .line_height(px(line))
                .font_weight(FontWeight::SEMIBOLD)
        }
    };
    provide_context(
        TextViewParts::new()
            .part(Part::Heading1, heading(24.0, 30.0))
            .part(Part::Heading2, heading(20.0, 27.0))
            .part(Part::Heading3, heading(17.0, 24.0))
            .part(Part::Heading4, heading(16.0, 24.0))
            .part(Part::Heading5, heading(15.0, 23.0))
            .part(Part::Heading6, heading(14.0, 22.0))
            .part(Part::CodeBlock, |style| style.rounded(px(10.0)).p(px(12.0)))
            .part(Part::CodeBlockCode, |style| {
                style.text_size(px(13.0)).line_height(px(20.0))
            })
            .part(Part::MathBlock, move |style| {
                style.bg(p.background()).p(px(8.0))
            }),
    );
    text_view()
        .markdown(move || Some(SharedString::from(content())))
        .options(move || MarkdownOptions {
            streaming: streaming(),
            ..MarkdownOptions::default()
        })
        .selectable(true)
        .on_link_press(|event: &TextViewLinkPressEvent| {
            let target = event.url.to_string();
            if Url::parse(&target).is_ok_and(|url| {
                matches!(url.scheme(), "https" | "http")
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none()
            }) {
                spawn(async move {
                    open_url(target).await;
                });
            }
        })
        .code_block_actions(vec![
            TextViewCodeAction::new("copy").with_label("Copy code"),
        ])
        .gap(px(12.0))
        .text_size(px(15.0))
        .line_height(px(24.0))
        .text_color(move || p.ink())
}
