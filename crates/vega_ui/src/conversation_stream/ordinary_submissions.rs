use super::*;
use gpui_kit::Role;
use vega_conversation::types::{
    AttemptId, CommitFact, OrdinarySubmission, PrimaryFact, RetryBlocker, RetryEligibility,
    SubmissionFailure, SubmissionId,
};

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

impl ConversationStream {
    pub(crate) fn ordinary_pending_before_run(&self) -> bool {
        !self.actions.running
            && self
                .ordinary
                .iter()
                .any(|row| matches!(row.state, OrdinaryStatus::Pending))
    }

    pub fn begin_ordinary_pending(
        &mut self,
        submission: OrdinarySubmission,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.thread.id != request.thread_id
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
        if snapshot.thread_id != self.thread.id
            || self
                .ordinary
                .iter()
                .any(|row| row.submission.id() == snapshot.id)
        {
            return;
        }
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
        self.ordinary.push(OrdinaryRow {
            submission,
            attempt: attempt.id,
            state: OrdinaryStatus::Pending,
            focus: cx.focus_handle().tab_stop(true),
            checked: None,
        });
        self.refresh_ordinary_submissions(cx);
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
        let (message, label, action) = match row.state {
            OrdinaryStatus::Committed => return None,
            OrdinaryStatus::Pending => ("待提交", "取消", OrdinaryAction::Cancel),
            OrdinaryStatus::Unknown => (
                "发送结果尚未确认；核实前不能重试",
                "核实状态",
                OrdinaryAction::CheckStatus,
            ),
            OrdinaryStatus::Failed(
                failure,
                RetryEligibility::Uncommitted | RetryEligibility::CommittedNotStarted,
            ) => (
                if failure == SubmissionFailure::Cancelled {
                    "已取消"
                } else {
                    match failure {
                        SubmissionFailure::PreflightRejected => {
                            "模型或 API Key 不可用；请检查设置后重试"
                        }
                        SubmissionFailure::SpawnFailed => "无法启动发送任务；请重试",
                        SubmissionFailure::PreparationFailed => {
                            "发送准备失败；请检查模型、权限与工作目录后重试"
                        }
                        SubmissionFailure::CommitFailed => "消息未能保存；请重试",
                        SubmissionFailure::LaunchFailed => "消息已保留，但运行未启动",
                        _ => "发送未完成",
                    }
                },
                "重试",
                OrdinaryAction::Retry,
            ),
            OrdinaryStatus::Failed(_, RetryEligibility::Blocked(blocker)) => (
                match blocker {
                    RetryBlocker::CleanupIncomplete => "清理尚未确认；暂不能重试",
                    RetryBlocker::HistoryChanged => "会话已变化；请查看历史后再发送",
                    RetryBlocker::WorkersActive | RetryBlocker::Unsealed => "正在结束本次发送",
                    _ => "无法确认安全重试；请先核实状态",
                },
                "核实状态",
                OrdinaryAction::CheckStatus,
            ),
        };
        let message = row.checked.unwrap_or(message);
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
                .child(message)
                .child(
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
                        .child(label),
                )
                .into_any_element(),
        )
    }
}
