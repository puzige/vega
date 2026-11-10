use super::*;
#[cfg(feature = "nsh-reading-integration")]
pub mod command_control_regressions;
mod nsh_view;
#[cfg(feature = "nsh-reading-integration")]
pub use nsh_view::mounted_regressions;

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
    pub draft: bool,
    pub action: ShellRecoveryAction,
}

impl ShellRecoveryRequested {
    fn matches_route(&self, thread: &Thread, draft: bool, sequence: u64) -> bool {
        self.source.id == thread.id
            && self.source.project_id == thread.project_id
            && same_recovery_authority(&self.source, thread)
            && self.draft == draft
            && self.sequence == sequence
    }
}

pub(super) fn same_recovery_authority(source: &Thread, current: &Thread) -> bool {
    source.backend == current.backend
        && source.mode == current.mode
        && source.permission_mode == current.permission_mode
        && source.permission_revision == current.permission_revision
        && source.draft_authority_epoch == current.draft_authority_epoch
        && source.default_permission == current.default_permission
        && source.full_access_grant == current.full_access_grant
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryPresentation {
    Hidden,
    Summary(&'static str),
    Details,
}

fn recovery_presentation(draft: bool, blocked: bool, unavailable: bool) -> RecoveryPresentation {
    if !draft {
        RecoveryPresentation::Details
    } else if unavailable {
        RecoveryPresentation::Summary("ui.shell_recovery.status_unavailable")
    } else if blocked {
        RecoveryPresentation::Summary("ui.shell_recovery.paused")
    } else {
        RecoveryPresentation::Hidden
    }
}

pub enum ShellRecoveryOutcome {
    Projection(ShellRecoveryProjection),
    Confirmation(Box<ShellRecoveryConfirmation>),
}

impl EventEmitter<ShellRecoveryRequested> for ConversationStream {}

pub(crate) struct ShellRecoveryUi {
    sequence: u64,
    pending: Option<&'static str>,
    pending_read: bool,
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
    summary: Option<nsh_view::Presentation>,
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
    fn dismiss(&mut self, sequence: &mut u64, pending: &mut Option<&'static str>) {
        if !self.submitted {
            self.confirmation = None;
            *sequence = sequence.saturating_add(1);
            *pending = None;
        }
    }
}

impl ShellRecoveryUi {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sequence: 0,
            pending: None,
            pending_read: false,
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
            summary: None,
        }
    }
}

impl ConversationStream {
    pub(super) fn reset_shell_recovery(&mut self, cx: &mut Context<Self>) {
        let sequence = self.shell_recovery.sequence.saturating_add(1);
        self.shell_recovery = ShellRecoveryUi::new(cx);
        self.shell_recovery.sequence = sequence;
        let stream = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = stream.update(cx, |this, cx| this.refresh_shell_recovery(cx));
        });
    }

    pub(super) fn dismiss_shell_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shell_recovery.observation = ProjectionId::new();
        self.shell_recovery.opened = false;
        self.shell_recovery.decision.dismiss(
            &mut self.shell_recovery.sequence,
            &mut self.shell_recovery.pending,
        );
        self.shell_recovery.confirmation_identity = ProjectionId::new();
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

    pub(super) fn request_shell_control(&mut self, operation_id: String, cx: &mut Context<Self>) {
        if (self.shell_recovery.pending.is_some() && !self.shell_recovery.pending_read)
            || self.shell_recovery.sequence.checked_add(1).is_none()
        {
            for card in self.tool_cards.values() {
                card.update(cx, |card, cx| {
                    if card.finish_shell_control(&operation_id, true) {
                        cx.notify();
                    }
                });
            }
            return;
        }
        self.shell_recovery.pending = None;
        self.request_shell_recovery(ShellRecoveryAction::Cancel(operation_id), cx);
    }

    pub(super) fn request_shell_recovery(
        &mut self,
        action: ShellRecoveryAction,
        cx: &mut Context<Self>,
    ) {
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
        self.shell_recovery.pending_read = matches!(&action, ShellRecoveryAction::Read);
        self.shell_recovery.error = None;
        cx.emit(ShellRecoveryRequested {
            source: self.thread.clone(),
            sequence,
            draft: self.draft_route,
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
        if !request.matches_route(&self.thread, self.draft_route, self.shell_recovery.sequence) {
            return;
        }
        self.shell_recovery.observation = ProjectionId::new();
        self.shell_recovery.pending = None;
        self.shell_recovery.pending_read = false;
        let mut card_control = false;
        if let ShellRecoveryAction::Cancel(operation_id) = &request.action {
            for card in self.tool_cards.values() {
                card.update(cx, |card, cx| {
                    if card.finish_shell_control(operation_id, result.is_err()) {
                        card_control = true;
                        cx.notify();
                    }
                });
            }
        }
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
                if !card_control {
                    self.shell_recovery.error = Some(error);
                }
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
