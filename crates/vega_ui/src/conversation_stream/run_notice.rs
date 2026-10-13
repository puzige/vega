use super::*;
use vega_conversation::types::TaskSummaryOutcome;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RunNoticeKind {
    Retrying { attempt: u32, max_retries: u32 },
    Failure(RunFailureKind),
    Interrupted,
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
            RunNoticeKind::Interrupted => "已中断".into(),
        }
    }

    pub(crate) fn render(&self, colors: &ThemeColors) -> AnyElement {
        if matches!(self.kind, RunNoticeKind::Retrying { .. }) {
            return gpui_kit::div()
                .id(format!("assistant-network-retry-{}", self.message_id))
                .debug_selector(|| "assistant-network-retry".into())
                .w_full()
                .min_w_0()
                .py_1()
                .text_size(px(Typography::BODY))
                .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                .text_color(colors.text_secondary)
                .aria_label(self.text())
                .child(self.text())
                .into_any_element();
        }
        let message_id = self.message_id.clone();
        let color = match self.kind {
            RunNoticeKind::Retrying { .. } => colors.text_secondary,
            RunNoticeKind::Failure(_) => colors.danger,
            RunNoticeKind::Interrupted => colors.warning,
        };
        gpui_kit::div()
            .debug_selector(move || format!("run-notice-{message_id}"))
            .w_full()
            .min_w_0()
            .py_1()
            .child(
                gpui_kit::div()
                    .when(matches!(self.kind, RunNoticeKind::Failure(_)), |row| {
                        row.debug_selector(|| "assistant-run-failure".into())
                    })
                    .when(matches!(self.kind, RunNoticeKind::Interrupted), |row| {
                        row.debug_selector(|| "task-outcome-status".into())
                    })
                    .child(render_rows::status_notice(self.text(), color, colors)),
            )
            .into_any_element()
    }

    pub(crate) fn estimated_height(&self, width: f32) -> f32 {
        if matches!(self.kind, RunNoticeKind::Retrying { .. }) {
            return Typography::BODY * Typography::BODY_LINE_HEIGHT
                + 2. * Layout::CONVERSATION_INLINE_GAP;
        }
        let content_width =
            (width - 2. * Layout::STATUS_PADDING - 2. - Layout::ICON_SIZE - Layout::STATUS_GAP)
                .max(1.);
        let chars_per_line = (content_width / (Typography::BODY * 0.56)).max(1.) as usize;
        let lines = self
            .text()
            .lines()
            .map(|line| display_width(line).div_ceil(chars_per_line).max(1))
            .sum::<usize>()
            .max(1);
        lines as f32 * Typography::BODY * Typography::BODY_LINE_HEIGHT
            + 2. * Layout::STATUS_PADDING
            + 10.
    }
}

#[derive(Default)]
struct RunNoticeSource {
    failure: Option<RunFailureKind>,
    status: Option<RunActivityStatus>,
    tail: Option<usize>,
    end: usize,
}

impl ConversationStream {
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
                if visible {
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
                let kind = if let Some(failure) = source.failure {
                    Some(RunNoticeKind::Failure(failure))
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
                        Some(RunActivityStatus::Interrupted) => Some(RunNoticeKind::Interrupted),
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
                    && self.run_notices.values().any(|notice| {
                        notice.message_id == *message_id
                            && notice.kind == RunNoticeKind::Failure(*failure)
                    })
            })
    }
}
