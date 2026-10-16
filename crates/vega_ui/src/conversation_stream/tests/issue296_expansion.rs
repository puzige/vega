use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "expansion".into(),
            seq: 1,
        },
        cx,
    );
}

fn summary(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::SummaryDelta {
            message_id: "expansion".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn reasoning(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ThinkingDelta {
            message_id: "expansion".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn completed_command(
    stream: &mut ConversationStream,
    id: &str,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallApproved {
            call_id: id.into(),
            approval: Approval::Once,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallRunning { call_id: id.into() },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: ToolResult {
                status: ToolCallStatus::Success,
                output: String::new(),
                reused: false,
                exit_code: Some(0),
                duration_ms: Some(1),
                truncated: Some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn raw_stage_manual_open_preserves_thinking_and_tool_order(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-raw-order");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Inspecting files", cx);
        for id in ["first", "second"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "run commands");
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    let thinking = visual.debug_bounds("thinking-toggle").unwrap();
    let first = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert_eq!(first.top() - thinking.top(), px(25.));
    assert_eq!(second.top() - first.top(), px(25.));
    assert!(visual.debug_bounds("thinking-content").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
}

#[gpui_kit::test]
async fn tool_before_reasoning_keeps_arrival_order_inside_one_stage(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-tool-before-reasoning");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("second", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    stream.update(&mut visual, |stream, cx| {
        reasoning(stream, "Inspecting result", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("third", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        let order = group
            .segment_children(0)
            .iter()
            .map(|child| match child {
                RunActivityChild::Thinking(_) => "thinking",
                RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => "tool",
                RunActivityChild::Artifact(_) => "artifact",
            })
            .collect::<Vec<_>>();
        assert_eq!(order, ["tool", "thinking", "tool"]);
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
                .count(),
            1
        );
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
        assert_eq!(group.segment_row_count(0, cx), 1);
    });
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-2").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    click(&mut visual, "run-stage-toggle-0");
    let first = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    let header = visual.debug_bounds("run-stage-toggle-0").unwrap();
    let thinking = visual.debug_bounds("thinking-toggle").unwrap();
    let third = visual.debug_bounds("run-stage-tool-0-2").unwrap();
    assert_eq!(second.top() - first.top(), px(25.));
    assert!(header.bottom() <= first.top());
    assert!(second.bottom() <= thinking.top());
    assert_eq!(third.top() - thinking.top(), px(25.));
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + 5. * Layout::CONVERSATION_TOOL_HEIGHT
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_row_count(0, cx),
            5
        );
    });
}

#[gpui_kit::test]
async fn repeated_reasoning_and_commands_share_one_compact_stage(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "compact-reasoning-tools");
    let (first, second) = stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Checking the first step", cx);
        let first = stream.active_thinking.clone().unwrap();
        completed_command(stream, "first", cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: " \n\t".into(),
            },
            cx,
        );
        assert!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .test_clock()
                .2
                == run_activity::RunPhase::Thinking
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "expansion".into(),
                text_start_bytes: " \n\t".len(),
            },
            cx,
        );
        reasoning(stream, "Checking the second step", cx);
        let second = stream.active_thinking.clone().unwrap();
        completed_command(stream, "second", cx);
        (first, second)
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "run commands");
        for id in ["first", "second"] {
            let card = stream.tool_cards[id].read(cx);
            assert_eq!(card.activity_state(), crate::tool_card::ToolActivityState::Success);
            assert!(!card.needs_attention());
        }
        assert_eq!(group.compact_tool_groups(0).len(), 1);
        assert_eq!(group.compact_tool_groups(0)[0].read(cx).aggregate_summary(cx), "Completed: run commands");
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
        assert_eq!(
            group
                .segment_children(0)
                .iter()
                .map(|child| match child {
                    RunActivityChild::Thinking(_) => "thinking",
                    RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => "tool",
                    RunActivityChild::Artifact(_) => "artifact",
                })
                .collect::<Vec<_>>(),
            ["thinking", "tool", "thinking", "tool"]
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
                .count(),
            1
        );
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == " \n\t")));
    });
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
    });
    stream.read_with(&visual, |stream, cx| {
        assert!(
            !stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_expanded(0)
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    let first_thinking = visual
        .debug_bounds(Box::leak(
            format!("run-stage-source-{}", first.entity_id()).into_boxed_str(),
        ))
        .unwrap();
    let second_thinking = visual
        .debug_bounds(Box::leak(
            format!("run-stage-source-{}", second.entity_id()).into_boxed_str(),
        ))
        .unwrap();
    let first_tool = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second_tool = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert!(first_thinking.bottom() <= first_tool.top());
    assert!(first_tool.bottom() <= second_thinking.top());
    assert!(second_thinking.bottom() <= second_tool.top());
}

#[gpui_kit::test]
async fn visible_body_splits_stage_after_preserved_blank_text(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "compact-body-boundary");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Before blank text", cx);
        completed_command(stream, "first", cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: " \n".into(),
            },
            cx,
        );
        reasoning(stream, "Still before body", cx);
        completed_command(stream, "second", cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: "Visible answer.".into(),
            },
            cx,
        );
        reasoning(stream, "After body", cx);
        completed_command(stream, "third", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let stages = stream
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry {
                StreamEntry::RunActivitySegment { segment, .. } => Some((index, *segment)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].1, 0);
        assert_eq!(stages[1].1, 1);
        let body = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Visible answer."))
            .unwrap();
        assert!(stages[0].0 < body && body < stages[1].0);
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == " \n")));
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.compact_tool_groups(0).len(), 1);
        assert!(group.compact_tool_groups(1).is_empty());
    });
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-toggle-1").is_some());
    visual.update(|window, cx| {
        stream.update(cx, |stream, cx| {
            stream.resume_tail(cx);
            stream.focus_composer(window, cx);
        });
    });
    stream.update(&mut visual, |stream, cx| {
        confirm_test_final(stream, "expansion", cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        let responses = &stream.run_responses["expansion"];
        assert!(phase::process_copies(
            "expansion",
            responses,
            &stream.entries,
            &stream.entry_identities
        )
        .is_some());
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == " \n")));
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Final answer.")));
        assert!(stream.run_activity_groups["expansion"].read(cx).process_in_details());
    });
}

#[gpui_kit::test]
async fn summary_cycles_share_tools_without_duplicate_reasoning(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "compact-summary-tools");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "Checking inputs", cx);
        completed_command(stream, "first", cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: "\n".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "expansion".into(),
                text_start_bytes: 1,
            },
            cx,
        );
        summary(stream, "Running checks", cx);
        completed_command(stream, "second", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "run commands");
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.compact_tool_groups(0).len(), 1);
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
                .count(),
            1
        );
    });
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
}

#[gpui_kit::test]
async fn folded_stage_hides_running_pending_and_failed_tool_status(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "compact-tool-attention");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Running checks", cx);
        for id in ["first", "second"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: id.into(),
                    approval: Approval::Once,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallRunning { call_id: id.into() },
                cx,
            );
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "run commands");
        assert!(stream.run_notice_projection(cx).is_empty());
        assert_eq!(
            group.compact_tool_groups(0)[0]
                .read(cx)
                .aggregate_summary(cx),
            "In progress: run commands"
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("pending", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-2").is_none());
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "run commands");
        assert!(stream.run_notice_projection(cx).is_empty());
        assert!(stream.tool_cards["pending"].read(cx).needs_attention());
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "pending".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: "pending".into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "pending".into(),
                result: ToolResult {
                    status: ToolCallStatus::Failed,
                    output: "synthetic failure".into(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-2").is_none());
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    assert!(visual.debug_bounds("assistant-run-failure").is_none());
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.tool_cards["pending"].read(cx).needs_attention());
        assert!(
            stream.tool_cards["pending"]
                .read(cx)
                .visible_text()
                .contains("Command failed")
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_row_count(0, cx),
            1
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "run commands"
        );
        assert!(stream.run_notice_projection(cx).is_empty());
    });
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-tool-0-2").is_some());
}

#[gpui_kit::test]
async fn tool_before_reasoning_keeps_its_position_when_compacted(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "compact-tool-first");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        completed_command(stream, "first", cx);
        reasoning(stream, "Checking the command", cx);
        completed_command(stream, "second", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    let first = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let header = visual.debug_bounds("run-stage-toggle-0").unwrap();
    let thinking = visual.debug_bounds("thinking-toggle").unwrap();
    let second = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert!(header.bottom() <= first.top());
    assert!(first.bottom() <= thinking.top());
    assert!(thinking.bottom() <= second.top());
}
