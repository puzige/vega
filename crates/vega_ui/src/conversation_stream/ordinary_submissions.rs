use super::*;
use gpui_kit::Role;
use vega_conversation::types::{
    AttemptId, CommitFact, OrdinarySubmission, PrimaryFact, RetryBlocker, RetryEligibility,
    SubmissionFailure, SubmissionId,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub enum OrdinaryAction {
    Cancel,
    Retry,
    CheckStatus,
}

pub struct OrdinaryActionRequested {
    pub thread_id: String,
    pub submission: SubmissionId,
    pub attempt: AttemptId,
    pub action: OrdinaryAction,
}

pub(crate) struct OrdinaryRow {
    submission: OrdinarySubmission,
    attempt: AttemptId,
    state: OrdinaryStatus,
    focus: FocusHandle,
    checked: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrdinaryStatus {
    Pending,
    Committed,
    Failed(SubmissionFailure, RetryEligibility),
    Unknown,
}

struct OrdinaryControls {
    message: &'static str,
    detail: Option<&'static str>,
    action: Option<(&'static str, OrdinaryAction)>,
}

impl OrdinaryStatus {
    fn controls(
        self,
        retry: RetryEligibility,
        checked: Option<&'static str>,
    ) -> Option<OrdinaryControls> {
        if matches!(self, Self::Pending | Self::Committed) {
            return None;
        }
        if retry == RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated) {
            let unknown = self == Self::Unknown;
            return Some(OrdinaryControls {
                message: if unknown {
                    "ui.ordinary_send.permission_invalidated_unknown"
                } else {
                    "ui.ordinary_send.permission_invalidated"
                },
                detail: unknown.then_some(checked).flatten(),
                action: unknown
                    .then_some(("ui.ordinary_send.check_status", OrdinaryAction::CheckStatus)),
            });
        }
        let (message, label, action) = match self {
            Self::Pending | Self::Committed => return None,
            Self::Unknown => (
                "ui.ordinary_send.unknown",
                "ui.ordinary_send.check_status",
                OrdinaryAction::CheckStatus,
            ),
            Self::Failed(
                failure,
                RetryEligibility::Uncommitted | RetryEligibility::CommittedNotStarted,
            ) => (
                if failure == SubmissionFailure::Cancelled {
                    "ui.cancelled"
                } else {
                    match failure {
                        SubmissionFailure::PreflightRejected => "ui.ordinary_send.preflight_failed",
                        SubmissionFailure::SpawnFailed => "ui.ordinary_send.spawn_failed",
                        SubmissionFailure::PreparationFailed => {
                            "ui.ordinary_send.preparation_failed"
                        }
                        SubmissionFailure::CommitFailed => "ui.ordinary_send.commit_failed",
                        SubmissionFailure::LaunchFailed => "ui.ordinary_send.launch_failed",
                        _ => "ui.ordinary_send.incomplete",
                    }
                },
                "ui.retry",
                OrdinaryAction::Retry,
            ),
            Self::Failed(_, RetryEligibility::Blocked(blocker)) => (
                match blocker {
                    RetryBlocker::CleanupIncomplete => "ui.ordinary_send.cleanup_incomplete",
                    RetryBlocker::HistoryChanged => "ui.ordinary_send.history_changed",
                    RetryBlocker::WorkersActive | RetryBlocker::Unsealed => {
                        "ui.ordinary_send.finishing"
                    }
                    _ => "ui.ordinary_send.retry_unconfirmed",
                },
                "ui.ordinary_send.check_status",
                OrdinaryAction::CheckStatus,
            ),
        };
        Some(OrdinaryControls {
            message: checked.unwrap_or(message),
            detail: None,
            action: Some((label, action)),
        })
    }
}

pub(super) fn update_user_history_text(
    entry: &mut StreamEntry,
    text: &str,
    next_block: &mut u64,
) -> bool {
    if let StreamEntry::User { copy, lines } = entry
        && copy.original_text() != text
    {
        copy.clear_retained();
        copy.replace_text(text);
        *lines = user_message_lines(*next_block, text);
        *next_block += 1;
        true
    } else {
        false
    }
}

impl ConversationStream {
    pub(crate) fn ordinary_pending_before_run(&self) -> bool {
        !self.actions.running
            && self
                .ordinary
                .iter()
                .any(|row| matches!(row.state, OrdinaryStatus::Pending))
    }

    pub(super) fn cancel_ordinary_pending(&self, cx: &mut Context<Self>) -> bool {
        if self.actions.running {
            return false;
        }
        let Some(row) = self
            .ordinary
            .iter()
            .rev()
            .find(|row| matches!(row.state, OrdinaryStatus::Pending))
        else {
            return false;
        };
        cx.emit(OrdinaryActionRequested {
            thread_id: self.thread.id.clone(),
            submission: row.submission.id(),
            attempt: row.attempt,
            action: OrdinaryAction::Cancel,
        });
        true
    }

    pub fn begin_ordinary_pending(
        &mut self,
        submission: OrdinarySubmission,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.thread.id != request.thread_id
            || self
                .ordinary
                .iter()
                .any(|row| row.submission.id() == submission.id())
            || submission.raw().draft_revision() != request.draft_revision
        {
            return false;
        }
        let original_images = self
            .submitted_attachments
            .iter()
            .filter(|(id, _)| request.attachment_ids.contains(id))
            .map(|(_, image)| image.clone())
            .collect::<Vec<_>>();
        let image_identity =
            self.durable_entry_identity(submission.user_message_id(), "user-images", None);
        self.project_ordinary_submission(submission, cx);
        if original_images.len() == request.images.len()
            && !original_images.is_empty()
            && let Some(index) = self
                .entry_identities
                .iter()
                .position(|identity| identity.key == image_identity.key)
            && let Some(StreamEntry::UserImages { images }) = self.entries.get_mut(index)
        {
            *images = original_images;
        }
        if self.input.read(cx).content_revision() == request.draft_revision {
            self.input.update(cx, TextInput::clear);
        }
        self.attachments
            .retain(|(id, _)| !request.attachment_ids.contains(id));
        self.submitted_attachments.clear();
        self.pending_submission_revision = None;
        self.composer_submit_pending = false;
        self.pending_behavior_override = None;
        self.history_cursor = None;
        self.history_draft = None;
        if !request.content.is_empty() {
            self.composer_history
                .push(vega_conversation::privacy::safe_live_text(&request.content));
        }
        cx.notify();
        true
    }

    pub fn project_ordinary_submission(
        &mut self,
        submission: OrdinarySubmission,
        cx: &mut Context<Self>,
    ) {
        let snapshot = submission.snapshot();
        if snapshot.thread_id != self.thread.id || snapshot.retired {
            return;
        }
        let tracked = self
            .ordinary
            .iter()
            .any(|row| row.submission.id() == snapshot.id);
        let Some(attempt) = snapshot.attempts.last() else {
            return;
        };
        self.ensure_entry_identities();
        let text_identity =
            self.durable_entry_identity(&snapshot.user_message_id, "user-text", None);
        let image_identity =
            self.durable_entry_identity(&snapshot.user_message_id, "user-images", None);
        let first = self.entries.len();
        if !snapshot.raw.text().is_empty()
            && !self
                .entry_identities
                .iter()
                .any(|identity| identity.key == text_identity.key)
        {
            let text = vega_conversation::privacy::safe_live_text(snapshot.raw.text());
            let block = self.user_block_seq;
            self.user_block_seq += 1;
            let index = self.entries.len();
            self.entries.push(StreamEntry::User {
                copy: MessageCopy::new(&text),
                lines: user_message_lines(block, &text),
            });
            self.set_entry_identity(index, text_identity);
        }
        if !snapshot.raw.images().is_empty()
            && !self
                .entry_identities
                .iter()
                .any(|identity| identity.key == image_identity.key)
        {
            let images = self.history_image_previews(snapshot.raw.images().to_vec(), cx);
            let index = self.entries.len();
            self.entries.push(StreamEntry::UserImages { images });
            self.set_entry_identity(index, image_identity);
        }
        if self.entries.len() > first {
            self.list_append(first);
        }
        if !tracked {
            self.ordinary.push(OrdinaryRow {
                submission,
                attempt: attempt.id,
                state: OrdinaryStatus::Pending,
                focus: cx.focus_handle().tab_stop(true),
                checked: None,
            });
        }
        self.refresh_ordinary_submissions(cx);
    }

    pub(crate) fn take_ordinary_history_entries(
        &mut self,
    ) -> Vec<(StreamEntryIdentity, StreamEntry)> {
        self.ensure_entry_identities();
        std::mem::take(&mut self.entry_identities)
            .into_iter()
            .zip(std::mem::take(&mut self.entries))
            .filter(|(identity, entry)| {
                matches!(
                    entry,
                    StreamEntry::User { .. } | StreamEntry::UserImages { .. }
                ) && self.ordinary.iter().any(|row| {
                    identity.message_id.as_deref() == Some(row.submission.user_message_id())
                })
            })
            .collect()
    }

    pub(crate) fn restore_ordinary_history_entries(
        &mut self,
        retained: Vec<(StreamEntryIdentity, StreamEntry)>,
        cx: &mut Context<Self>,
    ) {
        let first = self.entries.len();
        for (identity, mut entry) in retained {
            if let Some(index) = self
                .entry_identities
                .iter()
                .position(|current| current.key == identity.key)
            {
                match (&entry, &self.entries[index]) {
                    (StreamEntry::User { .. }, StreamEntry::User { copy, .. }) => {
                        let text = copy.original_text();
                        update_user_history_text(&mut entry, &text, &mut self.user_block_seq);
                        self.entries[index] = entry;
                    }
                    (
                        StreamEntry::UserImages { images: retained },
                        StreamEntry::UserImages { images: current },
                    ) if retained
                        .iter()
                        .map(|image| &image.image)
                        .eq(current.iter().map(|image| &image.image)) =>
                    {
                        self.entries[index] = entry;
                    }
                    _ => {}
                }
            } else if self.ordinary.iter().any(|row| {
                identity.message_id.as_deref() == Some(row.submission.user_message_id())
                    && !row.submission.snapshot().retired
            }) {
                if let StreamEntry::User { copy, .. } = &entry {
                    let text = vega_conversation::privacy::safe_live_text(&copy.original_text());
                    update_user_history_text(&mut entry, &text, &mut self.user_block_seq);
                }
                self.entries.push(entry);
                self.entry_identities.push(identity);
            } else if let StreamEntry::User { copy, .. } = entry {
                copy.clear_retained();
            }
        }
        self.list_append(first);
        let submissions = self
            .ordinary
            .iter()
            .map(|row| row.submission.clone())
            .collect::<Vec<_>>();
        for submission in submissions {
            self.project_ordinary_submission(submission, cx);
        }
        self.refresh_ordinary_submissions(cx);
        self.order_ordinary_history_entries();
    }

    pub(crate) fn order_ordinary_history_entries(&mut self) {
        let anchor = self.scroll_anchor_snapshot();
        let keys = self
            .entry_identities
            .iter()
            .filter(|identity| {
                self.ordinary.iter().any(|row| {
                    identity.message_id.as_deref() == Some(row.submission.user_message_id())
                })
            })
            .map(|identity| identity.key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            let Some(from) = self
                .entry_identities
                .iter()
                .position(|identity| identity.key == key)
            else {
                continue;
            };
            let sequence = self.entry_identities[from].sequence;
            let to = self
                .entry_identities
                .iter()
                .enumerate()
                .find_map(|(index, identity)| {
                    if index == from {
                        return None;
                    }
                    let ordinary = self.ordinary.iter().any(|row| {
                        identity.message_id.as_deref() == Some(row.submission.user_message_id())
                    });
                    let projected_sequence = identity.sequence.or_else(|| {
                        let message_id = identity.message_id.as_deref()?;
                        self.entry_identities.iter().find_map(|projection| {
                            (projection.message_id.as_deref() == Some(message_id))
                                .then_some(projection.sequence)
                                .flatten()
                        })
                    });
                    let follows = match (sequence, projected_sequence) {
                        (Some(sequence), Some(other)) => other > sequence,
                        (Some(_), None) => ordinary,
                        (None, _) => false,
                    };
                    follows.then_some(index)
                })
                .unwrap_or(self.entries.len());
            let to = if to > from { to - 1 } else { to };
            if from == to {
                continue;
            }
            let entry = self.entries.remove(from);
            let identity = self.entry_identities.remove(from);
            self.shift_entry_indices(from, 1);
            self.list.splice(from..from + 1, 0);
            self.entries.insert(to, entry);
            self.entry_identities.insert(to, identity);
            self.grow_entry_indices(to, 1);
            self.list.splice(to..to, 1);
        }
        self.refresh_assistant_roles();
        if !anchor.following_tail {
            self.restore_scroll_anchor(&anchor);
        }
    }

    pub fn refresh_ordinary_submissions(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for row in &mut self.ordinary {
            let mut snapshot = row.submission.snapshot();
            if let Some(attempt) = snapshot.attempts.last()
                && !attempt.sealed
                && attempt.workers == 0
            {
                let _ = row.submission.seal(attempt.id);
                snapshot = row.submission.snapshot();
            }
            let Some(attempt) = snapshot.attempts.last() else {
                continue;
            };
            let state = if snapshot.unavailable
                || attempt.abnormal
                || matches!(
                    attempt.commit,
                    CommitFact::Unknown | CommitFact::CommitInFlight
                ) && attempt.sealed
            {
                OrdinaryStatus::Unknown
            } else if let Some(failure) = attempt.failure {
                if attempt.primary == PrimaryFact::Started {
                    OrdinaryStatus::Committed
                } else {
                    OrdinaryStatus::Failed(failure, snapshot.retry)
                }
            } else if matches!(attempt.commit, CommitFact::Committed(_)) {
                OrdinaryStatus::Committed
            } else {
                OrdinaryStatus::Pending
            };
            if row.attempt != attempt.id || row.state != state {
                row.attempt = attempt.id;
                row.state = state;
                row.checked = None;
                changed = true;
            }
            if let CommitFact::Committed(receipt) = &attempt.commit {
                for identity in &mut self.entry_identities {
                    if identity.message_id.as_deref() == Some(receipt.user_message_id.as_str())
                        && identity.sequence != Some(receipt.user_seq)
                    {
                        identity.sequence = Some(receipt.user_seq);
                        changed = true;
                    }
                }
            }
            if attempt.sealed
                && attempt.workers == 0
                && attempt.primary == PrimaryFact::Started
                && !attempt.abnormal
                && matches!(attempt.commit, CommitFact::Committed(_))
            {
                let _ = row.submission.retire_after_history_handoff(attempt.id);
            }
        }
        if changed {
            self.reset_composer_stopping_if_idle();
            cx.notify();
        }
    }

    pub fn ordinary_status_checked(
        &mut self,
        submission: SubmissionId,
        attempt: AttemptId,
        message: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .ordinary
            .iter_mut()
            .find(|row| row.submission.id() == submission && row.attempt == attempt)
        {
            row.checked = Some(message);
            cx.notify();
        }
    }

    /// A stopped turn already states its outcome with one stop line. The send
    /// row must not repeat it as a failed send with a retry action.
    fn attempt_shows_stop_line(&self, row: &OrdinaryRow, cx: &App) -> bool {
        let snapshot = row.submission.snapshot();
        let Some(attempt) = snapshot.attempts.last() else {
            return false;
        };
        self.run_activity_groups
            .get(&attempt.assistant_message_id)
            .is_some_and(|group| group.read(cx).status() == RunActivityStatus::Interrupted)
    }

    pub(crate) fn ordinary_controls(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let identity = self.entry_identities.get(index)?;
        let row = self
            .ordinary
            .iter()
            .find(|row| identity.message_id.as_deref() == Some(row.submission.user_message_id()))?;
        if self.attempt_shows_stop_line(row, cx) {
            return None;
        }
        let final_kind = if row.submission.raw().images().is_empty() {
            "user-text"
        } else {
            "user-images"
        };
        if identity.key
            != self
                .durable_entry_identity(row.submission.user_message_id(), final_kind, None)
                .key
        {
            return None;
        }
        let controls = row
            .state
            .controls(row.submission.snapshot().retry, row.checked)?;
        let submission = row.submission.id();
        let attempt = row.attempt;
        let thread = self.thread.id.clone();
        let keyboard_thread = thread.clone();
        let colors = theme(cx).colors;
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(Typography::BODY))
                .text_color(colors.text_secondary)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(vega_i18n::t(controls.message))
                        .children(controls.detail.map(vega_i18n::t)),
                )
                .children(controls.action.map(|(label, action)| {
                    let label = vega_i18n::t(label);
                    div()
                        .id(format!(
                            "ordinary-action-{}",
                            row.submission.user_message_id()
                        ))
                        .role(Role::Button)
                        .aria_label(label)
                        .key_context("PendingMessageControl")
                        .track_focus(&row.focus)
                        .tab_stop(true)
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                        .focus_visible(move |style| style.border_1().border_color(colors.accent))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                cx.emit(OrdinaryActionRequested {
                                    thread_id: thread.clone(),
                                    submission,
                                    attempt,
                                    action,
                                });
                            }),
                        )
                        .on_action(cx.listener(
                            move |_, _: &ActivatePendingMessageControl, _, cx| {
                                cx.stop_propagation();
                                cx.emit(OrdinaryActionRequested {
                                    thread_id: keyboard_thread.clone(),
                                    submission,
                                    attempt,
                                    action,
                                });
                            },
                        ))
                        .child(label)
                }))
                .into_any_element(),
        )
    }
}
