use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use gpui_kit::{prelude::*, *};
use vega_conversation::types::{PermissionResetError, StartupError};
use vega_theme::{Motion, theme};
use vega_ui::nsh::ProjectionId;

use crate::loading_logo as logo;
#[path = "startup_controller/model.rs"]
mod model;
#[path = "startup_controller/presentation.rs"]
mod presentation;
#[path = "startup_controller/view.rs"]
mod view;

use model::{Action, Authority, Snapshot};
use presentation::Presentation;

pub(crate) struct StartupView {
    running: bool,
    loading_epoch: Instant,
    focus: FocusHandle,
    error: Option<StartupError>,
    session: ProjectionId,
    projection: ProjectionId,
    presentation: Option<Presentation>,
    selected: Option<Action>,
    animation_pending: bool,
}

impl StartupView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        vega_ui::interaction::observe_system_motion(window, cx);
        cx.observe_window_activation(window, |_, _, cx| cx.notify())
            .detach();
        let focus = cx.focus_handle().tab_stop(true);
        cx.on_focus(&focus, window, |_, _, cx| cx.notify()).detach();
        cx.on_blur(&focus, window, |_, _, cx| cx.notify()).detach();
        Self {
            running: false,
            loading_epoch: Instant::now(),
            focus,
            error: None,
            session: ProjectionId::new(),
            projection: ProjectionId::new(),
            presentation: None,
            selected: None,
            animation_pending: false,
        }
    }

    pub(crate) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        self.error = None;
        self.loading_epoch = Instant::now();
        self.session = ProjectionId::new();
        self.projection = ProjectionId::new();
        self.selected = None;
        window.focus(&self.focus, cx);
        cx.notify();
        let (sender, receiver) = mpsc::channel();
        if std::thread::Builder::new()
            .name("vega-startup".into())
            .spawn(move || {
                let appearance = vega_store::config::config_path()
                    .ok()
                    .and_then(|path| vega_store::config::read_from(&path).ok());
                let result = match crate::updater::acknowledge_startup() {
                    Ok(()) => vega_conversation::startup::prepare_default_workspace(),
                    Err(error) => {
                        tracing::error!(%error, "update launch acknowledgment failed");
                        Err(StartupError::WorkerUnavailable)
                    }
                };
                let _ = sender.send((appearance, result));
            })
            .is_err()
        {
            self.fail(StartupError::WorkerUnavailable, window, cx);
            return;
        }
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let (appearance, result) = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (None, Err(StartupError::WorkerUnavailable))
                    }
                };
                let _ = handle.update(cx, |_, window, cx| {
                    if this.upgrade().is_none() {
                        return;
                    }
                    match result {
                        Ok(workspace) => crate::open_workspace(workspace, window, cx),
                        Err(error) => {
                            crate::apply_appearance(appearance.as_ref(), cx);
                            let _ = this.update(cx, |this, cx| this.fail(error, window, cx));
                        }
                    }
                });
                break;
            }
        })
        .detach();
    }

    fn fail(&mut self, error: StartupError, window: &mut Window, cx: &mut Context<Self>) {
        self.running = false;
        self.error = Some(error);
        self.projection = ProjectionId::new();
        self.selected = Some(Action::Retry);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn is_current_root(&self, window: &Window, cx: &Context<Self>) -> bool {
        window
            .root::<Self>()
            .flatten()
            .is_some_and(|root| root.entity_id() == cx.entity_id())
    }

    fn authority(&self) -> Option<Authority> {
        let page = self.presentation.as_ref()?;
        Some(Authority {
            session: self.session.clone(),
            mount: page.mount.clone(),
            projection: self.projection.clone(),
            running: self.running,
            error: self.error,
            recovery: page.recovery,
        })
    }

    fn snapshot(&self, window: &Window, cx: &Context<Self>) -> Option<Snapshot> {
        Some(Snapshot {
            frame: ProjectionId::new(),
            authority: self.authority()?,
            colors: theme(cx).colors,
            selected: self
                .focus
                .is_focused(window)
                .then_some(self.selected)
                .flatten(),
            phase: self.moving(window, cx).then(|| {
                (self.loading_epoch.elapsed().as_secs_f32() * 1000.
                    / Motion::THINKING_SHIMMER_MS as f32)
                    .fract()
            }),
            language: vega_i18n::language(),
            appearance: theme(cx).appearance,
        })
    }

    fn moving(&self, window: &Window, cx: &Context<Self>) -> bool {
        self.error.is_none()
            && self
                .presentation
                .as_ref()
                .is_some_and(|page| !page.recovery)
            && self.is_current_root(window, cx)
            && window.viewport_size().width > px(0.)
            && window.viewport_size().height > px(0.)
            && window.is_window_active()
            && !cx.reduce_motion()
    }
}

impl Render for StartupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_presentation(window, cx)
    }
}

fn error_key(error: StartupError) -> &'static str {
    match error {
        StartupError::PathsUnavailable => "app.startup.paths",
        StartupError::StorageUnavailable => "app.startup.storage",
        StartupError::ConfigurationUnavailable => "app.startup.configuration",
        StartupError::AutomationRecovery => "app.startup.automation",
        StartupError::WorkerUnavailable => "app.startup.worker",
        StartupError::PermissionReset(error) => match error {
            PermissionResetError::Unavailable => "app.startup.reset_unavailable",
            PermissionResetError::Busy => "app.startup.reset_busy",
            PermissionResetError::UnsettledWork => "app.startup.reset_unsettled",
            PermissionResetError::ConfigurationConflict => "app.startup.reset_conflict",
            PermissionResetError::ConfigurationUnavailable => "app.startup.reset_configuration",
            PermissionResetError::OutcomeUnknown => "app.startup.reset_unknown",
        },
    }
}
