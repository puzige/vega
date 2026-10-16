use gpui_kit::prelude::*;
use gpui_kit::*;
use std::sync::{Arc, OnceLock};
use vega_conversation::types::ImageAttachment;
use vega_theme::{Layout, Typography, theme};

use crate::icons::{Icon, icon, icon_button};

#[derive(Clone, Copy)]
pub enum ImagePreviewAction {
    Open,
    Save,
    Applications,
    DismissApplications,
    ChooseApplication(usize),
    Close,
}

pub struct ImagePreview {
    image: ImageAttachment,
    pixels: Arc<OnceLock<Arc<Image>>>,
    name: String,
    focus: FocusHandle,
    thumbnail_focus: FocusHandle,
    save_focus: FocusHandle,
    open_focus: FocusHandle,
    busy: bool,
    thumbnail_visible: bool,
    status: Option<&'static str>,
    applications: Option<Vec<String>>,
    application_focuses: Vec<FocusHandle>,
    trigger_bounds: Option<Bounds<Pixels>>,
    dismissed_on_trigger: bool,
    focus_menu: bool,
}

impl EventEmitter<ImagePreviewAction> for ImagePreview {}

impl ImagePreview {
    pub fn new(image: ImageAttachment, name: String, cx: &mut Context<Self>) -> Self {
        let view = Self::pending(image, name, cx);
        let target = view.pixels.clone();
        let pending = view.image.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let format = match pending.mime_type() {
                        "image/jpeg" => ImageFormat::Jpeg,
                        "image/webp" => ImageFormat::Webp,
                        _ => ImageFormat::Png,
                    };
                    let _ = target.set(Arc::new(Image::from_bytes(
                        format,
                        pending.bytes().to_vec(),
                    )));
                })
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        view
    }

    pub(crate) fn pending(image: ImageAttachment, name: String, cx: &mut Context<Self>) -> Self {
        Self {
            image,
            pixels: Arc::new(OnceLock::new()),
            name,
            focus: cx.focus_handle(),
            thumbnail_focus: cx.focus_handle().tab_stop(true),
            save_focus: cx.focus_handle().tab_stop(true),
            open_focus: cx.focus_handle().tab_stop(true),
            busy: false,
            thumbnail_visible: false,
            status: None,
            applications: None,
            application_focuses: Vec::new(),
            trigger_bounds: None,
            dismissed_on_trigger: false,
            focus_menu: false,
        }
    }

    pub fn thumbnail_visible(&self) -> bool {
        self.thumbnail_visible
    }
    pub fn reset_thumbnail(&mut self) {
        self.thumbnail_visible = false;
    }
    pub fn thumbnail_tracker(view: &Entity<Self>) -> AnyElement {
        let view = view.clone();
        canvas(
            move |bounds, window, cx| {
                let visible = bounds.intersect(&window.content_mask().bounds);
                view.update(cx, |view, _| {
                    view.thumbnail_visible =
                        visible.size.width > px(0.) && visible.size.height > px(0.)
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
    pub fn image(&self) -> &ImageAttachment {
        &self.image
    }
    pub fn pixels(&self) -> Arc<OnceLock<Arc<Image>>> {
        self.pixels.clone()
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn thumbnail_focus(&self) -> FocusHandle {
        self.thumbnail_focus.clone()
    }
    pub fn status(&self) -> Option<&'static str> {
        self.status
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    pub fn begin(&mut self, cx: &mut Context<Self>) -> bool {
        if self.busy {
            return false;
        }
        self.busy = true;
        self.status = None;
        self.applications = None;
        cx.notify();
        true
    }
    pub fn finish(&mut self, status: Option<&'static str>, cx: &mut Context<Self>) {
        self.busy = false;
        self.status = status;
        cx.notify();
    }
    pub fn set_applications(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        self.busy = false;
        self.status = names
            .is_empty()
            .then_some("没有可用的应用，请保存到本地后打开");
        self.application_focuses = names
            .iter()
            .map(|_| cx.focus_handle().tab_stop(true))
            .collect();
        self.focus_menu = !names.is_empty();
        self.applications = (!names.is_empty()).then_some(names);
        cx.notify();
    }
    pub fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.applications = None;
        cx.notify();
    }
    pub fn action(&mut self, action: ImagePreviewAction, cx: &mut Context<Self>) {
        if matches!(action, ImagePreviewAction::Applications) {
            if self.dismissed_on_trigger {
                self.dismissed_on_trigger = false;
                return;
            }
            if self.applications.take().is_some() {
                cx.emit(ImagePreviewAction::DismissApplications);
                cx.notify();
                return;
            }
        }
        if self.busy && !matches!(action, ImagePreviewAction::Open | ImagePreviewAction::Close) {
            return;
        }
        if matches!(
            action,
            ImagePreviewAction::ChooseApplication(_) | ImagePreviewAction::Close
        ) {
            self.applications = None;
        }
        cx.emit(action);
    }

    pub fn thumbnail(view: &Entity<Self>, cx: &App) -> AnyElement {
        Self::thumbnail_sized(view, Layout::ATTACHMENT_THUMBNAIL, cx)
    }

    pub(crate) fn thumbnail_sized(view: &Entity<Self>, size: f32, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        let state = view.read(cx);
        let click = view.clone();
        let key = view.clone();
        div()
            .id(("image-thumbnail", view.entity_id()))
            .debug_selector(move || format!("image-thumbnail-{}", click.entity_id()))
            .track_focus(&state.thumbnail_focus)
            .tab_stop(true)
            .aria_label(format!("预览 {}", state.name))
            .relative()
            .size(px(size))
            .cursor_pointer()
            .rounded_md()
            .hover(move |s| s.border_1().border_color(colors.text_secondary))
            .focus_visible(move |s| s.border_1().border_color(colors.brand_primary))
            .on_click({
                let view = view.clone();
                move |_, _, cx| view.update(cx, |v, cx| v.action(ImagePreviewAction::Open, cx))
            })
            .on_key_down(move |event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    key.update(cx, |v, cx| v.action(ImagePreviewAction::Open, cx));
                    cx.stop_propagation();
                }
            })
            .when_some(state.pixels.get().cloned(), |el, pixels| {
                el.child(
                    img(pixels)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .rounded_md(),
                )
            })
            .child(Self::thumbnail_tracker(view))
            .into_any_element()
    }
}

impl Focusable for ImagePreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImagePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_menu)
            && self.open_focus.is_focused(window)
            && let Some(first) = self.application_focuses.first()
        {
            first.focus(window, cx);
        }
        let colors = theme(cx).colors;
        let name = self.name.clone();
        let save = icon_button(
            Icon::ArrowDown,
            "保存到本地",
            colors,
            cx.listener(|this, _, _, cx| this.action(ImagePreviewAction::Save, cx)),
        )
        .size(px(Layout::IMAGE_PREVIEW_CONTROL))
        .track_focus(&self.save_focus)
        .debug_selector(|| "image-preview-save".into())
        .focus_visible(move |s| s.border_1().border_color(colors.brand_primary))
        .when(self.busy, |el| el.opacity(0.5));
        let open = div()
            .id("image-open-with")
            .debug_selector(|| "image-preview-open-with".into())
            .track_focus(&self.open_focus)
            .tab_stop(true)
            .relative()
            .h(px(Layout::IMAGE_PREVIEW_CONTROL))
            .px_2()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .rounded_md()
            .text_size(px(Typography::METADATA))
            .cursor_pointer()
            .hover(move |s| s.bg(colors.bg_hover))
            .focus_visible(move |s| s.border_1().border_color(colors.brand_primary))
            .on_click(
                cx.listener(|this, _, _, cx| this.action(ImagePreviewAction::Applications, cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.action(ImagePreviewAction::Applications, cx);
                    cx.stop_propagation();
                }
            }))
            .child("打开方式")
            .child(icon(Icon::ChevronDown, colors.text_secondary))
            .child({
                let view = cx.entity().downgrade();
                canvas(
                    move |bounds, _, cx| {
                        let _ = view.update(cx, |view, _| view.trigger_bounds = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .when(self.busy, |el| el.opacity(0.5));
        let menu = self.applications.as_ref().map(|names| {
            div()
                .id("image-applications")
                .debug_selector(|| "image-preview-applications".into())
                .absolute()
                .top(px(Layout::IMAGE_PREVIEW_HEADER))
                .right(px(Layout::IMAGE_PREVIEW_HEADER_PADDING))
                .w(px(Layout::SIDEBAR_WIDTH))
                .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
                .overflow_y_scroll()
                .bg(colors.bg_elevated)
                .border_1()
                .border_color(colors.border_subtle)
                .rounded_md()
                .shadow_md()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.dismissed_on_trigger = this
                        .trigger_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position));
                    this.applications = None;
                    cx.emit(ImagePreviewAction::DismissApplications);
                    cx.notify();
                }))
                .children(names.iter().enumerate().map(|(index, name)| {
                    div()
                        .id(("image-app", index))
                        .debug_selector(move || format!("image-preview-app-{index}"))
                        .track_focus(&self.application_focuses[index])
                        .tab_stop(true)
                        .h_8()
                        .px_2()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(move |s| s.bg(colors.bg_hover))
                        .child(name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_focus.focus(window, cx);
                            this.action(ImagePreviewAction::ChooseApplication(index), cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_focus.focus(window, cx);
                                this.action(ImagePreviewAction::ChooseApplication(index), cx);
                                cx.stop_propagation();
                            }
                        }))
                }))
                .into_any_element()
        });
        div()
            .id("image-preview")
            .debug_selector(|| "image-preview".into())
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(colors.bg_subtle)
            .text_color(colors.text_primary)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.focus.contains_focused(window, cx) {
                    if this.applications.take().is_some() {
                        cx.emit(ImagePreviewAction::DismissApplications);
                        this.open_focus.focus(window, cx);
                        cx.notify();
                    } else {
                        this.action(ImagePreviewAction::Close, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .debug_selector(|| "image-preview-header".into())
                    .h(px(Layout::IMAGE_PREVIEW_HEADER))
                    .flex_shrink_0()
                    .px(px(Layout::IMAGE_PREVIEW_HEADER_PADDING))
                    .flex()
                    .items_center()
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .bg(colors.bg_base)
                    .child(
                        div()
                            .id("image-preview-name")
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(Typography::BODY))
                            .tooltip(move |_, cx| crate::icons::tooltip(name.clone(), cx))
                            .child(self.name.clone()),
                    )
                    .child(open)
                    .child(save),
            )
            .when_some(self.status, |el, status| {
                el.child(
                    div()
                        .debug_selector(|| "image-preview-status".into())
                        .px_3()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(status),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .p(px(Layout::IMAGE_PREVIEW_PADDING))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match self.pixels.get() {
                        Some(pixels) => img(pixels.clone())
                            .id("image-preview-pixels")
                            .debug_selector(|| "image-preview-pixels".into())
                            .size_full()
                            .max_w(px(self.image.width() as f32))
                            .max_h(px(self.image.height() as f32))
                            .object_fit(ObjectFit::Contain)
                            .with_loading(|| {
                                div()
                                    .debug_selector(|| "image-preview-loading".into())
                                    .child("加载中")
                                    .into_any_element()
                            })
                            .with_fallback(|| {
                                div()
                                    .debug_selector(|| "image-preview-error".into())
                                    .child("图片无法显示，请保存到本地后打开")
                                    .into_any_element()
                            })
                            .into_any_element(),
                        None => div()
                            .debug_selector(|| "image-preview-loading".into())
                            .child("加载中")
                            .into_any_element(),
                    }),
            )
            .children(menu.map(|menu| deferred(menu).with_priority(2)))
    }
}
