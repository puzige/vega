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
async fn issue296_expansion_p07_stage_opens_compact_source_order(cx: &mut TestAppContext) {
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
    click(&mut visual, "run-stage-toggle-0");
    let thinking = visual
        .debug_bounds("thinking-toggle")
        .expect("compact source entry");
    let first = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    let second = visual.debug_bounds("run-stage-tool-0-1").unwrap();
    assert_eq!(first.top() - thinking.top(), px(25.));
    assert_eq!(second.top() - first.top(), px(25.));
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("stage-thinking-content").is_none());
    assert_eq!(
        visual.debug_bounds("thinking-icon").unwrap().left(),
        visual
            .debug_bounds("run-stage-tool-0-0-icon")
            .unwrap()
            .left()
    );
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
        summary(stream, original, cx);
        stream.active_thinking.clone().unwrap()
    });
    visual.run_until_parked();
    click(&mut visual, "run-stage-toggle-0");
    assert!(!card.read_with(&visual, |card, _| card.expanded));
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    click(&mut visual, "thinking-toggle");
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
        summary(
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
        card.append("\n\nMore reasoning.", true, usize::MAX);
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
        summary(stream, "Synthetic phase", cx);
        stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        assert!(stream.apply_artifact_card("c", artifact.clone(), cx));
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("later", "pwd"),
            },
            cx,
        );
        assert!(stream.artifact_card_is_adjacent("c", cx));
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds(selector).is_some());
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
async fn issue296_expansion_p07_first_render_moves_notice_after_new_text(cx: &mut TestAppContext) {
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
        assert_eq!(
            stream.run_notices.keys().copied().collect::<Vec<_>>(),
            [index]
        );
    });
    visual.run_until_parked();
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
