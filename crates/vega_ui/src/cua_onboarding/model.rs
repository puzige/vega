use super::CallbackIdentity;
use super::controller::{
    CardAction, CardController, CardEffect, CardView, CheckState, CheckTicket,
};
use super::session::{self, CleanupStatus, InspectionIntent, ProbeClient, ServiceStatus};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Global};
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::ComputerUseError;

#[derive(Clone)]
struct SharedPreparation(Entity<PreparationModel>);

impl Global for SharedPreparation {}

pub(crate) struct PreparationSnapshot {
    pub view: CardView,
    pub retained: bool,
    pub feedback: Vec<PreparationFeedback>,
    pub guide_warning: Option<&'static str>,
}

pub(crate) struct PreparationFeedback {
    pub message: String,
    pub warning: bool,
}

pub(crate) struct PreparationModel {
    controller: CardController,
    client: Option<ProbeClient>,
    pending: Option<CheckTicket>,
    service_status: ServiceStatus,
    adapter_error: Option<session::AdapterError>,
    owners: Vec<CallbackIdentity>,
    poll: Option<CancellationToken>,
    initialized: bool,
    retained: bool,
}

pub(crate) fn shared(cx: &mut App) -> Entity<PreparationModel> {
    if let Some(shared) = cx.try_global::<SharedPreparation>() {
        return shared.0.clone();
    }
    session::install_quit_hook(cx);
    let model = cx.new(PreparationModel::new);
    cx.set_global(SharedPreparation(model.clone()));
    model
}

impl PreparationModel {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|model, _| model.stop()).detach();
        Self {
            controller: CardController::new("", Path::new("")),
            client: None,
            pending: None,
            service_status: session::service_status(),
            adapter_error: None,
            owners: Vec::new(),
            poll: None,
            initialized: false,
            retained: false,
        }
    }

    pub(crate) fn acquire(
        &mut self,
        automatic_check: bool,
        cx: &mut Context<Self>,
    ) -> CallbackIdentity {
        let owner = CallbackIdentity::default();
        self.owners.push(owner.clone());
        self.service_status = session::service_status();
        if self.poll.is_none() {
            let cancel = CancellationToken::new();
            self.poll = Some(cancel.clone());
            cx.spawn(async move |weak, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    if cancel.is_cancelled() {
                        break;
                    }
                    if weak
                        .update(cx, |model, cx| {
                            if !cancel.is_cancelled() {
                                model.tick(cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        if automatic_check && !self.initialized {
            self.initialized = true;
            if let Some(error) = self.service_status.last_inspection_error {
                for effect in self.controller.act(CardAction::Recheck) {
                    if let CardEffect::Inspect { ticket, .. } = effect {
                        self.controller.complete(ticket, Err(error));
                    }
                }
                self.retained = true;
            } else if self.service_status.can_start_automatically() {
                self.inspect(InspectionIntent::Automatic, cx);
            }
        }
        cx.notify();
        owner
    }

    pub(crate) fn release(
        &mut self,
        owner: &CallbackIdentity,
        close_guide: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.owners.iter().position(|current| current == owner) else {
            return;
        };
        self.owners.remove(index);
        if close_guide || self.owners.is_empty() {
            self.cancel_inspection();
        }
        self.service_status = session::service_status();
        self.stop_if_unowned();
        cx.notify();
    }

    pub(crate) fn recheck(&mut self, cx: &mut Context<Self>) {
        if !self.owners.is_empty() {
            self.initialized = true;
            self.inspect(InspectionIntent::Manual, cx);
        }
    }

    fn inspect(&mut self, intent: InspectionIntent, cx: &mut Context<Self>) {
        for effect in self.controller.act(CardAction::Recheck) {
            if let CardEffect::Inspect {
                ticket,
                prompt,
                probe_direct_capture,
            } = effect
            {
                self.retained = true;
                let result = if self.client.is_none() {
                    ProbeClient::open().map(|client| self.client = Some(client))
                } else {
                    Ok(())
                }
                .and_then(|()| {
                    self.client
                        .as_ref()
                        .ok_or(session::AdapterError::Unavailable)?
                        .inspect(ticket.clone(), prompt, probe_direct_capture, intent)
                });
                match result {
                    Ok(()) => self.pending = Some(ticket),
                    Err(error) => {
                        self.adapter_error = Some(error);
                        self.controller
                            .complete(ticket, Err(ComputerUseError::Unavailable));
                        self.pending = None;
                        self.client = None;
                    }
                }
            }
        }
        self.service_status = session::service_status();
        cx.notify();
    }

    fn cancel_inspection(&mut self) {
        self.controller.cancel_inspection();
        self.pending = None;
        if let Some(client) = self.client.take() {
            client.cancel();
        }
        self.retained = true;
    }

    fn stop(&mut self) {
        self.cancel_inspection();
        if let Some(cancel) = self.poll.take() {
            cancel.cancel();
        }
    }

    fn stop_if_unowned(&mut self) {
        if self.owners.is_empty() && self.service_status.cleanup != CleanupStatus::Reclaiming {
            self.stop();
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        for _ in 0..16 {
            let Some(client) = self.client.as_ref() else {
                break;
            };
            match client.try_recv() {
                Ok(Some(response)) => {
                    if self.pending.as_ref() != Some(&response.ticket) {
                        continue;
                    }
                    self.pending = None;
                    if response.result.is_ok() {
                        self.adapter_error = None;
                    }
                    self.controller.complete(response.ticket, response.result);
                    self.retained = false;
                    cx.notify();
                }
                Ok(None) => break,
                Err(error) => {
                    if let Some(ticket) = self.pending.take() {
                        self.controller
                            .complete(ticket, Err(ComputerUseError::Unavailable));
                    }
                    self.adapter_error = Some(error);
                    self.client = None;
                    self.retained = true;
                    cx.notify();
                    break;
                }
            }
        }
        let status = session::service_status();
        if status != self.service_status {
            self.service_status = status;
            cx.notify();
        }
        self.stop_if_unowned();
    }

    pub(crate) fn snapshot(&self) -> PreparationSnapshot {
        let view = self.controller.view();
        let guide_warning = if matches!(self.service_status.cleanup, CleanupStatus::Failed(_)) {
            Some(vega_i18n::t(
                "ui.cleanup_failed_retry_in_permissions_and_checks",
            ))
        } else if self.service_status.failure.is_some() || self.adapter_error.is_some() {
            Some(vega_i18n::t(
                "ui.checks_are_unavailable_open_permissions_and_checks",
            ))
        } else if self.service_status.last_close_error.is_some()
            && self.service_status.cleanup != CleanupStatus::Clear
        {
            Some(vega_i18n::t(
                "ui.cleanup_is_unconfirmed_open_permissions_and_checks",
            ))
        } else if matches!(view.check_state, CheckState::Busy | CheckState::Failed(_))
            || self.service_status.last_inspection_error.is_some()
        {
            Some(vega_i18n::t(
                "ui.checks_are_incomplete_open_permissions_and_checks",
            ))
        } else {
            None
        };
        let mut feedback = Vec::new();
        let mut push = |message: String, warning| {
            feedback.push(PreparationFeedback { message, warning });
        };
        match view.check_state {
            CheckState::Idle => push(
                vega_i18n::t("ui.permissions_have_not_been_checked_check_again").into(),
                false,
            ),
            CheckState::Checking => push(
                if view.component.is_some() {
                    vega_i18n::t("ui.checking_permissions_previous_results_remain_below")
                } else {
                    vega_i18n::t("ui.checking_permissions")
                }
                .into(),
                false,
            ),
            CheckState::Observed => {}
            CheckState::Busy => push(
                vega_i18n::t("ui.another_task_is_using_computer_control_stop_that").into(),
                true,
            ),
            CheckState::Unknown => push(
                vega_i18n::t("ui.some_permission_states_are_unknown_check_system_settings").into(),
                true,
            ),
            CheckState::Failed(error) => push(
                vega_i18n::t_args(
                    "ui.error_resolve_the_issue_then_check_again",
                    &[("error", &error.user_message())],
                ),
                true,
            ),
        }
        if self.retained && view.check_state != CheckState::Checking {
            push(
                vega_i18n::t("ui.the_status_below_is_retained_check_again_to").into(),
                false,
            );
        }
        if let Some(error) = &self.adapter_error {
            push(error.to_string(), true);
        }
        if let Some(error) = &self.service_status.failure {
            push(error.to_string(), true);
        }
        if let Some(error) = self.service_status.last_inspection_error
            && view.check_state != CheckState::Failed(error)
            && !(view.check_state == CheckState::Busy && error == ComputerUseError::Busy)
        {
            push(
                vega_i18n::t_args(
                    "ui.last_check_failed_error",
                    &[("error", &error.user_message())],
                ),
                true,
            );
        }
        if let Some(error) = self.service_status.last_close_error {
            let recovery = if self.service_status.cleanup == CleanupStatus::Clear {
                vega_i18n::t("ui.cleanup_has_since_completed_you_can_check_again")
            } else {
                vega_i18n::t("ui.cleanup_is_still_unconfirmed_check_again_to_retry")
            };
            push(
                vega_i18n::t_args(
                    "ui.error_while_closing_the_check_error_recovery",
                    &[("error", &error.user_message()), ("recovery", recovery)],
                ),
                true,
            );
        }
        match self.service_status.cleanup {
            CleanupStatus::Clear => {}
            CleanupStatus::Reclaiming => push(
                vega_i18n::t("ui.cleaning_up_permission_check_resources").into(),
                false,
            ),
            CleanupStatus::Failed(error) => push(
                vega_i18n::t_args(
                    "ui.permission_check_cleanup_failed_error_computer_control_may",
                    &[("error", &error.user_message())],
                ),
                true,
            ),
        }
        PreparationSnapshot {
            view,
            retained: self.retained,
            feedback,
            guide_warning,
        }
    }
}
