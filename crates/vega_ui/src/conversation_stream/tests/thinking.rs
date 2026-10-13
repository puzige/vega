use super::*;
use crate::conversation_stream::thinking::{
    THINKING_BLOCK_BYTES, THINKING_DELTA_BYTES, THINKING_VIEW_BLOCKS, THINKING_VIEW_BYTES,
};

fn thinking(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::ThinkingDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

fn summary(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::SummaryDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

fn start(id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: id.into(),
        seq: 2,
    }
}

fn finish(id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: None,
    }
}

fn cards(stream: &ConversationStream, cx: &App) -> Vec<Entity<ThinkingBlock>> {
    stream
        .entries
        .iter()
        .flat_map(|entry| match entry {
            StreamEntry::RunActivitySegment { group, segment } => group
                .read(cx)
                .segment_children(*segment)
                .into_iter()
                .filter_map(|child| match child {
                    RunActivityChild::Thinking(card) => Some(card),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[gpui_kit::test]
async fn i61_thinking_blocks_follow_real_event_order_and_expand(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "thinking-timeline");
    stream.update(cx, |stream, cx| {
        for event in [
            start("m"),
            thinking("m", "先检查"),
            thinking("m", "条件"),
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "开始执行".into(),
            },
            ConversationEvent::ToolCallProposed {
                call: bash_call("thinking-tool", "pwd"),
            },
            thinking("m", "再验证结果"),
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "回答".into(),
            },
            finish("m"),
        ] {
            stream.apply_event(event, cx);
        }
    });
    cx.run_until_parked();
    let blocks = stream.read_with(cx, |stream, cx| {
        let kinds: Vec<_> = stream
            .entries
            .iter()
            .map(|entry| match entry {
                StreamEntry::RunActivity { .. } => "run-activity",
                StreamEntry::RunActivitySegment { .. } => "run-activity-segment",
                StreamEntry::Assistant { .. } => "text",
                _ => "unexpected",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "run-activity",
                "run-activity-segment",
                "text",
                "run-activity-segment",
                "text"
            ]
        );
        let proposal_order = stream
            .entries
            .iter()
            .flat_map(|entry| match entry {
                StreamEntry::RunActivity { .. } => Vec::new(),
                StreamEntry::RunActivitySegment { group, segment } => group
                    .read(cx)
                    .segment_children(*segment)
                    .into_iter()
                    .map(|child| match child {
                        RunActivityChild::Thinking(_) => "thinking".to_string(),
                        RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => {
                            "tool".to_string()
                        }
                        RunActivityChild::Artifact(_) => "artifact".to_string(),
                    })
                    .collect(),
                StreamEntry::Assistant { model, .. } => vec![
                    model
                        .committed_lines
                        .iter()
                        .chain(model.pending_lines.iter())
                        .flat_map(|line| line.spans.iter())
                        .map(|span| span.text.as_str())
                        .collect::<String>(),
                ],
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            proposal_order,
            ["thinking", "开始执行", "tool", "thinking", "回答"]
        );
        let activity_kinds = stream
            .run_activity_groups
            .get("m")
            .expect("run activity group")
            .read(cx)
            .children()
            .into_iter()
            .map(|child| match child {
                RunActivityChild::Thinking(_) => "thinking",
                RunActivityChild::Tool(_) => "tool",
                RunActivityChild::ToolGroup(_) => "tool-group",
                RunActivityChild::Artifact(_) => "artifact",
            })
            .collect::<Vec<_>>();
        assert_eq!(activity_kinds, ["thinking", "tool", "thinking"]);
        let cards = cards(stream, cx);
        assert_eq!(cards[0].read(cx).text, "先检查条件");
        assert_eq!(cards[1].read(cx).text, "再验证结果");
        assert!(stream.active_thinking.is_none());
        cards
    });
    open_run_details(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-timeline-segment-0").is_some());
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
    let toggle = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("real thinking header");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    let source = visual
        .debug_bounds("thinking-toggle")
        .expect("independent source toggle");
    visual.simulate_click(source.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_some());
    assert!(
        blocks
            .iter()
            .any(|block| block.read_with(cx, |block, _| block.expanded))
    );
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let toggle = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("thinking header");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("thinking-content")
            .is_some()
    );
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("thinking-content")
            .is_none()
    );
}

#[gpui_kit::test]
async fn i61_thinking_acceptance_fences_and_terminal_cleanup(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-fences");
    stream.update(cx, |stream, cx| {
        stream.apply_event(thinking("m", "before start"), cx);
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("foreign", "wrong message"), cx);
        stream.apply_event(thinking("m", ""), cx);
        assert!(cards(stream, cx).is_empty());
        stream.apply_event(thinking("m", "only reasoning"), cx);
        stream.apply_event(finish("foreign"), cx);
        assert!(stream.active_thinking.is_some());
        stream.apply_event(finish("m"), cx);
        stream.apply_event(finish("m"), cx);
        stream.apply_event(thinking("m", "late"), cx);
        assert_eq!(cards(stream, cx)[0].read(cx).text, "only reasoning");
        assert!(stream.active_thinking.is_none());
        assert!(stream.active_agent_message.is_none());
        stream.apply_event(start("next"), cx);
        stream.apply_event(thinking("m", "previous message"), cx);
        stream.apply_event(thinking("next", "cancelled reasoning"), cx);
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "next".into(),
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(thinking("next", "after cancel"), cx);
        assert_eq!(cards(stream, cx)[1].read(cx).text, "cancelled reasoning");
        assert!(stream.active_thinking.is_none());
        stream.apply_event(start("plain"), cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "plain".into(),
                delta: "ordinary answer".into(),
            },
            cx,
        );
        stream.apply_event(finish("plain"), cx);
        assert_eq!(cards(stream, cx).len(), 2);
        stream.apply_event(start("failed"), cx);
        stream.apply_event(thinking("failed", "retained before error"), cx);
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("failed".into()),
                error: Arc::new(std::io::Error::other("owned thinking test failure").into()),
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(thinking("failed", "after error"), cx);
        assert_eq!(cards(stream, cx)[2].read(cx).text, "retained before error");
        assert!(stream.active_thinking.is_none());
    });
    // A separate route has no access to the first route's live-only blocks.
    let (_, other, _) = open_controller_stream(cx, "thinking-other-thread");
    assert!(other.read_with(cx, |stream, cx| cards(stream, cx).is_empty()));
}

#[gpui_kit::test]
async fn i61_utf8_near_view_limit_does_not_create_empty_thinking_block(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-utf8-limit");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        for block in 0..4 {
            for chunk in 0..4 {
                let bytes = if block == 3 && chunk == 3 {
                    THINKING_DELTA_BYTES - 1
                } else {
                    THINKING_DELTA_BYTES
                };
                stream.apply_event(thinking("m", &"x".repeat(bytes)), cx);
            }
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "m".into(),
                    delta: "answer boundary".into(),
                },
                cx,
            );
        }
        assert_eq!(stream.thinking_bytes, THINKING_VIEW_BYTES - 1);
        stream.apply_event(thinking("m", "界"), cx);
        assert_eq!(cards(stream, cx).len(), 4);
        assert!(
            cards(stream, cx)
                .iter()
                .all(|card| !card.read(cx).text.is_empty())
        );
        assert!(
            cards(stream, cx)
                .last()
                .expect("retained block")
                .read(cx)
                .truncated
        );
        assert!(stream.active_thinking.is_none());
    });
}

#[gpui_kit::test]
async fn i61_thinking_bounds_are_utf8_safe_and_cumulative(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-bounds");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", &"界".repeat(THINKING_DELTA_BYTES)), cx);
        let card = cards(stream, cx)[0].clone();
        assert!(card.read(cx).text.len() <= THINKING_DELTA_BYTES);
        assert!(card.read(cx).truncated);
        for _ in 0..8 {
            stream.apply_event(thinking("m", &"x".repeat(THINKING_DELTA_BYTES)), cx);
        }
        assert_eq!(card.read(cx).text.len(), THINKING_BLOCK_BYTES);
        stream.apply_event(finish("m"), cx);
        for n in 0..8 {
            let id = format!("next-{n}");
            stream.apply_event(start(&id), cx);
            for _ in 0..4 {
                stream.apply_event(thinking(&id, &"x".repeat(THINKING_DELTA_BYTES)), cx);
            }
            stream.apply_event(finish(&id), cx);
        }
        assert_eq!(stream.thinking_bytes, THINKING_VIEW_BYTES);
        assert_eq!(
            cards(stream, cx)
                .iter()
                .map(|card| card.read(cx).text.len())
                .sum::<usize>(),
            THINKING_VIEW_BYTES
        );
    });
    let (_, stream, _) = open_controller_stream(cx, "thinking-block-count");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        for _ in 0..THINKING_VIEW_BLOCKS + 10 {
            stream.apply_event(thinking("m", "x"), cx);
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "m".into(),
                    delta: "y".into(),
                },
                cx,
            );
        }
        assert_eq!(cards(stream, cx).len(), THINKING_VIEW_BLOCKS);
        assert!(
            cards(stream, cx)
                .last()
                .expect("last bounded block")
                .read(cx)
                .truncated
        );
    });
}

#[gpui_kit::test]
async fn issue103_thinking_body_is_bounded(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue103-thinking");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", &"reasoning line\n\n".repeat(80)), cx);
        stream.run_activity_groups.values().for_each(|group| {
            group.update(cx, |group, cx| group.reveal_segment(0, cx));
        });
        cards(stream, cx)[0].update(cx, |card, cx| card.set_expanded(true, cx));
    });
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, cx| cards(stream, cx)[0].read(cx).expanded));
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let body = visual.debug_bounds("thinking-content").expect("body");
    assert!(
        body.size.height <= px(240.),
        "thinking body: {:?}",
        body.size.height
    );
}

#[gpui_kit::test]
async fn issue103_thinking_scroll_survives_streaming_and_reopen(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue103-thinking-scroll");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", &"reasoning line\n\n".repeat(80)), cx);
        stream.run_activity_groups.values().for_each(|group| {
            group.update(cx, |group, cx| group.reveal_segment(0, cx));
        });
        cards(stream, cx)[0].update(cx, |card, cx| card.set_expanded(true, cx));
    });
    cx.run_until_parked();
    let block = stream.read_with(cx, |stream, cx| cards(stream, cx)[0].clone());
    assert!(block.read_with(cx, |block, _| block.expanded));
    let scroll = block.read_with(cx, |block, _| block.scroll_handle());
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let toggle = visual.debug_bounds("run-stage-toggle-0").expect("header");
    let body = visual.debug_bounds("thinking-content").expect("body");
    assert!(
        scroll.max_offset().y > px(1000.),
        "full reasoning remains reachable"
    );
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: body.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100.))),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-100.));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", &"new reasoning\n\n".repeat(20)), cx)
    });
    visual.run_until_parked();
    assert_eq!(
        scroll.offset().y,
        px(-100.),
        "streaming must not follow the bottom"
    );
    assert_eq!(
        visual.debug_bounds("run-stage-toggle-0").expect("header"),
        toggle
    );
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-100.), "reopen retains offset");
    block.update(&mut visual, |block, cx| {
        block.text = "short".into();
        block.append("", false, 0);
        cx.notify();
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(0.));
    assert_eq!(scroll.max_offset().y, px(0.));
    assert!(
        visual
            .debug_bounds("thinking-content")
            .expect("short body")
            .size
            .height
            < px(240.)
    );
    block.update(&mut visual, |block, cx| {
        block.text.clear();
        block.append("", false, 0);
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
}

#[gpui_kit::test]
async fn i71_live_heading_replaces_thinking_label_without_changing_activity_expansion(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "summary-live-title");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "## 检"), cx);
        stream.apply_event(thinking("m", "查条件"), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "检查条件"
    );
    assert!(
        !stream.read_with(cx, |stream, app| cards(stream, app)[0].read(app).expanded),
        "v3 thought details remain collapsed while their title updates"
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(thinking("m", "\n\n## 现在验证结果"), cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "现在验证结果"
    );
    open_run_details(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let toggle = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("production toggle");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    let source = visual.debug_bounds("thinking-toggle").unwrap();
    visual.simulate_click(source.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_some());
}

#[gpui_kit::test]
async fn i343_live_stage_title_follows_latest_readable_thinking_or_summary(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "live-thinking-title");
    cx.update(|cx| {
        cx.set_reduce_motion(false);
        cx.set_global(shimmer::PaintProbe::default());
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "## Aliyun server:"), cx);
    });
    open_run_details(window, cx);
    for (event, expected) in [
        (summary("m", " \n## "), "Aliyun server:"),
        (
            thinking("m", "\nCache hit rate is unclear."),
            "Cache hit rate is unclear.",
        ),
        (summary("m", "Summary A"), "Summary A"),
        (
            thinking("m", " Further investigation"),
            "Cache hit rate is unclear. Further investigation",
        ),
        (summary("m", "\nSummary B"), "Summary B"),
        (
            thinking("m", "\n\n```rust\n## fake heading\n```\n**"),
            "Summary B",
        ),
    ] {
        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        cx.run_until_parked();
        stream.read_with(cx, |stream, app| {
            assert_eq!(cards(stream, app)[0].read(app).title(), expected);
        });
        assert!(
            gpui_kit::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("run-stage-title-0")
                .is_some()
        );
        cx.update(|cx| cx.global::<shimmer::PaintProbe>().0.borrow_mut().clear());
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            let samples = cx.global::<shimmer::PaintProbe>().0.borrow();
            assert_eq!(
                samples
                    .iter()
                    .rev()
                    .find(|sample| sample.moving)
                    .map(|sample| sample.text.as_str()),
                Some(expected)
            );
        });
    }
    stream.update(cx, |stream, cx| {
        stream.apply_event(finish("m"), cx);
        stream.apply_event(thinking("m", "\nlate content"), cx);
    });
    stream.read_with(cx, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "Summary B");
        assert!(card.source_text().contains("Further investigation"));
        assert!(card.source_text().contains("Summary B"));
    });
}

#[gpui_kit::test]
async fn i343_latest_readable_source_fallback_bounds_and_terminal(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "summary-priority");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "initial useful fragment"), cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "initial useful fragment"
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: " \n**".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "检查摘要**\nbody under heading".into(),
            },
            cx,
        );
        assert_eq!(cards(stream, cx)[0].read(cx).title(), "body under heading");
        stream.apply_event(thinking("m", "\n## raw now takes over"), cx);
        assert_eq!(cards(stream, cx)[0].read(cx).title(), "raw now takes over");
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "\n```rust\n# not a heading\n```\n## 下个阶段\n正文".into(),
            },
            cx,
        );
        stream.apply_event(finish("m"), cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "\n## stale".into(),
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "正文"
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("next"), cx);
        stream.apply_event(thinking("next", &"中文 fragment ".repeat(150)), cx);
        stream.apply_event(thinking("next", "最新尾部"), cx);
        stream.apply_event(thinking("next", "\n\n**"), cx);
    });
    stream.read_with(cx, |stream, app| {
        let cards = cards(stream, app);
        let title = cards[1].read(app).title();
        assert!(title.chars().count() <= 96);
        assert!(title.ends_with("最新尾部"));
    });
}

#[gpui_kit::test]
async fn i71_summary_and_thinking_share_display_budget(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "summary-shared-limit");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        for n in 0..8 {
            let delta = "界".repeat(THINKING_DELTA_BYTES / 3);
            let event = if n % 2 == 0 {
                thinking("m", &delta)
            } else {
                ConversationEvent::SummaryDelta {
                    message_id: "m".into(),
                    delta,
                }
            };
            stream.apply_event(event, cx);
        }
    });
    stream.read_with(cx, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert!(card.truncated);
        assert!(stream.thinking_bytes <= THINKING_BLOCK_BYTES);
        assert!(card.title().chars().count() <= 96);
    });
}

#[gpui_kit::test]
async fn i71_fenced_summary_keeps_received_content(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-fenced-body");
    let content = "```rust\nlet foo_bar = 1;\n```";
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: content.into(),
            },
            cx,
        );
        stream.run_activity_groups.values().for_each(|group| {
            group.update(cx, |group, cx| group.reveal_segment(0, cx));
        });
        cards(stream, cx)[0].update(cx, |card, cx| card.set_expanded(true, cx));
    });
    stream.read_with(cx, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "思考过程");
        assert_eq!(card.visible_text(), content);
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("thinking-content").is_some());
}

#[gpui_kit::test]
async fn issue296_v3_thinking_follows_only_when_reader_is_at_its_tail(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-thinking-tail");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", &"reasoning line\n\n".repeat(80)), cx);
        stream.run_activity_groups.values().for_each(|group| {
            group.update(cx, |group, cx| group.reveal_segment(0, cx));
        });
        cards(stream, cx)[0].update(cx, |card, cx| card.set_expanded(true, cx));
    });
    cx.run_until_parked();
    let block = stream.read_with(cx, |stream, cx| cards(stream, cx)[0].clone());
    let scroll = block.read_with(cx, |block, _| block.scroll_handle());
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    scroll.set_offset(gpui_kit::point(px(0.), -scroll.max_offset().y));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", &"new reasoning\n\n".repeat(20)), cx)
    });
    visual.run_until_parked();
    assert!((scroll.offset().y + scroll.max_offset().y).abs() <= px(1.));
    scroll.set_offset(gpui_kit::point(px(0.), px(-100.)));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "another line\n"), cx)
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-100.));
}

#[gpui_kit::test]
async fn issue296_v3_thinking_short_content_follows_through_first_overflow(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-thinking-first-overflow");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "first short line\n"), cx);
        stream.run_activity_groups.values().for_each(|group| {
            group.update(cx, |group, cx| group.reveal_segment(0, cx));
        });
        cards(stream, cx)[0].update(cx, |card, cx| card.set_expanded(true, cx));
    });
    cx.run_until_parked();
    let block = stream.read_with(cx, |stream, cx| cards(stream, cx)[0].clone());
    let scroll = block.read_with(cx, |block, _| block.scroll_handle());
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(scroll.max_offset().y, px(0.));
    assert!(
        visual
            .debug_bounds("thinking-content")
            .expect("expanded")
            .size
            .height
            < px(240.)
    );
    for _ in 0..18 {
        stream.update(&mut visual, |stream, cx| {
            stream.apply_event(thinking("m", "another short line\n\n"), cx);
        });
        visual.run_until_parked();
        assert!((scroll.offset().y + scroll.max_offset().y).abs() <= px(1.));
    }
    assert!(scroll.max_offset().y > px(0.));
    assert_eq!(
        visual
            .debug_bounds("thinking-content")
            .expect("bounded")
            .size
            .height,
        px(240.)
    );
    scroll.set_offset(gpui_kit::point(px(0.), px(-24.)));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "reader stays here\n"), cx);
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-24.));
}
