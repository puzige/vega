use super::*;

fn loading_indicator(colors: ThemeColors) -> AnyElement {
    div()
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
    colors: ThemeColors,
) -> gpui_kit::Img {
    gpui_kit::img(image)
        .size_full()
        .max_w(px(dimensions.0 as f32))
        .max_h(px(dimensions.1 as f32))
        .object_fit(gpui_kit::ObjectFit::Contain)
        .rounded(px(Layout::SCREENSHOT_RADIUS - Layout::SCREENSHOT_BORDER))
        .with_loading(move || loading_indicator(colors))
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

    fn image_content(&self, cx: &App) -> AnyElement {
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
                    colors,
                )
                .into_any_element(),
                None => loading_indicator(colors),
            })
            .into_any_element()
    }
}

impl Render for ScreenshotCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let (width, height) = self.thumbnail_size();
        div()
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
                    .child(self.image_content(cx))
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
