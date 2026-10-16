//!  R1–R4: explicit, bounded attachment intents owned by this stream.
use super::*;
use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths, Image, PathPromptOptions};
use std::path::PathBuf;
use vega_conversation::attachments::{MAX_IMAGE_BYTES, MAX_IMAGES, MAX_TURN_IMAGE_BYTES};

#[derive(Clone)]
pub(crate) struct ImagePreview {
    pub image: ImageAttachment,
    pub(super) preview: Arc<std::sync::OnceLock<Arc<Image>>>,
    pub(super) view: Entity<crate::image_preview::ImagePreview>,
}

impl ImagePreview {
    pub(crate) fn new(image: ImageAttachment, name: String, cx: &mut App) -> Self {
        let view = cx.new(|cx| crate::image_preview::ImagePreview::new(image.clone(), name, cx));
        let preview = view.read(cx).pixels();
        Self {
            image,
            preview,
            view,
        }
    }
}

pub(crate) fn image_name(prefix: &str, ordinal: usize, image: &ImageAttachment) -> String {
    let extension = match image.mime_type() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    format!("{prefix}-{ordinal}.{extension}")
}

const IMPORT_ERROR: &str =
    "图片添加失败：请选择有效的 PNG、JPEG 或 WebP；最多 4 张，单张 8 MiB，总计 16 MiB。";

impl ConversationStream {
    pub(crate) fn history_image_previews(
        &mut self,
        images: Vec<ImageAttachment>,
        cx: &mut Context<Self>,
    ) -> Vec<ImagePreview> {
        images
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                let name = image_name("图片", index + 1, &image);
                let preview = ImagePreview::new(image, name, cx);
                self.observe_image(&preview.view, cx);
                preview
            })
            .collect()
    }

    pub(crate) fn observe_image(
        &self,
        view: &Entity<crate::image_preview::ImagePreview>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(
            view,
            |this, image, action: &crate::image_preview::ImagePreviewAction, cx| {
                if this.owns_image_preview(&image, cx) {
                    cx.emit(ImagePreviewRequested {
                        image: image.clone(),
                        action: *action,
                    });
                }
            },
        )
        .detach();
        cx.observe(view, |_, _, cx| cx.notify()).detach();
    }

    pub(crate) fn reset_image_visibility(&self, cx: &mut App) {
        for item in self.pending_ui.items.values() {
            for image in &item.images {
                image.view.update(cx, |view, _| view.reset_thumbnail());
            }
        }
        for (_, image) in &self.attachments {
            image.view.update(cx, |view, _| view.reset_thumbnail());
        }
        for entry in &self.entries {
            match entry {
                StreamEntry::UserImages { images } => {
                    for image in images {
                        image.view.update(cx, |view, _| view.reset_thumbnail());
                    }
                }
                StreamEntry::Screenshot { card } => {
                    let image = card.read(cx).image.view.clone();
                    image.update(cx, |view, _| view.reset_thumbnail());
                }
                _ => {}
            }
        }
    }

    pub fn owns_image_preview(
        &self,
        image: &Entity<crate::image_preview::ImagePreview>,
        cx: &App,
    ) -> bool {
        self.attachments
            .iter()
            .any(|(_, entry)| &entry.view == image)
            || self
                .pending_ui
                .items
                .values()
                .any(|item| item.images.iter().any(|entry| &entry.view == image))
            || self.entries.iter().any(|entry| match entry {
                StreamEntry::UserImages { images } => {
                    images.iter().any(|entry| &entry.view == image)
                }
                StreamEntry::Screenshot { card } => &card.read(cx).image.view == image,
                _ => false,
            })
    }

    pub(crate) fn paste_images(&mut self, item: Arc<ClipboardItem>, cx: &mut Context<Self>) {
        self.import_images(
            move || {
                // R3: reject aggregate cardinality/bytes before any decoder work.
                let mut count = 0usize;
                let mut bytes = 0usize;
                for entry in item.entries() {
                    match entry {
                        ClipboardEntry::Image(image) => {
                            count = count.saturating_add(1);
                            bytes = bytes.saturating_add(image.bytes().len());
                            if image.bytes().len() > MAX_IMAGE_BYTES {
                                return Err(());
                            }
                        }
                        ClipboardEntry::ExternalPaths(paths) => {
                            count = count.saturating_add(paths.paths().len())
                        }
                        ClipboardEntry::String(_) => {}
                    }
                    if count > MAX_IMAGES || bytes > MAX_TURN_IMAGE_BYTES {
                        return Err(());
                    }
                }
                let mut images = Vec::new();
                for entry in item.entries() {
                    match entry {
                        ClipboardEntry::Image(image) => images.push(
                            ImageAttachment::from_bytes(image.bytes().to_vec()).map_err(|_| ())?,
                        ),
                        ClipboardEntry::ExternalPaths(paths) => images.extend(
                            vega_conversation::attachments::import_images(paths.paths())
                                .map_err(|_| ())?,
                        ),
                        ClipboardEntry::String(_) => {}
                    }
                }
                Ok(images)
            },
            cx,
        );
    }

    pub(crate) fn drop_images(
        &mut self,
        paths: &ExternalPaths,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import_image_paths(paths.paths().to_vec(), cx);
    }

    fn import_image_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.import_images(
            move || vega_conversation::attachments::import_images(&paths).map_err(|_| ()),
            cx,
        );
    }

    pub(crate) fn pick_images(&mut self, cx: &mut Context<Self>) {
        if self.attachment_import_pending {
            return;
        }
        self.attachment_generation = self.attachment_generation.wrapping_add(1);
        let generation = self.attachment_generation;
        self.attachment_import_pending = true;
        cx.notify();
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择 PNG、JPEG 或 WebP 图片".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picked.await;
            let _ = this.update(cx, |this, cx| {
                if this.attachment_generation != generation {
                    return;
                }
                this.attachment_import_pending = false;
                match result {
                    Ok(Ok(Some(paths))) => this.import_image_paths(paths, cx),
                    Ok(Ok(None)) => {}
                    _ => {
                        this.attachment_error = Some(IMPORT_ERROR);
                        cx.notify();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn import_images(
        &mut self,
        load: impl FnOnce() -> Result<Vec<ImageAttachment>, ()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.attachment_import_pending {
            return;
        }
        self.attachment_generation = self.attachment_generation.wrapping_add(1);
        let generation = self.attachment_generation;
        self.attachment_import_pending = true;
        self.attachment_error = None;
        let existing: Vec<_> = self
            .attachments
            .iter()
            .map(|(_, image)| image.image.clone())
            .collect();
        let work = cx.background_executor().spawn(async move {
            let imported = load()?;
            if imported.is_empty() {
                return Err(());
            }
            let mut all = existing;
            all.extend(imported.iter().cloned());
            vega_conversation::attachments::validate_images(&all).map_err(|_| ())?;
            Ok(imported)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.attachment_generation != generation {
                    return;
                }
                this.attachment_import_pending = false;
                match result {
                    Ok(images) => {
                        for image in images {
                            this.attachment_generation = this.attachment_generation.wrapping_add(1);
                            let name =
                                image_name("图片", this.attachment_generation as usize, &image);
                            let preview = ImagePreview::new(image, name, cx);
                            this.observe_image(&preview.view, cx);
                            this.attachments.push((this.attachment_generation, preview));
                        }
                    }
                    Err(()) => this.attachment_error = Some(IMPORT_ERROR),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_image(&mut self, id: u64, cx: &mut Context<Self>) {
        // R4: retire pending work before editing the batch; ACK clears IDs only.
        self.attachment_generation = self.attachment_generation.wrapping_add(1);
        self.attachment_import_pending = false;
        self.attachments.retain(|(owned, _)| *owned != id);
        cx.notify();
    }

    pub(crate) fn cancel_image_import(&mut self, cx: &mut Context<Self>) {
        self.attachment_generation = self.attachment_generation.wrapping_add(1);
        self.attachment_import_pending = false;
        cx.notify();
    }

    pub(crate) fn render_attachments(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .debug_selector(|| "composer-attachments".into())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(self.attachments.iter().map(|(id, image)| {
                        let id = *id;
                        div().relative().child(thumbnail(image, cx)).child(
                            crate::icons::icon_button(
                                crate::icons::Icon::Close,
                                format!("移除图片 {id}"),
                                colors,
                                cx.listener(move |this, _, _, cx| this.remove_image(id, cx)),
                            )
                            .absolute()
                            .top_0()
                            .right_0()
                            .debug_selector(move || format!("remove-image-{id}"))
                            .bg(colors.bg_elevated)
                            .rounded_full(),
                        )
                    })),
            )
            .when(self.attachment_import_pending, |el| {
                el.child("正在添加图片…")
            })
            .when_some(self.attachment_error, |el, message| {
                el.child(render_rows::status_notice(
                    message.to_string(),
                    colors.danger,
                    &colors,
                ))
            })
            .into_any_element()
    }
}

fn thumbnail(image: &ImagePreview, cx: &App) -> AnyElement {
    div()
        .debug_selector(|| "attachment-thumbnail".into())
        .child(crate::image_preview::ImagePreview::thumbnail(
            &image.view,
            cx,
        ))
        .into_any_element()
}

pub(crate) fn render_user_images(images: &[ImagePreview], cx: &App) -> AnyElement {
    div()
        .debug_selector(|| "user-message-images".into())
        .w_full()
        .pt(px(Layout::USER_MESSAGE_PADDING_TOP))
        .pb(px(Layout::MESSAGE_PADDING_BOTTOM))
        .flex_shrink_0()
        .flex()
        .justify_end()
        .flex_wrap()
        .gap_2()
        .children(images.iter().map(|image| thumbnail(image, cx)))
        .into_any_element()
}
