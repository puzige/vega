use super::*;
use std::time::Duration;

impl VegaWindow {
    pub(crate) fn start_updater(&mut self, cx: &mut Context<Self>) {
        let receiver = self.updater.start();
        cx.spawn(async move |owner, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(crate::updater::Event::State(state)) => {
                        if owner
                            .update(cx, |owner, cx| {
                                if state.phase == UpdatePhase::Ready
                                    && owner.updater.state.phase != UpdatePhase::Ready
                                {
                                    owner.update_notice_dismissed = false;
                                }
                                owner.updater.state = state;
                                owner.publish_updater(cx);
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(crate::updater::Event::Quit) => {
                        let _ = owner.update(cx, |_, cx| cx.quit());
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        if owner.upgrade().is_none() {
                            break;
                        }
                    }
                }
            }
        })
        .detach();
    }

    pub(super) fn localized_update_projection(&self) -> UpdateProjection {
        let mut projection = self.updater.state.clone();
        if let Some(message) = crate::app_localization::translated(&projection.message) {
            projection.message = message.into();
        }
        if let Some(version) = crate::app_localization::translated(&projection.current_version) {
            projection.current_version = version.into();
        }
        projection
    }

    pub(crate) fn publish_updater(&self, cx: &mut Context<Self>) {
        if let Some(view) = &self.settings_view {
            let projection = self.localized_update_projection();
            view.update(cx, |view, cx| view.apply_update_projection(projection, cx));
        }
        cx.notify();
    }

    pub(crate) fn request_update(&mut self, request: UpdateRequest, cx: &mut Context<Self>) {
        match request {
            UpdateRequest::ReleasePage => {
                cx.open_url(crate::updater::RELEASE_PAGE);
                return;
            }
            UpdateRequest::Later => self.update_notice_dismissed = true,
            UpdateRequest::Install => {
                if self.updater.state.phase != UpdatePhase::Ready || crate::updater::installing() {
                    return;
                }
                let artifact_busy = self
                    .artifact_controller
                    .active
                    .iter()
                    .chain(self.artifact_controller.retained.values())
                    .any(|route| {
                        route.terminal_in_flight.is_some() || !route.terminal_queue.is_empty()
                    });
                if !self.agent_controller.active.is_empty()
                    || self.agent_controller.preparation_stream.is_some()
                    || self.trusted_actions.is_busy()
                    || self.reasoning_save_pending.is_some()
                    || self.workspace_has_terminals()
                    || artifact_busy
                {
                    self.updater.state.message = vega_i18n::t("app.update.tasks_running").into();
                    self.publish_updater(cx);
                    return;
                }
                crate::updater::set_installing(true);
            }
            _ => {}
        }
        if !self.updater.request(request) && matches!(request, UpdateRequest::Install) {
            crate::updater::set_installing(false);
            self.updater.state.message = vega_i18n::t("app.update.busy").into();
        }
        self.publish_updater(cx);
    }
}
