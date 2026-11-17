use super::*;

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
}

impl Render for ScreenshotCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = self.thumbnail_size();
        crate::image_preview::ImagePreview::screenshot_thumbnail(
            &self.image.view,
            (
                width + Layout::SCREENSHOT_BORDER * 2.0,
                height + Layout::SCREENSHOT_BORDER * 2.0,
            ),
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
        let name = attachments::image_name("screenshot", ordinal, &image);
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
