use super::*;
use gpui_kit::{TestAppContext, WindowHandle};
use vega_conversation::history::AssistantStatus;
use vega_conversation::types::{
    SubagentSnapshot, SubagentState, SummaryCost, TaskSummaryOutcome, ThreadStatus,
};

const START: i64 = 1_793_422_074_855;

fn stream(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    cx.add_window(|_, cx| {
        ConversationStream::new(
            Thread {
                reasoning_preferences: None,
                parent_thread_id: None,
                forked_from_thread_id: None,
                forked_from_message_id: None,
                id: "parent".into(),
                backend: TaskBackend::Native,
                project_id: String::new(),
                title: String::new(),
                mode: ThreadMode::Execute,
                permission_mode: PermissionMode::Confirm,
                permission_revision: 0,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "test-model".into(),
                status: ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: START,
                updated_at: START,
            },
            cx,
        )
    })
}

fn child(owner: &str) -> SubagentSnapshot {
    SubagentSnapshot {
        agent_id: format!("child-{owner}"),
        parent_thread_id: "parent".into(),
        parent_message_id: owner.into(),
        message_id: format!("execution-{owner}"),
        name: "review".into(),
        state: SubagentState::Completed,
        result: String::new(),
        truncated: false,
        checkpoint: None,
        release_state: "released".into(),
        ready_for_review: false,
    }
}

fn assistant(id: &str, seq: i64) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: id.into(),
        content: "reply".into(),
        status: AssistantStatus::Done,
        execution_duration_ms: Some(1),
    }
}

fn user(id: &str, seq: i64) -> HistoryEntry {
    HistoryEntry::UserText {
        seq,
        message_id: id.into(),
        content: "next".into(),
    }
}

fn time(id: &str, value: Option<i64>) -> HistoryEntry {
    HistoryEntry::MessageTimestamp {
        message_id: id.into(),
        sent_at_ms: value,
    }
}

fn page(mut entries: Vec<HistoryEntry>, subagents: Vec<SubagentSnapshot>) -> HistoryPage {
    entries.splice(0..0, [time("earlier-user", None), user("earlier-user", 0)]);
    HistoryPage {
        interrupted_tail: None,
        subagents,
        entries,
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(10),
    }
}

fn prefix_owners(stream: &ConversationStream) -> Vec<&str> {
    stream
        .entry_identities
        .iter()
        .filter(|identity| stream.timestamps.prefixes.contains_key(&identity.key))
        .filter_map(|identity| identity.message_id.as_deref())
        .collect()
}

#[gpui_kit::test]
fn historical_subagents_preserve_strict_user_gap(cx: &mut TestAppContext) {
    for (gap, expected) in [
        (3_599_999, false),
        (3_600_000, false),
        (3_600_001, true),
        (67_369_812, true),
    ] {
        let window = stream(cx);
        window
            .update(cx, |stream, window, cx| {
                stream.apply_history_page(
                    page(
                        vec![
                            time("a", Some(START)),
                            assistant("a", 1),
                            time("u", Some(START + gap)),
                            user("u", 2),
                        ],
                        vec![child("a")],
                    ),
                    cx,
                );
                assert!(
                    stream
                        .entries
                        .iter()
                        .any(|entry| matches!(entry, StreamEntry::Subagents { .. })
                            && entry.row_count(cx) == 0)
                );
                stream.refresh_timestamps(window, cx);
                assert_eq!(
                    prefix_owners(stream),
                    if expected { vec!["u"] } else { vec![] }
                );
                let prefixes = stream.timestamps.prefixes.clone();
                stream.timestamps.dirty = true;
                stream.refresh_timestamps(window, cx);
                assert_eq!(stream.timestamps.prefixes, prefixes);
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn live_subagent_insertion_keeps_existing_prefix(cx: &mut TestAppContext) {
    let window = stream(cx);
    window
        .update(cx, |stream, window, cx| {
            stream.apply_history_page(
                page(
                    vec![
                        time("a", Some(START)),
                        assistant("a", 1),
                        time("u", Some(START + 3_600_001)),
                        user("u", 2),
                    ],
                    vec![],
                ),
                cx,
            );
            stream.refresh_timestamps(window, cx);
            assert_eq!(prefix_owners(stream), vec!["u"]);
            let prefixes = stream.timestamps.prefixes.clone();
            stream.apply_subagent(child("a"), cx);
            stream.refresh_timestamps(window, cx);
            assert_eq!(stream.timestamps.prefixes, prefixes);
            stream.apply_subagent(child("a"), cx);
            stream.refresh_timestamps(window, cx);
            assert_eq!(stream.timestamps.prefixes, prefixes);
            assert_eq!(
                stream
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry, StreamEntry::Subagents { .. }))
                    .count(),
                1
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn multiple_subagent_groups_preserve_assistant_gap(cx: &mut TestAppContext) {
    for (gap, expected) in [(600_000, false), (600_001, true)] {
        let window = stream(cx);
        window
            .update(cx, |stream, window, cx| {
                stream.apply_history_page(
                    page(
                        vec![
                            time("a", Some(START)),
                            assistant("a", 1),
                            time("b", Some(START + gap)),
                            assistant("b", 2),
                            time("u", Some(START + gap + 3_600_001)),
                            user("u", 3),
                        ],
                        vec![child("a"), child("b")],
                    ),
                    cx,
                );
                stream.refresh_timestamps(window, cx);
                assert_eq!(
                    prefix_owners(stream),
                    if expected { vec!["b", "u"] } else { vec!["u"] }
                );
                assert!(
                    stream
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| matches!(entry, StreamEntry::Subagents { .. }))
                        .all(|(index, _)| stream
                            .timestamp_prefix(index, vega_theme::LIGHT)
                            .is_none())
                );
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn subagents_do_not_bridge_missing_time_or_explicit_breaks(cx: &mut TestAppContext) {
    for case in 0..4 {
        let window = stream(cx);
        window
            .update(cx, |stream, window, cx| {
                let mut entries = vec![time("a", (case != 0).then_some(START)), assistant("a", 1)];
                match case {
                    1 => entries.push(HistoryEntry::TimestampBreakBefore {
                        message_id: "u".into(),
                    }),
                    2 => entries.push(HistoryEntry::TimestampBoundary {
                        message_id: "a".into(),
                        text_start_bytes: 0,
                    }),
                    3 => entries.push(HistoryEntry::RunContinuation {
                        previous_assistant_id: "a".into(),
                        user_message_id: "u".into(),
                        next_assistant_id: "b".into(),
                    }),
                    _ => {}
                }
                entries.extend([time("u", Some(START + 3_600_001)), user("u", 2)]);
                stream.apply_history_page(page(entries, vec![child("a")]), cx);
                stream.refresh_timestamps(window, cx);
                assert!(stream.timestamps.prefixes.is_empty(), "case {case}");
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn zero_row_summary_still_breaks_comparison_after_subagents(cx: &mut TestAppContext) {
    let window = stream(cx);
    window
        .update(cx, |stream, window, cx| {
            stream.apply_history_page(
                page(
                    vec![
                        time("a", Some(START)),
                        assistant("a", 1),
                        HistoryEntry::Summary {
                            seq: 1,
                            summary: TaskCostSummary {
                                message_id: "a".into(),
                                outcome: TaskSummaryOutcome::Completed,
                                usage: None,
                                cost: SummaryCost::Unavailable,
                                duration_ms: None,
                                tool_count: 0,
                                cache_hit_percent: None,
                            },
                        },
                        time("u", Some(START + 3_600_001)),
                        user("u", 2),
                    ],
                    vec![child("a")],
                ),
                cx,
            );
            assert!(
                stream
                    .entries
                    .iter()
                    .any(|entry| matches!(entry, StreamEntry::Summary { .. })
                        && entry.row_count(cx) == 0)
            );
            stream.refresh_timestamps(window, cx);
            assert!(stream.timestamps.prefixes.is_empty());
        })
        .unwrap();
}
