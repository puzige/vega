use super::*;
use crate::conversation_stream::thinking::{
    THINKING_BLOCK_BYTES, THINKING_DELTA_BYTES, THINKING_VIEW_BLOCKS, THINKING_VIEW_BYTES,
};

pub(super) fn thinking(id: &str, delta: &str) -> ConversationEvent {
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

pub(super) fn start(id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: id.into(),
        seq: 2,
    }
}

pub(super) fn finish(id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: None,
    }
}

pub(super) fn cards(stream: &ConversationStream, cx: &App) -> Vec<Entity<ThinkingBlock>> {
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
async fn i61_utf8_near_view_limit_yields_old_body_for_latest_thinking(cx: &mut TestAppContext) {
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
        assert_eq!(cards(stream, cx).len(), 5);
        assert!(cards(stream, cx)[0].read(cx).text.is_empty());
        assert!(cards(stream, cx)[0].read(cx).truncated);
        assert_eq!(cards(stream, cx)[4].read(cx).text, "界");
        assert!(stream.active_thinking.is_some());
        assert!(stream.thinking_bytes <= THINKING_VIEW_BYTES);
    });
}

#[gpui_kit::test]
async fn i61_thinking_bounds_are_utf8_safe_and_cumulative(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-bounds");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        let mut received = "界".repeat(THINKING_DELTA_BYTES);
        stream.apply_event(thinking("m", &received), cx);
        let card = cards(stream, cx)[0].clone();
        assert_eq!(card.read(cx).text.len(), 3 * THINKING_DELTA_BYTES);
        assert!(!card.read(cx).truncated);
        for chunk in 0..8 {
            let delta = format!("{}{chunk}", "x".repeat(THINKING_DELTA_BYTES - 1));
            received.push_str(&delta);
            stream.apply_event(thinking("m", &delta), cx);
            let retained = &card.read(cx).text;
            assert!(retained.len() >= THINKING_BLOCK_BYTES - THINKING_DELTA_BYTES);
            assert!(retained.len() <= THINKING_BLOCK_BYTES);
            let start = received.len() - retained.len();
            assert!(received.is_char_boundary(start));
            assert_eq!(retained, &received[start..]);
            assert!(retained.ends_with(&delta));
            assert_eq!(stream.thinking_bytes, retained.len());
        }
        assert!(card.read(cx).truncated);
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
        let retained = cards(stream, cx);
        assert_eq!(retained.len(), THINKING_VIEW_BLOCKS + 10);
        assert_eq!(
            retained
                .iter()
                .filter(|card| card.read(cx).has_source_text())
                .count(),
            THINKING_VIEW_BLOCKS
        );
        assert!(
            retained
                .iter()
                .take(10)
                .all(|card| card.read(cx).truncated && !card.read(cx).has_source_text())
        );
        assert_eq!(retained.last().expect("latest block").read(cx).text, "x");
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
        block.test_replace_source("short", cx);
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
        block.test_replace_source("", cx);
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
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(0, app)
        }),
        "Thinking"
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
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(0, app)
        }),
        "Thinking"
    );
    open_run_details(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let toggle = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("production toggle");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_some());
}

#[gpui_kit::test]
async fn raw_reasoning_stage_keeps_preview_inside_manual_disclosure(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "raw-reasoning-stage");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "Reviewing files"), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Thinking");
        assert!(!group.segment_expanded(0));
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nReading implementation"), cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        assert_eq!(
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(0, app),
            "Thinking"
        );
        assert_eq!(
            cards(stream, app)[0].read(app).title(),
            "Reading implementation"
        );
    });
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nChecking result"), cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Thinking");
        assert!(group.segment_expanded(0));
        assert_eq!(cards(stream, app)[0].read(app).title(), "Checking result");
    });
    assert!(visual.debug_bounds("thinking-content").is_some());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nLater result"), cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    stream.read_with(&visual, |stream, app| {
        assert!(
            !stream.run_activity_groups["m"]
                .read(app)
                .segment_expanded(0)
        );
        assert_eq!(cards(stream, app)[0].read(app).title(), "Later result");
    });
}

#[gpui_kit::test]
async fn summary_takeover_releases_hidden_stage_reading_focus(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-takeover-focus");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "Checking source"), cx);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        let group = stream.read(cx).run_activity_groups["m"].clone();
        assert!(group.read(cx).reading(window, cx));
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(summary("m", "Checking result"), cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_none());
    visual.update(|window, cx| {
        let group = stream.read(cx).run_activity_groups["m"].clone();
        assert!(!group.read(cx).reading(window, cx));
    });
    stream.update(&mut visual, |stream, cx| {
        stream.resume_tail(cx);
        confirm_test_final(stream, "m", cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["m"]
                .read(cx)
                .process_in_details()
        );
    });
}

#[gpui_kit::test]
async fn summary_stage_nests_tools_without_duplicate_reasoning(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-stage-tools");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(summary("m", "Reviewing issue"), cx);
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Reviewing issue");
        assert!(!group.segment_expanded(0));
    });
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(stream.read_with(&visual, |stream, app| {
        !stream.run_activity_groups["m"]
            .read(app)
            .segment_expanded(0)
    }));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("summary-tool", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(summary("m", "Writing the fix"), cx);
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "summary-tool".into(),
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
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "run commands");
        assert!(!group.segment_expanded(0));
        assert!(stream.run_notice_projection(app).is_empty());
        let kinds = group
            .segment_children(0)
            .into_iter()
            .map(|child| match child {
                RunActivityChild::Thinking(_) => "thinking",
                RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => "tool",
                RunActivityChild::Artifact(_) => "artifact",
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["thinking", "tool", "thinking"]);
    });
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "Answer begins.".into(),
            },
            cx,
        );
        stream.apply_event(summary("m", "Checking after answer"), cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        assert_eq!(
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(1, app),
            "Checking after answer"
        );
        assert!(matches!(stream.entries[2], StreamEntry::Assistant { .. }));
    });
}

#[gpui_kit::test]
async fn live_stage_title_switches_to_summary_without_revealing_raw_thinking(
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
    for (event, expected, stage_title) in [
        (summary("m", " \n## "), "Aliyun server:", "Thinking"),
        (
            thinking("m", "\nCache hit rate is unclear."),
            "Cache hit rate is unclear.",
            "Thinking",
        ),
        (summary("m", "Summary A"), "Summary A", "Summary A"),
        (
            thinking("m", " Further investigation"),
            "Summary A",
            "Summary A",
        ),
        (summary("m", "\nSummary B"), "Summary B", "Summary B"),
        (
            thinking("m", "\n\n```rust\n## fake heading\n```\n**"),
            "Summary B",
            "Summary B",
        ),
    ] {
        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        cx.run_until_parked();
        stream.read_with(cx, |stream, app| {
            let card = cards(stream, app)[0].read(app);
            assert_eq!(card.title(), expected);
            assert!(!card.expanded);
            assert_eq!(
                stream.run_activity_groups["m"]
                    .read(app)
                    .stage_title(0, app),
                stage_title
            );
        });
        assert!(
            gpui_kit::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("thinking-content")
                .is_none()
        );
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
                Some(stage_title)
            );
        });
    }
    stream.read_with(cx, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "Summary B");
        assert!(card.text.contains("Further investigation"));
        assert_eq!(card.source_text(), " \n## Summary A\nSummary B");
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let stage = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("stage disclosure");
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_none());
    assert!(stream.read_with(&visual, |stream, app| {
        !stream.run_activity_groups["m"]
            .read(app)
            .segment_expanded(0)
    }));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(finish("m"), cx);
        stream.apply_event(thinking("m", "\nlate content"), cx);
    });
    stream.read_with(&visual, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "Summary B");
        assert_eq!(card.source_text(), " \n## Summary A\nSummary B");
    });
}

#[gpui_kit::test]
async fn summary_replacement_preserves_fallback_bounds_and_terminal(cx: &mut TestAppContext) {
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
        stream.apply_event(summary("m", " \n"), cx);
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "initial useful fragment");
        assert_eq!(card.source_text(), "initial useful fragment");
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "**".into(),
            },
            cx,
        );
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "initial useful fragment");
        assert_eq!(card.source_text(), " \n**");
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "检查摘要**\nbody under heading".into(),
            },
            cx,
        );
        assert_eq!(cards(stream, cx)[0].read(cx).title(), "body under heading");
        stream.apply_event(thinking("m", "\n## raw now takes over"), cx);
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "body under heading");
        assert!(!card.source_text().contains("raw now takes over"));
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
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
        let card = cards[1].read(app);
        assert!(card.title().chars().count() <= 96);
        assert!(card.title().ends_with("最新尾部"));
        assert_eq!(card.source_text(), card.text);
        assert_eq!(card.test_source_copy().original_text(), card.text);
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
        assert_eq!(card.title(), "Thinking");
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
