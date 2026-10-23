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
    pub guide_fault: Option<&'static str>,
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
    adapter_error: Option<String>,
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
        if self.owners.is_empty() {
            self.stop();
        }
        self.service_status = session::service_status();
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
                        self.adapter_error = Some(error.to_string());
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
                    self.adapter_error = Some(error.to_string());
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
    }

    pub(crate) fn snapshot(&self) -> PreparationSnapshot {
        let view = self.controller.view();
        let guide_fault = if matches!(self.service_status.cleanup, CleanupStatus::Failed(_)) {
            Some("准备资源回收失败，请前往权限与检测重试")
        } else if self.service_status.failure.is_some() || self.adapter_error.is_some() {
            Some("权限检测不可用，请前往权限与检测处理")
        } else if self.service_status.last_close_error.is_some()
            && self.service_status.cleanup != CleanupStatus::Clear
        {
            Some("准备资源回收未确认，请前往权限与检测处理")
        } else if matches!(view.check_state, CheckState::Busy | CheckState::Failed(_))
            || self.service_status.last_inspection_error.is_some()
        {
            Some("权限检测未完成，请前往权限与检测处理")
        } else {
            None
        };
        let mut feedback = Vec::new();
        let mut push = |message: String, warning| {
            feedback.push(PreparationFeedback { message, warning });
        };
        match view.check_state {
            CheckState::Idle => push("尚未检测，请重新检测权限状态。".into(), false),
            CheckState::Checking => push(
                if view.component.is_some() {
                    "正在检测权限，下方保留上次检测结果。"
                } else {
                    "正在检测权限…"
                }
                .into(),
                false,
            ),
            CheckState::Observed => {}
            CheckState::Busy => push(
                "另一个任务正在使用电脑操控。请先停止该任务，再重新检测。".into(),
                true,
            ),
            CheckState::Unknown => {
                push("部分权限状态未知。请检查系统设置后重新检测。".into(), true)
            }
            CheckState::Failed(error) => push(
                format!("{}。处理后可重新检测。", error.user_message()),
                true,
            ),
        }
        if self.retained && view.check_state != CheckState::Checking {
            push("下方为保留状态；请重新检测以确认当前权限。".into(), false);
        }
        if let Some(error) = &self.adapter_error {
            push(error.clone(), true);
        }
        if let Some(error) = &self.service_status.failure {
            push(error.clone(), true);
        }
        if let Some(error) = self.service_status.last_inspection_error
            && view.check_state != CheckState::Failed(error)
            && !(view.check_state == CheckState::Busy && error == ComputerUseError::Busy)
        {
            push(format!("上次检测失败：{}。", error.user_message()), true);
        }
        if let Some(error) = self.service_status.last_close_error {
            let recovery = if self.service_status.cleanup == CleanupStatus::Clear {
                "后续回收已完成，可重新检测。"
            } else {
                "回收尚未确认，请重新检测以重试。"
            };
            push(
                format!("关闭检测时发生错误：{}。{recovery}", error.user_message()),
                true,
            );
        }
        match self.service_status.cleanup {
            CleanupStatus::Clear => {}
            CleanupStatus::Reclaiming => push("正在回收准备检测资源…".into(), false),
            CleanupStatus::Failed(error) => push(
                format!(
                    "准备检测回收失败：{}。电脑操控可能仍被占用。请重新检测以重试；仍失败时请重启 Vega。",
                    error.user_message()
                ),
                true,
            ),
        }
        PreparationSnapshot {
            view,
            retained: self.retained,
            feedback,
            guide_fault,
        }
    }
}
