use super::*;
use gpui_kit::{Modifiers, VisualTestContext, point};

#[gpui_kit::test]
async fn issue296_v2_replacing_loaded_history_rebuilds_role_boundaries(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-replace");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "Old request"),
                    hydration_assistant(2, "Old body"),
                ],
                None,
            ),
            cx,
        );
        assert!(stream.replace_history_window(
            hydration_page(vec![hydration_assistant(9, "Located body")], None),
            "assistant-9",
            cx
        ));
        assert_eq!(stream.assistant_role_rows, vec![true]);
        assert_eq!(stream.entries.len(), 1);
        assert_eq!(
            stream.entry_identities[0].message_id.as_deref(),
            Some("assistant-9")
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-role-0").is_none());
    assert!(visual.debug_bounds("assistant-role-1").is_none());
}

#[gpui_kit::test]
async fn issue296_v2_history_prepend_keeps_reading_body_at_same_pixel(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-prepend");
    stream.update(cx, |stream, cx| {
        let mut entries = vec![hydration_assistant(
            10,
            &"Reading this same body line.\n\n".repeat(30),
        )];
        entries.extend((11..=35).map(|index| hydration_user(index, "Later request")));
        stream.apply_history_page(hydration_page(entries, Some(1)), cx);
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 0,
            offset_in_item: px(80.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let before = visual
        .debug_bounds("assistant-body-0")
        .expect("reading body");
    let anchor = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![hydration_assistant(9, "Earlier in this same turn")],
                None,
            ),
            cx,
        );
    });
    visual.run_until_parked();
    let after = visual
        .debug_bounds("assistant-body-1")
        .expect("same reading body");
    assert!((f32::from(after.top() - before.top())).abs() < 1.);
    assert_eq!(after.size, before.size);
    let restored = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(restored.identity, anchor.identity);
    assert_eq!(restored.offset_in_item_px, anchor.offset_in_item_px);
    assert!(!restored.following_tail);
    assert!(visual.debug_bounds("assistant-role-1").is_none());
}

#[gpui_kit::test]
async fn issue296_v2_prepend_inside_role_clamps_to_body_start(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-role-offset");
    stream.update(cx, |stream, cx| {
        let mut entries = vec![hydration_assistant(10, "Boundary body")];
        entries.extend((11..=35).map(|index| hydration_user(index, "Later request")));
        stream.apply_history_page(hydration_page(entries, Some(1)), cx);
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 0,
            offset_in_item: px(10.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let before = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(before.offset_in_item_px, 10.);
    stream.update(&mut visual, |stream, cx| {
        stream.apply_history_page(
            hydration_page(vec![hydration_assistant(9, "Earlier body")], None),
            cx,
        )
    });
    visual.run_until_parked();
    let after = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.offset_in_item_px, 10.);
    let body = visual.debug_bounds("assistant-body-1").expect("same body");
    let list = visual
        .debug_bounds("conversation-message-list")
        .expect("list");
    assert_eq!(body.top(), list.top() - px(10.));
}

#[gpui_kit::test]
async fn issue296_v2_standalone_tools_share_one_surface_in_both_themes(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-standalone");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        )
    });
    for (id, selector) in [
        (None, "standalone-tool-process"),
        (Some("two"), "standalone-tool-group-process"),
    ] {
        if let Some(id) = id {
            stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::ToolCallProposed {
                        call: bash_call(id, "pwd"),
                    },
                    cx,
                )
            });
        }
        for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
            let colors = palette.colors;
            cx.update(|cx| {
                cx.set_global(palette);
                cx.refresh_windows();
            });
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.run_until_parked();
            let surface = visual
                .debug_bounds(selector)
                .expect("standalone process surface");
            assert_eq!(
                visual.update(|window, _| {
                    let bounds = surface.scale(window.scale_factor());
                    window
                        .painted_quads()
                        .iter()
                        .filter(|quad| {
                            quad.bounds == bounds
                                && quad.background.as_solid() == Some(colors.bg_subtle.into())
                        })
                        .count()
                }),
                0,
                "v3 process rows have no persistent background"
            );
        }
    }
}

#[gpui_kit::test]
async fn issue296_v2_process_spacing_and_estimate_include_new_geometry(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-estimate");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_eq!(
        visual
            .debug_bounds("run-activity-group")
            .expect("initial thinking")
            .size
            .height,
        px(28.5)
    );
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights[0],
            28.5
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "run".into(),
                delta: "Inspecting".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let header = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("single header");
    visual.simulate_click(header.center(), Modifiers::default());
    visual.run_until_parked();
    let surface = visual
        .debug_bounds("run-timeline-segment-0")
        .expect("segment");
    let thinking = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("thinking row");
    let tool = visual.debug_bounds("run-stage-tool-0-0").expect("tool row");
    assert_eq!(tool.top() - thinking.bottom(), px(25.));
    assert_eq!(
        thinking.left(),
        surface.left() + px(Layout::PROCESS_PADDING_X)
    );
    assert_eq!(tool.left(), thinking.left());
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        let geometry = stream.message_anchor_geometry(cx, 600.);
        assert_eq!(geometry.entry_heights[0], 28.5);
        assert_eq!(geometry.entry_heights[1], 16. + 25. + 25. + 25.);
    });
    visual.simulate_click(tool.center(), Modifiers::default());
    visual.run_until_parked();
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        let geometry = stream.message_anchor_geometry(cx, 600.);
        assert_eq!(geometry.entry_heights[1], 16. + 25. + 25. + 25. + 4. + 98.);
        assert_eq!(geometry.entry_heights[0], 28.5);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "run".into(),
                delta: "Plain answer".into(),
            },
            cx,
        );
        stream.measured_entry_heights.clear();
        let geometry = stream.message_anchor_geometry(cx, 600.);
        assert_eq!(
            *geometry
                .entry_heights
                .last()
                .expect("unsynchronized assistant"),
            0.
        );
        assert!(matches!(
            stream.entries.last(),
            Some(StreamEntry::Assistant { model, copy, .. })
                if model.row_count() == 0 && copy.original_text() == "Plain answer"
        ));
    });
    visual.run_until_parked();
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        let geometry = stream.message_anchor_geometry(cx, 600.);
        assert_eq!(
            *geometry.entry_heights.last().expect("assistant height"),
            16. + Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT
        );
    });
    visual.run_until_parked();
    stream.update(&mut visual, |stream, _| {
        stream.capture_visible_entry_heights()
    });
    stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 600.);
        for (index, height) in geometry.entry_heights.iter().enumerate() {
            let bounds = stream.list.bounds_for_item(index).expect("measured row");
            assert_eq!(*height, f32::from(bounds.size.height));
        }
    });
}

#[gpui_kit::test]
async fn issue296_v2_roles_follow_real_user_turns_without_changing_copy(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-roles");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_assistant(1, "Historical continuation"),
                    hydration_assistant(2, "Another continuation"),
                    hydration_user(3, "Request"),
                    hydration_assistant(4, "Working on the request"),
                    hydration_assistant(5, "Result body"),
                    hydration_user(6, "Next request"),
                    hydration_assistant(7, "Next body"),
                ],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for selector in ["assistant-role-0", "assistant-role-3", "assistant-role-6"] {
        assert!(visual.debug_bounds(selector).is_none());
    }
    for selector in [
        "assistant-role-1",
        "assistant-role-2",
        "assistant-role-4",
        "assistant-role-5",
    ] {
        assert!(visual.debug_bounds(selector).is_none());
    }
    stream.read_with(&visual, |stream, _| {
        assert_eq!(stream.entries.len(), 7);
        for (index, expected) in [
            (0, "Historical continuation"),
            (3, "Working on the request"),
            (4, "Result body"),
        ] {
            let StreamEntry::Assistant { copy, .. } = &stream.entries[index] else {
                panic!("same assistant entry order");
            };
            assert_eq!(copy.visible_text().as_deref(), Some(expected));
        }
    });
}

#[gpui_kit::test]
async fn issue296_v2_process_surface_preserves_segment_order(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-process");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "run".into(),
                delta: "Inspecting".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "run".into(),
                delta: "Progress body".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let header = visual
        .debug_bounds("run-activity-group")
        .expect("process header");
    assert_eq!(header.size.height, px(28.5));
    assert!(visual.update(|window, cx| {
        let colors = theme(cx).colors;
        !window.painted_quads().iter().any(|quad| {
            quad.bounds == header.scale(window.scale_factor())
                && quad.background.as_solid() == Some(colors.bg_subtle.into())
        })
    }));
    stream.read_with(&visual, |stream, _| {
        assert!(matches!(stream.entries[0], StreamEntry::RunActivity { .. }));
        assert!(matches!(
            stream.entries[1],
            StreamEntry::RunActivitySegment { segment: 0, .. }
        ));
        assert!(matches!(stream.entries[2], StreamEntry::Assistant { .. }));
        assert!(matches!(
            stream.entries[3],
            StreamEntry::RunActivitySegment { segment: 1, .. }
        ));
    });
}

#[gpui_kit::test]
async fn issue296_v2_rail_rest_hover_and_focus_keep_marker_geometry(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-v2-rail");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (1..=40)
                    .map(|index| hydration_user(index, "Message"))
                    .collect(),
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for _ in 0..3 {
        stream.update(&mut visual, |_, cx| cx.notify());
        visual.run_until_parked();
    }
    let rail = visual.debug_bounds("message-anchor-rail").expect("rail");
    let marks = |visual: &mut VisualTestContext| {
        visual.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    (quad.bounds.size.height.as_f32() / scale - 2.).abs() < 0.1
                        && (quad.bounds.center().x.as_f32() / scale - f32::from(rail.center().x))
                            .abs()
                            < 0.1
                })
                .map(|quad| (quad.bounds, quad.background.as_solid()))
                .collect::<Vec<_>>()
        })
    };
    let rest = marks(&mut visual);
    assert_eq!(rest.len(), 9);
    visual.simulate_mouse_move(rail.center(), None, Modifiers::default());
    visual.run_until_parked();
    let hover = marks(&mut visual);
    assert_eq!(hover.len(), 9);
    assert!(
        hover
            .iter()
            .filter(|(_, color)| *color == Some(vega_theme::LIGHT.text_secondary.into()))
            .count()
            >= 1
    );
    assert_eq!(visual.debug_bounds("message-anchor-rail"), Some(rail));
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    visual.update(|window, cx| {
        stream
            .read(cx)
            .message_anchor_focus
            .clone()
            .focus(window, cx)
    });
    visual.run_until_parked();
    assert_eq!(marks(&mut visual).len(), 9);
    assert!(visual.debug_bounds("message-anchor-preview").is_some());
}
