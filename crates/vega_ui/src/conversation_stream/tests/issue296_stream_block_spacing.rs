use super::*;
use gpui_kit::VisualTestContext;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "spacing".into(),
            seq: 1,
        },
        cx,
    );
}

fn prose(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "spacing".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn tool(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
}

fn response(
    stream: &mut ConversationStream,
    id: &str,
    start: usize,
    end: usize,
    kind: vega_conversation::types::AssistantResponseKind,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: "spacing".into(),
            response: vega_conversation::types::AssistantResponseMetadata {
                response_id: id.into(),
                text_start_bytes: start,
                text_end_bytes: end,
                kind,
            },
        },
        cx,
    );
}

#[gpui_kit::test]
async fn issue296_spacing_b01_first_plain_tool_uses_clock_inset(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "spacing-first-tool");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        tool(stream, "first", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let divider = visual.debug_bounds("run-activity-divider").unwrap();
    let first = visual.debug_bounds("tool-activity-single-row").unwrap();
    assert_eq!(first.top() - divider.bottom(), px(16.));
    assert_eq!(first.size.height, px(25.));
    stream.read_with(&visual, |stream, cx| {
        let inset = stream.stage_top_insets(cx)[&1];
        let StreamEntry::RunActivitySegment { group, segment } = &stream.entries[1] else {
            panic!("first operation must be a segment");
        };
        assert_eq!(
            group
                .read(cx)
                .estimated_segment_height_with_top(*segment, 736., inset, cx),
            41.
        );
    });
}

#[gpui_kit::test]
async fn issue296_spacing_b02_prose_to_plain_tools_matches_stage_inset(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "spacing-prose-tool");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        prose(stream, "First explanation.", cx);
        tool(stream, "first", cx);
        tool(stream, "second", cx);
        prose(stream, "Second explanation.", cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "spacing".into(),
                delta: "Next stage".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let first_prose = visual.debug_bounds("assistant-body-1").unwrap();
    let tool_stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    let second_prose = visual.debug_bounds("assistant-body-3").unwrap();
    let next_stage = visual.debug_bounds("run-stage-toggle-1").unwrap();
    assert_eq!(tool_stage.top() - first_prose.bottom(), px(24.));
    assert_eq!(next_stage.top() - second_prose.bottom(), px(24.));
    assert_eq!(second_prose.top() - tool_stage.bottom(), px(0.));
    stream.read_with(&visual, |stream, cx| {
        let inset = stream.stage_top_insets(cx)[&2];
        let StreamEntry::RunActivitySegment { group, segment } = &stream.entries[2] else {
            panic!("plain tools must share a segment");
        };
        assert_eq!(
            group
                .read(cx)
                .estimated_segment_height_with_top(*segment, 736., inset, cx),
            49.
        );
    });
    visual.simulate_click(tool_stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    let first_tool = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second_tool = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    let second_prose = visual.debug_bounds("assistant-body-3").unwrap();
    assert!(tool_stage.bottom() <= first_tool.top());
    assert!(first_tool.bottom() <= second_tool.top());
    assert!(second_tool.bottom() <= second_prose.top());
}

#[gpui_kit::test]
async fn issue296_spacing_b03_final_details_keep_prose_tool_boundaries(cx: &mut TestAppContext) {
    use vega_conversation::types::AssistantResponseKind;
    let (window, stream, _) = open_controller_stream(cx, "spacing-final");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        prose(stream, "A", cx);
        response(stream, "a", 0, 1, AssistantResponseKind::Commentary, cx);
        tool(stream, "first", cx);
        tool(stream, "first-next", cx);
        prose(stream, "B", cx);
        response(stream, "b", 1, 2, AssistantResponseKind::Commentary, cx);
        tool(stream, "second", cx);
        prose(stream, "OK", cx);
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.list.pause_following_tail();
        response(stream, "final", 2, 4, AssistantResponseKind::Final, cx);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "spacing".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.resume_tail(cx);
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-content").is_none());
    assert!(visual.debug_bounds("assistant-body-5").is_some());
    stream.read_with(&visual, |stream, cx| {
        let group = &stream.run_activity_groups["spacing"];
        assert_eq!(stream.run_details(group, cx).len(), 5);
        let text = stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "ABOK");
    });
    open_run_details(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let process_selectors = stream.read_with(&visual, |stream, cx| {
        stream
            .run_details(&stream.run_activity_groups["spacing"], cx)
            .into_iter()
            .filter_map(|(_, detail)| match detail {
                phase::RunDetail::Assistant(index) => match &stream.entries[index] {
                    StreamEntry::Assistant { copy, .. } => {
                        Some(format!("run-process-message-{}", copy.id))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let first = visual.debug_bounds("run-detail-row-1").unwrap();
    let same_group = visual.debug_bounds("run-detail-row-2").unwrap();
    let second = visual.debug_bounds("run-detail-row-4").unwrap();
    assert_eq!(first.size.height, px(49.));
    assert_eq!(same_group.size.height, px(25.));
    assert_eq!(second.size.height, px(49.));
    assert_eq!(
        visual
            .debug_bounds(Box::leak(process_selectors[0].clone().into_boxed_str()))
            .unwrap()
            .bottom(),
        first.top()
    );
    assert_eq!(first.bottom(), same_group.top());
    assert_eq!(
        visual
            .debug_bounds(Box::leak(process_selectors[1].clone().into_boxed_str()))
            .unwrap()
            .bottom(),
        second.top()
    );
    let header = visual.debug_bounds("run-activity-toggle").unwrap();
    visual.simulate_click(header.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-activity-content").is_none());
    assert!(visual.debug_bounds("assistant-body-5").is_some());
}

#[gpui_kit::test]
async fn issue296_spacing_b04_hydrated_plain_tool_matches_live_boundary(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "spacing-hydrated");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    HistoryEntry::AssistantText {
                        seq: 1,
                        message_id: "spacing".into(),
                        content: "Before tool".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: None,
                    },
                    HistoryEntry::Tool {
                        seq: 1,
                        message_id: "spacing".into(),
                        call_id: "history-tool".into(),
                        status: ToolCallStatus::Success,
                        approval: None,
                        input: None,
                        result: None,
                    },
                ],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let prose = visual.debug_bounds("assistant-body-1").unwrap();
    let operation = visual.debug_bounds("tool-activity-single-row").unwrap();
    assert_eq!(operation.top() - prose.bottom(), px(24.));
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(stream.stage_top_insets(cx)[&2], 24.);
        let geometry = stream.message_anchor_geometry(cx, 600.);
        assert_eq!(geometry.entry_heights[2], 49.);
    });
}

#[gpui_kit::test]
async fn issue296_spacing_b04_standalone_tool_after_prose_has_one_outer_inset(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "spacing-standalone");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        prose(stream, "Before delayed tool", cx);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "spacing".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        tool(stream, "delayed", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let index = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Tool { .. }))
            .expect("tool without a live run");
        assert_eq!(stream.stage_top_insets(cx)[&index], 24.);
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights[index],
            49.
        );
    });
    let prose = visual.debug_bounds("assistant-body-1").unwrap();
    let operation = visual.debug_bounds("tool-activity-single-row").unwrap();
    assert_eq!(operation.top() - prose.bottom(), px(24.));
}

#[gpui_kit::test]
async fn issue296_spacing_b04_standalone_group_after_prose_has_one_outer_inset(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "spacing-standalone-group");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        prose(stream, "Before delayed tools", cx);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "spacing".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        tool(stream, "delayed-first", cx);
        tool(stream, "delayed-second", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let index = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::ToolGroup { .. }))
            .expect("tools without a live run must form one group");
        assert_eq!(stream.stage_top_insets(cx)[&index], 24.);
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights[index],
            49.
        );
    });
    let prose = visual.debug_bounds("assistant-body-1").unwrap();
    let group = visual.debug_bounds("tool-activity-group-toggle").unwrap();
    assert_eq!(group.top() - prose.bottom(), px(24.));
}

#[gpui_kit::test]
async fn issue296_spacing_b04_artifact_only_segment_keeps_its_original_height(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::AssistantResponseKind;

    for secondary_button in [false, true] {
        let repo = tempfile::tempdir().unwrap();
        let workspace = Arc::new(vega_conversation::GitWorkspaceService::new(repo.path()).unwrap());
        let service =
            vega_conversation::ArtifactService::new(workspace, "p".into(), "t".into(), 1).unwrap();
        let call = ToolCall {
        id: "c".into(),
        tool: "write".into(),
        input_json: serde_json::json!({"audit_version":"write_edit_v1","tool":"write","path":"synthetic.txt","content_bytes":1,"fingerprint_v1":"a".repeat(64)}).to_string(),
    };
        let projection = service
        .capture(
            &call,
            &ToolResult {
                status: ToolCallStatus::Success,
                output: serde_json::json!({"path":"synthetic.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}).to_string(),
                reused: false,
                exit_code: None,
                duration_ms: None,
                truncated: Some(false),
                invalid: None,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact = cx.new(|cx| ArtifactCard::new("t".into(), "p".into(), projection, cx));
        let (window, stream, _) = open_controller_stream(cx, "spacing-artifact");
        stream.update(cx, |stream, cx| {
            begin(stream, cx);
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            assert!(stream.apply_artifact_card("c", artifact.clone(), cx));
            prose(stream, "OK", cx);
        });
        cx.run_until_parked();
        let focus = window
            .update(cx, |_, window, cx| {
                let first = artifact.read(cx).focus_handle(cx);
                first.focus(window, cx);
                if secondary_button {
                    for _ in 0..20 {
                        window.focus_next(cx);
                        if artifact.read(cx).contains_focus(window, cx) && !first.is_focused(window)
                        {
                            break;
                        }
                    }
                    assert!(artifact.read(cx).contains_focus(window, cx));
                    assert!(!first.is_focused(window));
                }
                window.focused(cx).expect("artifact button focus")
            })
            .unwrap();
        stream.update(cx, |stream, cx| {
            response(stream, "final", 0, 2, AssistantResponseKind::Final, cx);
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "spacing".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: None,
                },
                cx,
            );
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            assert!(focus.is_focused(window));
            assert!(artifact.read(cx).contains_focus(window, cx));
        });
        let expected = stream.read_with(&visual, |stream, cx| {
            let group = &stream.run_activity_groups["spacing"];
            assert!(group.read(cx).process_in_details());
            let (index, segment) = stream
                .entries
                .iter()
                .enumerate()
                .find_map(|(index, entry)| match entry {
                    StreamEntry::RunActivitySegment { segment, .. } => Some((index, *segment)),
                    _ => None,
                })
                .unwrap();
            let expected = artifact.read(cx).row_count() as f32 * ROW_HEIGHT;
            let inset = stream.stage_top_insets(cx)[&index];
            assert_eq!(
                group
                    .read(cx)
                    .estimated_segment_height_with_top(segment, 600., inset, cx),
                expected
            );
            expected
        });
        assert_eq!(
            visual
                .debug_bounds("run-timeline-segment-0")
                .unwrap()
                .size
                .height,
            px(expected)
        );
    }
}
