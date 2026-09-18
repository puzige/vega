use super::*;

fn timeline(stream: &ConversationStream) -> Vec<String> {
    stream
        .entries
        .iter()
        .map(|entry| match entry {
            StreamEntry::Assistant { model, failure, .. } => {
                let text: String = model
                    .committed_lines
                    .iter()
                    .chain(model.pending_lines.iter())
                    .flat_map(|line| line.spans.iter())
                    .map(|span| span.text.as_str())
                    .collect();
                if failure.is_some() {
                    format!("failed:{text}")
                } else {
                    text
                }
            }
            StreamEntry::Tool { card } => stream
                .tool_cards
                .iter()
                .find_map(|(id, owned)| (owned == card).then_some(id.clone()))
                .unwrap_or_else(|| "unknown-tool".into()),
            StreamEntry::User { .. } => "user".into(),
            StreamEntry::Summary { .. } => "summary".into(),
            StreamEntry::Plan { .. } => "plan".into(),
            StreamEntry::Artifact { .. } => "artifact".into(),
            StreamEntry::Permission { .. } => "permission".into(),
        })
        .collect()
}

#[gpui_kit::test]
async fn r70_live_mounted_text_and_tools_keep_proposal_order(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "r70-live");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "m".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "甲".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("a", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "乙".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("b", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("c", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "b".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "b".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "ok".into(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "丙".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["甲", "a", "乙", "b", "c", "丙"]
    );
    assert_eq!(stream.read_with(cx, |stream, _| stream.tool_cards.len()), 3);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "m".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["甲", "a", "乙", "b", "c", "丙"]
    );
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("conversation-column").is_some());
}

#[gpui_kit::test]
async fn r70_terminal_only_tool_stays_after_text(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "r70-terminal-only");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "m".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "甲".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "invalid".into(),
                result: ToolResult {
                    status: ToolCallStatus::Rejected,
                    output: String::new(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "m".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["甲", "invalid"]
    );
    assert_eq!(stream.read_with(cx, |stream, _| stream.tool_cards.len()), 1);
}

#[gpui_kit::test]
async fn r70_tool_before_first_text_has_no_empty_assistant_segment(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "r70-tool-first");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "m".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "后来".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "m".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["first", "后来"]
    );
}

#[gpui_kit::test]
async fn r70_hydrated_page_preserves_segments_and_summary(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "r70-hydrated");
    let tool = |seq: i64, id: &str| HistoryEntry::Tool {
        seq,
        message_id: "m".into(),
        call_id: id.into(),
        status: ToolCallStatus::Success,
        approval: None,
        input: None,
        result: None,
    };
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "问"),
                    HistoryEntry::AssistantText {
                        seq: 2,
                        message_id: "m".into(),
                        content: "甲".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                    },
                    tool(1, "a"),
                    HistoryEntry::AssistantText {
                        seq: 2,
                        message_id: "m".into(),
                        content: "乙".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                    },
                    tool(2, "b"),
                    tool(3, "c"),
                    HistoryEntry::AssistantText {
                        seq: 2,
                        message_id: "m".into(),
                        content: "丙".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                    },
                    hydration_summary("m"),
                ],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["user", "甲", "a", "乙", "b", "c", "丙", "summary"]
    );
}

#[gpui_kit::test]
async fn r70_hydrated_failure_after_last_tool_is_at_tail(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "r70-failed-tail");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    HistoryEntry::AssistantText {
                        seq: 1,
                        message_id: "m".into(),
                        content: "甲".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                    },
                    HistoryEntry::Tool {
                        seq: 1,
                        message_id: "m".into(),
                        call_id: "a".into(),
                        status: ToolCallStatus::Failed,
                        approval: None,
                        input: None,
                        result: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 1,
                        message_id: "m".into(),
                        content: String::new(),
                        status: vega_conversation::history::AssistantStatus::Failed,
                    },
                ],
                None,
            ),
            cx,
        )
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["甲", "a", "failed:"]
    );
}

#[gpui_kit::test]
async fn r70_page_prepend_during_tool_gap_keeps_followup_text(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "r70-prepend");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live".into(),
                seq: 4,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "甲".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("tool", "pwd"),
            },
            cx,
        );
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "旧问题"),
                    hydration_assistant(2, "旧回答"),
                ],
                Some(1),
            ),
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "乙".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "live".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| timeline(stream)),
        ["user", "旧回答", "甲", "tool", "乙"]
    );
}
