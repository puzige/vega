//! Issue 63 R1–R4: explicit, bounded attachment intents owned by this stream.
use super::*;
use gpui_kit::{
    ClipboardEntry, ClipboardItem, ExternalPaths, Image, ImageFormat, PathPromptOptions, img,
};
use std::path::PathBuf;
use vega_conversation::attachments::{MAX_IMAGE_BYTES, MAX_IMAGES, MAX_TURN_IMAGE_BYTES};

#[derive(Clone)]
pub(crate) struct ImagePreview {
    pub image: ImageAttachment,
    preview: Arc<std::sync::OnceLock<Arc<Image>>>,
}

impl ImagePreview {
    pub(crate) fn new(image: ImageAttachment) -> Self {
        let format = match image.mime_type() {
            "image/jpeg" => ImageFormat::Jpeg,
            "image/webp" => ImageFormat::Webp,
            _ => ImageFormat::Png,
        };
        let preview = Arc::new(std::sync::OnceLock::from(Arc::new(Image::from_bytes(
            format,
            image.bytes().to_vec(),
        ))));
        Self { image, preview }
    }
}

const IMPORT_ERROR: &str =
    "图片添加失败：请选择有效的 PNG、JPEG 或 WebP；最多 4 张，单张 8 MiB，总计 16 MiB。";

impl ConversationStream {
    pub(crate) fn history_image_previews(
        &mut self,
        images: Vec<ImageAttachment>,
        cx: &mut Context<Self>,
    ) -> Vec<ImagePreview> {
        let previews: Vec<_> = images
            .into_iter()
            .map(|image| ImagePreview {
                image,
                preview: Arc::new(std::sync::OnceLock::new()),
            })
            .collect();
        let pending = previews.clone();
        let work = cx.background_executor().spawn(async move {
            for target in pending {
                let ready = ImagePreview::new(target.image);
                if let Some(image) = ready.preview.get() {
                    let _ = target.preview.set(image.clone());
                }
            }
        });
        cx.spawn(async move |this, cx| {
            work.await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        previews
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
            Ok(imported
                .into_iter()
                .map(ImagePreview::new)
                .collect::<Vec<_>>())
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
                            this.attachments.push((this.attachment_generation, image));
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
                        div().relative().child(thumbnail(image)).child(
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
                el.child(div().text_color(colors.danger).child(message))
            })
            .into_any_element()
    }
}

fn thumbnail(image: &ImagePreview) -> AnyElement {
    div()
        .w(px(Layout::ATTACHMENT_THUMBNAIL))
        .h(px(Layout::ATTACHMENT_THUMBNAIL))
        .when_some(image.preview.get().cloned(), |el, preview| {
            el.child(img(preview).size_full().rounded_md())
        })
        .into_any_element()
}

pub(crate) fn render_user_images(images: &[ImagePreview]) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .children(images.iter().map(thumbnail))
        .into_any_element()
}
