use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

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
async fn issue296_expansion_p06_first_stage_uses_clock_spacing(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-spacing");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "Synthetic phase", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let divider = visual.debug_bounds("run-activity-divider").unwrap();
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    assert_eq!(stage.top() - divider.bottom(), px(16.));
}

#[gpui_kit::test]
async fn issue343_t9_summary_hides_tools_until_stage_opens(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-rows");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "**Synthetic phase**\n\nSummary continuation.", cx);
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
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    let first = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert_eq!(second.top() - first.top(), px(25.));
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("stage-thinking-content").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
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
        assert_eq!(group.stage_title(0, cx), "Commands");
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
async fn issue296_expansion_p07_final_opens_flat_tools(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-final");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "**Synthetic phase**", cx);
        for id in ["first", "second"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
        confirm_test_final(stream, "expansion", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "run-activity-toggle");
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
}

#[gpui_kit::test]
async fn issue296_expansion_p07_markdown_selection_and_original_copy(cx: &mut TestAppContext) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    let original =
        "**Synthetic phase**\n\nAlpha你好 😀 suffix\n\n[Safe label](https://example.com)";
    let card = stream.update(&mut visual, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, original, cx);
        stream.active_thinking.clone().unwrap()
    });
    visual.run_until_parked();
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    click(&mut visual, "run-stage-toggle-0");
    assert!(card.read_with(&visual, |card, _| card.expanded));
    let copy = card.read_with(&visual, |card, _| card.test_source_copy());
    assert_eq!(copy.original_text(), original);
    assert_eq!(
        copy.visible_text().as_deref(),
        Some("Synthetic phase\n\nAlpha你好 😀 suffix\n\nSafe label")
    );
    let text = "Alpha你好 😀 suffix";
    let layout = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .rev()
            .find(|(value, _, _)| value == text)
            .unwrap()
            .1
            .clone()
    });
    let start =
        layout.position_for_index(0).unwrap() + gpui_kit::point(px(0.5), layout.line_height() / 2.);
    let end = layout
        .position_for_index(text.find(" suffix").unwrap())
        .unwrap()
        + gpui_kit::point(px(2.), layout.line_height() / 2.);
    visual.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
    assert!(copy.has_selected_text());
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Alpha你好 😀")
    );
    visual.update(|_, cx| copy.copy(cx));
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(original)
    );
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-content").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-content").is_some());
    assert!(card.read_with(&visual, |card, _| card.expanded));
}

#[gpui_kit::test]
async fn issue296_expansion_p06_visible_predecessor_and_estimate(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-predecessor");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: "Visible introduction.".into(),
            },
            cx,
        );
        summary(stream, "First stage after prose", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let prose = visual.debug_bounds("assistant-body-1").unwrap();
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    assert_eq!(stage.top() - prose.bottom(), px(24.));
    stream.read_with(&visual, |stream, cx| {
        let insets = stream.stage_top_insets(cx);
        assert_eq!(insets[&2], 24.);
        let group = &stream.run_activity_groups["expansion"];
        assert_eq!(
            group
                .read(cx)
                .estimated_segment_height_with_top(0, 736., insets[&2], cx),
            49.
        );
    });
    stream.update(&mut visual, |stream, cx| {
        if let StreamEntry::Assistant { model, .. } = &mut stream.entries[1] {
            *model = StreamModel::default();
        }
        let group = stream.run_activity_groups["expansion"].clone();
        let empty = group.update(cx, |group, cx| group.start_segment(cx));
        stream.entries.insert(
            2,
            StreamEntry::RunActivitySegment {
                group,
                segment: empty,
            },
        );
        stream.list_insert(2);
        stream.set_entry_identity(
            2,
            StreamEntryIdentity {
                key: "empty-derived".into(),
                message_id: Some("expansion".into()),
                sequence: None,
            },
        );
        assert_eq!(stream.stage_top_insets(cx)[&3], 16.);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "expansion".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "second".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "second".into(),
                delta: "Second run".into(),
            },
            cx,
        );
        let index = stream.entries.len() - 1;
        assert_eq!(stream.stage_top_insets(cx)[&index], 16.);
    });
}

#[gpui_kit::test]
async fn issue296_expansion_p07_bounded_details_independent_reading_and_tail(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-bounded");
    let card = stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(
            stream,
            &format!(
                "**Synthetic phase**\n\n{}",
                "Long reasoning paragraph.\n\n".repeat(30)
            ),
            cx,
        );
        let card = stream.active_thinking.clone().unwrap();
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("tool-{index}"), "pwd"),
                },
                cx,
            );
        }
        card
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "run-stage-toggle-0");
    let details = visual.debug_bounds("run-stage-content-0").unwrap();
    assert_eq!(details.size.height, px(320.));
    assert!(!card.read_with(&visual, |card, _| card.expanded));
    click(&mut visual, "thinking-toggle");
    let source = visual.debug_bounds("thinking-content").unwrap();
    assert_eq!(source.size.height, px(240.));
    let scroll = card.read_with(&visual, |card, _| card.scroll_handle());
    assert!(scroll.max_offset().y > px(0.));
    scroll.set_offset(gpui_kit::point(px(0.), px(-40.)));
    card.update(&mut visual, |card, cx| {
        card.append("\n\nMore reasoning.", false, usize::MAX);
        cx.notify();
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-40.));
    click(&mut visual, "run-stage-toggle-0");
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    stream.update(&mut visual, |stream, cx| {
        confirm_test_final(stream, "expansion", cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    click(&mut visual, "run-activity-toggle");
    assert!(card.read_with(&visual, |card, _| card.expanded));
    assert_eq!(
        visual.debug_bounds("thinking-content").unwrap().size.height,
        px(240.)
    );
    assert_eq!(
        visual
            .debug_bounds("run-activity-content")
            .unwrap()
            .size
            .height,
        px(320.)
    );
}

#[gpui_kit::test]
async fn issue296_expansion_p07_artifact_splits_sections_in_source_order(cx: &mut TestAppContext) {
    let repo = tempfile::tempdir().unwrap();
    let workspace =
        std::sync::Arc::new(vega_conversation::GitWorkspaceService::new(repo.path()).unwrap());
    let service =
        vega_conversation::ArtifactService::new(workspace, "p".into(), "t".into(), 1).unwrap();
    let call = ToolCall { id: "c".into(), tool: "write".into(), input_json: serde_json::json!({"audit_version":"write_edit_v1","tool":"write","path":"synthetic.txt","content_bytes":1,"fingerprint_v1":"a".repeat(64)}).to_string() };
    let projection = service.capture(&call, &ToolResult { status: ToolCallStatus::Success, output: serde_json::json!({"path":"synthetic.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}).to_string(), reused:false, exit_code:None, duration_ms:None, truncated:Some(false), invalid:None }, CancellationToken::new()).await.unwrap().unwrap();
    let artifact = cx
        .new(|cx| crate::artifact_card::ArtifactCard::new("t".into(), "p".into(), projection, cx));
    let selector = Box::leak(format!("run-artifact-{}", artifact.entity_id()).into_boxed_str());
    let (window, stream, _) = open_controller_stream(cx, "expansion-artifact");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Synthetic phase", cx);
        stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        assert!(stream.apply_artifact_card("c", artifact.clone(), cx));
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("later", "pwd"),
            },
            cx,
        );
        assert!(stream.artifact_card_is_adjacent("c", cx));
        assert!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .compact_tool_groups(0)
                .is_empty()
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds(selector).is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        let artifact_rows = artifact.read(cx).row_count();
        assert_eq!(group.segment_row_count(0, cx), 1 + artifact_rows);
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP
                + Layout::CONVERSATION_TOOL_HEIGHT
                + artifact_rows as f32 * ROW_HEIGHT
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    let first = visual.debug_bounds("run-stage-content-0").unwrap();
    let middle = visual.debug_bounds(selector).unwrap();
    let last = visual.debug_bounds("run-stage-content-0-1").unwrap();
    assert_eq!(first.size.height, px(50.));
    assert_eq!(last.size.height, px(25.));
    assert_eq!(first.bottom(), middle.top());
    assert_eq!(middle.bottom(), last.top());
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights[1],
            16. + 25. + 50. + 2. * ROW_HEIGHT + 25.
        );
    });
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds(selector).is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    assert!(visual.debug_bounds("run-stage-content-0").is_none());
    assert!(visual.debug_bounds("run-stage-content-0-1").is_none());
}

#[gpui_kit::test]
async fn issue296_expansion_p06_empty_prose_keeps_first_stage_and_copy(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-empty-prose");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "Prior question"),
                    hydration_assistant(2, "Prior answer"),
                ],
                None,
            ),
            cx,
        );
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: "\n\n".into(),
            },
            cx,
        );
        summary(stream, "Synthetic phase", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let divider = visual.debug_bounds("run-activity-divider").unwrap();
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    assert_eq!(stage.top() - divider.bottom(), px(16.));
    stream.read_with(&visual, |stream, cx| {
        let (index, copy) = stream
            .entries
            .iter()
            .enumerate()
            .find_map(|(index, entry)| match entry {
                StreamEntry::Assistant { copy, .. } if copy.original_text() == "\n\n" => {
                    Some((index, copy))
                }
                _ => None,
            })
            .unwrap();
        assert!(stream.process_entry_hidden(index, cx));
        assert_eq!(copy.original_text(), "\n\n");
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights[index],
            0.
        );
        assert_eq!(
            stream.entry_identities[index].message_id.as_deref(),
            Some("expansion")
        );
    });
}

#[gpui_kit::test]
async fn issue296_expansion_p07_first_render_keeps_only_terminal_notice(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-first-render");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("rejected", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "rejected".into(),
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
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let index = stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "expansion".into(),
                    delta: "New visible body.".into(),
                },
                cx,
            );
            let index = stream.entries.len() - 1;
            assert!(matches!(
                &stream.entries[index],
                StreamEntry::Assistant { model, .. } if model.row_count() == 0
            ));
            index
        });
        window.refresh();
        window.draw(cx).clear(cx);
        let stream = stream.read(cx);
        assert!(matches!(
            &stream.entries[index],
            StreamEntry::Assistant { model, .. } if model.row_count() > 0
        ));
        assert!(stream.run_notices.is_empty());
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-notice-expansion").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("expansion".into()),
                error: Arc::new(std::io::Error::other("PRIVATE_RUNTIME_SENTINEL").into()),
                execution_duration_ms: Some(100),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let notice = visual.debug_bounds("run-notice-expansion").unwrap();
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(stream.run_notices.len(), 1);
        let (index, terminal) = stream.run_notices.iter().next().unwrap();
        assert_eq!(terminal.text(), RunFailureKind::Runtime.message());
        assert_eq!(terminal.message_id, "expansion");
        assert!(matches!(&stream.entries[*index], StreamEntry::Assistant { copy, failure: Some(RunFailureKind::Runtime), .. } if copy.original_text() == "New visible body."));
        assert_eq!(stream.tool_cards["rejected"].read(cx).activity_state(), crate::tool_card::ToolActivityState::Rejected);
    });
    let body = visual.debug_bounds("assistant-body-2").unwrap();
    assert!(notice.top() >= body.bottom());
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue296_expansion_p07_empty_failure_estimate_matches_real_body(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "expansion-empty-failure");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![HistoryEntry::AssistantText {
                    seq: 1,
                    message_id: "empty-failed".into(),
                    content: String::new(),
                    status: vega_conversation::history::AssistantStatus::Failed,
                    execution_duration_ms: None,
                }],
                None,
            ),
            cx,
        )
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let body = visual.debug_bounds("assistant-body-0").unwrap();
    let notice = visual.debug_bounds("run-notice-empty-failed").unwrap();
    assert_eq!(body.size.height, px(16.));
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        let height = stream.message_anchor_geometry(cx, 600.).entry_heights[0];
        assert_eq!(
            message_anchors::estimate_stream_entry_height(
                &stream.entries[0],
                cx,
                stream.workspace_width,
                true,
            ),
            f32::from(body.size.height)
        );
        assert!((height - f32::from(body.size.height + notice.size.height)).abs() < 0.25);
        assert_eq!(stream.run_notices.len(), 1);
        assert!(matches!(
            &stream.entries[0],
            StreamEntry::Assistant {
                failure: Some(RunFailureKind::Persisted),
                ..
            }
        ));
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
        assert_eq!(group.stage_title(0, cx), "Commands");
        for id in ["first", "second"] {
            let card = stream.tool_cards[id].read(cx);
            assert_eq!(card.activity_state(), crate::tool_card::ToolActivityState::Success);
            assert!(!card.needs_attention());
        }
        assert_eq!(group.compact_tool_groups(0).len(), 1);
        assert_eq!(group.compact_tool_groups(0)[0].read(cx).aggregate_summary(cx), "已运行命令");
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
    visual.update(|window, cx| {
        let group = stream.read(cx).run_activity_groups["expansion"].read(cx);
        assert!(!group.reading(window, cx));
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
        assert_eq!(group.stage_title(0, cx), "Commands");
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
async fn issue343_t13_gpt_summary_nests_mixed_tools_and_failed_searches(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "gpt-summary-mixed-tools");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "Checking files", cx);
        let calls = [
            ToolCall {
                id: "find".into(),
                tool: "glob".into(),
                input_json: r#"{"pattern":"*.rs"}"#.into(),
            },
            ToolCall {
                id: "read".into(),
                tool: "read".into(),
                input_json: r#"{"path":"synthetic.rs"}"#.into(),
            },
            bash_call("command", "pwd"),
            ToolCall {
                id: "edit".into(),
                tool: "edit".into(),
                input_json: serde_json::json!({
                    "audit_version": "write_edit_v1",
                    "tool": "edit",
                    "path": "synthetic.rs",
                    "old_string_bytes": 3,
                    "new_string_bytes": 4,
                    "fingerprint_v1": "a".repeat(64)
                })
                .to_string(),
            },
            ToolCall {
                id: "search-first".into(),
                tool: "grep".into(),
                input_json: r#"{"pattern":"missing"}"#.into(),
            },
            ToolCall {
                id: "search-second".into(),
                tool: "grep".into(),
                input_json: r#"{"pattern":"missing"}"#.into(),
            },
            ToolCall {
                id: "search-third".into(),
                tool: "grep".into(),
                input_json: r#"{"pattern":"missing"}"#.into(),
            },
        ];
        for call in calls {
            let id = call.id.clone();
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: id.clone(),
                    approval: Approval::Once,
                },
                cx,
            );
            stream.apply_event(ConversationEvent::ToolCallRunning { call_id: id.clone() }, cx);
            let (status, output, exit_code, duration_ms, truncated) = match id.as_str() {
                "command" => (
                    ToolCallStatus::Success,
                    "synthetic result".to_owned(),
                    Some(0),
                    Some(1),
                    Some(false),
                ),
                "edit" => (
                    ToolCallStatus::Success,
                    r#"{"path":"synthetic.rs","bytes_written":4,"replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#.to_owned(),
                    None,
                    None,
                    Some(false),
                ),
                id if id.starts_with("search-") => (
                    ToolCallStatus::Failed,
                    "synthetic failure".to_owned(),
                    None,
                    None,
                    None,
                ),
                _ => (
                    ToolCallStatus::Success,
                    "synthetic result".to_owned(),
                    None,
                    None,
                    Some(false),
                ),
            };
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: id.clone(),
                    result: ToolResult {
                        status,
                        output,
                        reused: false,
                        exit_code,
                        duration_ms,
                        truncated,
                        invalid: None,
                    },
                },
                cx,
            );
        }
        summary(stream, "Reviewing results", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert!(
            group
                .stage_title(0, cx)
                .starts_with("File searches, read files")
        );
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert!(stream.run_notice_projection(cx).is_empty());
        assert_eq!(
            group.estimated_segment_height(0, 736., cx),
            Layout::CONVERSATION_STAGE_TOP + Layout::CONVERSATION_TOOL_HEIGHT
        );
    });
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    assert!(visual.debug_bounds("assistant-run-failure").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    visual.simulate_resize(gpui_kit::size(px(480.), px(800.)));
    visual.run_until_parked();
    let title = visual.debug_bounds("run-stage-title-0").unwrap();
    let chevron = visual.debug_bounds("run-stage-chevron-0").unwrap();
    assert!(title.size.width > px(0.));
    assert!(title.right() <= chevron.left());
    for index in 0..7 {
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("run-stage-tool-0-{index}").into_boxed_str()
                ))
                .is_none()
        );
    }
    click(&mut visual, "run-stage-toggle-0");
    let mut previous = visual.debug_bounds("run-stage-toggle-0").unwrap();
    for index in 0..7 {
        let row = visual
            .debug_bounds(Box::leak(
                format!("run-stage-tool-0-{index}").into_boxed_str(),
            ))
            .unwrap();
        assert!(previous.bottom() <= row.top());
        previous = row;
    }
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_row_count(0, cx),
            10
        );
        for id in ["search-first", "search-second", "search-third"] {
            assert!(stream.tool_cards[id].read(cx).needs_attention());
        }
    });
}

#[gpui_kit::test]
async fn issue343_t14_raw_thinking_stays_collapsed_after_new_tools(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "raw-thinking-follow-up-tools");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Checking input", cx);
        completed_command(stream, "first", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    click(&mut visual, "run-stage-toggle-0");
    stream.update(&mut visual, |stream, cx| {
        reasoning(stream, " and checking output", cx);
        completed_command(stream, "second", cx);
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert!(!group.segment_expanded(0));
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "Commands");
    });
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
        assert_eq!(group.stage_title(0, cx), "Commands");
        assert!(stream.run_notice_projection(cx).is_empty());
        assert_eq!(
            group.compact_tool_groups(0)[0]
                .read(cx)
                .aggregate_summary(cx),
            "正在处理：运行命令"
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
        assert_eq!(group.stage_title(0, cx), "Commands");
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
                .contains("失败")
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
            "Commands"
        );
        assert!(stream.run_notice_projection(cx).is_empty());
    });
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-tool-0-2").is_some());
}

#[gpui_kit::test]
async fn issue343_t16_permission_request_stays_outside_thinking_disclosure(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "permission-outside-thinking");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "Checking a protected command", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("permission-command", "printf protected"),
            },
            cx,
        );
    });
    let queue = stream.read_with(cx, |stream, _| stream.permission_queue());
    let decision = request_permission(&queue, "permission-command", "printf protected");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let stage = visual.debug_bounds("run-timeline-segment-0").unwrap();
    let permission = visual.debug_bounds("conversation-permission-card").unwrap();
    assert!(stage.bottom() <= permission.top());
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Permission { .. }))
                .count(),
            1
        );
        assert!(stream.active_permission.is_some());
        assert!(stream.run_notice_projection(cx).is_empty());
    });
    click(&mut visual, "run-stage-toggle-0");
    let stage = visual.debug_bounds("run-timeline-segment-0").unwrap();
    let permission = visual.debug_bounds("conversation-permission-card").unwrap();
    assert!(stage.bottom() <= permission.top());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    drop(decision);
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

#[gpui_kit::test]
async fn issue343_t17_single_thought_tracks_explicit_completion_across_body_stages(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-thought-lifecycle");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "First reasoning", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Thinking");
        assert_eq!(group.segment_row_count(0, cx), 1);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-content").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "expansion".into(),
                delta: "Visible body.".into(),
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Thought"
        );
        reasoning(stream, "Second reasoning", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Thought");
        assert_eq!(group.stage_title(1, cx), "Thinking");
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "expansion".into(),
                response: AssistantResponseMetadata {
                    response_id: "commentary".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "Visible body.".len(),
                    kind: AssistantResponseKind::Commentary,
                },
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Thought");
        assert_eq!(group.stage_title(1, cx), "Thought");
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "expansion".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(100),
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Thought");
        assert_eq!(group.stage_title(1, cx), "Thought");
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
}

#[gpui_kit::test]
async fn issue343_t18_pure_summary_aggregate_shows_latest_only_in_title(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "pure-summary-aggregate");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "First summary", cx);
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "expansion".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        reasoning(stream, "Second thought", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "First summary");
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(
            group.test_visible_segment_thinking_texts(0, cx),
            vec!["Second thought".to_owned()]
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    let second_card = stream.read_with(&visual, |stream, cx| {
        stream.run_activity_groups["expansion"]
            .read(cx)
            .segment_children(0)
            .into_iter()
            .filter_map(|child| match child {
                RunActivityChild::Thinking(card) => Some(card),
                _ => None,
            })
            .next_back()
            .unwrap()
    });
    visual.update(|window, cx| {
        second_card.read(cx).test_focus().focus(window, cx);
        assert!(second_card.read(cx).header_focused(window));
    });
    stream.update(&mut visual, |stream, cx| {
        summary(stream, "Second summary", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Second summary");
        assert_eq!(group.segment_row_count(0, cx), 2);
        assert_eq!(
            group.test_visible_segment_thinking_texts(0, cx),
            vec!["First summary".to_owned()]
        );
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(
            stream.read(cx).run_activity_groups["expansion"]
                .read(cx)
                .segment_header_focus(0)
                .is_focused(window)
        );
    });
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 2);
        assert_eq!(
            group.test_visible_segment_thinking_texts(0, cx),
            vec!["First summary".to_owned()]
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("shell", "pwd"),
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Commands");
        assert_eq!(group.segment_row_count(0, cx), 4);
        assert_eq!(
            group.test_visible_segment_thinking_texts(0, cx),
            vec!["First summary".to_owned(), "Second summary".to_owned()]
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
}

#[gpui_kit::test]
async fn issue343_t18_summary_promotes_to_bounded_tool_activity(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-tool-activity");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "Reviewing project", cx);
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Reviewing project"
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "read".into(),
                    tool: "read".into(),
                    input_json: r#"{"file_path":"/synthetic/README.md"}"#.into(),
                },
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Read files");
        assert_eq!(group.segment_row_count(0, cx), 1);
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "read".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: "read".into(),
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Read files"
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "read".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "synthetic".into(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Read files"
        );
        completed_command(stream, "shell", cx);
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Read files, commands"
        );
        summary(stream, "Updated summary", cx);
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "Read files, commands"
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_row_count(0, cx),
            1
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-icon-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    click(&mut visual, "run-stage-toggle-0");
    let summaries = stream.read_with(&visual, |stream, cx| {
        stream.run_activity_groups["expansion"]
            .read(cx)
            .segment_children(0)
            .into_iter()
            .filter_map(|child| match child {
                RunActivityChild::Thinking(card) => Some(card.entity_id()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let first_summary = visual
        .debug_bounds(Box::leak(
            format!("run-stage-source-{}", summaries[0]).into_boxed_str(),
        ))
        .unwrap();
    let latest_summary = visual
        .debug_bounds(Box::leak(
            format!("run-stage-source-{}", summaries[1]).into_boxed_str(),
        ))
        .unwrap();
    let read = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let shell = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert!(first_summary.bottom() <= read.top());
    assert!(read.bottom() <= shell.top());
    assert!(shell.bottom() <= latest_summary.top());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 5);
        assert_eq!(
            group.test_stage_tool_icon(0, cx),
            Some(crate::icons::Icon::BookOpen)
        );
    });
}

#[gpui_kit::test]
async fn issue343_t18_multiple_tool_categories_keep_first_icon_and_bounded_title(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "bounded-tool-categories");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        for call in [
            ToolCall {
                id: "read".into(),
                tool: "read".into(),
                input_json: r#"{"file_path":"/synthetic/README.md"}"#.into(),
            },
            bash_call("shell", "sensitive-value-never-in-title"),
            ToolCall {
                id: "find".into(),
                tool: "glob".into(),
                input_json: r#"{"pattern":"*.rs"}"#.into(),
            },
            ToolCall {
                id: "search".into(),
                tool: "grep".into(),
                input_json: r#"{"pattern":"private-term"}"#.into(),
            },
        ] {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        }
        let group = stream.run_activity_groups["expansion"].read(cx);
        let title = group.stage_title(0, cx);
        assert_eq!(
            title,
            "Read files, commands, file searches, content searches"
        );
        assert!(title.chars().count() <= 96);
        assert!(!title.contains("sensitive-value") && !title.contains("private-term"));
        assert_eq!(
            group.test_stage_tool_icon(0, cx),
            Some(crate::icons::Icon::BookOpen)
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "read".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: "read".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "read".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "synthetic".into(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(
            group.stage_title(0, cx),
            "Read files, commands, file searches, content searches"
        );
        assert_eq!(
            group.test_stage_tool_icon(0, cx),
            Some(crate::icons::Icon::BookOpen)
        );
    });
}

#[gpui_kit::test]
async fn issue343_t20_command_title_ignores_running_and_failed_outcomes(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "category-only-mixed-outcomes");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        completed_command(stream, "success", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("failed", "synthetic failure"),
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Commands");
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "failed".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: "failed".into(),
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Commands");
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "failed".into(),
                result: ToolResult {
                    status: ToolCallStatus::Failed,
                    output: "synthetic failure".into(),
                    reused: false,
                    exit_code: Some(1),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Commands");
        assert_eq!(
            group.test_stage_tool_icon(0, cx),
            Some(crate::icons::Icon::Terminal)
        );
        assert_eq!(
            stream.tool_cards["failed"].read(cx).activity_state(),
            crate::tool_card::ToolActivityState::Failed
        );
    });
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_some());
}

#[gpui_kit::test]
async fn issue343_t20_rejected_commands_keep_category_title(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "category-only-rejections");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        for id in ["first", "second"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "synthetic rejected command"),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: id.into(),
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
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Commands");
        for id in ["first", "second"] {
            assert_eq!(
                stream.tool_cards[id].read(cx).activity_state(),
                crate::tool_card::ToolActivityState::Rejected
            );
        }
    });
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_some());
}

#[gpui_kit::test]
async fn issue343_t19_second_tool_promotes_focused_single_row_and_retry_restores_it(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-tool-promotes");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .segment_row_count(0, cx),
            1
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    super::issue296_stream_polish::focus_row(&mut visual, "tool-activity-single-row");
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "expansion".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("second", "pwd"),
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "Commands");
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-single-row").is_none());
    assert!(visual.debug_bounds("run-stage-icon-0").is_some());
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    visual.update(|window, cx| {
        assert!(
            stream.read(cx).run_activity_groups["expansion"]
                .read(cx)
                .segment_header_focus(0)
                .is_focused(window)
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ResponseRetrying {
                message_id: "expansion".into(),
                text_start_bytes: 0,
                attempt: 1,
                max_retries: 3,
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    assert!(visual.debug_bounds("assistant-network-retry").is_some());
    stream.read_with(&visual, |stream, cx| {
        assert!(!stream.tool_cards["first"].read(cx).is_expanded());
    });
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.tool_cards["first"].read(cx).is_expanded());
    });
}

#[gpui_kit::test]
async fn issue343_t19_tool_then_thinking_preserves_open_single_tool(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "single-tool-then-thinking");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    click(&mut visual, "tool-activity-single-row");
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.tool_cards["first"].read(cx).is_expanded());
    });
    stream.update(&mut visual, |stream, cx| reasoning(stream, "  \n", cx));
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    stream.update(&mut visual, |stream, cx| {
        reasoning(stream, "Checking result", cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("tool-activity-single-row").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert!(group.segment_expanded(0));
        assert!(stream.tool_cards["first"].read(cx).is_expanded());
        assert_eq!(group.segment_row_count(0, cx), 6);
    });
}

#[gpui_kit::test]
async fn issue343_t19_second_thinking_block_promotes_only_after_real_boundary(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-thought-promotes");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        reasoning(stream, "First", cx);
        reasoning(stream, " continued", cx);
        summary(stream, "First summary", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "First summary");
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "expansion".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["expansion"]
                .read(cx)
                .stage_title(0, cx),
            "First summary"
        );
        reasoning(stream, "Second", cx);
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "First summary");
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ResponseRetrying {
                message_id: "expansion".into(),
                text_start_bytes: 0,
                attempt: 1,
                max_retries: 3,
            },
            cx,
        );
        let group = stream.run_activity_groups["expansion"].read(cx);
        assert_eq!(group.segment_row_count(0, cx), 1);
        assert_eq!(group.stage_title(0, cx), "First summary");
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-chevron-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("assistant-network-retry").is_some());
}
