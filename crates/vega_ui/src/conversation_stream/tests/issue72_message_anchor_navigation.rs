use super::message_anchors::{
    MESSAGE_ANCHOR_MEASURED_CACHE_LIMIT, MESSAGE_ANCHOR_PREVIEW_LIMIT, sanitize_anchor_preview,
};
use super::*;
use gpui_kit::{KeyDownEvent, Keystroke, Modifiers, VisualTestContext, point};
use std::sync::{Arc, Mutex};

fn issue72_page(count: usize) -> HistoryPage {
    HistoryPage {
        entries: (0..count)
            .map(|index| HistoryEntry::AssistantText {
                seq: index as i64 + 1,
                message_id: format!("persisted-answer-{index}"),
                content: format!(
                    "答案 {index}。{}",
                    "不同长度的消息文本。".repeat(index % 7 + 1)
                ),
                status: vega_conversation::history::AssistantStatus::Done,
            })
            .collect(),
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(count as i64),
    }
}

#[gpui_kit::test]
async fn anchors_use_unique_durable_message_ids_and_safe_text_projections(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue72-projection");
    let user_secret = "Bearer user-secret-value";
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                entries: vec![
                    HistoryEntry::UserText {
                        seq: 1,
                        message_id: "user-message-id".into(),
                        content: format!("  问题\n  {user_secret}  "),
                    },
                    HistoryEntry::AssistantText {
                        seq: 2,
                        message_id: "assistant-message-id".into(),
                        content: "## **已解析** `文本`".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                    },
                    HistoryEntry::Tool {
                        seq: 3,
                        message_id: "tool-only-message-id".into(),
                        call_id: "private-call-id".into(),
                        status: ToolCallStatus::Success,
                        approval: None,
                        input: Some(ToolCardInputProjection::ReadOnly {
                            tool: ReadOnlyToolKind::Read,
                            permission_path: Some("/private/tool/input".into()),
                        }),
                        result: Some(ToolCardResultProjection::ReadOnly {
                            status: ToolCallStatus::Success,
                            output: "private full tool output".into(),
                            reused: false,
                        }),
                    },
                    hydration_summary("assistant-message-id"),
                ],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(3),
            },
            cx,
        );
    });

    let anchors = stream.read_with(cx, |stream, _| stream.message_anchor_projections());
    assert_eq!(
        anchors
            .iter()
            .map(|anchor| anchor.message_id.as_str())
            .collect::<Vec<_>>(),
        [
            "user-message-id",
            "assistant-message-id",
            "tool-only-message-id"
        ]
    );
    assert_eq!(anchors[0].preview, "问题 [凭据已隐藏]");
    assert!(anchors[1].preview.contains("已解析"));
    assert!(anchors[1].preview.contains("文本"));
    assert!(!anchors[1].preview.contains("##"));
    assert!(!anchors[1].preview.contains("**"));
    assert!(!anchors[1].preview.contains('`'));
    assert_eq!(anchors[2].preview, "工具活动");
    assert!(!anchors[2].preview.contains("private"));
}

#[test]
fn preview_normalization_redacts_common_credentials_and_is_bounded() {
    assert_eq!(
        sanitize_anchor_preview("  first\n\tsecond   sk-proj-secret_value   "),
        "first second [凭据已隐藏]"
    );
    let bounded = sanitize_anchor_preview(&"长文本 ".repeat(80));
    assert!(bounded.chars().count() <= MESSAGE_ANCHOR_PREVIEW_LIMIT + 1);
    assert!(bounded.ends_with('…'));
}

#[gpui_kit::test]
async fn rail_is_hidden_for_short_content_and_shown_for_long_overflow(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-visibility");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(issue72_page(2), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    assert!(visual.debug_bounds("message-anchor-rail").is_none());

    stream.update(&mut visual, |stream, cx| {
        stream.replace_history_window(issue72_page(80), "persisted-answer-79", cx);
    });
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    assert!(visual.debug_bounds("message-anchor-rail").is_some());
    assert!(visual.debug_bounds("conversation-scroll").is_some());
    let geometry = stream.read_with(&visual, |stream, cx| {
        stream.message_anchor_geometry(cx, 800.0)
    });
    assert_eq!(geometry.anchors.len(), 80);
    assert!(
        geometry
            .entry_heights
            .windows(2)
            .any(|pair| (pair[0] - pair[1]).abs() > 1.0)
    );
    assert!(
        geometry
            .anchors
            .windows(2)
            .all(|pair| pair[0].fraction < pair[1].fraction)
    );
}

#[gpui_kit::test]
async fn measured_entry_height_cache_stays_bounded_while_scrolling(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-bounded-measurements");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(issue72_page(700), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for item_ix in [0, 350, 699] {
        stream.update(&mut visual, |stream, cx| {
            stream.list.scroll_to(gpui_kit::ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        visual.draw(
            point(px(0.), px(0.)),
            gpui_kit::size(px(1200.), px(800.)),
            |_, _| stream.clone().into_any_element(),
        );
        let cached = stream.read_with(&visual, |stream, _| stream.measured_entry_heights.len());
        assert!(cached > 0, "visible rows should be measured at {item_ix}");
        assert!(
            cached <= MESSAGE_ANCHOR_MEASURED_CACHE_LIMIT,
            "scroll position {item_ix} retained {cached} measured rows"
        );
    }
}

#[gpui_kit::test]
async fn empty_thread_still_displays_recoverable_location_status(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-empty-location-status");
    stream.update(cx, |stream, cx| {
        stream.apply_message_location_status(MessageLocationStatus::NotFound, cx);
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    assert!(visual.debug_bounds("message-anchor-status").is_some());
}

#[gpui_kit::test]
async fn mouse_and_keyboard_anchor_navigation_emit_real_message_ids(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-interaction");
    let requested = Arc::new(Mutex::new(Vec::<MessageLocationRequested>::new()));
    let capture = requested.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, request: &MessageLocationRequested, _| {
            capture
                .lock()
                .expect("request capture")
                .push(request.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(issue72_page(80), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("long session rail");
    let target = stream.read_with(&visual, |stream, _| {
        stream
            .message_anchor_projections()
            .get(38)
            .expect("target anchor")
            .message_id
            .clone()
    });
    let y = stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, f32::from(rail.size.height));
        let index = geometry
            .anchors
            .iter()
            .position(|anchor| anchor.message_id == target)
            .expect("target position");
        rail.top() + rail.size.height * geometry.anchors[index].fraction
    });
    let rail_point = point(rail.center().x, y);
    visual.simulate_mouse_move(rail_point, None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_hovered
            .as_deref()
            .map(str::to_string)),
        Some(target.clone())
    );
    visual.simulate_click(rail_point, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        requested
            .lock()
            .expect("mouse request")
            .last()
            .unwrap()
            .message_id,
        target
    );

    let before_wheel = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (top.item_ix, top.offset_in_item)
    });
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: rail_point,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(60.))),
        ..Default::default()
    });
    cx.run_until_parked();
    let after_wheel = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (top.item_ix, top.offset_in_item)
    });
    assert_ne!(
        after_wheel, before_wheel,
        "wheel input over the rail scrolls the list"
    );

    let focus = stream.read_with(&visual, |stream, _| stream.message_anchor_focus.clone());
    window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .expect("focus rail");
    let before = stream.read_with(&visual, |stream, _| {
        stream.message_anchor_keyboard_id.clone()
    });
    let keystroke = Keystroke::parse("down").expect("Down keystroke");
    visual.simulate_event(KeyDownEvent {
        keystroke,
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
    let after = stream.read_with(&visual, |stream, _| {
        stream.message_anchor_keyboard_id.clone()
    });
    assert_ne!(after, before);
    let keystroke = Keystroke::parse("enter").expect("Enter keystroke");
    visual.simulate_event(KeyDownEvent {
        keystroke,
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
    let keyboard_target = stream
        .read_with(&visual, |stream, _| {
            stream.message_anchor_keyboard_id.clone()
        })
        .expect("keyboard target");
    assert_eq!(
        requested
            .lock()
            .expect("keyboard request")
            .last()
            .unwrap()
            .message_id,
        keyboard_target
    );
}
