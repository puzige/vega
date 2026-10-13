use super::*;
use crate::interaction::InteractionExt;

pub(crate) struct ScreenshotCard {
    image: attachments::ImagePreview,
    saving: bool,
    expanded: bool,
    status: Option<&'static str>,
}

impl ScreenshotCard {
    pub(crate) fn new(image: ImageAttachment, cx: &mut Context<Self>) -> Self {
        let preview = attachments::ImagePreview {
            image,
            preview: Default::default(),
        };
        let pending = preview.clone();
        cx.spawn(async move |this, cx| {
            let ready = cx
                .background_executor()
                .spawn(async move { attachments::ImagePreview::new(pending.image) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.image = ready;
                cx.notify();
            });
        })
        .detach();
        Self {
            image: preview,
            saving: false,
            expanded: false,
            status: None,
        }
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
}

impl Render for ScreenshotCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let expanded = self.expanded;
        div()
            .debug_selector(|| "assistant-screenshot".into())
            .w_full()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.image.preview.get().cloned(), |container, image| {
                container.child(
                    gpui_kit::img(image)
                        .w_full()
                        .max_h(px(if expanded {
                            Layout::CONVERSATION_MAX_WIDTH
                        } else {
                            Layout::DISCLOSURE_CONTENT_MAX_HEIGHT
                        }))
                        .object_fit(gpui_kit::ObjectFit::Contain),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("screenshot-expand")
                            .debug_selector(|| "screenshot-expand".into())
                            .tab_stop(true)
                            .focusable()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .interaction_surface(
                                colors.bg_base,
                                colors.bg_hover,
                                colors.bg_active,
                                true,
                            )
                            .focus_visible(move |style| {
                                style.border_1().border_color(colors.brand_primary)
                            })
                            .on_key_down(cx.listener(
                                |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.expanded = !this.expanded;
                                        cx.notify();
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            }))
                            .child(if expanded {
                                "收起图片"
                            } else {
                                "查看大图"
                            }),
                    )
                    .child(
                        div()
                            .id("screenshot-save")
                            .debug_selector(|| "screenshot-save".into())
                            .tab_stop(true)
                            .focusable()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .interaction_surface(
                                colors.bg_base,
                                colors.bg_hover,
                                colors.bg_active,
                                true,
                            )
                            .focus_visible(move |style| {
                                style.border_1().border_color(colors.brand_primary)
                            })
                            .on_key_down(cx.listener(
                                |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.save(cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
                            .child(if self.saving {
                                "保存中…"
                            } else {
                                "保存图片"
                            }),
                    ),
            )
            .when_some(self.status, |container, status| {
                container.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(status),
                )
            })
    }
}

impl ConversationStream {
    pub(crate) fn screenshot_card(
        &mut self,
        image: ImageAttachment,
        cx: &mut Context<Self>,
    ) -> Entity<ScreenshotCard> {
        let card = cx.new(|cx| ScreenshotCard::new(image, cx));
        cx.observe(&card, |this, card, cx| {
            let index = this.entries.iter().position(
                |entry| matches!(entry, StreamEntry::Screenshot { card: owned } if owned == &card),
            );
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        card
    }
}
