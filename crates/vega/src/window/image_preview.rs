use super::*;
use vega_ui::image_preview::{ImagePreview, ImagePreviewAction};

mod native;

pub(super) struct ActiveImagePreview {
    pub(super) stream: Entity<ConversationStream>,
    pub(super) image: Entity<ImagePreview>,
    generation: u64,
    project: Option<String>,
}

pub(super) struct ImagePreviewController {
    pub(super) active: Option<ActiveImagePreview>,
    pub(super) previous_maximized: Option<[bool; 2]>,
    generation: u64,
    route: Option<(Option<String>, Option<gpui_kit::EntityId>, Option<String>)>,
    route_epoch: u64,
    sequence: u64,
    operation: Option<(
        Entity<ImagePreview>,
        tokio_util::sync::CancellationToken,
        u64,
    )>,
    applications: Vec<native::ImageApplication>,
    cache: native::ImageExportCache,
    system: Arc<dyn native::ImageSystem>,
    timeout: std::time::Duration,
}

impl Drop for ImagePreviewController {
    fn drop(&mut self) {
        self.retire();
    }
}

impl ImagePreviewController {
    pub(super) fn retire(&mut self) {
        if let Some((image, cancel, _)) = self.operation.take() {
            cancel.cancel();
            self.cache.release_unopened(image.entity_id());
        }
        if let Some(active) = self.active.take() {
            self.cache.release_unopened(active.image.entity_id());
        }
    }

    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            active: None,
            previous_maximized: None,
            generation: 0,
            route: None,
            route_epoch: 0,
            sequence: 0,
            operation: None,
            applications: Vec::new(),
            cache: native::ImageExportCache::for_app(cx),
            system: Arc::new(native::NativeImageSystem),
            timeout: std::time::Duration::from_secs(5),
        }
    }
}

impl VegaWindow {
    fn image_source_current(
        &self,
        stream: &Entity<ConversationStream>,
        image: &Entity<ImagePreview>,
        cx: &App,
    ) -> bool {
        self.stream_view.as_ref().is_some_and(|(id, current)| {
            current == stream
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| &thread.id == id)
        }) && stream.read(cx).owns_image_preview(image, cx)
    }

    fn sync_image_route(&mut self, cx: &App) {
        let route = (
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone()),
            self.stream_view
                .as_ref()
                .map(|(_, stream)| stream.entity_id()),
            cx.global::<SelectedProject>().0.clone(),
        );
        if self.image_preview.route.as_ref() != Some(&route) {
            self.image_preview.route = Some(route);
            self.image_preview.route_epoch = self.image_preview.route_epoch.wrapping_add(1);
        }
    }

    pub(super) fn sync_image_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_image_route(cx);
        if self.image_preview.active.as_ref().is_some_and(|active| {
            !self.image_source_current(&active.stream, &active.image, cx)
                || active.project != cx.global::<SelectedProject>().0
        }) {
            self.close_image_preview(window, cx);
        }
        self.restore_image_workspace_size(window, cx);
    }

    pub(super) fn close_image_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.image_preview.active.take();
        self.cancel_image_operation(cx);
        self.remove_image_workspace();
        if let Some(active) = active {
            active.image.update(cx, |view, cx| view.dismiss_menu(cx));
            self.image_preview
                .cache
                .release_unopened(active.image.entity_id());
            if active.project == cx.global::<SelectedProject>().0
                && self.image_source_current(&active.stream, &active.image, cx)
                && active.image.read(cx).thumbnail_visible()
            {
                window.focus(&active.image.read(cx).thumbnail_focus(), cx);
            } else {
                self.workspace_focus_composer(window, cx);
            }
        }
        cx.notify();
    }

    fn cancel_image_operation(&mut self, cx: &mut Context<Self>) {
        if let Some((image, cancel, _)) = self.image_preview.operation.take() {
            cancel.cancel();
            image.update(cx, |view, cx| view.finish(None, cx));
            self.image_preview.cache.release_unopened(image.entity_id());
        }
        self.image_preview.applications.clear();
    }

    pub(super) fn handle_image_preview(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ImagePreviewRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_image_preview(window, cx);
        if !self.image_source_current(&stream, &request.image, cx) {
            return;
        }
        let image = request.image.clone();
        match request.action {
            ImagePreviewAction::Open => {
                if self
                    .image_preview
                    .active
                    .as_ref()
                    .is_none_or(|active| active.image != image)
                {
                    self.cancel_image_operation(cx);
                    if let Some(previous) = self.image_preview.active.take() {
                        previous.image.update(cx, |view, cx| view.dismiss_menu(cx));
                        self.image_preview
                            .cache
                            .release_unopened(previous.image.entity_id());
                    }
                    self.image_preview.generation = self.image_preview.generation.wrapping_add(1);
                    self.image_preview.active = Some(ActiveImagePreview {
                        stream,
                        image,
                        generation: self.image_preview.generation,
                        project: cx.global::<SelectedProject>().0.clone(),
                    });
                }
                self.show_image_workspace(window, cx);
            }
            ImagePreviewAction::Close => {
                if self
                    .image_preview
                    .active
                    .as_ref()
                    .is_some_and(|active| active.image == image)
                {
                    self.close_image_preview(window, cx);
                }
            }
            ImagePreviewAction::Save => {
                let Some(generation) = self
                    .image_preview
                    .active
                    .as_ref()
                    .filter(|active| active.image == image)
                    .map(|active| active.generation)
                else {
                    return;
                };
                self.save_preview_image(stream, image, Some(generation), cx);
            }
            ImagePreviewAction::DismissApplications => {
                self.cancel_image_operation(cx);
                self.image_preview.cache.release_unopened(image.entity_id());
            }
            ImagePreviewAction::Applications => self.load_image_applications(stream, image, cx),
            ImagePreviewAction::ChooseApplication(index) => {
                self.open_preview_image(stream, image, index, cx)
            }
        }
    }

    fn image_request_current(
        &self,
        stream: &Entity<ConversationStream>,
        image: &Entity<ImagePreview>,
        pane: Option<u64>,
        cx: &App,
    ) -> bool {
        self.image_source_current(stream, image, cx)
            && pane.is_none_or(|generation| {
                self.image_preview.active.as_ref().is_some_and(|active| {
                    active.image == *image
                        && active.generation == generation
                        && active.project == cx.global::<SelectedProject>().0
                })
            })
    }

    fn save_preview_image(
        &mut self,
        stream: Entity<ConversationStream>,
        image: Entity<ImagePreview>,
        pane: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if !image.update(cx, |view, cx| view.begin(cx)) {
            return;
        }
        let directory = std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let picked = cx.prompt_for_new_path(&directory, Some(image.read(cx).name()));
        let bytes = image.read(cx).image().clone();
        let project = cx.global::<SelectedProject>().0.clone();
        let route_epoch = self.image_preview.route_epoch;
        cx.spawn(async move |this, cx| {
            let chosen = picked.await;
            let current = this
                .update(cx, |this, cx| {
                    this.sync_image_route(cx);
                    this.image_preview.route_epoch == route_epoch
                        && this.image_request_current(&stream, &image, pane, cx)
                        && project == cx.global::<SelectedProject>().0
                })
                .unwrap_or(false);
            if !current {
                image.update(cx, |view, cx| view.finish(None, cx));
                return;
            }
            let status = match chosen {
                Ok(Ok(Some(path))) => {
                    let success = cx
                        .background_executor()
                        .spawn(async move {
                            vega_conversation::attachments::save_screenshot(&bytes, &path).is_ok()
                        })
                        .await;
                    Some(if success {
                        "图片已保存"
                    } else {
                        "保存失败，请选择新的文件名重试"
                    })
                }
                Ok(Ok(None)) => None,
                _ => Some("保存失败，请重试"),
            };
            let _ = this.update(cx, |this, cx| {
                this.sync_image_route(cx);
                let current = this.image_preview.route_epoch == route_epoch
                    && this.image_request_current(&stream, &image, pane, cx)
                    && project == cx.global::<SelectedProject>().0;
                image.update(cx, |view, cx| {
                    view.finish(current.then_some(status).flatten(), cx)
                });
            });
        })
        .detach();
    }

    fn load_image_applications(
        &mut self,
        stream: Entity<ConversationStream>,
        image: Entity<ImagePreview>,
        cx: &mut Context<Self>,
    ) {
        let Some(generation) = self
            .image_preview
            .active
            .as_ref()
            .filter(|active| active.image == image)
            .map(|active| active.generation)
        else {
            return;
        };
        if !image.update(cx, |view, cx| view.begin(cx)) {
            return;
        }
        self.cancel_image_operation(cx);
        let cancel = tokio_util::sync::CancellationToken::new();
        self.image_preview.sequence = self.image_preview.sequence.wrapping_add(1);
        let sequence = self.image_preview.sequence;
        self.image_preview.operation = Some((image.clone(), cancel.clone(), sequence));
        let cache = self.image_preview.cache.clone();
        let system = self.image_preview.system.clone();
        let source = image.entity_id();
        let bytes = image.read(cx).image().clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let file = cache.materialize(source, &bytes, &cancel)?;
                    if cancel.is_cancelled() {
                        return Err(native::ImageOpenError::Cancelled);
                    }
                    system.applications(&file.path)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.image_request_current(&stream, &image, Some(generation), cx)
                    || !this
                        .image_preview
                        .operation
                        .as_ref()
                        .is_some_and(|(_, _, current)| *current == sequence)
                {
                    return;
                }
                this.image_preview.operation = None;
                match result {
                    Ok(applications) => {
                        let names = applications.iter().map(|app| app.label.clone()).collect();
                        this.image_preview.applications = applications;
                        image.update(cx, |view, cx| view.set_applications(names, cx));
                    }
                    Err(error) => {
                        image.update(cx, |view, cx| view.finish(Some(error.message()), cx))
                    }
                }
            });
        })
        .detach();
    }

    fn open_preview_image(
        &mut self,
        stream: Entity<ConversationStream>,
        image: Entity<ImagePreview>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(generation) = self
            .image_preview
            .active
            .as_ref()
            .filter(|active| active.image == image)
            .map(|active| active.generation)
        else {
            return;
        };
        let Some(application) = self.image_preview.applications.get(index).cloned() else {
            return;
        };
        if !image.update(cx, |view, cx| view.begin(cx)) {
            return;
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        self.image_preview.sequence = self.image_preview.sequence.wrapping_add(1);
        let sequence = self.image_preview.sequence;
        self.image_preview.operation = Some((image.clone(), cancel.clone(), sequence));
        let cache = self.image_preview.cache.clone();
        let system = self.image_preview.system.clone();
        let timeout = self.image_preview.timeout;
        let source = image.entity_id();
        let bytes = image.read(cx).image().clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let file = cache.materialize(source, &bytes, &cancel)?;
                    let result =
                        native::launch(system.as_ref(), &application, &file, &cancel, timeout);
                    if result.is_err() {
                        cache.release_failed(&file);
                    }
                    result
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.image_request_current(&stream, &image, Some(generation), cx)
                    || !this
                        .image_preview
                        .operation
                        .as_ref()
                        .is_some_and(|(_, _, current)| *current == sequence)
                {
                    return;
                }
                this.image_preview.operation = None;
                image.update(cx, |view, cx| {
                    view.finish(result.err().map(|error| error.message()), cx)
                });
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
