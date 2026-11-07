use super::*;
mod nsh_view;

use crate::nsh::ProjectionId;
use vega_conversation::types::{
    ShellRecoveryConfirmation, ShellRecoveryError, ShellRecoveryProjection,
};

#[derive(Clone)]
pub enum ShellRecoveryAction {
    Read,
    Cancel(String),
    Check(ShellRecoveryProjection),
    Retry(ShellRecoveryProjection),
    Prepare(ShellRecoveryProjection),
    Resume(Box<ShellRecoveryConfirmation>),
}

#[derive(Clone)]
pub struct ShellRecoveryRequested {
    pub source: Thread,
    pub sequence: u64,
    pub action: ShellRecoveryAction,
}

pub enum ShellRecoveryOutcome {
    Projection(ShellRecoveryProjection),
    Confirmation(Box<ShellRecoveryConfirmation>),
}

impl EventEmitter<ShellRecoveryRequested> for ConversationStream {}

pub(crate) struct ShellRecoveryUi {
    sequence: u64,
    pending: Option<&'static str>,
    pub(super) opened: bool,
    pub(super) projection: Option<ShellRecoveryProjection>,
    decision: RecoveryDecision<ShellRecoveryConfirmation>,
    error: Option<ShellRecoveryError>,
    refresh: Option<gpui_kit::Task<()>>,
    review_focus: FocusHandle,
    safe_focus: FocusHandle,
    observation: ProjectionId,
    projection_identity: ProjectionId,
    confirmation_identity: ProjectionId,
    presentation: Option<nsh_view::Presentation>,
}

struct RecoveryDecision<T> {
    confirmation: Option<Box<T>>,
    submitted: bool,
}

impl<T> Default for RecoveryDecision<T> {
    fn default() -> Self {
        Self {
            confirmation: None,
            submitted: false,
        }
    }
}

impl<T> RecoveryDecision<T> {
    fn dismiss(&mut self) {
        if !self.submitted {
            self.confirmation = None;
        }
    }
}

impl ShellRecoveryUi {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sequence: 0,
            pending: None,
            opened: false,
            projection: None,
            decision: RecoveryDecision::default(),
            error: None,
            refresh: None,
            review_focus: cx.focus_handle(),
            safe_focus: cx.focus_handle(),
            observation: ProjectionId::new(),
            projection_identity: ProjectionId::new(),
            confirmation_identity: ProjectionId::new(),
            presentation: None,
        }
    }
}

impl ConversationStream {
    pub(super) fn dismiss_shell_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shell_recovery.observation = ProjectionId::new();
        self.shell_recovery.opened = false;
        if self.shell_recovery.pending.is_none() {
            self.shell_recovery.decision.dismiss();
        }
        self.shell_recovery.review_focus.focus(window, cx);
        cx.notify();
    }

    pub fn refresh_shell_recovery(&mut self, cx: &mut Context<Self>) {
        if self.thread.backend == TaskBackend::Native
            && !self.thread.project_id.is_empty()
            && self.shell_recovery.pending.is_none()
            && self.shell_recovery.decision.confirmation.is_none()
        {
            self.request_shell_recovery(ShellRecoveryAction::Read, cx);
        }
    }

    fn request_shell_recovery(&mut self, action: ShellRecoveryAction, cx: &mut Context<Self>) {
        if self.shell_recovery.pending.is_some() {
            return;
        }
        let Some(sequence) = self.shell_recovery.sequence.checked_add(1) else {
            return;
        };
        let label = match &action {
            ShellRecoveryAction::Retry(_) => "ui.shell_recovery.retrying",
            ShellRecoveryAction::Resume(_) => "ui.shell_recovery.resuming",
            ShellRecoveryAction::Cancel(_) => "ui.shell_session.stopping",
            _ => "ui.shell_recovery.checking",
        };
        if matches!(&action, ShellRecoveryAction::Resume(_)) {
            self.shell_recovery.decision.submitted = true;
        }
        self.shell_recovery.observation = ProjectionId::new();
        self.shell_recovery.sequence = sequence;
        self.shell_recovery.pending = Some(label);
        self.shell_recovery.error = None;
        cx.emit(ShellRecoveryRequested {
            source: self.thread.clone(),
            sequence,
            action,
        });
        cx.notify();
    }

    pub fn finish_shell_recovery(
        &mut self,
        request: &ShellRecoveryRequested,
        result: Result<ShellRecoveryOutcome, ShellRecoveryError>,
        cx: &mut Context<Self>,
    ) {
        if request.source.id != self.thread.id
            || request.source.project_id != self.thread.project_id
            || request.sequence != self.shell_recovery.sequence
        {
            return;
        }
        self.shell_recovery.observation = ProjectionId::new();
        self.shell_recovery.pending = None;
        match result {
            Ok(ShellRecoveryOutcome::Projection(projection)) => {
                self.shell_recovery.projection_identity = ProjectionId::new();
                for command in &projection.commands {
                    if command.thread_id == self.thread.id
                        && let Some(card) = self.tool_cards.get(&command.call_id)
                    {
                        card.update(cx, |card, cx| {
                            if card.apply_shell_command(command) {
                                cx.notify();
                            }
                        });
                    }
                }
                self.shell_recovery.decision = RecoveryDecision::default();
                if projection.rows.is_empty() {
                    self.shell_recovery.opened = false;
                }
                self.shell_recovery.projection = Some(projection);
            }
            Ok(ShellRecoveryOutcome::Confirmation(confirmation)) => {
                self.shell_recovery.confirmation_identity = ProjectionId::new();
                self.shell_recovery.decision = RecoveryDecision {
                    confirmation: Some(confirmation),
                    submitted: false,
                };
            }
            Err(error) => {
                if error == ShellRecoveryError::Changed {
                    self.shell_recovery.decision = RecoveryDecision::default();
                }
                self.shell_recovery.error = Some(error);
            }
        }
        let refresh = self
            .shell_recovery
            .projection
            .as_ref()
            .is_some_and(|projection| {
                !projection.active.is_empty()
                    || projection.commands.iter().any(|command| {
                        command.thread_id == self.thread.id
                            && matches!(
                                command.state.as_str(),
                                "prepared" | "running" | "finishing" | "stopping"
                            )
                    })
            });
        if refresh && self.shell_recovery.refresh.is_none() {
            self.shell_recovery.refresh = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.shell_recovery.refresh = None;
                    this.refresh_shell_recovery(cx);
                });
            }));
        }
        cx.notify();
    }
}

fn recovery_reason(reason: &str) -> String {
    let mut keys = Vec::new();
    for reason in reason.split(", ") {
        let key = match reason {
            "control_continuity_lost" | "session_expired" => "ui.shell_recovery.reason_restart",
            "process_identity_changed" | "group_continuity_lost" => {
                "ui.shell_recovery.reason_identity"
            }
            "cleanup_budget_exhausted" => "ui.shell_recovery.reason_budget",
            "initial_identity_unavailable"
            | "descendant_observation_failed"
            | "process_observation_failed"
            | "process_identity_capacity" => "ui.shell_recovery.reason_observation",
            "group_signal_failed"
            | "descendant_signal_failed"
            | "child_wait_failed"
            | "process_cleanup_unresolved"
            | "termination_failed" => "ui.shell_recovery.reason_process",
            "credential_cleanup_failed" => "ui.shell_recovery.reason_credentials",
            "cleanup_commit_unconfirmed" => "ui.shell_recovery.reason_storage",
            "output_incomplete" | "output_failed" | "output_pipes_unavailable" => {
                "ui.shell_recovery.reason_output"
            }
            _ => "ui.shell_recovery.reason_resource",
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys.into_iter()
        .map(vega_i18n::t)
        .collect::<Vec<_>>()
        .join(" ")
}
