use super::*;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "visual".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "visual".into(),
            started_at: Instant::now() - Duration::from_secs(12),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "visual".into(),
            delta: "检查合成项目。\n\n记录测试结果。".into(),
        },
        cx,
    );
}

#[gpui_kit::test]
async fn issue296_visual_v01_clock_divider_column_and_no_role(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-clock");
    stream.update(cx, begin);
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(1400.), px(900.)));
    visual.run_until_parked();
    let column = visual.debug_bounds("conversation-column").unwrap();
    let header = visual.debug_bounds("run-activity-toggle").unwrap();
    let divider = visual
        .debug_bounds("run-activity-divider")
        .expect("plain clock divider");
    let body = visual.debug_bounds("assistant-body-1").unwrap();
    assert_eq!(column.size.width, px(736.));
    assert_eq!(header.size.height, px(23.));
    assert_eq!(divider.size.height, px(1.));
    assert_eq!(divider.left(), column.left());
    assert_eq!(body.left(), column.left());
    assert!(visual.debug_bounds("run-activity-icon").is_none());
    assert!(visual.debug_bounds("assistant-role-1").is_none());
    assert_eq!(divider.top() - header.bottom(), px(4.5));
}

#[gpui_kit::test]
async fn issue296_visual_v03_summary_nests_tools_until_open(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-stage");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "visual".into(),
                delta: "检查项目配置".into(),
            },
            cx,
        );
        for id in ["one", "two"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let stage = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("inline stage");
    assert!(visual.debug_bounds("run-stage-title-0").is_some());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("tool-activity-single-row").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    let first = visual
        .debug_bounds("run-stage-tool-0-0")
        .expect("first direct tool");
    let second = visual
        .debug_bounds("run-stage-tool-0-1")
        .expect("second direct tool");
    assert_eq!(second.top() - first.top(), px(25.));
    assert_eq!(first.left(), stage.left());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
}

fn summary(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::SummaryDelta {
            message_id: "visual".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn click_stage(visual: &mut gpui_kit::VisualTestContext) {
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn issue296_visual_v02_text_bubble_geometry_colors_and_original(cx: &mut TestAppContext) {
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    let user = "合成用户请求保留换行和完整原文。".repeat(30);
    selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| *target = Some((user.clone(), None)));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_history_page(hydration_page(vec![hydration_user(0, &user)], None), cx);
        begin(stream, cx);
    });
    visual.simulate_resize(gpui_kit::size(px(1400.), px(1800.)));
    visual.run_until_parked();
    let column = visual.debug_bounds("conversation-column").unwrap();
    let bubble = visual.debug_bounds("user-message-bubble").unwrap();
    assert!((bubble.size.width - column.size.width * 0.7).abs() < px(1.));
    assert_eq!(bubble.right(), column.right());
    let layouts = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        ["检查合成项目。", "记录测试结果。"].map(|text| {
            runs.iter()
                .rev()
                .find(|(value, _, _)| value == text)
                .unwrap()
                .1
                .clone()
        })
    });
    let user_layout = selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| target.take().unwrap().1.unwrap());
    assert_eq!(user_layout.bounds().left() - bubble.left(), px(16.));
    for layout in &layouts {
        assert_eq!(layout.line_layouts()[0].font_size(), px(13.5));
        assert_eq!(layout.line_height(), px(23.));
    }
    assert_eq!(
        layouts[1].bounds().top() - layouts[0].bounds().top(),
        px(38.5)
    );
    visual.update(|window, cx| {
        let colors = theme(cx).colors;
        assert!(
            window
                .painted_quads()
                .iter()
                .any(|quad| quad.bounds == bubble.scale(window.scale_factor())
                    && quad.background.as_solid() == Some(colors.conversation_bubble.into())
                    && quad.corner_radii.top_left.as_f32() / window.scale_factor() == 16.)
        );
    });
    stream.read_with(&visual, |stream, _| {
        let StreamEntry::User { copy, .. } = &stream.entries[0] else {
            panic!("user")
        };
        assert_eq!(copy.original_text(), user);
    });
    let start = layouts[0].position_for_index(0).unwrap() + gpui_kit::point(px(0.5), px(11.5));
    let end = layouts[0]
        .position_for_index("检查合成项目。".len())
        .unwrap()
        + gpui_kit::point(px(1.), px(11.5));
    visual.simulate_mouse_down(start, MouseButton::Left, gpui_kit::Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), gpui_kit::Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, gpui_kit::Modifiers::default());
    visual.run_until_parked();
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("检查合成项目。")
    );
}

#[gpui_kit::test]
async fn summary_replaces_raw_source_across_multiple_stages(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-sources");
    let (first, second) = stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "First phase", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "Original independent reasoning.".into(),
            },
            cx,
        );
        summary(stream, "Second phase\nFull summary continuation.", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("two", "pwd"),
            },
            cx,
        );
        let cards = stream.run_activity_groups["visual"]
            .read(cx)
            .segment_children(0)
            .into_iter()
            .filter_map(|child| {
                if let RunActivityChild::Thinking(card) = child {
                    Some(card)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        (cards[0].clone(), cards[1].clone())
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", first.entity_id()).into_boxed_str(),
            ))
            .is_none()
    );
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", second.entity_id()).into_boxed_str(),
            ))
            .is_none()
    );
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    first.read_with(&visual, |card, _| {
        assert_eq!(card.source_text(), "First phase")
    });
    second.read_with(&visual, |card, _| {
        assert_eq!(
            card.source_text(),
            "Second phase\nFull summary continuation."
        )
    });
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .stage_title(0, cx),
            "Waiting to run 2 commands"
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "Later raw reasoning.".into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["visual"].read(cx);
        assert_eq!(group.stage_title(0, cx), "Waiting to run 2 commands");
        assert_eq!(
            group
                .segment_children(0)
                .iter()
                .filter(|child| matches!(child, RunActivityChild::Thinking(_)))
                .count(),
            3
        );
    });
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    click_stage(&mut visual);
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_some());
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", second.entity_id()).into_boxed_str(),
            ))
            .is_some()
    );
}

#[gpui_kit::test]
async fn issue296_visual_v06_visible_tool_reading_defers_final(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-hidden-reading");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "检查配置", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    click_stage(&mut visual);
    let tool = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    visual.simulate_click(tool.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-tool-0-0-detail").is_some());
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    stream.update(&mut visual, |stream, cx| {
        confirm_test_final(stream, "visual", cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.tool_cards["one"].read(cx).is_expanded());
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .collapse_pending()
        );
        assert!(
            !stream.run_activity_groups["visual"]
                .read(cx)
                .process_in_details()
        );
    });
    assert!(visual.debug_bounds("run-stage-tool-0-0-detail").is_some());
    let tool = visual.debug_bounds("run-stage-tool-0-0").unwrap();
    visual.simulate_click(tool.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    click_stage(&mut visual);
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .process_in_details()
        );
    });
}

#[gpui_kit::test]
async fn issue296_visual_v06_stage_anchor_reveals_inline_and_focus_defers_final(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "visual-stage-anchor");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "检查锚点".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
        let index = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
            .unwrap();
        let anchor = ThreadScrollAnchor {
            identity: Some(stream.entry_identities[index].key.clone()),
            message_id: Some("visual".into()),
            offset_in_item_px: 0.,
            following_tail: false,
        };
        assert!(stream.restore_process_anchor(&anchor, cx));
        assert_eq!(stream.list.logical_scroll_top().item_ix, index);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    click_stage(&mut visual);
    click_stage(&mut visual);
    stream.update(&mut visual, |stream, cx| {
        stream.resume_tail(cx);
        confirm_test_final(stream, "visual", cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .collapse_pending()
        )
    });
    click_stage(&mut visual);
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .process_in_details()
        )
    });
}

#[gpui_kit::test]
async fn issue296_visual_v07_stage_shimmer_theme_reduce_motion_and_hover(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-shimmer");
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "检查活动摘要".into(),
            },
            cx,
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        for reduce in [false, true] {
            visual.update(|window, cx| {
                cx.set_global(palette);
                cx.set_reduce_motion(reduce);
                cx.set_global(shimmer::PaintProbe::default());
                window.simulate_next_frame(cx);
            });
            visual.run_until_parked();
            let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
            let title = visual.debug_bounds("run-stage-title-0").unwrap();
            let arrow = visual.debug_bounds("run-stage-chevron-0").unwrap();
            visual.simulate_mouse_move(stage.center(), None, gpui_kit::Modifiers::default());
            visual.run_until_parked();
            assert!(issue296_single_activity::chevron_painted(
                &mut visual,
                "run-stage-chevron-0"
            ));
            assert_eq!(visual.debug_bounds("run-stage-title-0"), Some(title));
            assert_eq!(visual.debug_bounds("run-stage-chevron-0"), Some(arrow));
            visual.update(|window, cx| {
                assert!(
                    !window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.bounds == stage.scale(window.scale_factor())
                            && quad.background.as_solid() == Some(colors.bg_hover.into()))
                );
                let samples = cx.global::<shimmer::PaintProbe>().0.borrow();
                let sample = samples.last().expect("stage paint");
                assert_eq!(sample.moving, !reduce);
                for color in &sample.colors {
                    let color = gpui_kit::Rgba::from(*color);
                    assert!(
                        color.r >= colors.conversation_stage.r.min(colors.bg_base.r) - 0.001
                            && color.r <= colors.conversation_stage.r.max(colors.bg_base.r) + 0.001
                    );
                }
                if reduce {
                    assert!(
                        sample
                            .colors
                            .iter()
                            .all(|color| *color == colors.conversation_stage.into())
                    );
                }
            });
            visual.simulate_mouse_move(
                gpui_kit::point(px(1.), px(1.)),
                None,
                gpui_kit::Modifiers::default(),
            );
            visual.run_until_parked();
            assert!(!issue296_single_activity::chevron_painted(
                &mut visual,
                "run-stage-chevron-0"
            ));
        }
    }
}

#[gpui_kit::test]
async fn issue296_visual_v05_two_stages_final_reopens_source_order(cx: &mut TestAppContext) {
    use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};
    let (window, stream, _) = open_controller_stream(cx, "visual-final");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "visual".into(),
                seq: 1,
            },
            cx,
        );
        for (index, (body, title, id)) in
            [("A", "First phase", "one"), ("B", "Second phase", "two")]
                .into_iter()
                .enumerate()
        {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "visual".into(),
                    delta: body.into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: "visual".into(),
                    response: AssistantResponseMetadata {
                        response_id: id.into(),
                        text_start_bytes: index,
                        text_end_bytes: index + 1,
                        kind: AssistantResponseKind::Commentary,
                    },
                },
                cx,
            );
            summary(stream, title, cx);
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let a = visual.debug_bounds("assistant-body-1").unwrap();
    let first = visual.debug_bounds("run-stage-toggle-0").unwrap();
    let b = visual.debug_bounds("assistant-body-3").unwrap();
    let second = visual.debug_bounds("run-stage-toggle-1").unwrap();
    assert!(a.bottom() <= first.top() && first.bottom() <= b.top() && b.bottom() <= second.top());
    stream.update(&mut visual, |stream, cx| {
        confirm_test_final(stream, "visual", cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_none());
    assert!(visual.debug_bounds("run-stage-toggle-1").is_none());
    let clock = visual.debug_bounds("run-activity-toggle").unwrap();
    visual.simulate_click(clock.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        let details = stream.run_details(&stream.run_activity_groups["visual"], cx);
        let kinds = details
            .iter()
            .map(|(_, detail)| match detail {
                phase::RunDetail::Assistant(index) => match &stream.entries[*index] {
                    StreamEntry::Assistant { copy, .. } => copy.original_text().to_owned(),
                    _ => panic!("body"),
                },
                phase::RunDetail::Operation(RunActivityChild::Thinking(card)) => {
                    card.read(cx).source_text()
                }
                phase::RunDetail::Operation(RunActivityChild::Tool(_)) => "tool".into(),
                _ => panic!("unexpected detail"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            ["A", "First phase", "tool", "B", "Second phase", "tool"]
        );
    });
}

#[gpui_kit::test]
async fn issue296_visual_v08_markers_stay_at_pane_edge_inside_narrow_window(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "visual-markers");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (1..=30)
                    .map(|index| hydration_user(index, "Synthetic message"))
                    .collect(),
                None,
            ),
            cx,
        )
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    for width in [1400., 480.] {
        visual.simulate_resize(gpui_kit::size(px(width), px(800.)));
        visual.run_until_parked();
        let rail = visual.debug_bounds("message-anchor-rail").unwrap();
        assert_eq!(rail.center().x, px(16.));
        assert_eq!(rail.size.height, px(90.));
        let mut ticks = visual.update(|window, cx| {
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    quad.bounds.size.width.as_f32() == 6. * window.scale_factor()
                        && quad.bounds.size.height.as_f32() == 2. * window.scale_factor()
                        && quad.background.as_solid()
                            == Some(theme(cx).colors.conversation_marker.into())
                })
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>()
        });
        ticks.sort_by(|a, b| a.top().partial_cmp(&b.top()).unwrap());
        assert_eq!(ticks.len(), 8);
        let scale = visual.update(|window, _| window.scale_factor());
        assert!(ticks.windows(2).all(|pair| {
            [10. * scale, 20. * scale].contains(&(pair[1].top().as_f32() - pair[0].top().as_f32()))
        }));
    }
}

#[gpui_kit::test]
async fn issue296_visual_v06_new_thinking_stays_closed_until_independent_reading(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "visual-follow-new-thought");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "First phase".into(),
            },
            cx,
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    click_stage(&mut visual);
    let card = stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "new reasoning\n".into(),
            },
            cx,
        );
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .segment_expanded(0)
        );
        let card = stream.active_thinking.clone().unwrap();
        assert!(!card.read(cx).expanded);
        card
    });
    visual.run_until_parked();
    let row = visual
        .debug_bounds(Box::leak(
            format!("run-stage-source-{}", card.entity_id()).into_boxed_str(),
        ))
        .unwrap();
    visual.simulate_click(row.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(card.read_with(&visual, |card, _| card.expanded));
    let scroll = card.read_with(&visual, |card, _| card.scroll_handle());
    for _ in 0..20 {
        stream.update(&mut visual, |stream, cx| {
            stream.apply_event(
                ConversationEvent::ThinkingDelta {
                    message_id: "visual".into(),
                    delta: "another reasoning line\n\n".into(),
                },
                cx,
            )
        });
        visual.run_until_parked();
        assert!((scroll.offset().y + scroll.max_offset().y).abs() <= px(1.));
    }
    assert!(scroll.max_offset().y > px(0.));
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    stream.update(&mut visual, |stream, cx| {
        confirm_test_final(stream, "visual", cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .collapse_pending()
        )
    });
}
