//! Bounded read-only file pane. Filesystem access remains in conversation/app.
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use vega_conversation::types::PaletteFilePreview;
use vega_theme::{Typography, theme};
/// Requests a revalidated Finder reveal of this relative path.
pub struct RevealFileRequested {
    pub relative_path: String,
}
/// Read-only project file contents, displayed in the workspace pane.
pub struct FilePreview {
    preview: PaletteFilePreview,
    focus: FocusHandle,
    markdown: Option<crate::conversation_stream::StreamModel>,
}
impl EventEmitter<RevealFileRequested> for FilePreview {}
impl FilePreview {
    /// Construct from a validated bounded worker projection.
    pub fn new(preview: PaletteFilePreview, cx: &mut Context<Self>) -> Self {
        let markdown = if preview.relative_path.ends_with(".md")
            || preview.relative_path.ends_with(".markdown")
        {
            let mut stream = vega_markdown::MarkdownStream::new();
            stream.append(&preview.content);
            stream.finish();
            let mut model = crate::conversation_stream::StreamModel::default();
            model.sync(
                &stream.snapshot(),
                &crate::conversation_stream::StreamCounters::default(),
            );
            Some(model)
        } else {
            None
        };
        Self {
            markdown,
            preview,
            focus: cx.focus_handle(),
        }
    }
    /// Workspace tab label.
    pub fn title(&self) -> &str {
        &self.preview.relative_path
    }
}
impl Focusable for FilePreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for FilePreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let body = if let Some(model) = &self.markdown {
            crate::conversation_stream::markdown_item(
                model,
                None,
                &colors,
                &format!("file-preview-{}", cx.entity_id()),
            )
        } else {
            div()
                .font_family("Menlo")
                .children(
                    self.preview
                        .content
                        .lines()
                        .enumerate()
                        .map(|(index, line)| {
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    div()
                                        .w(px(40.))
                                        .flex_shrink_0()
                                        .text_color(colors.text_tertiary)
                                        .child((index + 1).to_string()),
                                )
                                .child(line.to_string())
                        }),
                )
                .into_any_element()
        };
        div()
            .id("file-preview")
            .debug_selector(|| "file-preview".into())
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(Typography::METADATA))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(self.preview.relative_path.replace(['\n', '\r'], " ")),
                    )
                    .child("只读")
                    .child(
                        div()
                            .id("file-reveal")
                            .cursor_pointer()
                            .text_color(colors.text_secondary)
                            .child("在 Finder 中显示")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.emit(RevealFileRequested {
                                        relative_path: this.preview.relative_path.clone(),
                                    })
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("file-preview-scroll")
                    .overflow_scroll()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .text_size(px(Typography::CODE))
                    .child(body)
                    .scroll_edge_fade(colors.bg_base, ScrollFade::reading()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui_kit::test]
    fn scroll_edge_file_preview_keeps_header_outside_fade_and_clears_after_shrink(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
        let window = cx.open_window(size(px(300.), px(240.)), |_, cx| {
            FilePreview::new(
                PaletteFilePreview {
                    relative_path: "sample.txt".into(),
                    content: (0..80).map(|_| "A bounded file line.\n").collect(),
                },
                cx,
            )
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                let scale = window.scale_factor();
                let quads = window.painted_quads();
                let fade = quads
                    .iter()
                    .find(|quad| quad.background.as_solid().is_none())
                    .expect("file content fade");
                assert!(fade.bounds.top().as_f32() / scale >= 40.);
                assert_eq!(fade.content_mask.bounds.size.height.as_f32() / scale, 24.);
                let expected = gpui_kit::linear_gradient(
                    0.,
                    gpui_kit::linear_color_stop(vega_theme::LIGHT.bg_base, 0.),
                    gpui_kit::linear_color_stop(
                        vega_theme::LIGHT.bg_base.opacity(0.),
                        24. / (fade.bounds.size.height.as_f32() / scale),
                    ),
                );
                assert_eq!(fade.background, expected);
            })
            .expect("real file preview draw");
        window
            .update(cx, |view, _, cx| {
                view.preview.content = "short".into();
                cx.notify();
            })
            .expect("shorten file");
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .all(|quad| quad.background.as_solid().is_some())
                );
            })
            .expect("short file has no fade");
    }
}
