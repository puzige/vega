use super::*;
use vega_ui::cua_onboarding::lifecycle::TaskContext;
use vega_ui::cua_onboarding::{self as guidance, GuidanceCard, PreparationRequested};
mod preparation_error;

use preparation_error::Failure;

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
    failed: Option<Failure>,
    error_presentation: std::cell::RefCell<Option<Entity<preparation_error::NativeHost>>>,
}

impl OnboardingHost {
    pub(super) fn close(&mut self, cx: &mut App) {
        self.hide_error(cx);
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
            Err(error) => self.onboarding.failed = Some(Failure::new(origin, error.to_string())),
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
            .is_some_and(|failure| !failure.origin.is_current(self, cx))
        {
            self.onboarding.failed = None;
        }
        if self
            .onboarding
            .failed
            .as_ref()
            .is_none_or(|failure| !preparation_error::route_allows(self, failure, cx))
        {
            self.hide_preparation_error(cx);
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
            self.onboarding.failed = Some(Failure::new(
                origin.clone(),
                vega_i18n::t("app.setup.pending").into(),
            ));
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
                self.onboarding.failed = Some(Failure::new(
                    origin.clone(),
                    vega_i18n::t("app.setup.task_unavailable").into(),
                ));
                cx.notify();
            }
        }
    }

    pub(super) fn render_preparation_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(failure) = self
            .onboarding
            .failed
            .as_ref()
            .filter(|failure| preparation_error::route_allows(self, failure, cx))
        else {
            self.onboarding.hide_error(cx);
            return None;
        };
        let owner = cx.weak_entity();
        let mut presentation = self.onboarding.error_presentation.borrow_mut();
        let host = presentation
            .get_or_insert_with(|| cx.new(|cx| preparation_error::NativeHost::new(owner, cx)));
        Some(
            preparation_error::NativeRenderOnce {
                host: host.clone(),
                failure: failure.clone(),
            }
            .into_any_element(),
        )
    }

    pub(super) fn hide_preparation_error(&self, cx: &mut Context<Self>) {
        self.onboarding.hide_error(cx);
    }

    fn retry_preparation(&mut self, observed: &Failure, cx: &mut Context<Self>) {
        if self.onboarding.failed.as_ref().is_some_and(|current| {
            current.matches(observed) && preparation_error::route_allows(self, current, cx)
        }) {
            self.open_preparation(observed.origin.clone(), cx);
        }
    }
}
