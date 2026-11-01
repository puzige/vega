use super::*;
use gpui_kit::{TestAppContext, WindowHandle};
use vega_conversation::history::AssistantStatus;
use vega_conversation::types::{
    ContextCompactionStatus, ContextCompactionStatusRecord, ContextCompactionUsageState,
    ConversationStopReason, SubagentSnapshot, SubagentState, SummaryCost, TaskSummaryOutcome,
    ThreadStatus,
};

const SENT: i64 = 1_793_492_000_000;

fn open_stream(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    cx.add_window(|_, cx| {
        ConversationStream::new(
            Thread {
                reasoning_preferences: None,
                parent_thread_id: None,
                forked_from_thread_id: None,
                forked_from_message_id: None,
                id: "conversation".into(),
                backend: TaskBackend::Native,
                project_id: String::new(),
                title: String::new(),
                mode: ThreadMode::Execute,
                permission_mode: PermissionMode::Confirm,
                permission_revision: 0,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "model".into(),
                status: ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: SENT,
                updated_at: SENT,
            },
            cx,
        )
    })
}

fn timestamp(owner: &str, value: Option<i64>) -> HistoryEntry {
    HistoryEntry::MessageTimestamp {
        message_id: owner.into(),
        sent_at_ms: value,
    }
}

fn user(owner: &str, seq: i64) -> HistoryEntry {
    HistoryEntry::UserText {
        message_id: owner.into(),
        seq,
        content: "question".into(),
    }
}

fn assistant(owner: &str, seq: i64) -> HistoryEntry {
    HistoryEntry::AssistantText {
        message_id: owner.into(),
        seq,
        content: "answer".into(),
        status: AssistantStatus::Done,
        execution_duration_ms: Some(8_000),
    }
}

fn summary(owner: &str, outcome: TaskSummaryOutcome) -> TaskCostSummary {
    TaskCostSummary {
        message_id: owner.into(),
        outcome,
        usage: None,
        cost: SummaryCost::Unavailable,
        duration_ms: Some(8_000),
        tool_count: 0,
        cache_hit_percent: None,
    }
}

fn page(entries: Vec<HistoryEntry>) -> HistoryPage {
    HistoryPage {
        interrupted_tail: None,
        subagents: Vec::new(),
        entries,
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(4),
    }
}

fn initial() -> Vec<HistoryEntry> {
    vec![
        timestamp("first", None),
        user("first", 1),
        timestamp("reply", Some(SENT)),
        assistant("reply", 2),
    ]
}

fn next_user(gap: i64) -> Vec<HistoryEntry> {
    vec![timestamp("next", Some(SENT + gap)), user("next", 3)]
}

fn prefix_owners(stream: &ConversationStream) -> Vec<&str> {
    stream
        .entry_identities
        .iter()
        .filter(|identity| stream.timestamps.prefixes.contains_key(&identity.key))
        .filter_map(|identity| identity.message_id.as_deref())
        .collect()
}

fn assert_next(stream: &ConversationStream, expected: bool) {
    assert_eq!(
        prefix_owners(stream),
        if expected { vec!["next"] } else { vec![] }
    );
    if expected {
        let index = stream
            .entry_identities
            .iter()
            .position(|identity| identity.message_id.as_deref() == Some("next"))
            .unwrap();
        assert!(stream.timestamp_prefix(index, vega_theme::LIGHT).is_some());
    }
    for (index, entry) in stream.entries.iter().enumerate() {
        if matches!(
            entry,
            StreamEntry::Summary { .. } | StreamEntry::Subagents { .. }
        ) {
            assert!(stream.timestamp_prefix(index, vega_theme::LIGHT).is_none());
        }
    }
}

#[gpui_kit::test]
fn finished_reply_with_hidden_summary_preserves_strict_hour_gap(cx: &mut TestAppContext) {
    for (gap, expected) in [
        (3_599_999, false),
        (3_600_000, false),
        (3_600_001, true),
        (7_320_611, true),
    ] {
        let handle = open_stream(cx);
        handle
            .update(cx, |stream, window, cx| {
                stream
                    .apply_history_page(page(vec![timestamp("first", None), user("first", 1)]), cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "reply".into(),
                        seq: 2,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::MessageTimestamp {
                        message_id: "reply".into(),
                        sent_at_ms: Some(SENT),
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::TextDelta {
                        message_id: "reply".into(),
                        delta: "answer".into(),
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::MessageFinished {
                        message_id: "reply".into(),
                        stop_reason: ConversationStopReason::End,
                        execution_duration_ms: Some(8_000),
                    },
                    cx,
                );
                stream.apply_task_summary(summary("reply", TaskSummaryOutcome::Completed), cx);
                assert!(stream.active_agent_message.is_none());
                assert!(
                    stream
                        .entries
                        .iter()
                        .any(|entry| matches!(entry, StreamEntry::Summary { .. })
                            && entry.row_count(cx) == 0)
                );
                stream.apply_event(
                    ConversationEvent::PendingMessageDelivered {
                        pending_id: "pending".into(),
                        message_id: "next".into(),
                        seq: 3,
                        content: "question".into(),
                        images: Vec::new(),
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::MessageTimestamp {
                        message_id: "next".into(),
                        sent_at_ms: Some(SENT + gap),
                    },
                    cx,
                );
                for _ in 0..2 {
                    stream.timestamps.dirty = true;
                    stream.refresh_timestamps(window, cx);
                    assert_next(stream, expected);
                }
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn restored_hidden_summary_preserves_user_and_assistant_thresholds(cx: &mut TestAppContext) {
    for (is_user, threshold) in [(true, 3_600_000), (false, 600_000)] {
        for (gap, expected) in [
            (threshold - 1, false),
            (threshold, false),
            (threshold + 1, true),
        ] {
            let handle = open_stream(cx);
            handle
                .update(cx, |stream, window, cx| {
                    let mut entries = initial();
                    entries.push(HistoryEntry::Summary {
                        seq: 2,
                        summary: summary("reply", TaskSummaryOutcome::Completed),
                    });
                    entries.push(timestamp("next", Some(SENT + gap)));
                    entries.push(if is_user {
                        user("next", 3)
                    } else {
                        assistant("next", 3)
                    });
                    stream.apply_history_page(page(entries), cx);
                    stream.refresh_timestamps(window, cx);
                    assert_next(stream, expected);
                })
                .unwrap();
        }
    }
}

#[gpui_kit::test]
fn pagination_and_subagent_insertion_do_not_erase_summary_gap(cx: &mut TestAppContext) {
    let handle = open_stream(cx);
    handle
        .update(cx, |stream, window, cx| {
            let mut entries = initial();
            entries.push(HistoryEntry::Summary {
                seq: 2,
                summary: summary("reply", TaskSummaryOutcome::Completed),
            });
            stream.apply_history_page(page(entries), cx);
            stream.apply_newer_history_page(page(next_user(7_320_611)), cx);
            for _ in 0..2 {
                stream.apply_subagent(
                    SubagentSnapshot {
                        agent_id: "review".into(),
                        parent_thread_id: "conversation".into(),
                        parent_message_id: "reply".into(),
                        message_id: "review-run".into(),
                        name: "review".into(),
                        state: SubagentState::Completed,
                        result: String::new(),
                        truncated: false,
                        checkpoint: None,
                        release_state: "released".into(),
                        ready_for_review: false,
                    },
                    cx,
                );
                stream.timestamps.dirty = true;
                stream.refresh_timestamps(window, cx);
                assert_next(stream, true);
            }
        })
        .unwrap();
}

#[gpui_kit::test]
fn visible_terminal_summaries_remain_boundaries(cx: &mut TestAppContext) {
    for outcome in [TaskSummaryOutcome::Failed, TaskSummaryOutcome::Interrupted] {
        let handle = open_stream(cx);
        handle
            .update(cx, |stream, window, cx| {
                stream.apply_history_page(page(initial()), cx);
                stream.apply_task_summary(summary("reply", outcome), cx);
                assert!(
                    stream
                        .entries
                        .iter()
                        .any(|entry| matches!(entry, StreamEntry::Summary { .. })
                            && entry.row_count(cx) > 0)
                );
                stream.apply_newer_history_page(page(next_user(7_320_611)), cx);
                stream.refresh_timestamps(window, cx);
                assert_next(stream, false);
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn hidden_summary_does_not_bridge_missing_time_or_explicit_boundaries(cx: &mut TestAppContext) {
    for boundary in 0..4 {
        let handle = open_stream(cx);
        handle
            .update(cx, |stream, window, cx| {
                let mut entries = initial();
                if boundary == 0 {
                    entries.push(timestamp("reply", None));
                }
                entries.push(HistoryEntry::Summary {
                    seq: 2,
                    summary: summary("reply", TaskSummaryOutcome::Completed),
                });
                match boundary {
                    1 => entries.push(HistoryEntry::TimestampBreakBefore {
                        message_id: "next".into(),
                    }),
                    2 => entries.push(HistoryEntry::TimestampBoundary {
                        message_id: "reply".into(),
                        text_start_bytes: 0,
                    }),
                    3 => entries.push(HistoryEntry::RunContinuation {
                        previous_assistant_id: "reply".into(),
                        user_message_id: "next".into(),
                        next_assistant_id: "continued".into(),
                    }),
                    _ => {}
                }
                entries.extend(next_user(7_320_611));
                stream.apply_history_page(page(entries), cx);
                stream.refresh_timestamps(window, cx);
                assert_next(stream, false);
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn invisible_compaction_still_blocks_comparison_after_hidden_summary(cx: &mut TestAppContext) {
    for status in [
        ContextCompactionStatus::Succeeded,
        ContextCompactionStatus::Cancelled,
    ] {
        let handle = open_stream(cx);
        handle
            .update(cx, |stream, window, cx| {
                stream.apply_history_page(page(initial()), cx);
                stream.apply_task_summary(summary("reply", TaskSummaryOutcome::Completed), cx);
                let record = ContextCompactionStatusRecord {
                    generation: 1,
                    status,
                    updated_at: SENT,
                    estimated_tokens: None,
                    input_budget: None,
                    target_tokens: None,
                    source_version: None,
                    failure: None,
                    usage: ContextCompactionUsageState::Unknown,
                };
                let index = stream.entries.len();
                stream.entries.push(StreamEntry::ContextCompaction {
                    model: "model".into(),
                    record,
                    restored: false,
                });
                let identity = stream.durable_entry_identity("reply", "compaction", Some(2));
                stream.set_entry_identity(index, identity);
                stream.list_append(index);
                assert_eq!(stream.entries[index].row_count(cx), 0);
                stream.apply_newer_history_page(page(next_user(7_320_611)), cx);
                stream.refresh_timestamps(window, cx);
                assert_next(stream, false);
            })
            .unwrap();
    }
}
