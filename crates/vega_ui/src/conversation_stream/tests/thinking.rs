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
    }
}

fn cards(stream: &ConversationStream) -> Vec<Entity<ThinkingBlock>> {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Thinking { card } => Some(card.clone()),
            _ => None,
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
                StreamEntry::Thinking { .. } => "thinking",
                StreamEntry::Assistant { .. } => "text",
                StreamEntry::Tool { .. } => "tool",
                StreamEntry::ToolGroup { .. } => "tool-group",
                _ => "unexpected",
            })
            .collect();
        assert_eq!(kinds, ["thinking", "text", "tool", "thinking", "text"]);
        let cards = cards(stream);
        assert_eq!(cards[0].read(cx).text, "先检查条件");
        assert_eq!(cards[1].read(cx).text, "再验证结果");
        assert!(stream.active_thinking.is_none());
        cards
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("thinking-block").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
    let toggle = visual
        .debug_bounds("thinking-toggle")
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
        .debug_bounds("thinking-toggle")
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
        assert!(cards(stream).is_empty());
        stream.apply_event(thinking("m", "only reasoning"), cx);
        stream.apply_event(finish("foreign"), cx);
        assert!(stream.active_thinking.is_some());
        stream.apply_event(finish("m"), cx);
        stream.apply_event(finish("m"), cx);
        stream.apply_event(thinking("m", "late"), cx);
        assert_eq!(cards(stream)[0].read(cx).text, "only reasoning");
        assert!(stream.active_thinking.is_none());
        assert!(stream.active_agent_message.is_none());
        stream.apply_event(start("next"), cx);
        stream.apply_event(thinking("m", "previous message"), cx);
        stream.apply_event(thinking("next", "cancelled reasoning"), cx);
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "next".into(),
            },
            cx,
        );
        stream.apply_event(thinking("next", "after cancel"), cx);
        assert_eq!(cards(stream)[1].read(cx).text, "cancelled reasoning");
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
        assert_eq!(cards(stream).len(), 2);
        stream.apply_event(start("failed"), cx);
        stream.apply_event(thinking("failed", "retained before error"), cx);
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("failed".into()),
                error: Arc::new(std::io::Error::other("owned thinking test failure").into()),
            },
            cx,
        );
        stream.apply_event(thinking("failed", "after error"), cx);
        assert_eq!(cards(stream)[2].read(cx).text, "retained before error");
        assert!(stream.active_thinking.is_none());
    });
    // A separate route has no access to the first route's live-only blocks.
    let (_, other, _) = open_controller_stream(cx, "thinking-other-thread");
    assert!(other.read_with(cx, |stream, _| cards(stream).is_empty()));
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
        assert_eq!(cards(stream).len(), 4);
        assert!(
            cards(stream)
                .iter()
                .all(|card| !card.read(cx).text.is_empty())
        );
        assert!(
            cards(stream)
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
        let card = cards(stream)[0].clone();
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
            cards(stream)
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
        assert_eq!(cards(stream).len(), THINKING_VIEW_BLOCKS);
        assert!(
            cards(stream)
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
        stream.apply_event(thinking("m", &"reasoning line\n".repeat(80)), cx);
    });
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, cx| cards(stream)[0].read(cx).expanded));
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
        stream.apply_event(thinking("m", &"reasoning line\n".repeat(80)), cx);
    });
    cx.run_until_parked();
    let block = stream.read_with(cx, |stream, _| cards(stream)[0].clone());
    assert!(block.read_with(cx, |block, _| block.expanded));
    let scroll = block.read_with(cx, |block, _| block.scroll_handle());
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let toggle = visual.debug_bounds("thinking-toggle").expect("header");
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
        stream.apply_event(thinking("m", &"new reasoning\n".repeat(20)), cx)
    });
    visual.run_until_parked();
    assert_eq!(
        scroll.offset().y,
        px(-100.),
        "streaming must not follow the bottom"
    );
    assert_eq!(
        visual.debug_bounds("thinking-toggle").expect("header"),
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
        cx.notify();
    });
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("thinking-content")
            .expect("empty body")
            .size
            .height
            < px(240.)
    );
}
