use super::*;
use gpui_kit::{ListOffset, Modifiers, VisualTestContext};

fn fixture(
    cx: &mut TestAppContext,
    final_state: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _) = open_controller_stream(cx, "sticky-disclosure");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "sticky".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "sticky".into(),
                delta: "Synthetic process".into(),
            },
            cx,
        );
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("tool-{index}"), "pwd"),
                },
                cx,
            );
        }
        if final_state {
            confirm_test_final(stream, "sticky", cx);
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "sticky".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
            stream.composer_submit_pending = true;
            stream.accept_composer_submission(&"Following visible content.\n\n".repeat(80), cx);
        } else {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "sticky".into(),
                    delta: "Following visible content.\n\n".repeat(80),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    (window, stream)
}

fn position(
    stream: &Entity<ConversationStream>,
    index: usize,
    offset: f32,
    visual: &mut VisualTestContext,
) {
    stream.update(visual, |stream, cx| {
        stream.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(offset),
        });
        cx.notify();
    });
    visual.run_until_parked();
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn assert_outer_scroll(cx: &mut TestAppContext, final_state: bool) {
    let (window, stream) = fixture(cx, final_state);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let index = usize::from(!final_state);
    let selector = if final_state {
        "run-activity-toggle"
    } else {
        "run-stage-toggle-0"
    };
    position(&stream, index, 0., &mut visual);
    let natural = visual.debug_bounds(selector).unwrap();
    position(&stream, index, 20., &mut visual);
    assert_eq!(
        visual.debug_bounds(selector).unwrap().top(),
        natural.top() - px(20.)
    );
    position(&stream, index, 0., &mut visual);
    click(&mut visual, selector);
    position(&stream, index, 100., &mut visual);
    let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
    let header = visual
        .debug_bounds(selector)
        .expect("same header remains mounted");
    assert_eq!(header.top(), viewport.top());
    click(&mut visual, selector);
    let header = visual
        .debug_bounds(selector)
        .expect("collapsed header remains reachable");
    assert!(header.top() >= viewport.top() && header.bottom() <= viewport.bottom());
    stream.read_with(&visual, |stream, cx| {
        assert!(!stream.list.is_following_tail());
        assert_eq!(stream.list.logical_scroll_top().item_ix, index);
        for child in stream.run_activity_groups["sticky"].read(cx).children() {
            match child {
                RunActivityChild::Tool(card) => assert!(!card.read(cx).is_expanded()),
                RunActivityChild::ToolGroup(group) => assert!(
                    group
                        .read(cx)
                        .children()
                        .iter()
                        .all(|card| !card.read(cx).is_expanded())
                ),
                _ => {}
            }
        }
    });
}

#[gpui_kit::test]
async fn issue296_sticky_s02_live_outer_scroll(cx: &mut TestAppContext) {
    assert_outer_scroll(cx, false);
}

#[gpui_kit::test]
async fn issue296_sticky_s02_final_outer_scroll(cx: &mut TestAppContext) {
    assert_outer_scroll(cx, true);
}

#[gpui_kit::test]
async fn issue296_sticky_s02_empty_process_does_not_pin(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "sticky-empty");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "empty".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "empty".into(),
                delta: "Body without process.\n\n".repeat(80),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    position(&stream, 0, 0., &mut visual);
    click(&mut visual, "run-activity-toggle");
    position(&stream, 0, 10., &mut visual);
    let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
    assert_eq!(
        visual.debug_bounds("run-activity-toggle").unwrap().top(),
        viewport.top() - px(10.)
    );
    assert!(visual.debug_bounds("run-activity-content").is_none());
}

fn wheel(visual: &mut VisualTestContext, position: gpui_kit::Point<Pixels>, delta: f32) {
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
        modifiers: Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn issue296_sticky_s01_header_wheel_reaches_outer_scroll(cx: &mut TestAppContext) {
    for final_state in [false, true] {
        let (window, stream) = fixture(cx, final_state);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let index = usize::from(!final_state);
        let selector = if final_state {
            "run-activity-toggle"
        } else {
            "run-stage-toggle-0"
        };
        let content = if final_state {
            "run-activity-content"
        } else {
            "run-stage-content-0"
        };
        position(&stream, index, 0., &mut visual);
        let header = visual.debug_bounds(selector).unwrap();
        wheel(&mut visual, header.center(), -40.);
        stream.read_with(&visual, |stream, _| {
            let actual = stream.list.logical_scroll_top();
            assert!(actual.item_ix > index || actual.offset_in_item > px(0.));
        });
        position(&stream, index, 0., &mut visual);
        click(&mut visual, selector);
        let details = visual.debug_bounds(content).unwrap();
        wheel(&mut visual, details.center(), -10000.);
        position(&stream, index, 100., &mut visual);
        let header = visual.debug_bounds(selector).unwrap();
        let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
        assert_eq!(header.top(), viewport.top());
        wheel(&mut visual, header.center(), -40.);
        stream.read_with(&visual, |stream, _| {
            let actual = stream.list.logical_scroll_top();
            assert!(actual.item_ix > index || actual.offset_in_item > px(100.));
        });
    }
}

#[gpui_kit::test]
async fn issue296_sticky_s01_inner_scroll_and_boundary_propagation(cx: &mut TestAppContext) {
    for final_state in [false, true] {
        let (window, stream) = fixture(cx, final_state);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let index = usize::from(!final_state);
        let selector = if final_state {
            "run-activity-toggle"
        } else {
            "run-stage-toggle-0"
        };
        let content = if final_state {
            "run-activity-content"
        } else {
            "run-stage-content-0"
        };
        position(&stream, index, 0., &mut visual);
        click(&mut visual, selector);
        let header = visual.debug_bounds(selector).unwrap();
        let details = visual.debug_bounds(content).unwrap();
        assert_eq!(details.size.height, px(320.));
        let outer = stream.read_with(&visual, |stream, _| stream.list.logical_scroll_top());
        for delta in [-80., -10000.] {
            wheel(&mut visual, details.center(), delta);
            assert_eq!(visual.debug_bounds(selector), Some(header));
            stream.read_with(&visual, |stream, _| {
                let actual = stream.list.logical_scroll_top();
                assert_eq!(actual.item_ix, outer.item_ix);
                assert_eq!(actual.offset_in_item, outer.offset_in_item);
            });
        }
        wheel(&mut visual, details.center(), -80.);
        stream.read_with(&visual, |stream, _| {
            assert!(stream.list.logical_scroll_top().offset_in_item > outer.offset_in_item);
        });
        click(&mut visual, selector);
        assert!(visual.debug_bounds(content).is_none());
    }
}

fn focus_header(visual: &mut VisualTestContext, selector: &'static str) {
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| window.blur(cx));
    for _ in 0..40 {
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        let bounds = visual.debug_bounds(selector).unwrap();
        if visual.update(|window, cx| {
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(window.scale_factor())
                    && quad.background.as_solid() == Some(theme(cx).colors.bg_hover.into())
            })
        }) {
            return;
        }
    }
    panic!("header keyboard focus");
}

#[gpui_kit::test]
async fn issue296_sticky_s04_theme_width_keyboard_and_single_target(cx: &mut TestAppContext) {
    for final_state in [false, true] {
        let (window, stream) = fixture(cx, final_state);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let index = usize::from(!final_state);
        let selector = if final_state {
            "run-activity-toggle"
        } else {
            "run-stage-toggle-0"
        };
        let arrow = if final_state {
            "run-activity-chevron"
        } else {
            "run-stage-chevron-0"
        };
        let content = if final_state {
            "run-activity-content"
        } else {
            "run-stage-content-0"
        };
        for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
            for width in [480., 1200.] {
                visual.update(|_, cx| cx.set_global(palette));
                visual.simulate_resize(gpui_kit::size(px(width), px(800.)));
                position(&stream, index, 0., &mut visual);
                click(&mut visual, selector);
                position(&stream, index, 100., &mut visual);
                let header = visual.debug_bounds(selector).unwrap();
                let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
                assert_eq!(header.top(), viewport.top());
                assert!(header.left() >= viewport.left() && header.right() <= viewport.right());
                visual.simulate_mouse_move(
                    gpui_kit::point(px(1.), px(1.)),
                    None,
                    Modifiers::default(),
                );
                visual.run_until_parked();
                assert!(issue296_single_activity::chevron_painted(
                    &mut visual,
                    arrow
                ));
                visual.update(|window, _| {
                    assert!(window.painted_quads().iter().any(|quad| {
                        quad.bounds == header.scale(window.scale_factor())
                            && quad.background.as_solid() == Some(palette.colors.bg_base.into())
                    }))
                });
                focus_header(&mut visual, selector);
                let focus = visual.update(|window, cx| window.focused(cx).unwrap());
                for key in ["enter", "space"] {
                    visual.simulate_keystrokes(key);
                    visual.run_until_parked();
                    assert!(visual.debug_bounds(content).is_none());
                    let collapsed = visual.debug_bounds(selector).unwrap();
                    assert!(collapsed.top() >= viewport.top());
                    assert_eq!(
                        visual.update(|window, cx| window.focused(cx)),
                        Some(focus.clone())
                    );
                    stream.read_with(&visual, |stream, _| {
                        assert!(!stream.list.is_following_tail())
                    });
                    if key == "enter" {
                        visual.simulate_keystrokes("space");
                        visual.run_until_parked();
                        position(&stream, index, 100., &mut visual);
                    }
                }
                stream.read_with(&visual, |stream, cx| {
                    assert!(
                        stream.run_activity_groups["sticky"]
                            .read(cx)
                            .children()
                            .iter()
                            .all(|child| match child {
                                RunActivityChild::Tool(card) => !card.read(cx).is_expanded(),
                                RunActivityChild::ToolGroup(group) => group
                                    .read(cx)
                                    .children()
                                    .iter()
                                    .all(|card| !card.read(cx).is_expanded()),
                                _ => true,
                            })
                    );
                });
            }
        }
    }
}

#[gpui_kit::test]
async fn issue296_sticky_s05_prepend_updates_and_flow_geometry(cx: &mut TestAppContext) {
    let (window, stream) = fixture(cx, false);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    position(&stream, 1, 0., &mut visual);
    click(&mut visual, "run-stage-toggle-0");
    let (identity, heights) = stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        (
            stream.entry_identities[1].key.clone(),
            stream.message_anchor_geometry(cx, 600.).entry_heights,
        )
    });
    let natural = visual.debug_bounds("run-timeline-segment-0").unwrap();
    position(&stream, 1, 100., &mut visual);
    stream.update(&mut visual, |stream, cx| {
        stream.measured_entry_heights.clear();
        assert_eq!(
            stream.message_anchor_geometry(cx, 600.).entry_heights,
            heights
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "sticky".into(),
                delta: "Later text.".into(),
            },
            cx,
        );
        stream.apply_history_page(
            hydration_page(vec![hydration_user(0, "Older synthetic request")], None),
            cx,
        );
    });
    visual.run_until_parked();
    let index = stream.read_with(&visual, |stream, _| {
        stream
            .entry_identities
            .iter()
            .position(|item| item.key == identity)
            .unwrap()
    });
    assert!(index > 1);
    position(&stream, index, 100., &mut visual);
    assert_eq!(
        visual.debug_bounds("run-timeline-segment-0").unwrap().size,
        natural.size
    );
    click(&mut visual, "run-stage-toggle-0");
    stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        assert_eq!(stream.entry_identities[top.item_ix].key, identity);
        assert_eq!(top.offset_in_item, px(0.));
        assert!(!stream.list.is_following_tail());
    });
    assert!(visual.debug_bounds("run-stage-toggle-0").unwrap().top() >= px(0.));
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-content-0").is_some());
}

#[gpui_kit::test]
async fn issue296_sticky_s03_artifact_gap_pushout_and_later_section(cx: &mut TestAppContext) {
    let repo = tempfile::tempdir().unwrap();
    let workspace = Arc::new(vega_conversation::GitWorkspaceService::new(repo.path()).unwrap());
    let service =
        vega_conversation::ArtifactService::new(workspace, "p".into(), "t".into(), 1).unwrap();
    let call = ToolCall { id: "c".into(), tool: "write".into(), input_json: serde_json::json!({"audit_version":"write_edit_v1","tool":"write","path":"synthetic.txt","content_bytes":1,"fingerprint_v1":"a".repeat(64)}).to_string() };
    let projection = service.capture(&call, &ToolResult { status: ToolCallStatus::Success, output: serde_json::json!({"path":"synthetic.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}).to_string(), reused:false, exit_code:None, duration_ms:None, truncated:Some(false), invalid:None }, CancellationToken::new()).await.unwrap().unwrap();
    let artifact = cx.new(|cx| ArtifactCard::new("t".into(), "p".into(), projection, cx));
    let artifact_selector =
        Box::leak(format!("run-artifact-{}", artifact.entity_id()).into_boxed_str());
    let (window, stream, _) = open_controller_stream(cx, "sticky-artifact");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "sticky".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "sticky".into(),
                delta: "Synthetic source".into(),
            },
            cx,
        );
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("before-{index}"), "pwd"),
                },
                cx,
            );
        }
        stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        assert!(stream.apply_artifact_card("c", artifact, cx));
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("after-{index}"), "pwd"),
                },
                cx,
            );
        }
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "sticky".into(),
                delta: "Later source body.\n\n".repeat(80),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    position(&stream, 1, 0., &mut visual);
    click(&mut visual, "run-stage-toggle-0");
    let owner = visual.debug_bounds("run-timeline-segment-0").unwrap();
    let first = visual.debug_bounds("run-stage-content-0").unwrap();
    let middle = visual.debug_bounds(artifact_selector).unwrap();
    let last = visual.debug_bounds("run-stage-content-0-1").unwrap();
    assert_eq!(first.bottom(), middle.top());
    assert_eq!(middle.bottom(), last.top());
    let first_end = f32::from(first.bottom() - owner.top());
    position(&stream, 1, first_end - 10., &mut visual);
    let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
    let pushed = visual.debug_bounds("run-stage-toggle-0").unwrap();
    assert_eq!(pushed.top(), viewport.top() - px(15.));
    assert_eq!(pushed.bottom(), viewport.top() + px(10.));
    visual.simulate_click(
        gpui_kit::point(pushed.center().x, viewport.top() + px(20.)),
        Modifiers::default(),
    );
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-content-0").is_some());
    position(&stream, 1, first_end + 10., &mut visual);
    assert!(visual.debug_bounds("run-stage-toggle-0").unwrap().bottom() < viewport.top());
    assert!(visual.debug_bounds("run-stage-content-0-1").unwrap().top() < viewport.bottom());
    visual.simulate_click(
        gpui_kit::point(pushed.center().x, viewport.top() + px(5.)),
        Modifiers::default(),
    );
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-content-0-1").is_some());
    let later_offset = f32::from(last.top() - owner.top()) + 80.;
    position(&stream, 1, later_offset, &mut visual);
    assert_eq!(
        visual.debug_bounds("run-stage-toggle-0").unwrap().top(),
        viewport.top()
    );
    click(&mut visual, "run-stage-toggle-0");
    assert!(visual.debug_bounds("run-stage-content-0-1").is_none());
    assert!(visual.debug_bounds(artifact_selector).is_some());
    assert!(visual.debug_bounds("run-stage-toggle-0").unwrap().top() >= viewport.top());
    stream.read_with(&visual, |stream, _| {
        assert_eq!(stream.list.logical_scroll_top().item_ix, 1);
        assert!(!stream.list.is_following_tail());
    });
}

#[gpui_kit::test]
async fn issue296_sticky_s03_final_owner_exit_and_next_run(cx: &mut TestAppContext) {
    let (window, stream) = fixture(cx, true);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "second".into(),
                seq: 3,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "second".into(),
                delta: "Second process".into(),
            },
            cx,
        );
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("second-{index}"), "pwd"),
                },
                cx,
            );
        }
        confirm_test_final(stream, "second", cx);
        stream.composer_submit_pending = true;
        stream.accept_composer_submission(&"Trailing content.\n\n".repeat(80), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    position(&stream, 0, 0., &mut visual);
    click(&mut visual, "run-activity-toggle");
    let natural = visual.debug_bounds("run-activity-group").unwrap();
    position(
        &stream,
        0,
        f32::from(natural.size.height) - 10.,
        &mut visual,
    );
    let viewport = stream.read_with(&visual, |stream, _| stream.list.viewport_bounds());
    let pushed = visual.debug_bounds("run-activity-toggle").unwrap();
    assert_eq!(pushed.bottom(), viewport.top() + px(10.));
    position(&stream, 0, f32::from(natural.size.height) + 5., &mut visual);
    if let Some(header) = visual.debug_bounds("run-activity-toggle") {
        assert!(header.bottom() <= viewport.top());
    }
    let index = stream.read_with(&visual, |stream, _| {
        stream
            .entries
            .iter()
            .zip(&stream.entry_identities)
            .position(|(entry, identity)| {
                matches!(entry, StreamEntry::RunActivity { .. })
                    && identity.message_id.as_deref() == Some("second")
            })
            .unwrap()
    });
    position(&stream, index, 0., &mut visual);
    click(&mut visual, "run-activity-toggle");
    position(&stream, index, 100., &mut visual);
    assert_eq!(
        visual.debug_bounds("run-activity-toggle").unwrap().top(),
        viewport.top()
    );
    click(&mut visual, "run-activity-toggle");
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.run_activity_groups["sticky"].read(cx).expanded());
        assert!(!stream.run_activity_groups["second"].read(cx).expanded());
        assert_eq!(stream.list.logical_scroll_top().item_ix, index);
    });
}
