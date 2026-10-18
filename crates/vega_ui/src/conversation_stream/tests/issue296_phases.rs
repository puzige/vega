use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    begin_with_stage(
        stream,
        ConversationEvent::SummaryDelta {
            message_id: "run".into(),
            delta: "Reviewing changes".into(),
        },
        cx,
    );
}

fn begin_with_stage(
    stream: &mut ConversationStream,
    stage: ConversationEvent,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "run".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "run".into(),
            started_at: Instant::now() - Duration::from_secs(22),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "run".into(),
            delta: "检查。".into(),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: "run".into(),
            response: AssistantResponseMetadata {
                response_id: "commentary".into(),
                text_start_bytes: 0,
                text_end_bytes: "检查。".len(),
                kind: AssistantResponseKind::Commentary,
            },
        },
        cx,
    );
    stream.apply_event(stage, cx);
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "run".into(),
            delta: "答案。".into(),
        },
        cx,
    );
}

fn final_event() -> ConversationEvent {
    ConversationEvent::AssistantResponseCompleted {
        message_id: "run".into(),
        response: AssistantResponseMetadata {
            response_id: "final".into(),
            text_start_bytes: "检查。".len(),
            text_end_bytes: "检查。答案。".len(),
            kind: AssistantResponseKind::Final,
        },
    }
}

fn terminal() -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: "run".into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: Some(23_100),
    }
}

fn select_phase_text(
    stream: &Entity<ConversationStream>,
    visual: &mut gpui_kit::VisualTestContext,
    text: &str,
    end: usize,
    mode: &str,
) {
    selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| *target = Some((text.into(), None)));
    stream.update(visual, |_, cx| cx.notify());
    visual.run_until_parked();
    let layout = selection::SELECTION_TARGET_LAYOUT.with_borrow_mut(|target| {
        target
            .take()
            .and_then(|(_, layout)| layout)
            .unwrap_or_else(|| panic!("mode={mode}: visible selection text"))
    });
    let start =
        layout.position_for_index(0).unwrap() + gpui_kit::point(px(0.5), layout.line_height() / 2.);
    let end_position = layout.position_for_index(end).unwrap()
        + gpui_kit::point(px(0.5), layout.line_height() / 2.);
    visual.simulate_mouse_down(start, MouseButton::Left, gpui_kit::Modifiers::default());
    visual.simulate_mouse_move(
        end_position,
        Some(MouseButton::Left),
        gpui_kit::Modifiers::default(),
    );
    visual.simulate_mouse_up(
        end_position,
        MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        &text[..end],
        "mode={mode}: native selection"
    );
    assert!(
        visual.update(gpui_kit::base::TextSelection::has_selection),
        "mode={mode}: active selection"
    );
}

#[gpui_kit::test]
async fn issue296_phase_final_hides_only_process_once_and_anchor_reopens(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-phase-final");
    stream.update(cx, begin);
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        assert!(
            (0..stream.entries.len())
                .any(|index| matches!(stream.entries[index], StreamEntry::Assistant { .. }))
        );
        assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)));
        stream.apply_event(final_event(), cx);
        stream.apply_event(terminal(), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            1
        );
        assert_eq!(
            stream.run_activity_groups["run"].read(cx).test_label(),
            "Worked for 23s"
        );
    });
    let copy_before = stream.read_with(cx, |stream, _| match &stream.entries[1] {
        StreamEntry::Assistant { copy, .. } => (copy.id, copy.original_text()),
        _ => panic!("commentary"),
    });
    stream.update(cx, |stream, cx| {
        assert!(stream.reveal_loaded_message("run", cx));
        stream.apply_event(final_event(), cx);
        stream.apply_event(terminal(), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            1
        );
        let details = stream.run_details(&stream.run_activity_groups["run"], cx);
        let copies = details
            .iter()
            .filter_map(|(_, detail)| match detail {
                phase::RunDetail::Assistant(index) => match &stream.entries[*index] {
                    StreamEntry::Assistant { copy, .. } => Some((copy.id, copy.original_text())),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(copies, vec![copy_before.clone()]);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-toggle").is_some());
    let details = visual
        .debug_bounds("run-activity-content")
        .expect("commentary details");
    let body = visual
        .debug_bounds(Box::leak(
            format!("run-process-message-{}", copy_before.0).into_boxed_str(),
        ))
        .expect("commentary markdown");
    assert!(body.top() >= details.top() && body.bottom() <= details.bottom());
}

#[gpui_kit::test]
async fn issue416_final_collapses_once_despite_reading_and_preserves_visible_focus(
    cx: &mut TestAppContext,
) {
    for mode in [
        "manual",
        "backscroll",
        "selection",
        "focus",
        "thinking-selection",
        "thinking-focus",
        "composer",
        "final-selection",
    ] {
        let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
        stream.update(&mut visual, |stream, cx| {
            if mode == "backscroll" {
                stream.apply_history_page(
                    hydration_page(
                        (1..40)
                            .map(|index| hydration_user(index, "Earlier request"))
                            .collect(),
                        None,
                    ),
                    cx,
                );
            }
            if matches!(mode, "focus" | "thinking-selection" | "thinking-focus") {
                begin_with_stage(
                    stream,
                    ConversationEvent::ThinkingDelta {
                        message_id: "run".into(),
                        delta: "Reviewing changes".into(),
                    },
                    cx,
                );
            } else {
                begin(stream, cx);
            }
        });
        visual.run_until_parked();
        let focus = stream.update(&mut visual, |stream, cx| {
            let group = stream.run_activity_groups["run"].clone();
            match mode {
                "manual" => group.update(cx, |group, cx| group.reveal(cx)),
                "backscroll" => stream.list.scroll_by(px(-600.)),
                "thinking-selection" | "thinking-focus" => {
                    assert!(
                        group.update(cx, |group, cx| group.reveal_segment(0, cx)),
                        "mode={mode}: reveal thinking source"
                    );
                }
                _ => {}
            }
            group.read(cx).segment_header_focus(0)
        });
        visual.run_until_parked();
        if mode == "focus" {
            assert!(
                visual.debug_bounds("run-stage-toggle-0").is_some(),
                "mode={mode}: visible stage entry"
            );
            visual.update(|window, cx| focus.focus(window, cx));
        }
        if mode == "thinking-focus" {
            visual.update(|window, cx| {
                let card = stream.read(cx).run_activity_groups["run"]
                    .read(cx)
                    .children()
                    .into_iter()
                    .find_map(|child| match child {
                        RunActivityChild::Thinking(card) => Some(card),
                        _ => None,
                    })
                    .unwrap();
                card.read(cx).test_focus().focus(window, cx);
            });
        }
        match mode {
            "selection" => select_phase_text(&stream, &mut visual, "检查。", "检查".len(), mode),
            "thinking-selection" => select_phase_text(
                &stream,
                &mut visual,
                "Reviewing changes",
                "Reviewing".len(),
                mode,
            ),
            "final-selection" => {
                select_phase_text(&stream, &mut visual, "答案。", "答案".len(), mode)
            }
            _ => {}
        }
        if mode == "composer" {
            visual.update(|window, cx| {
                stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
            });
        }
        visual.run_until_parked();
        if matches!(mode, "thinking-selection" | "thinking-focus") {
            assert!(
                visual.debug_bounds("thinking-content").is_some(),
                "mode={mode}: visible thinking source"
            );
        }
        if matches!(mode, "selection" | "thinking-selection" | "final-selection") {
            assert!(
                visual.update(gpui_kit::base::TextSelection::has_selection),
                "mode={mode}: native selection before Final"
            );
        }
        visual.update(|window, cx| {
            let state = stream.read(cx);
            if mode == "focus" {
                assert!(
                    focus.is_focused(window),
                    "mode={mode}: stage entry focused before Final"
                );
            } else if matches!(mode, "selection" | "final-selection") {
                assert!(
                    state.selection_focus.is_focused(window),
                    "mode={mode}: message focused before Final"
                );
            } else if matches!(mode, "thinking-selection" | "thinking-focus") {
                let card = state.run_activity_groups["run"]
                    .read(cx)
                    .children()
                    .into_iter()
                    .find_map(|child| match child {
                        RunActivityChild::Thinking(card) => Some(card),
                        _ => None,
                    })
                    .unwrap();
                assert!(
                    card.read(cx).test_focus().is_focused(window),
                    "mode={mode}: thinking source focused before Final"
                );
            } else if mode == "composer" {
                assert!(
                    state.input.read(cx).focus_handle(cx).is_focused(window),
                    "mode={mode}: Composer focused before Final"
                );
            } else if mode == "backscroll" {
                assert!(
                    !state.following_tail(),
                    "mode={mode}: detached before Final"
                );
            }
        });
        stream.update(&mut visual, |stream, cx| {
            stream.apply_event(final_event(), cx);
            stream.apply_event(terminal(), cx);
        });
        visual.run_until_parked();
        stream.read_with(&visual, |stream, cx| {
            let group = stream.run_activity_groups["run"].read(cx);
            assert!(group.process_in_details(), "mode={mode}");
            assert!(!group.expanded(), "mode={mode}");
            assert!(!group.collapse_pending(), "mode={mode}");
            assert_eq!(
                stream
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                        _ => None,
                    })
                    .collect::<String>(),
                "检查。答案。",
                "mode={mode}: original message bytes"
            );
            assert_eq!(
                (0..stream.entries.len())
                    .filter(|index| stream.process_entry_hidden(*index, cx))
                    .count(),
                1,
                "mode={mode}"
            );
            if mode == "backscroll" {
                assert!(!stream.following_tail(), "mode={mode}");
            }
            for (index, entry) in stream.entries.iter().enumerate() {
                if let StreamEntry::Assistant { copy, .. } = entry {
                    assert_eq!(
                        copy.has_selected_text(),
                        mode == "final-selection" && !stream.process_entry_hidden(index, cx),
                        "mode={mode}"
                    );
                }
            }
            for child in group.children() {
                if let RunActivityChild::Thinking(card) = child {
                    assert!(
                        !card.read(cx).test_source_copy().has_selected_text(),
                        "mode={mode}"
                    );
                }
            }
        });
        visual.update(|window, cx| {
            let state = stream.read(cx);
            if mode == "composer" {
                assert!(
                    state.input.read(cx).focus_handle(cx).is_focused(window),
                    "mode={mode}: Composer focus after Final"
                );
            } else if mode == "final-selection" {
                assert!(
                    state.selection_focus.is_focused(window),
                    "mode={mode}: Final focus after confirmation"
                );
            }
        });
        if matches!(
            mode,
            "selection" | "focus" | "thinking-selection" | "thinking-focus"
        ) {
            let group = stream.read_with(&visual, |stream, _| {
                stream.run_activity_groups["run"].clone()
            });
            assert_run_disclosure_keyboard_focus(&group, &mut visual, mode);
        }
    }
}

#[gpui_kit::test]
async fn issue416_final_clears_only_hidden_native_message_selection(cx: &mut TestAppContext) {
    for mode in ["process", "final", "older"] {
        let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
        stream.update(&mut visual, |stream, cx| {
            if mode == "older" {
                stream.apply_history_page(
                    hydration_page(
                        vec![HistoryEntry::AssistantText {
                            seq: 1,
                            message_id: "older".into(),
                            content: "Earlier answer.".into(),
                            status: vega_conversation::history::AssistantStatus::Done,
                            execution_duration_ms: None,
                        }],
                        None,
                    ),
                    cx,
                );
            }
            begin(stream, cx);
        });
        let (text, end) = match mode {
            "process" => ("检查。", "检查".len()),
            "final" => ("答案。", "答案".len()),
            _ => ("Earlier answer.", "Earlier".len()),
        };
        selection::SELECTION_TARGET_LAYOUT
            .with_borrow_mut(|target| *target = Some((text.into(), None)));
        stream.update(&mut visual, |_, cx| cx.notify());
        visual.run_until_parked();
        let layout = selection::SELECTION_TARGET_LAYOUT
            .with_borrow_mut(|target| target.take().unwrap().1.expect("selected message layout"));
        let start = layout.position_for_index(0).unwrap()
            + gpui_kit::point(px(0.5), layout.line_height() / 2.);
        let end = layout.position_for_index(end).unwrap()
            + gpui_kit::point(px(0.5), layout.line_height() / 2.);
        visual.simulate_mouse_down(start, MouseButton::Left, gpui_kit::Modifiers::default());
        visual.simulate_mouse_move(end, Some(MouseButton::Left), gpui_kit::Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, gpui_kit::Modifiers::default());
        visual.run_until_parked();
        let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
        assert!(!selected.is_empty(), "{mode}");
        let originals = stream.read_with(&visual, |stream, _| {
            stream
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::Assistant { copy, .. } => Some((copy.id, copy.original_text())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        stream.update(&mut visual, |stream, cx| {
            stream.apply_event(final_event(), cx);
        });
        visual.run_until_parked();
        let after = visual.update(gpui_kit::base::TextSelection::selected_text);
        assert_eq!(
            after,
            if mode == "process" { "" } else { &selected },
            "{mode}"
        );
        visual.update(|window, cx| {
            let state = stream.read(cx);
            let group = state.run_activity_groups["run"].clone();
            assert!(group.read(cx).process_in_details());
            assert!(!group.read(cx).expanded());
            if mode != "process" {
                assert!(state.selection_focus.is_focused(window));
            }
        });
        if mode == "process" {
            let group = stream.read_with(&visual, |stream, _| {
                stream.run_activity_groups["run"].clone()
            });
            assert_run_disclosure_keyboard_focus(
                &group,
                &mut visual,
                &format!("native-message-{mode}"),
            );
        }
        stream.update(&mut visual, |stream, cx| {
            assert_eq!(
                stream
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        StreamEntry::Assistant { copy, .. } =>
                            Some((copy.id, copy.original_text())),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                originals
            );
            stream.run_activity_groups["run"]
                .clone()
                .update(cx, |group, cx| group.reveal(cx));
            stream.apply_event(final_event(), cx);
            stream.apply_event(terminal(), cx);
        });
        visual.run_until_parked();
        stream.read_with(&visual, |stream, cx| {
            assert!(stream.run_activity_groups["run"].read(cx).expanded());
        });
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            after,
            "{mode}"
        );
    }
}

#[gpui_kit::test]
async fn issue296_phase_invalid_utf8_metadata_and_unknown_history_keep_body(
    cx: &mut TestAppContext,
) {
    for start in [1, 999] {
        let (_window, stream, _) = open_controller_stream(cx, "issue296-invalid-phase");
        stream.update(cx, |stream, cx| {
            begin(stream, cx);
            let ConversationEvent::AssistantResponseCompleted {
                message_id,
                mut response,
            } = final_event()
            else {
                unreachable!()
            };
            response.text_start_bytes = start;
            stream.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id,
                    response,
                },
                cx,
            );
            stream.apply_event(terminal(), cx);
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)))
        });
    }
    let (_window, stream, _) = open_controller_stream(cx, "issue296-unknown-phase");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(terminal(), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)))
    });
}

#[gpui_kit::test]
async fn issue296_run_clock_keeps_start_across_phase_and_terminal(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue296-clock");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        let group = stream.run_activity_groups["run"].clone();
        let start = group.read(cx).test_clock().0;
        assert!(matches!(
            group.read(cx).test_clock().2,
            run_activity::RunPhase::Responding
        ));
        assert_eq!(group.read(cx).test_label(), "Working for 22s");
        stream.apply_event(
            ConversationEvent::RunStarted {
                message_id: "run".into(),
                started_at: Instant::now(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("clock-tool", "pwd"),
            },
            cx,
        );
        assert_eq!(group.read(cx).test_clock().0, start);
        assert!(group.read(cx).test_clock().2 == run_activity::RunPhase::Working);
        stream.apply_event(terminal(), cx);
        assert_eq!(group.read(cx).test_label(), "Worked for 23s");
        assert!(!group.read(cx).test_clock().1);
    });
}

#[gpui_kit::test]
async fn issue296_phase_history_round_trip_is_conservative_and_duration_is_optional(
    cx: &mut TestAppContext,
) {
    for mode in [
        "trusted",
        "legacy",
        "invalid",
        "conflict",
        "missing-duration",
        "interrupted",
    ] {
        let (_window, stream, _) = open_controller_stream(cx, mode);
        let mut history = vec![
            HistoryEntry::AssistantText {
                seq: 1,
                message_id: "run".into(),
                content: "检查。".into(),
                status: if mode == "interrupted" {
                    vega_conversation::history::AssistantStatus::Interrupted
                } else {
                    vega_conversation::history::AssistantStatus::Done
                },
                execution_duration_ms: (mode != "missing-duration").then_some(23100),
            },
            HistoryEntry::Tool {
                seq: 2,
                message_id: "run".into(),
                call_id: "history-tool".into(),
                status: ToolCallStatus::Success,
                approval: None,
                input: None,
                result: None,
            },
            HistoryEntry::AssistantText {
                seq: 3,
                message_id: "run".into(),
                content: "答案。".into(),
                status: vega_conversation::history::AssistantStatus::Done,
                execution_duration_ms: (mode != "missing-duration").then_some(23100),
            },
        ];
        if mode == "interrupted"
            && let HistoryEntry::AssistantText { status, .. } = &mut history[2]
        {
            *status = vega_conversation::history::AssistantStatus::Interrupted;
        }
        if mode != "legacy" {
            history.push(HistoryEntry::AssistantResponseMetadata {
                seq: 4,
                message_id: "run".into(),
                response: AssistantResponseMetadata {
                    response_id: "commentary".into(),
                    text_start_bytes: 0,
                    text_end_bytes: 9,
                    kind: AssistantResponseKind::Commentary,
                },
            });
            let ConversationEvent::AssistantResponseCompleted {
                message_id,
                mut response,
            } = final_event()
            else {
                unreachable!()
            };
            if mode == "invalid" {
                response.text_start_bytes = 1;
            }
            history.push(HistoryEntry::AssistantResponseMetadata {
                seq: 5,
                message_id: message_id.clone(),
                response: response.clone(),
            });
            if mode == "conflict" {
                response.text_start_bytes = 0;
            }
            history.push(HistoryEntry::AssistantResponseMetadata {
                seq: 6,
                message_id,
                response,
            });
        }
        stream.update(cx, |stream, cx| {
            stream.apply_history_page(hydration_page(history, None), cx)
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            let hidden = (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count();
            assert_eq!(
                hidden,
                usize::from(matches!(mode, "trusted" | "missing-duration")),
                "{mode}"
            );
            let text = stream
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(text, "检查。答案。");
            assert_eq!(
                stream.run_activity_groups["run"].read(cx).test_label(),
                match mode {
                    "missing-duration" => "Completed",
                    "interrupted" => "Stopped after 23s",
                    _ => "Worked for 23s",
                }
            );
        });
    }
}

#[gpui_kit::test]
async fn issue296_phase_rejected_tools_and_permissions_survive_final_collapse(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-phase-safety");
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
                    output: "Tool error: permission denied".into(),
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
            ConversationEvent::ToolCallProposed {
                call: bash_call("pending", "printf pending"),
            },
            cx,
        );
    });
    let queue = stream.read_with(cx, |stream, _| stream.permission_queue());
    let decision = request_permission(&queue, "pending", "printf pending");
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        assert!(stream.active_permission.is_some());
        stream.apply_event(final_event(), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        let group = stream.run_activity_groups["run"].read(cx);
        assert!(!group.test_projection().2);
        assert_eq!(group.segment_row_count(1, cx), 0);
        assert!(stream.run_notice_projection(cx).is_empty());
        assert!(stream.active_permission.is_some());
        assert!(
            stream.tool_cards["rejected"]
                .read(cx)
                .visible_text()
                .contains("Command rejected")
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("permission-card-heading").is_some());
    assert!(visual.debug_bounds("run-activity-exception").is_none());
    assert!(visual.debug_bounds("run-notice-run").is_none());
    drop(decision);
}

#[gpui_kit::test]
async fn issue296_phase_unclassified_body_conflicts_and_failed_final_preserve_content(
    cx: &mut TestAppContext,
) {
    for mode in ["unclassified", "conflict", "failed"] {
        let (_window, stream, _) = open_controller_stream(cx, mode);
        stream.update(cx, |stream, cx| {
            begin(stream, cx);
            if mode == "unclassified" {
                stream.run_responses.clear();
            }
            stream.apply_event(final_event(), cx);
            if mode == "conflict" {
                let ConversationEvent::AssistantResponseCompleted {
                    message_id,
                    mut response,
                } = final_event()
                else {
                    unreachable!()
                };
                response.text_start_bytes = 0;
                stream.apply_event(
                    ConversationEvent::AssistantResponseCompleted {
                        message_id,
                        response,
                    },
                    cx,
                );
            }
            if mode == "failed" {
                stream.apply_event(
                    ConversationEvent::Error {
                        message_id: Some("run".into()),
                        error: Arc::new(std::io::Error::other("failure").into()),
                        execution_duration_ms: Some(23100),
                    },
                    cx,
                );
            } else {
                stream.apply_event(terminal(), cx);
            }
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert!(
                (0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)),
                "{mode}"
            )
        });
    }
}

struct PhaseVisibilityHarness {
    stream: Entity<ConversationStream>,
    visible: bool,
}

impl Render for PhaseVisibilityHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.visible, |view| view.child(self.stream.clone()))
    }
}

#[gpui_kit::test]
async fn issue296_run_clock_and_shimmer_stop_when_not_painted_or_terminal(cx: &mut TestAppContext) {
    init_permission_test(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(false);
        cx.set_global(shimmer::PaintProbe::default());
    });
    let stream = cx.new(|cx| ConversationStream::new(permission_thread(), cx));
    let owned = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| PhaseVisibilityHarness {
                stream: owned,
                visible: true,
            })
        })
        .unwrap()
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::RunStarted {
                message_id: "run".into(),
                started_at: Instant::now() - Duration::from_millis(900),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "run".into(),
                delta: "Reviewing changes".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let group = stream.read_with(cx, |stream, _| stream.run_activity_groups["run"].clone());
    let card = stream.read_with(cx, |stream, _| stream.active_thinking.clone().unwrap());
    let epoch = card.read_with(cx, |card, _| card.shimmer_epoch);
    assert!(group.read_with(cx, |group, _| group.test_clock().1));
    cx.executor().advance_clock(Duration::from_millis(999));
    cx.run_until_parked();
    assert_eq!(group.read_with(cx, |group, _| group.test_clock_ticks()), 0);
    cx.update(|cx| {
        let samples = cx.global::<shimmer::PaintProbe>().0.borrow();
        let last = samples.last().expect("painted current summary");
        assert_eq!(last.text, "Reviewing changes");
        assert!(last.moving);
        assert!(
            last.colors.windows(2).any(|colors| colors[0] != colors[1]),
            "sweep changes glyph colors spatially"
        );
    });
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "run".into(),
                delta: " carefully".into(),
            },
            cx,
        )
    });
    assert_eq!(card.read_with(cx, |card, _| card.shimmer_epoch), epoch);
    cx.update(|cx| cx.set_reduce_motion(true));
    stream.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            !cx.global::<shimmer::PaintProbe>()
                .0
                .borrow()
                .last()
                .unwrap()
                .moving
        )
    });
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| cx.global::<shimmer::PaintProbe>().0.borrow_mut().clear());
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(!group.read_with(cx, |group, _| group.test_clock().1));
    for _ in 0..3 {
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
    cx.update(|cx| assert!(cx.global::<shimmer::PaintProbe>().0.borrow().is_empty()));
    assert!(!group.read_with(cx, |group, _| group.test_clock().1));
    window
        .update(cx, |view, _, cx| {
            view.visible = true;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(group.read_with(cx, |group, _| group.test_clock().1));
    stream.update(cx, |stream, cx| stream.apply_event(terminal(), cx));
    cx.run_until_parked();
    assert!(!group.read_with(cx, |group, _| group.test_clock().1));
    assert!(card.read_with(cx, |card, _| card.shimmer_epoch.is_none()));
}

#[gpui_kit::test]
async fn issue296_phase_permission_wait_takes_priority_over_parallel_tools_and_text(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-wait-priority");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("parallel", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "parallel".into(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("pending", "printf pending"),
            },
            cx,
        );
    });
    let queue = stream.read_with(cx, |stream, _| stream.permission_queue());
    let decision = request_permission(&queue, "pending", "printf pending");
    cx.run_until_parked();
    for event in [
        ConversationEvent::ToolCallRunning {
            call_id: "parallel".into(),
        },
        ConversationEvent::ToolCallFinished {
            call_id: "parallel".into(),
            result: ToolResult {
                status: ToolCallStatus::Success,
                output: "ok".into(),
                reused: false,
                exit_code: Some(0),
                duration_ms: Some(1),
                truncated: Some(false),
                invalid: None,
            },
        },
        ConversationEvent::TextDelta {
            message_id: "run".into(),
            delta: "等待批准。".into(),
        },
        ConversationEvent::SummaryDelta {
            message_id: "run".into(),
            delta: "Waiting for permission".into(),
        },
    ] {
        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            let group = stream.run_activity_groups["run"].read(cx);
            assert!(group.test_clock().2 == run_activity::RunPhase::Waiting);
            assert_eq!(group.test_label(), "Awaiting confirmation for 22s");
            assert!(stream.active_permission.is_some());
            assert!(group.children().iter().all(|child| match child {
                RunActivityChild::Thinking(card) => card.read(cx).shimmer_epoch.is_none(),
                _ => true,
            }));
        });
    }
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(decision.await, PermissionDecision::Once);
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!(stream.active_permission.is_none());
        assert!(
            stream.run_activity_groups["run"].read(cx).test_clock().2
                == run_activity::RunPhase::Thinking
        );
        assert!(
            stream
                .active_thinking
                .as_ref()
                .unwrap()
                .read(cx)
                .shimmer_epoch
                .is_none()
        );
    });
}

struct ShimmerBoundsHarness;

impl Render for ShimmerBoundsHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "long-shimmer-row".into())
            .w(px(100.))
            .h(px(Layout::DISCLOSURE_HEADER_HEIGHT))
            .child(shimmer::text(
                "正在检查与整理很长的真实摘要，内容需要省略并保持行高稳定".into(),
                Instant::now(),
                window,
                cx,
            ))
    }
}

#[gpui_kit::test]
async fn issue296_phase_long_shimmer_summary_ellipsizes_without_changing_header_height(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    cx.update(|cx| cx.set_global(shimmer::PaintProbe::default()));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| ShimmerBoundsHarness))
            .unwrap()
    });
    cx.run_until_parked();
    let sample = cx.update(|cx| {
        cx.global::<shimmer::PaintProbe>()
            .0
            .borrow()
            .last()
            .unwrap()
            .text
            .clone()
    });
    assert!(sample.ends_with('…'));
    assert!(sample.starts_with("正在"));
    assert!(sample.len() < "正在检查与整理很长的真实摘要，内容需要省略并保持行高稳定".len());
    let bounds = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("long-shimmer-row")
        .unwrap();
    assert_eq!(bounds.size.height, px(Layout::DISCLOSURE_HEADER_HEIGHT));
}
