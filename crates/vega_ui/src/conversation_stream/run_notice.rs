use super::privacy::PrivacyButton;
use super::*;
use crate::icons::Icon;
use vega_conversation::types::TaskSummaryOutcome;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RunNoticeKind {
    Retrying {
        attempt: u32,
        max_retries: u32,
    },
    Failure(RunFailureKind),
    FailureWithCompaction(
        RunFailureKind,
        Option<vega_conversation::types::ContextCompactionFailureCode>,
    ),
    ContextFailure,
    CompactionFailure(Option<vega_conversation::types::ContextCompactionFailureCode>),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RunNotice {
    pub(crate) message_id: String,
    pub(crate) kind: RunNoticeKind,
}

impl RunNotice {
    pub(crate) fn text(&self) -> String {
        match &self.kind {
            RunNoticeKind::Retrying {
                attempt,
                max_retries,
            } => format!("正在重新连接… {attempt}/{max_retries}"),
            RunNoticeKind::Failure(failure) => failure.message(),
            RunNoticeKind::FailureWithCompaction(failure, compaction) => format!(
                "{}\n{}",
                context_control::status_label_for(
                    vega_conversation::types::ContextCompactionStatus::Failed,
                    *compaction
                ),
                failure.message()
            ),
            RunNoticeKind::ContextFailure => context_control::status_label_for(
                vega_conversation::types::ContextCompactionStatus::Failed,
                Some(vega_conversation::types::ContextCompactionFailureCode::OverLimit),
            )
            .into(),
            RunNoticeKind::CompactionFailure(failure) => context_control::status_label_for(
                vega_conversation::types::ContextCompactionStatus::Failed,
                *failure,
            )
            .into(),
        }
    }

    pub(crate) fn can_resume_credentials(&self) -> bool {
        matches!(
            self.kind,
            RunNoticeKind::Failure(
                RunFailureKind::CredentialExposureBlocked
                    | RunFailureKind::CredentialCheckUnavailable
                    | RunFailureKind::CredentialProjectionInvalid(_)
            ) | RunNoticeKind::FailureWithCompaction(
                RunFailureKind::CredentialExposureBlocked
                    | RunFailureKind::CredentialCheckUnavailable
                    | RunFailureKind::CredentialProjectionInvalid(_),
                _
            )
        )
    }

    pub(crate) fn render(
        &self,
        colors: &ThemeColors,
        thread_id: String,
        cx: &mut Context<ConversationStream>,
    ) -> AnyElement {
        if matches!(self.kind, RunNoticeKind::Retrying { .. }) {
            return gpui_kit::div()
                .id(format!("assistant-network-retry-{}", self.message_id))
                .debug_selector(|| "assistant-network-retry".into())
                .w_full()
                .min_w_0()
                .py_1()
                .aria_label(self.text())
                .child(render_rows::status_notice_with_icon(
                    self.text(),
                    colors.warning,
                    colors,
                    Icon::Wifi,
                ))
                .into_any_element();
        }
        let recovery_id = self.message_id.clone();
        let credential_failure = self.can_resume_credentials();
        let message_id = self.message_id.clone();
        let color = match self.kind {
            RunNoticeKind::Retrying { .. } => colors.text_secondary,
            RunNoticeKind::Failure(_)
            | RunNoticeKind::FailureWithCompaction(_, _)
            | RunNoticeKind::ContextFailure
            | RunNoticeKind::CompactionFailure(_) => colors.danger,
        };
        gpui_kit::div()
            .debug_selector(move || format!("run-notice-{message_id}"))
            .w_full()
            .min_w_0()
            .py_1()
            .child(
                gpui_kit::div()
                    .when(
                        matches!(
                            self.kind,
                            RunNoticeKind::Failure(_)
                                | RunNoticeKind::FailureWithCompaction(_, _)
                                | RunNoticeKind::ContextFailure
                                | RunNoticeKind::CompactionFailure(_)
                        ),
                        |row| row.debug_selector(|| "assistant-run-failure".into()),
                    )
                    .child(render_rows::plain_error_notice(self.text(), color)),
            )
            .when(credential_failure, |row| {
                row.child(
                    gpui_kit::div()
                        .flex()
                        .gap_2()
                        .child(
                            PrivacyButton::new(
                                format!("credential-continue-{}", self.message_id),
                                *colors,
                            )
                            .label("Continue")
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    cx.emit(CredentialContinuationRequested {
                                        thread_id: thread_id.clone(),
                                        message_id: recovery_id.clone(),
                                    });
                                },
                            )),
                        )
                        .child(
                            PrivacyButton::new(
                                format!("credential-settings-{}", self.message_id),
                                *colors,
                            )
                            .label("Check credential settings")
                            .on_click(|_, _, cx| {
                                cx.set_global(crate::settings::SettingsOpen(true));
                                cx.refresh_windows();
                            }),
                        ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn estimated_height(&self, width: f32) -> f32 {
        let retrying = matches!(self.kind, RunNoticeKind::Retrying { .. });
        let content_width = if retrying {
            (width - 2. * Layout::STATUS_PADDING - 2. - Layout::ICON_SIZE - Layout::STATUS_GAP)
                .max(1.)
        } else {
            width.max(1.)
        };
        let chars_per_line = (content_width / (Typography::BODY * 0.56)).max(1.) as usize;
        let lines = self
            .text()
            .lines()
            .map(|line| display_width(line).div_ceil(chars_per_line).max(1))
            .sum::<usize>()
            .max(1);
        lines as f32 * Typography::BODY * Typography::BODY_LINE_HEIGHT
            + if retrying {
                2. * Layout::STATUS_PADDING + 10.
            } else {
                8.
            }
            + if self.can_resume_credentials() {
                Typography::SIDEBAR_LINE_HEIGHT
            } else {
                0.
            }
    }
}

#[derive(Default)]
struct RunNoticeSource {
    failure: Option<RunFailureKind>,
    compaction_failure: Option<Option<vega_conversation::types::ContextCompactionFailureCode>>,
    status: Option<RunActivityStatus>,
    tail: Option<usize>,
    end: usize,
}

impl ConversationStream {
    pub(crate) fn remember_run_failure(
        &mut self,
        message_id: &str,
        failure: RunFailureKind,
    ) -> RunFailureKind {
        let remembered = self
            .run_failures
            .entry(message_id.to_owned())
            .or_insert(failure);
        if !matches!(failure, RunFailureKind::Runtime | RunFailureKind::Persisted)
            || matches!(
                remembered,
                RunFailureKind::Runtime | RunFailureKind::Persisted
            )
        {
            *remembered = failure;
        }
        *remembered
    }

    pub(super) fn compaction_reports_run_failure(
        &self,
        message_id: &str,
        failure: RunFailureKind,
    ) -> bool {
        self.entries.iter().zip(&self.entry_identities).any(|(entry, identity)| {
            identity.message_id.as_deref() == Some(message_id)
                && matches!(entry, StreamEntry::ContextCompaction { record, .. }
                    if record.status == vega_conversation::types::ContextCompactionStatus::Failed
                    && matches!((record.failure, failure),
                        (Some(vega_conversation::types::ContextCompactionFailureCode::OverLimit), RunFailureKind::ContextOverLimit)
                        | (Some(vega_conversation::types::ContextCompactionFailureCode::ReasoningLimit), RunFailureKind::ReasoningLimit(_))))
        })
    }

    pub(crate) fn compaction_failure_is_represented(&self, index: usize) -> bool {
        let Some(StreamEntry::ContextCompaction { record, .. }) = self.entries.get(index) else {
            return false;
        };
        if record.status != vega_conversation::types::ContextCompactionStatus::Failed {
            return false;
        }
        self.entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.as_ref())
            .is_some_and(|id| {
                self.run_notices.values().any(|notice| {
                    notice.message_id == *id && match notice.kind {
                        RunNoticeKind::CompactionFailure(code) | RunNoticeKind::FailureWithCompaction(_, code) => code == record.failure,
                        RunNoticeKind::ContextFailure => record.failure == Some(vega_conversation::types::ContextCompactionFailureCode::OverLimit),
                        RunNoticeKind::Failure(RunFailureKind::ReasoningLimit(_)) => record.failure == Some(vega_conversation::types::ContextCompactionFailureCode::ReasoningLimit),
                        _ => false,
                    }
                })
            })
    }

    pub(crate) fn run_notice_projection(&self, cx: &App) -> HashMap<usize, RunNotice> {
        let mut ends = vec![self.entries.len(); self.entries.len()];
        let mut end = self.entries.len();
        for (index, entry) in self.entries.iter().enumerate().rev() {
            ends[index] = end;
            if matches!(entry, StreamEntry::User { .. }) {
                end = index;
            }
        }
        let mut sources = HashMap::<String, RunNoticeSource>::new();
        for (index, (entry, identity)) in
            self.entries.iter().zip(&self.entry_identities).enumerate()
        {
            if matches!(
                entry,
                StreamEntry::User { .. } | StreamEntry::UserImages { .. }
            ) {
                continue;
            }
            let Some(message_id) = &identity.message_id else {
                continue;
            };
            let source = sources
                .entry(message_id.clone())
                .or_insert_with(|| RunNoticeSource {
                    end: ends[index],
                    ..Default::default()
                });
            if index < source.end && !self.process_entry_hidden(index, cx) {
                let visible = !matches!(entry, StreamEntry::RunActivitySegment { group, segment } if group.read(cx).segment_row_count(*segment, cx) == 0);
                if visible
                    && !(source.tail.is_some()
                        && matches!(entry, StreamEntry::ContextCompaction { restored: true, .. }))
                {
                    source.tail = Some(index);
                }
            }
            match entry {
                StreamEntry::Assistant {
                    failure: Some(failure),
                    ..
                } => {
                    if source.failure.is_none() || *failure != RunFailureKind::Persisted {
                        source.failure = Some(*failure);
                    }
                }
                StreamEntry::RunActivity { group } => source.status = Some(group.read(cx).status()),
                StreamEntry::ContextCompaction { record, .. }
                    if record.status
                        == vega_conversation::types::ContextCompactionStatus::Failed =>
                {
                    source.compaction_failure = Some(record.failure);
                    if record.failure
                        == Some(
                            vega_conversation::types::ContextCompactionFailureCode::ReasoningLimit,
                        )
                        && matches!(source.failure, None | Some(RunFailureKind::Persisted))
                    {
                        source.failure = Some(RunFailureKind::ReasoningLimit(None));
                    }
                }
                StreamEntry::Summary { card }
                    if !matches!(
                        source.status,
                        Some(RunActivityStatus::Failed | RunActivityStatus::Interrupted)
                    ) =>
                {
                    source.status = match card.read(cx).summary().outcome {
                        TaskSummaryOutcome::Failed => Some(RunActivityStatus::Failed),
                        TaskSummaryOutcome::Interrupted => Some(RunActivityStatus::Interrupted),
                        _ => source.status,
                    };
                }
                _ => {}
            }
        }
        for (message_id, failure) in &self.run_failures {
            if let Some(source) = sources.get_mut(message_id) {
                source.failure = Some(*failure);
            }
        }
        sources
            .into_iter()
            .filter_map(|(message_id, source)| {
                let kind = if source.compaction_failure.is_some()
                    && matches!(source.failure, None | Some(RunFailureKind::Persisted))
                {
                    Some(RunNoticeKind::CompactionFailure(
                        source.compaction_failure.flatten(),
                    ))
                } else if let Some(failure) = source.failure {
                    Some(
                        if let Some(compaction) = source.compaction_failure.filter(|code| !matches!((code, failure),
                            (Some(vega_conversation::types::ContextCompactionFailureCode::OverLimit), RunFailureKind::ContextOverLimit)
                            | (Some(vega_conversation::types::ContextCompactionFailureCode::ReasoningLimit), RunFailureKind::ReasoningLimit(_)))) {
                            RunNoticeKind::FailureWithCompaction(failure, compaction)
                        } else if failure == RunFailureKind::ContextOverLimit
                            || (failure == RunFailureKind::Persisted
                                && self.compaction_reports_run_failure(
                                    &message_id,
                                    RunFailureKind::ContextOverLimit,
                                ))
                        {
                            RunNoticeKind::ContextFailure
                        } else {
                            RunNoticeKind::Failure(failure)
                        },
                    )
                } else if let Some((_, attempt, max_retries)) =
                    self.network_retry.as_ref().filter(|(id, _, _)| {
                        id == &message_id && source.status == Some(RunActivityStatus::Running)
                    })
                {
                    Some(RunNoticeKind::Retrying {
                        attempt: *attempt,
                        max_retries: *max_retries,
                    })
                } else {
                    match source.status {
                        Some(RunActivityStatus::Failed) => {
                            Some(RunNoticeKind::Failure(RunFailureKind::Persisted))
                        }
                        _ => None,
                    }
                }?;
                Some((source.tail?, RunNotice { message_id, kind }))
            })
            .collect()
    }

    pub(crate) fn refresh_run_notices(&mut self, cx: &App) {
        let notices = self.run_notice_projection(cx);
        let changed = self
            .run_notices
            .keys()
            .chain(notices.keys())
            .copied()
            .filter(|index| self.run_notices.get(index) != notices.get(index))
            .collect::<HashSet<_>>();
        self.run_notices = notices;
        for index in changed {
            self.invalidate_item(Some(index));
        }
    }

    pub(crate) fn controller_error_is_run_notice(&self) -> bool {
        self.controller_runtime_error
            .as_ref()
            .is_some_and(|(message_id, failure)| {
                self.controller_error.as_deref() == Some(failure.message().as_str())
                    && (self.compaction_reports_run_failure(message_id, *failure)
                        || self.run_notices.values().any(|notice| {
                            notice.message_id == *message_id
                                && (notice.kind == RunNoticeKind::Failure(*failure)
                                    || matches!(notice.kind, RunNoticeKind::FailureWithCompaction(actual, _) if actual == *failure)
                                    || matches!(
                                        (&notice.kind, failure),
                                        (
                                            RunNoticeKind::Failure(RunFailureKind::ReasoningLimit(
                                                _
                                            )),
                                            RunFailureKind::ReasoningLimit(_)
                                        )
                                    )
                                    || (*failure == RunFailureKind::ContextOverLimit
                                        && notice.kind == RunNoticeKind::ContextFailure))
                        }))
            })
    }
}
