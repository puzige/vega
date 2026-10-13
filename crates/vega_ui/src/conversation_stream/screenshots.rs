use super::*;

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
        .rounded(px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER))
        .with_loading(move || loading_indicator(preview, colors))
}

pub(crate) struct ScreenshotCard {
    pub(super) image: attachments::ImagePreview,
}

impl ScreenshotCard {
    pub(crate) fn new(image: ImageAttachment, name: String, cx: &mut Context<Self>) -> Self {
        Self {
            image: attachments::ImagePreview::new(image, name, cx),
        }
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

    fn open_preview(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.image.view.update(cx, |view, cx| {
            view.action(crate::image_preview::ImagePreviewAction::Open, cx)
        });
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
                    .id("screenshot-expand")
                    .debug_selector(|| "screenshot-expand".into())
                    .aria_label("查看截图大图")
                    .track_focus(&self.image.view.read(cx).thumbnail_focus())
                    .tab_stop(true)
                    .w_full()
                    .aspect_ratio(width / height)
                    .relative()
                    .rounded(px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER))
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
                    .child(self.image_content(false, cx))
                    .child(crate::image_preview::ImagePreview::thumbnail_tracker(
                        &self.image.view,
                    )),
            )
    }
}

impl ConversationStream {
    pub(crate) fn screenshot_card(
        &mut self,
        image: ImageAttachment,
        cx: &mut Context<Self>,
    ) -> Entity<ScreenshotCard> {
        let ordinal = self
            .entries
            .iter()
            .filter(|entry| matches!(entry, StreamEntry::Screenshot { .. }))
            .count()
            + 1;
        let name = attachments::image_name("截图", ordinal, &image);
        let card = cx.new(|cx| ScreenshotCard::new(image, name, cx));
        let view = card.read(cx).image.view.clone();
        self.observe_image(&view, cx);
        cx.observe(&view, {
            let card = card.downgrade();
            move |_, _, cx| {
                let _ = card.update(cx, |_, cx| cx.notify());
                cx.notify();
            }
        })
        .detach();
        card
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
        let pixels = Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            image.bytes().to_vec(),
        ));
        let mut element = screenshot_image(
            pixels,
            (image.width(), image.height()),
            false,
            vega_theme::Theme::light().colors,
        );
        let corners = &element.style().corner_radii;
        assert_eq!(corners.bottom_left, Some(px(7.0).into()));
        assert_eq!(corners.bottom_right, Some(px(7.0).into()));
        assert_eq!(corners.top_left, Some(px(7.0).into()));
        assert_eq!(corners.top_right, Some(px(7.0).into()));
    }

    #[gpui_kit::test]
    async fn issue75_screenshot_loading_reserves_image_geometry_and_keyboard_target(
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
        let card = cx.new(|cx| {
            let view = cx.new(|cx| {
                crate::image_preview::ImagePreview::pending(image.clone(), "截图-1.png".into(), cx)
            });
            ScreenshotCard {
                image: attachments::ImagePreview {
                    image: image.clone(),
                    preview: view.read(cx).pixels(),
                    view,
                },
            }
        });
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
        assert!(visual.debug_bounds("screenshot-save").is_none());
        assert_eq!(before.size.height, image_before.size.height + px(2.0));
        assert_eq!(image_before.top(), before.top() + px(1.0));
        let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = actions.clone();
        let source = card.read_with(cx, |card, _| card.image.view.clone());
        cx.update(|cx| {
            cx.subscribe(
                &source,
                move |_, action: &crate::image_preview::ImagePreviewAction, _| {
                    captured.borrow_mut().push(*action)
                },
            )
            .detach()
        });
        visual.update(|window, cx| {
            card.read(cx)
                .image
                .view
                .read(cx)
                .thumbnail_focus()
                .focus(window, cx)
        });
        visual.simulate_keystrokes("space");
        assert!(matches!(
            actions.borrow().last(),
            Some(crate::image_preview::ImagePreviewAction::Open)
        ));
        card.update(&mut visual, |card, cx| {
            card.image
                .preview
                .set(Arc::new(gpui_kit::Image::from_bytes(
                    gpui_kit::ImageFormat::Png,
                    image.bytes().to_vec(),
                )))
                .unwrap();
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
