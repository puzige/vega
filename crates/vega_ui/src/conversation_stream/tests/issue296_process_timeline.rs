use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

fn response(id: &str, start: usize, end: usize, kind: AssistantResponseKind) -> ConversationEvent {
    ConversationEvent::AssistantResponseCompleted {
        message_id: "timeline".into(),
        response: AssistantResponseMetadata {
            response_id: id.into(),
            text_start_bytes: start,
            text_end_bytes: end,
            kind,
        },
    }
}

fn append(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "timeline".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "timeline".into(),
            seq: 1,
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
                output: "synthetic output".into(),
                reused: false,
                exit_code: Some(0),
                duration_ms: Some(10),
                truncated: Some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn finish(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(response("final", 2, 4, AssistantResponseKind::Final), cx);
    stream.apply_event(
        ConversationEvent::MessageFinished {
            message_id: "timeline".into(),
            stop_reason: vega_conversation::types::ConversationStopReason::End,
            execution_duration_ms: None,
        },
        cx,
    );
}

fn staged(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    begin(stream, cx);
    append(stream, "A", cx);
    stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
    tool(stream, "one", cx);
    append(stream, "B", cx);
    stream.apply_event(response("b", 1, 2, AssistantResponseKind::Commentary), cx);
    tool(stream, "two", cx);
    append(stream, "OK", cx);
}

fn source_text(stream: &ConversationStream) -> String {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
async fn issue296_timeline_t01_live_tools_follow_their_body_without_header_duplicates(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "timeline-order");
    stream.update(cx, staged);
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let first = visual
        .debug_bounds("assistant-body-1")
        .expect("first commentary");
    let first_tool = visual
        .debug_bounds("run-timeline-segment-0")
        .expect("first inline tool");
    let second = visual
        .debug_bounds("assistant-body-3")
        .expect("second commentary");
    let second_tool = visual
        .debug_bounds("run-timeline-segment-1")
        .expect("second inline tool");
    assert!(first.bottom() <= first_tool.top());
    assert!(first_tool.bottom() <= second.top());
    assert!(second.bottom() <= second_tool.top());
    assert!(visual.debug_bounds("run-activity-content").is_none());
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream
                .run_details(&stream.run_activity_groups["timeline"], cx)
                .is_empty()
        );
        assert_eq!(source_text(stream), "ABOK");
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t02_final_folds_process_and_reopens_in_source_order(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "timeline-fold");
    stream.update(cx, staged);
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.list.pause_following_tail();
        finish(stream, cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!(!stream.following_tail());
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            2
        );
        assert_eq!(source_text(stream), "ABOK");
        let group = &stream.run_activity_groups["timeline"];
        let details = stream
            .run_details(group, cx)
            .into_iter()
            .map(|(_, detail)| match detail {
                phase::RunDetail::Assistant(index) => match &stream.entries[index] {
                    StreamEntry::Assistant { copy, .. } => copy.original_text(),
                    _ => unreachable!(),
                },
                phase::RunDetail::Operation(RunActivityChild::Tool(card)) => {
                    if card == stream.tool_cards["one"] {
                        "one".into()
                    } else {
                        "two".into()
                    }
                }
                _ => panic!("unexpected detail"),
            })
            .collect::<Vec<_>>();
        assert_eq!(details, ["A", "one", "B", "two"]);
    });
    open_run_details(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-content").is_some());
    assert!(visual.debug_bounds("run-timeline-segment-0").is_none());
    assert!(visual.debug_bounds("assistant-body-5").is_some());
}

#[gpui_kit::test]
async fn issue296_timeline_t03_consecutive_unicode_responses_preserve_copy_and_final(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-unicode");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        let mut offset = 0;
        for (id, text, kind) in [
            ("a", "检查。", AssistantResponseKind::Commentary),
            ("b", "继续✅", AssistantResponseKind::Commentary),
            ("final", "答案。", AssistantResponseKind::Final),
        ] {
            for ch in text.chars() {
                append(stream, &ch.to_string(), cx);
            }
            stream.apply_event(response(id, offset, offset + text.len(), kind), cx);
            offset += text.len();
        }
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "timeline".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert_eq!(source_text(stream), "检查。继续✅答案。");
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            2
        );
        let visible = stream
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry {
                StreamEntry::Assistant { copy, .. } if !stream.process_entry_hidden(index, cx) => {
                    Some(copy.original_text())
                }
                _ => None,
            })
            .collect::<String>();
        assert_eq!(visible, "答案。");
    });
}

#[gpui_kit::test]
async fn issue416_timeline_final_collapses_reading_without_followup_input(cx: &mut TestAppContext) {
    for mode in [
        "selection",
        "focus",
        "inline-focus",
        "backscroll",
        "manual",
        "tool-expanded",
    ] {
        let (window, stream, _) = open_controller_stream(cx, mode);
        stream.update(cx, |stream, cx| {
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
            staged(stream, cx);
        });
        cx.run_until_parked();
        let group = stream.read_with(cx, |stream, _| {
            stream.run_activity_groups["timeline"].clone()
        });
        stream.update(cx, |stream, cx| match mode {
            "selection" => {
                let StreamEntry::Assistant { copy, .. } = &stream.entries[1] else {
                    panic!("body")
                };
                copy.set_selected_text(copy.selection_generation(), "A".into());
            }
            "backscroll" => stream.list.scroll_by(px(-600.)),
            "manual" => group.update(cx, |group, cx| group.reveal(cx)),
            "tool-expanded" => stream.tool_cards["one"]
                .clone()
                .update(cx, |card, cx| card.set_expanded(true, cx)),
            _ => {}
        });
        if mode == "selection" {
            window
                .update(cx, |view, window, cx| {
                    view.stream
                        .read(cx)
                        .selection_focus
                        .clone()
                        .focus(window, cx)
                })
                .unwrap();
        }
        if mode == "focus" {
            window
                .update(cx, |_, window, cx| {
                    group.read(cx).test_focus().unwrap().focus(window, cx)
                })
                .unwrap();
        }
        if mode == "inline-focus" {
            let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
            let bounds = visual
                .debug_bounds("tool-activity-single-row")
                .expect("inline tool");
            visual.simulate_keystrokes("tab");
            visual.update(|window, cx| window.blur(cx));
            let mut focused = false;
            for _ in 0..20 {
                visual.update(|window, cx| window.focus_next(cx));
                visual.run_until_parked();
                focused = visual.update(|window, cx| {
                    window.painted_quads().iter().any(|quad| {
                        quad.bounds == bounds.scale(window.scale_factor())
                            && quad.background.as_solid() == Some(theme(cx).colors.bg_hover.into())
                    })
                });
                if focused {
                    break;
                }
            }
            assert!(focused, "real inline disclosure receives focus");
        }
        stream.update(cx, finish);
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert_eq!(
                (0..stream.entries.len())
                    .filter(|index| stream.process_entry_hidden(*index, cx))
                    .count(),
                2,
                "{mode}"
            );
            let group = stream.run_activity_groups["timeline"].read(cx);
            assert!(group.process_in_details());
            assert!(!group.expanded());
            assert!(!group.collapse_pending());
            assert_eq!(source_text(stream), "ABOK");
            if mode == "backscroll" {
                assert!(!stream.following_tail());
            }
        });
        window
            .update(cx, |view, window, cx| {
                view.stream
                    .update(cx, |stream, cx| stream.focus_composer(window, cx));
            })
            .unwrap();
        stream.update(cx, |stream, cx| {
            group.update(cx, |group, cx| group.reveal(cx));
            finish(stream, cx);
            cx.notify();
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert_eq!(
                (0..stream.entries.len())
                    .filter(|index| stream.process_entry_hidden(*index, cx))
                    .count(),
                2,
                "{mode}"
            );
            assert!(group.read(cx).expanded(), "{mode}");
        });
    }
}

fn history_body(seq: i64, message_id: &str, content: &str) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: message_id.into(),
        content: content.into(),
        status: vega_conversation::history::AssistantStatus::Done,
        execution_duration_ms: None,
    }
}

fn metadata(
    seq: i64,
    id: &str,
    start: usize,
    end: usize,
    kind: AssistantResponseKind,
) -> HistoryEntry {
    let ConversationEvent::AssistantResponseCompleted {
        message_id,
        response,
    } = response(id, start, end, kind)
    else {
        unreachable!()
    };
    HistoryEntry::AssistantResponseMetadata {
        seq,
        message_id,
        response,
    }
}

#[gpui_kit::test]
async fn issue296_timeline_t04_hydrated_split_preserves_existing_keys_and_reload_identity(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-hydrated");
    let entries = vec![
        history_body(1, "timeline", "检查。继续✅答案。"),
        history_body(2, "other", "Another run"),
        metadata(3, "a", 0, 9, AssistantResponseKind::Commentary),
        metadata(4, "b", 9, 18, AssistantResponseKind::Commentary),
        metadata(5, "final", 18, 27, AssistantResponseKind::Final),
    ];
    let keys = stream.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(entries.clone(), None), cx);
        assert_eq!(source_text(stream), "检查。继续✅答案。Another run");
        let originals = stream
            .entry_identities
            .iter()
            .filter(|identity| identity.message_id.as_deref() == Some("timeline"))
            .map(|identity| identity.key.clone())
            .collect::<Vec<_>>();
        assert!(originals[1].ends_with("assistant-text-0"));
        assert!(originals[2].ends_with("assistant-text-0-response-9"));
        assert!(originals[3].ends_with("assistant-text-0-response-18"));
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            2
        );
        assert_eq!(
            stream.run_activity_groups["timeline"].read(cx).test_label(),
            "Completed"
        );
        originals
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(hydration_page(entries, None), "timeline", cx));
        let restored = stream
            .entry_identities
            .iter()
            .filter(|identity| identity.message_id.as_deref() == Some("timeline"))
            .map(|identity| identity.key.clone())
            .collect::<Vec<_>>();
        assert_eq!(restored, keys);
        let anchor = ThreadScrollAnchor {
            identity: Some(keys[1].clone()),
            message_id: Some("timeline".into()),
            offset_in_item_px: 0.,
            following_tail: false,
        };
        assert!(stream.restore_process_anchor(&anchor, cx));
        assert_eq!(source_text(stream), "检查。继续✅答案。Another run");
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t05_unknown_invalid_and_gapped_history_remains_visible(
    cx: &mut TestAppContext,
) {
    for mode in ["unknown", "utf8", "bounds", "conflict", "gap"] {
        let (_, stream, _) = open_controller_stream(cx, mode);
        let mut entries = vec![history_body(1, "timeline", "检查。继续✅答案。")];
        if mode != "unknown" {
            entries.push(metadata(2, "a", 0, 9, AssistantResponseKind::Commentary));
            if mode != "gap" {
                entries.push(metadata(3, "b", 9, 18, AssistantResponseKind::Commentary));
            }
            entries.push(metadata(
                4,
                "final",
                match mode {
                    "utf8" => 19,
                    "bounds" => 100,
                    _ => 18,
                },
                27,
                AssistantResponseKind::Final,
            ));
            if mode == "conflict" {
                entries.push(metadata(5, "final", 9, 27, AssistantResponseKind::Final));
            }
        }
        stream.update(cx, |stream, cx| {
            stream.apply_history_page(hydration_page(entries, None), cx)
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert_eq!(source_text(stream), "检查。继续✅答案。");
            let visible = stream
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| match entry {
                    StreamEntry::Assistant { copy, .. }
                        if !stream.process_entry_hidden(index, cx) =>
                    {
                        Some(copy.original_text())
                    }
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(
                visible,
                if mode == "gap" {
                    "继续✅答案。"
                } else {
                    "检查。继续✅答案。"
                },
                "{mode}"
            );
            if mode != "gap" {
                assert_eq!(
                    stream
                        .entries
                        .iter()
                        .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
                        .count(),
                    1
                );
            }
        });
    }
}

#[gpui_kit::test]
async fn issue296_timeline_t05_failed_or_cancelled_after_final_reveals_process(
    cx: &mut TestAppContext,
) {
    for failed in [false, true] {
        let (_, stream, _) = open_controller_stream(cx, "timeline-revoked-final");
        stream.update(cx, staged);
        stream.update(cx, |stream, cx| {
            stream.apply_event(response("final", 2, 4, AssistantResponseKind::Final), cx)
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| assert!(stream.process_entry_hidden(1, cx)));
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                if failed {
                    ConversationEvent::Error {
                        message_id: Some("timeline".into()),
                        error: Arc::new(std::io::Error::other("synthetic failure").into()),
                        execution_duration_ms: None,
                    }
                } else {
                    ConversationEvent::Interrupted {
                        message_id: "timeline".into(),
                        execution_duration_ms: None,
                    }
                },
                cx,
            );
        });
        cx.run_until_parked();
        stream.read_with(cx, |stream, cx| {
            assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)));
            assert_eq!(
                stream.tool_entry_index(&stream.tool_cards["one"], cx),
                Some(2)
            );
            assert_eq!(source_text(stream), "ABOK");
        });
    }
}

#[gpui_kit::test]
async fn issue296_timeline_t07_tool_anchor_is_inline_until_folded(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-tool-anchor");
    stream.update(cx, staged);
    cx.run_until_parked();
    let anchor = stream.update(cx, |stream, cx| {
        assert_eq!(
            stream.tool_entry_index(&stream.tool_cards["one"], cx),
            Some(2)
        );
        let anchor = ThreadScrollAnchor {
            identity: Some(stream.entry_identities[2].key.clone()),
            message_id: Some("timeline".into()),
            offset_in_item_px: 0.,
            following_tail: false,
        };
        assert!(!stream.restore_process_anchor(&anchor, cx));
        finish(stream, cx);
        anchor
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        assert_eq!(
            stream.tool_entry_index(&stream.tool_cards["one"], cx),
            Some(0)
        );
        assert!(stream.restore_process_anchor(&anchor, cx));
        assert!(stream.run_activity_groups["timeline"].read(cx).expanded());
    });
}

#[gpui_kit::test]
async fn issue416_timeline_scrolled_back_final_collapses_before_scroll_resume(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "timeline-wheel");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (1..40)
                    .map(|index| hydration_user(index, "Earlier request"))
                    .collect(),
                None,
            ),
            cx,
        );
        staged(stream, cx);
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let viewport = visual
        .debug_bounds("conversation-message-list")
        .expect("list");
    let wheel = |delta| gpui_kit::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    };
    visual.simulate_event(wheel(600.));
    visual.run_until_parked();
    stream.update(&mut visual, |stream, cx| {
        assert!(!stream.following_tail());
        finish(stream, cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(!stream.following_tail());
        assert!(
            !stream.run_activity_groups["timeline"]
                .read(cx)
                .collapse_pending()
        );
        assert!(
            stream.run_activity_groups["timeline"]
                .read(cx)
                .process_in_details()
        );
        assert!(!stream.run_activity_groups["timeline"].read(cx).expanded());
        assert_eq!(source_text(stream), "ABOK");
    });
    visual.simulate_event(wheel(-10000.));
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.following_tail());
        assert!(
            !stream.run_activity_groups["timeline"]
                .read(cx)
                .collapse_pending()
        );
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t04_live_response_anchors_restore_the_same_hydrated_body(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-anchor-roundtrip");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        append(stream, "A", cx);
        stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
        stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
        append(stream, "B", cx);
        stream.apply_event(response("b", 1, 2, AssistantResponseKind::Commentary), cx);
        tool(stream, "one", cx);
        append(stream, "OK", cx);
        stream.apply_event(response("final", 2, 4, AssistantResponseKind::Final), cx);
    });
    let (anchors, active_before) = stream.read_with(cx, |stream, _| {
        (
            stream
                .entries
                .iter()
                .zip(&stream.entry_identities)
                .filter_map(|(entry, identity)| match entry {
                    StreamEntry::Assistant { copy, .. } => {
                        Some((identity.key.clone(), copy.original_text()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            stream.active_agent_message.clone(),
        )
    });
    stream.update(cx, |stream, cx| {
        stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
        assert_eq!(stream.active_agent_message, active_before);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "timeline".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        let entries = vec![
            history_body(1, "timeline", "AB"),
            HistoryEntry::Tool {
                seq: 2,
                message_id: "timeline".into(),
                call_id: "one".into(),
                status: ToolCallStatus::Success,
                approval: None,
                input: None,
                result: None,
            },
            history_body(3, "timeline", "OK"),
            metadata(4, "a", 0, 1, AssistantResponseKind::Commentary),
            metadata(5, "b", 1, 2, AssistantResponseKind::Commentary),
            metadata(6, "final", 2, 4, AssistantResponseKind::Final),
        ];
        assert!(stream.replace_history_window(hydration_page(entries, None), "timeline", cx));
        for (key, expected) in anchors {
            let index = stream
                .entry_identities
                .iter()
                .position(|identity| identity.key == key)
                .expect("same live identity");
            let StreamEntry::Assistant { copy, .. } = &stream.entries[index] else {
                panic!("assistant")
            };
            assert_eq!(copy.original_text(), expected, "{key}");
        }
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t04_other_run_copy_and_manual_expansion_survive_current_final(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-two-runs");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    history_body(1, "older", "Old body"),
                    HistoryEntry::AssistantResponseMetadata {
                        seq: 2,
                        message_id: "older".into(),
                        response: AssistantResponseMetadata {
                            response_id: "old-final".into(),
                            text_start_bytes: 0,
                            text_end_bytes: 8,
                            kind: AssistantResponseKind::Final,
                        },
                    },
                ],
                None,
            ),
            cx,
        );
        staged(stream, cx);
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.run_activity_groups["older"]
            .clone()
            .update(cx, |group, cx| group.reveal(cx));
        finish(stream, cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!(!stream.process_entry_hidden(0, cx));
        let StreamEntry::Assistant { copy, .. } = &stream.entries[1] else {
            panic!("older body")
        };
        assert!(stream.run_activity_groups["older"].read(cx).expanded());
        assert_eq!(copy.original_text(), "Old body");
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            2
        );
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t04_thinking_between_responses_keeps_hydrated_identity(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "timeline-thinking-anchor");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        append(stream, "A", cx);
        stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "timeline".into(),
                delta: "Considering the answer".into(),
            },
            cx,
        );
        append(stream, "OK", cx);
        stream.apply_event(response("final", 1, 3, AssistantResponseKind::Final), cx);
    });
    cx.run_until_parked();
    let final_key = stream.read_with(cx, |stream, _| {
        stream.entry_identities.last().unwrap().key.clone()
    });
    stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(
            hydration_page(
                vec![
                    history_body(1, "timeline", "AOK"),
                    metadata(2, "a", 0, 1, AssistantResponseKind::Commentary),
                    metadata(3, "final", 1, 3, AssistantResponseKind::Final)
                ],
                None
            ),
            "timeline",
            cx
        ));
        let index = stream
            .entry_identities
            .iter()
            .position(|identity| identity.key == final_key)
            .expect("same final identity");
        let StreamEntry::Assistant { copy, .. } = &stream.entries[index] else {
            panic!("final")
        };
        assert_eq!(copy.original_text(), "OK");
        assert!(!stream.process_entry_hidden(index, cx));
    });
}

#[gpui_kit::test]
async fn issue296_timeline_t04_hydrated_unknown_final_keeps_inline_tool_order(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "timeline-hydrated-order");
    stream.update(cx, |stream, cx| {
        let tool = |seq, id: &str| HistoryEntry::Tool {
            seq,
            message_id: "timeline".into(),
            call_id: id.into(),
            status: ToolCallStatus::Success,
            approval: None,
            input: None,
            result: None,
        };
        stream.apply_history_page(
            hydration_page(
                vec![
                    history_body(1, "timeline", "A"),
                    tool(2, "one"),
                    history_body(3, "timeline", "B"),
                    tool(4, "two"),
                    history_body(5, "timeline", "Unknown body"),
                ],
                None,
            ),
            cx,
        );
        assert_eq!(
            stream.tool_entry_index(&stream.tool_cards["one"], cx),
            Some(2)
        );
        assert_eq!(
            stream.tool_entry_index(&stream.tool_cards["two"], cx),
            Some(4)
        );
        assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)));
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let a = visual.debug_bounds("assistant-body-1").unwrap();
    let one = visual.debug_bounds("run-timeline-segment-0").unwrap();
    let b = visual.debug_bounds("assistant-body-3").unwrap();
    let two = visual.debug_bounds("run-timeline-segment-1").unwrap();
    assert!(a.bottom() <= one.top() && one.bottom() <= b.top() && b.bottom() <= two.top());
    assert!(visual.debug_bounds("run-activity-content").is_none());
}
