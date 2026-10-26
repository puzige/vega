use super::*;
use vega_ui::cua_onboarding::lifecycle::TaskContext;
use vega_ui::cua_onboarding::{self as guidance, GuidanceCard, PreparationRequested};
use vega_ui::interaction::InteractionExt;

#[derive(Clone)]
enum Origin {
    Settings,
    Task {
        stream: WeakEntity<ConversationStream>,
        thread_id: String,
        project: Option<String>,
        context: TaskContext,
    },
}

impl Origin {
    fn is_current(&self, owner: &VegaWindow, cx: &App) -> bool {
        match self {
            Self::Settings => true,
            Self::Task {
                stream,
                thread_id,
                context,
                ..
            } => {
                let Some(stream) = stream.upgrade() else {
                    return false;
                };
                let request = PreparationRequested {
                    thread_id: thread_id.clone(),
                    context: context.clone(),
                };
                if !request.is_current(stream.read(cx)) {
                    return false;
                }
                if let Some(active) = owner.agent_controller.active_stream_for_thread(thread_id) {
                    return active == stream;
                }
                if let Some((_, visible)) =
                    owner.stream_view.as_ref().filter(|(id, _)| id == thread_id)
                {
                    return visible == &stream;
                }
                owner
                    .automation_controller
                    .streams
                    .get(thread_id)
                    .is_some_and(|current| current == &stream)
            }
        }
    }
}

struct ActiveCard {
    window: WindowHandle<GuidanceCard>,
    lifetime: Arc<()>,
    origin: Origin,
    _stream_changed: Option<Subscription>,
    _stream_released: Option<Subscription>,
}

#[derive(Default)]
pub(super) struct OnboardingHost {
    active: Option<ActiveCard>,
    failed: Option<(Origin, String)>,
}

impl OnboardingHost {
    pub(super) fn close(&mut self, cx: &mut App) {
        if let Some(active) = self.active.take() {
            let _ = active
                .window
                .update(cx, |card, window, cx| card.close(window, cx));
        }
    }
}

impl VegaWindow {
    pub(super) fn request_settings_preparation(
        &mut self,
        pane: Option<guidance::PermissionPane>,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = pane {
            cx.open_url(pane.url());
        }
        self.sync_preparation(cx);
        if let Some(active) = &self.onboarding.active
            && active
                .window
                .update(cx, |card, window, cx| {
                    let open = card.is_open();
                    if open {
                        if let Some(pane) = pane {
                            card.seek_settings(pane, window, cx);
                        } else {
                            card.show_overview(window, cx);
                        }
                    }
                    open
                })
                .unwrap_or(false)
        {
            return;
        }
        self.open_preparation(Origin::Settings, cx);
        if let Some(pane) = pane
            && let Some(active) = &self.onboarding.active
        {
            let _ = active
                .window
                .update(cx, |card, window, cx| card.seek_settings(pane, window, cx));
        }
    }

    pub(super) fn request_task_preparation(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PreparationRequested,
        cx: &mut Context<Self>,
    ) {
        let current = stream.read(cx);
        if !request.is_current(current) {
            return;
        }
        let project =
            (!current.route_project_id().is_empty()).then(|| current.route_project_id().to_owned());
        self.open_preparation(
            Origin::Task {
                stream: stream.downgrade(),
                thread_id: request.thread_id.clone(),
                project,
                context: request.context.clone(),
            },
            cx,
        );
    }

    fn open_preparation(&mut self, origin: Origin, cx: &mut Context<Self>) {
        if !origin.is_current(self, cx) {
            return;
        }
        self.onboarding.close(cx);
        self.onboarding.failed = None;
        let lifetime = Arc::new(());
        let return_lifetime = lifetime.clone();
        let return_origin = origin.clone();
        let weak = cx.weak_entity();
        let task = match &origin {
            Origin::Settings => "",
            Origin::Task { thread_id, .. } => thread_id.as_str(),
        };
        let result = guidance::open(
            task,
            Box::new(move |task, cx| {
                let weak = weak.clone();
                let lifetime = return_lifetime.clone();
                let origin = return_origin.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |owner, cx| {
                        owner.return_from_preparation(&lifetime, &origin, &task, cx);
                    });
                });
            }),
            cx,
        );
        match result {
            Ok(window) => {
                let (changed, released) = match &origin {
                    Origin::Settings => (None, None),
                    Origin::Task { stream, .. } => match stream.upgrade() {
                        Some(stream) => {
                            let changed_lifetime = lifetime.clone();
                            let changed = cx.observe(&stream, move |owner, _, cx| {
                                owner.sync_preparation_lifetime(&changed_lifetime, cx);
                            });
                            let released_lifetime = lifetime.clone();
                            let released = cx.observe_release(&stream, move |owner, _, cx| {
                                if owner.owns_preparation(&released_lifetime) {
                                    owner.onboarding.close(cx);
                                    cx.notify();
                                }
                            });
                            (Some(changed), Some(released))
                        }
                        None => (None, None),
                    },
                };
                self.onboarding.active = Some(ActiveCard {
                    window,
                    lifetime,
                    origin,
                    _stream_changed: changed,
                    _stream_released: released,
                });
                self.sync_preparation(cx);
            }
            Err(error) => self.onboarding.failed = Some((origin, error.to_string())),
        }
        cx.notify();
    }

    fn owns_preparation(&self, lifetime: &Arc<()>) -> bool {
        self.onboarding
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(&active.lifetime, lifetime))
    }

    fn sync_preparation_lifetime(&mut self, lifetime: &Arc<()>, cx: &mut Context<Self>) {
        if self.owns_preparation(lifetime) {
            self.sync_preparation(cx);
        }
    }

    pub(super) fn sync_preparation(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .onboarding
            .active
            .as_ref()
            .is_some_and(|active| !active.origin.is_current(self, cx));
        if stale {
            self.onboarding.close(cx);
        }
        if self
            .onboarding
            .failed
            .as_ref()
            .is_some_and(|(origin, _)| !origin.is_current(self, cx))
        {
            self.onboarding.failed = None;
        }
    }

    fn return_from_preparation(
        &mut self,
        lifetime: &Arc<()>,
        origin: &Origin,
        task: &str,
        cx: &mut Context<Self>,
    ) {
        let Origin::Task {
            thread_id, project, ..
        } = origin
        else {
            return;
        };
        if !self.owns_preparation(lifetime) || task != thread_id || !origin.is_current(self, cx) {
            return;
        }
        self.onboarding.active = None;
        if cx
            .try_global::<vega_ui::navigation::TaskMutationState>()
            .is_some_and(|state| state.pending > 0)
        {
            self.onboarding.failed =
                Some((origin.clone(), vega_i18n::t("app.setup.pending").into()));
            cx.notify();
            return;
        }
        let result = self
            .file_backed_store_path(cx)
            .ok_or(NavigationError::Unavailable)
            .and_then(|database| {
                vega_conversation::navigation::NavigationService::new(database).resolve(
                    &NavigationRoute::Task {
                        project: project.clone(),
                        task: thread_id.clone(),
                    },
                )
            });
        match result {
            Ok(Some(thread)) if thread.id == *thread_id => {
                self.accept_palette_thread(thread, cx);
            }
            _ => {
                self.onboarding.failed = Some((
                    origin.clone(),
                    vega_i18n::t("app.setup.task_unavailable").into(),
                ));
                cx.notify();
            }
        }
    }

    pub(super) fn render_preparation_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (origin, error) = self.onboarding.failed.clone()?;
        let error = crate::app_localization::translated(&error)
            .map(str::to_owned)
            .unwrap_or(error);
        let colors = theme(cx).colors;
        let retry = div()
            .id("preparation-open-retry")
            .role(Role::Button)
            .aria_label(vega_i18n::t("app.setup.retry_label"))
            .focusable()
            .tab_stop(true)
            .px_2()
            .py_1()
            .rounded(px(Layout::PANEL_RADIUS))
            .cursor_pointer()
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(move |owner, _, _, cx| owner.retry_preparation(cx)))
            .child(vega_i18n::t("app.setup.retry"));
        let dismiss = div()
            .id("preparation-open-dismiss")
            .role(Role::Button)
            .aria_label(vega_i18n::t("app.setup.dismiss_label"))
            .focusable()
            .tab_stop(true)
            .px_2()
            .py_1()
            .rounded(px(Layout::PANEL_RADIUS))
            .cursor_pointer()
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(|owner, _, _, cx| {
                owner.onboarding.failed = None;
                cx.notify();
            }))
            .child(vega_i18n::t("app.setup.dismiss"));
        if !origin.is_current(self, cx) {
            return None;
        }
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(Layout::IMAGE_PREVIEW_GAP))
                .px_3()
                .py_2()
                .text_size(px(Typography::METADATA))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(colors.danger)
                        .child(error),
                )
                .child(retry)
                .child(dismiss)
                .into_any_element(),
        )
    }

    fn retry_preparation(&mut self, cx: &mut Context<Self>) {
        if let Some((origin, _)) = self.onboarding.failed.take() {
            self.open_preparation(origin, cx);
        }
    }
}
