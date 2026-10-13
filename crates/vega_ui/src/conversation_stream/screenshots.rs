use super::*;
use crate::icons::{Icon, icon_button};

fn loading_indicator(preview: bool, colors: ThemeColors) -> AnyElement {
    div()
        .debug_selector(move || {
            if preview {
                "screenshot-preview-loading"
            } else {
                "screenshot-loading"
            }
            .into()
        })
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(Typography::METADATA))
        .text_color(colors.text_secondary)
        .child("加载中")
        .into_any_element()
}

fn screenshot_image(
    image: Arc<gpui_kit::Image>,
    dimensions: (u32, u32),
    preview: bool,
    colors: ThemeColors,
) -> gpui_kit::Img {
    gpui_kit::img(image)
        .debug_selector(move || {
            if preview {
                "screenshot-preview-pixels"
            } else {
                "screenshot-pixels"
            }
            .into()
        })
        .size_full()
        .max_w(px(dimensions.0 as f32))
        .max_h(px(dimensions.1 as f32))
        .object_fit(gpui_kit::ObjectFit::Contain)
        .rounded_b(px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER))
        .with_loading(move || loading_indicator(preview, colors))
}

pub(crate) struct ScreenshotCard {
    image: attachments::ImagePreview,
    saving: bool,
    preview_open: bool,
    status: Option<&'static str>,
    thumbnail_focus: FocusHandle,
    save_focus: FocusHandle,
    preview_focus: FocusHandle,
    preview_save_focus: FocusHandle,
    preview_close_focus: FocusHandle,
}

struct ScreenshotOpened;

impl EventEmitter<ScreenshotOpened> for ScreenshotCard {}

impl ScreenshotCard {
    fn pending(image: ImageAttachment, cx: &mut Context<Self>) -> Self {
        Self {
            image: attachments::ImagePreview {
                image,
                preview: Default::default(),
            },
            saving: false,
            preview_open: false,
            status: None,
            thumbnail_focus: cx.focus_handle(),
            save_focus: cx.focus_handle(),
            preview_focus: cx.focus_handle(),
            preview_save_focus: cx.focus_handle(),
            preview_close_focus: cx.focus_handle(),
        }
    }

    pub(crate) fn new(image: ImageAttachment, cx: &mut Context<Self>) -> Self {
        let card = Self::pending(image, cx);
        let pending = card.image.image.clone();
        cx.spawn(async move |this, cx| {
            let ready = cx
                .background_executor()
                .spawn(async move { attachments::ImagePreview::new(pending) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.image = ready;
                cx.notify();
            });
        })
        .detach();
        card
    }

    fn thumbnail_size(&self) -> (f32, f32) {
        let width = self.image.image.width() as f32;
        let height = self.image.image.height() as f32;
        let scale = (Layout::SCREENSHOT_MAX_WIDTH / width)
            .min(Layout::SCREENSHOT_MAX_HEIGHT / height)
            .min(1.0);
        (
            (width * scale).max(Layout::SCREENSHOT_MIN_WIDTH),
            (height * scale).max(Layout::SCREENSHOT_MIN_HEIGHT),
        )
    }

    fn open_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_open = true;
        self.preview_focus.focus(window, cx);
        cx.emit(ScreenshotOpened);
        cx.notify();
    }

    fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_open = false;
        self.thumbnail_focus.focus(window, cx);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let extension = match self.image.image.mime_type() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let directory = std::env::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
        let picked =
            cx.prompt_for_new_path(&directory, Some(&format!("Vega-screenshot.{extension}")));
        self.saving = true;
        self.status = None;
        let image = self.image.image.clone();
        cx.spawn(async move |this, cx| {
            let result = match picked.await {
                Ok(Ok(Some(path))) => Some(
                    cx.background_executor()
                        .spawn(async move {
                            vega_conversation::attachments::save_screenshot(&image, &path)
                        })
                        .await
                        .is_ok(),
                ),
                Ok(Ok(None)) => None,
                _ => Some(false),
            };
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.status = result.map(|success| {
                    if success {
                        "截图已保存"
                    } else {
                        "保存失败，请选择新的文件名重试"
                    }
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_button(&self, preview: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = if self.saving {
            "保存中…"
        } else {
            "保存图片"
        };
        icon_button(
            Icon::ArrowDown,
            label,
            colors,
            cx.listener(|this, _, _, cx| this.save(cx)),
        )
        .size(px(Layout::SCREENSHOT_CONTROL_SIZE))
        .debug_selector(move || {
            if preview {
                "screenshot-preview-save"
            } else {
                "screenshot-save"
            }
            .into()
        })
        .track_focus(if preview {
            &self.preview_save_focus
        } else {
            &self.save_focus
        })
        .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
        .when(self.saving, |button| button.opacity(0.5))
        .into_any_element()
    }

    fn header_caption(&self, width: f32, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        let label = self.status.unwrap_or("");
        div()
            .id("screenshot-caption")
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_secondary)
            .when_some(self.status, |caption, status| {
                caption
                    .debug_selector(|| "screenshot-save-status".into())
                    .aria_label(status)
                    .tooltip(move |_, cx| crate::icons::tooltip(status, cx))
            })
            .when(width >= Layout::SCREENSHOT_LABEL_MIN_WIDTH, |caption| {
                caption.child(if self.status.is_some() {
                    if label.starts_with("保存失败") {
                        "保存失败".into()
                    } else {
                        label.to_owned()
                    }
                } else {
                    format!(
                        "{} × {}",
                        self.image.image.width(),
                        self.image.image.height()
                    )
                })
            })
            .when(
                width < Layout::SCREENSHOT_LABEL_MIN_WIDTH && self.status.is_some(),
                |caption| {
                    caption.child(crate::icons::icon(
                        if label.starts_with("保存失败") {
                            Icon::Warning
                        } else {
                            Icon::Check
                        },
                        colors.text_secondary,
                    ))
                },
            )
            .into_any_element()
    }

    fn image_content(&self, preview: bool, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(match self.image.preview.get() {
                Some(image) => screenshot_image(
                    image.clone(),
                    (self.image.image.width(), self.image.image.height()),
                    preview,
                    colors,
                )
                .into_any_element(),
                None => loading_indicator(preview, colors),
            })
            .into_any_element()
    }

    fn render_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let close = icon_button(
            Icon::Close,
            "关闭预览",
            colors,
            cx.listener(|this, _, window, cx| this.close_preview(window, cx)),
        )
        .size(px(Layout::SCREENSHOT_CONTROL_SIZE))
        .track_focus(&self.preview_close_focus)
        .debug_selector(|| "screenshot-preview-close".into())
        .focus_visible(move |style| style.border_1().border_color(colors.brand_primary));
        gpui_kit::deferred(
            div()
                .id("screenshot-preview")
                .debug_selector(|| "screenshot-preview".into())
                .track_focus(&self.preview_focus)
                .absolute()
                .inset_0()
                .p(px(Layout::SCREENSHOT_PREVIEW_MARGIN))
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.bg_sidebar.opacity(0.85))
                .occlude()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_preview(window, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_key_down(
                    cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "escape" => this.close_preview(window, cx),
                            "tab" => {
                                let on_save = this.preview_save_focus.is_focused(window);
                                let on_close = this.preview_close_focus.is_focused(window);
                                let next =
                                    if on_save || (!on_close && event.keystroke.modifiers.shift) {
                                        &this.preview_close_focus
                                    } else {
                                        &this.preview_save_focus
                                    };
                                next.focus(window, cx);
                            }
                            _ => return,
                        }
                        cx.stop_propagation();
                    }),
                )
                .child(
                    div()
                        .debug_selector(|| "screenshot-preview-panel".into())
                        .size_full()
                        .max_w(px(Layout::SCREENSHOT_PREVIEW_MAX_WIDTH))
                        .max_h(px(Layout::SCREENSHOT_PREVIEW_MAX_HEIGHT))
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .rounded(px(Layout::SCREENSHOT_RADIUS))
                        .bg(colors.bg_base)
                        .border_1()
                        .border_color(colors.border_subtle)
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .h(px(Layout::SCREENSHOT_HEADER_HEIGHT))
                                .flex_shrink_0()
                                .px_1()
                                .flex()
                                .items_center()
                                .gap_1()
                                .bg(colors.bg_elevated)
                                .rounded_t(
                                    px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(Typography::METADATA))
                                        .text_color(colors.text_secondary)
                                        .child(self.status.unwrap_or("截图预览")),
                                )
                                .child(self.save_button(true, cx))
                                .child(close),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(self.image_content(true, cx)),
                        ),
                ),
        )
        .with_priority(3)
        .into_any_element()
    }
}

impl Render for ScreenshotCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let (width, height) = self.thumbnail_size();
        div()
            .debug_selector(|| "assistant-screenshot".into())
            .w(px(width + Layout::SCREENSHOT_BORDER * 2.0))
            .max_w_full()
            .flex()
            .flex_col()
            .border_1()
            .border_color(colors.border_subtle)
            .rounded(px(Layout::SCREENSHOT_RADIUS))
            .overflow_hidden()
            .bg(colors.bg_elevated)
            .child(
                div()
                    .h(px(Layout::SCREENSHOT_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px_1()
                    .child(self.header_caption(width, cx))
                    .child(self.save_button(false, cx)),
            )
            .child(
                div()
                    .id("screenshot-expand")
                    .debug_selector(|| "screenshot-expand".into())
                    .aria_label("查看截图大图")
                    .track_focus(&self.thumbnail_focus)
                    .tab_stop(true)
                    .w_full()
                    .aspect_ratio(width / height)
                    .relative()
                    .rounded_b(px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER))
                    .cursor_pointer()
                    .hover(move |style| style.border_1().border_color(colors.text_secondary))
                    .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                    .tooltip(|_, cx| crate::icons::tooltip("查看大图 · Enter", cx))
                    .on_key_down(
                        cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_preview(window, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.open_preview(window, cx)))
                    .child(self.image_content(false, cx)),
            )
    }
}

impl ConversationStream {
    pub(crate) fn screenshot_card(
        &mut self,
        image: ImageAttachment,
        cx: &mut Context<Self>,
    ) -> Entity<ScreenshotCard> {
        let card = cx.new(|cx| ScreenshotCard::new(image, cx));
        cx.subscribe(&card, |this, card, _: &ScreenshotOpened, cx| {
            if this.entries.iter().any(
                |entry| matches!(entry, StreamEntry::Screenshot { card: owned } if owned == &card),
            ) {
                if let Some(previous) = this.screenshot_preview.replace(card.clone())
                    && previous != card
                {
                    previous.update(cx, |previous, _| previous.preview_open = false);
                }
                cx.notify();
            }
        })
        .detach();
        cx.observe(&card, |this, card, cx| {
            let index = this.entries.iter().position(
                |entry| matches!(entry, StreamEntry::Screenshot { card: owned } if owned == &card),
            );
            if this.screenshot_preview.as_ref() == Some(&card) && !card.read(cx).preview_open {
                this.screenshot_preview = None;
            }
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        card
    }

    pub(crate) fn render_screenshot_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let card = self.screenshot_preview.clone()?;
        let owned = self.entries.iter().any(
            |entry| matches!(entry, StreamEntry::Screenshot { card: owned } if owned == &card),
        );
        if !owned {
            self.screenshot_preview = None;
            card.update(cx, |card, _| card.preview_open = false);
            self.selection_focus.focus(window, cx);
            return None;
        }
        Some(card.update(cx, |card, cx| card.render_preview(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext, size};

    struct CardHarness(Entity<ScreenshotCard>);

    impl Render for CardHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().p_4().child(self.0.clone())
        }
    }

    #[test]
    fn issue75_screenshot_pixels_have_own_corner_mask() {
        let image = ImageAttachment::from_bytes(
            include_bytes!("tests/fixtures/screenshot-landscape.png").to_vec(),
        )
        .unwrap();
        let preview = attachments::ImagePreview::new(image);
        let mut element = screenshot_image(
            preview.preview.get().unwrap().clone(),
            (preview.image.width(), preview.image.height()),
            false,
            vega_theme::Theme::light().colors,
        );
        let corners = &element.style().corner_radii;
        assert_eq!(corners.bottom_left, Some(px(7.0).into()));
        assert_eq!(corners.bottom_right, Some(px(7.0).into()));
        assert!(corners.top_left.is_none() && corners.top_right.is_none());
    }

    #[gpui_kit::test]
    async fn issue75_screenshot_loading_reserves_metadata_geometry_and_keyboard_target(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SettingsOpen(false));
            crate::init(cx);
        });
        let image = ImageAttachment::from_bytes(
            include_bytes!("tests/fixtures/screenshot-landscape.png").to_vec(),
        )
        .unwrap();
        let card = cx.new(|cx| ScreenshotCard::pending(image.clone(), cx));
        let root = card.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| CardHarness(root)))
                .unwrap()
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(320.0), px(600.0)));
        visual.run_until_parked();
        let before = visual.debug_bounds("assistant-screenshot").unwrap();
        let image_before = visual.debug_bounds("screenshot-expand").unwrap();
        assert!(visual.debug_bounds("screenshot-loading").is_some());
        visual.update(|window, cx| {
            let focus = card.read(cx).save_focus.clone();
            focus.focus(window, cx);
        });
        visual.simulate_keystrokes("space");
        assert!(
            cx.did_prompt_for_new_path(),
            "thumbnail save is keyboard accessible while loading"
        );
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        assert!(visual.update(|window, cx| card.read(cx).save_focus.is_focused(window)));

        visual.update(|window, cx| {
            let focus = card.read(cx).thumbnail_focus.clone();
            focus.focus(window, cx);
        });
        visual.simulate_keystrokes("space");
        assert!(card.read_with(&visual, |card, _| card.preview_open));
        card.update(&mut visual, |card, cx| {
            card.image = attachments::ImagePreview::new(image);
            cx.notify();
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("screenshot-loading").is_none());
        assert_eq!(visual.debug_bounds("assistant-screenshot").unwrap(), before);
        assert_eq!(
            visual.debug_bounds("screenshot-expand").unwrap(),
            image_before
        );
    }
}
