mod nsh_preview;

use gpui_kit::prelude::*;
use gpui_kit::*;
use std::sync::{Arc, OnceLock};
use vega_conversation::types::ImageAttachment;
use vega_theme::Layout;

use crate::nsh::ProjectionId;

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
    source: Arc<OnceLock<nsh_core::element::SharedString>>,
    presentation: Option<nsh_preview::Presentation>,
    thumbnail_presentation: Option<nsh_preview::Presentation>,
    thumbnail_screenshot: bool,
    revision: u64,
    menu_revision: u64,
    application_id: Option<ProjectionId>,
    applications_requested: bool,
    busy: bool,
    thumbnail_visible: bool,
    status: Option<ImagePreviewStatus>,
    applications: Option<Vec<String>>,
}

#[derive(Clone, Copy)]
enum ImagePreviewStatus {
    Text(&'static str),
    Key(&'static str),
}

impl ImagePreviewStatus {
    fn render(self) -> &'static str {
        match self {
            Self::Text(text) => text,
            Self::Key(key) => vega_i18n::t(key),
        }
    }
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
        let source = Arc::new(OnceLock::new());
        let target = source.clone();
        let attachment = image.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let _ = target.set(nsh_preview::encoded_source(&attachment));
                })
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        Self {
            image,
            pixels: Arc::new(OnceLock::new()),
            name,
            focus: cx.focus_handle(),
            thumbnail_focus: cx.focus_handle().tab_stop(true),
            source,
            presentation: None,
            thumbnail_presentation: None,
            thumbnail_screenshot: false,
            revision: 0,
            menu_revision: 0,
            application_id: None,
            applications_requested: false,
            busy: false,
            thumbnail_visible: false,
            status: None,
            applications: None,
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
        .top_0()
        .left_0()
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
        self.status.map(ImagePreviewStatus::render)
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
        self.application_id = None;
        self.menu_revision = self.menu_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
        true
    }
    pub fn finish(&mut self, status: Option<&'static str>, cx: &mut Context<Self>) {
        self.busy = false;
        self.status = status.map(ImagePreviewStatus::Text);
        self.applications_requested = false;
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
    }
    pub fn finish_localized(&mut self, status: Option<&'static str>, cx: &mut Context<Self>) {
        self.busy = false;
        self.status = status.map(ImagePreviewStatus::Key);
        self.applications_requested = false;
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
    }
    pub fn set_applications(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        self.busy = false;
        self.status = names.is_empty().then_some(ImagePreviewStatus::Key(
            "ui.no_apps_are_available_save_locally_then_open",
        ));
        self.menu_revision = self.menu_revision.wrapping_add(1);
        self.application_id = (!names.is_empty()).then(ProjectionId::new);
        self.applications = (!names.is_empty()).then_some(names);
        self.applications_requested = false;
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
    }
    pub fn refresh_application_labels(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        let Some(applications) = self.applications.as_mut() else {
            return;
        };
        if applications.len() != names.len() || *applications == names {
            return;
        }
        *applications = names;
        cx.notify();
    }
    pub fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_revision = self.menu_revision.wrapping_add(1);
        self.applications = None;
        self.application_id = None;
        self.applications_requested = false;
        self.revision = self.revision.wrapping_add(1);
        cx.notify();
    }
    pub fn action(&mut self, action: ImagePreviewAction, cx: &mut Context<Self>) {
        if matches!(action, ImagePreviewAction::Applications)
            && (self.applications.is_some() || self.applications_requested)
        {
            self.dismiss_menu(cx);
            cx.emit(ImagePreviewAction::DismissApplications);
            return;
        }
        if self.busy
            && !matches!(
                action,
                ImagePreviewAction::Open
                    | ImagePreviewAction::Close
                    | ImagePreviewAction::DismissApplications
            )
        {
            return;
        }
        if let ImagePreviewAction::ChooseApplication(index) = action
            && self
                .applications
                .as_ref()
                .is_none_or(|names| index >= names.len())
        {
            return;
        }
        if matches!(action, ImagePreviewAction::Applications) {
            self.applications_requested = true;
        }
        if matches!(
            action,
            ImagePreviewAction::ChooseApplication(_)
                | ImagePreviewAction::DismissApplications
                | ImagePreviewAction::Close
        ) {
            self.dismiss_menu(cx);
        }
        self.revision = self.revision.wrapping_add(1);
        cx.emit(action);
        cx.notify();
    }

    pub fn thumbnail(view: &Entity<Self>, cx: &App) -> AnyElement {
        Self::thumbnail_sized(view, Layout::ATTACHMENT_THUMBNAIL, cx)
    }

    pub(crate) fn thumbnail_sized(view: &Entity<Self>, size: f32, _: &App) -> AnyElement {
        nsh_preview::ThumbnailHost {
            view: view.clone(),
            size: (size, size),
            screenshot: false,
        }
        .into_any_element()
    }

    pub(crate) fn screenshot_thumbnail(view: &Entity<Self>, size: (f32, f32)) -> AnyElement {
        nsh_preview::ThumbnailHost {
            view: view.clone(),
            size,
            screenshot: true,
        }
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
        self.render_nsh(false, window, cx)
    }
}
