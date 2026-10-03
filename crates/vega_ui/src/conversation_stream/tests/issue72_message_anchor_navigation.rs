use super::message_anchors::{
    MESSAGE_ANCHOR_MEASURED_CACHE_LIMIT, MESSAGE_ANCHOR_PREVIEW_LIMIT, sanitize_anchor_preview,
};
use super::*;
use gpui_kit::{KeyDownEvent, Keystroke, Modifiers, VisualTestContext, point};
use std::sync::{Arc, Mutex};

fn issue72_page(count: usize) -> HistoryPage {
    issue72_page_range(0, count, 1)
}

fn issue72_page_with_sensitive_preview(target_index: usize, secret: &str) -> HistoryPage {
    let mut page = issue72_page(80);
    if let HistoryEntry::AssistantText { content, .. } = &mut page.entries[target_index] {
        *content = format!("安全预览 Bearer {secret} {}", "长文本片段。".repeat(100));
    }
    page
}

fn issue72_page_range(start_index: usize, count: usize, content_repetitions: usize) -> HistoryPage {
    HistoryPage {
        entries: (0..count)
            .map(|offset| {
                let index = start_index + offset;
                HistoryEntry::AssistantText {
                    seq: index as i64 + 1,
                    message_id: format!("persisted-answer-{index}"),
                    content: format!(
                        "答案 {index}。{}",
                        "不同长度的消息文本。".repeat((index % 7 + 1) * content_repetitions)
                    ),
                    status: vega_conversation::history::AssistantStatus::Done,
                    execution_duration_ms: None,
                }
            })
            .collect(),
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some((start_index + count) as i64),
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
                        execution_duration_ms: None,
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

#[gpui_kit::test]
async fn run_activity_entries_contribute_geometry_without_duplicating_message_anchors(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue72-run-activity-anchors");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                entries: vec![
                    HistoryEntry::AssistantText {
                        seq: 1,
                        message_id: "run-with-answer".into(),
                        content: "answer after tool work".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: Some(3_000),
                    },
                    HistoryEntry::Tool {
                        seq: 2,
                        message_id: "run-with-answer".into(),
                        call_id: "run-tool".into(),
                        status: ToolCallStatus::Success,
                        approval: None,
                        input: None,
                        result: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 3,
                        message_id: "activity-only".into(),
                        content: String::new(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: Some(2_000),
                    },
                    HistoryEntry::AssistantText {
                        seq: 4,
                        message_id: "standalone-answer".into(),
                        content: "another answer".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: None,
                    },
                ],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(4),
            },
            cx,
        );
    });

    let (anchors, activity_indices, entry_heights) = stream.read_with(cx, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 800.0);
        (
            stream
                .message_anchor_projections()
                .into_iter()
                .map(|anchor| (anchor.entry_index, anchor.message_id))
                .collect::<Vec<_>>(),
            stream
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    matches!(
                        entry,
                        StreamEntry::RunActivity { .. } | StreamEntry::RunActivitySegment { .. }
                    )
                    .then_some(index)
                })
                .collect::<Vec<_>>(),
            geometry.entry_heights,
        )
    });

    assert_eq!(
        anchors,
        [
            (1, "run-with-answer".to_string()),
            (4, "standalone-answer".to_string()),
        ]
    );
    assert_eq!(activity_indices, [0, 2, 3]);
    assert_eq!(entry_heights.len(), 5);
    assert!(entry_heights.iter().all(|height| *height > 0.0));
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
async fn width_remeasure_preserves_anchor_identity_and_updates_rail_geometry(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-width-remeasure");
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(1100., cx);
        stream.apply_history_page(issue72_page_range(0, 80, 24), cx);
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 30,
            offset_in_item: px(11.),
        });
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(1100.), px(800.)));
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let before = stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 800.0);
        (
            stream.scroll_anchor_snapshot(),
            stream
                .message_anchor_projections()
                .into_iter()
                .map(|anchor| anchor.message_id)
                .collect::<Vec<_>>(),
            geometry.entry_heights,
            geometry
                .anchors
                .into_iter()
                .map(|anchor| (anchor.message_id, anchor.fraction))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(before.0.message_id.as_deref(), Some("persisted-answer-30"));
    assert!((before.0.offset_in_item_px - 11.).abs() < 1.0);

    visual.simulate_resize(gpui_kit::size(px(720.), px(800.)));
    stream.update(&mut visual, |stream, cx| {
        stream.set_workspace_width(720., cx)
    });
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(720.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after = stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 800.0);
        (
            stream.scroll_anchor_snapshot(),
            stream
                .message_anchor_projections()
                .into_iter()
                .map(|anchor| anchor.message_id)
                .collect::<Vec<_>>(),
            geometry.entry_heights,
            geometry
                .anchors
                .into_iter()
                .map(|anchor| (anchor.message_id, anchor.fraction))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(after.0.identity, before.0.identity);
    assert_eq!(after.0.message_id, before.0.message_id);
    assert!((after.0.offset_in_item_px - before.0.offset_in_item_px).abs() < 1.0);
    assert_eq!(
        after.1, before.1,
        "width changes must not reorder message IDs"
    );
    assert!(
        before
            .2
            .iter()
            .zip(&after.2)
            .any(|(old, new)| (old - new).abs() > 1.0),
        "narrower content should invalidate at least one estimated or measured row height"
    );
    assert_eq!(after.3.len(), before.3.len());
    assert!(
        after
            .3
            .iter()
            .zip(&before.3)
            .all(|((new_id, _), (old_id, _))| new_id == old_id),
        "the rail should map the remeasured geometry to the same messages"
    );
    assert!(after.3.windows(2).all(|pair| pair[0].1 < pair[1].1));
}

#[gpui_kit::test]
async fn prepending_neighbor_history_page_keeps_existing_anchor_identity_and_order(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-history-prepend");
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(1100., cx);
        stream.apply_history_page(issue72_page_range(20, 40, 12), cx);
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 15,
            offset_in_item: px(17.),
        });
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(1100.), px(800.)));
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let before = stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 800.0);
        (
            stream.scroll_anchor_snapshot(),
            stream
                .message_anchor_projections()
                .into_iter()
                .map(|anchor| anchor.message_id)
                .collect::<Vec<_>>(),
            geometry
                .anchors
                .into_iter()
                .map(|anchor| (anchor.entry_index, anchor.message_id, anchor.fraction))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(before.0.message_id.as_deref(), Some("persisted-answer-35"));

    stream.update(&mut visual, |stream, cx| {
        stream.apply_history_page(issue72_page_range(0, 20, 12), cx);
    });
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after = stream.read_with(&visual, |stream, cx| {
        let geometry = stream.message_anchor_geometry(cx, 800.0);
        (
            stream.scroll_anchor_snapshot(),
            stream
                .message_anchor_projections()
                .into_iter()
                .map(|anchor| anchor.message_id)
                .collect::<Vec<_>>(),
            geometry
                .anchors
                .into_iter()
                .map(|anchor| (anchor.entry_index, anchor.message_id, anchor.fraction))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(after.0.identity, before.0.identity);
    assert_eq!(after.0.message_id, before.0.message_id);
    assert!((after.0.offset_in_item_px - before.0.offset_in_item_px).abs() < 1.0);
    assert_eq!(after.1.len(), before.1.len() + 20);
    assert_eq!(&after.1[20..], before.1.as_slice());
    assert!(after.2.windows(2).all(|pair| pair[0].2 < pair[1].2));
    for (old_index, old_id, old_fraction) in &before.2 {
        let (new_index, new_id, new_fraction) = after
            .2
            .iter()
            .find(|(_, message_id, _)| message_id == old_id)
            .expect("each existing message remains in the rail");
        assert_eq!(new_id, old_id);
        assert_eq!(*new_index, old_index + 20);
        assert!(
            new_fraction > old_fraction,
            "prepended content should move existing anchor {old_id} down the normalized rail"
        );
    }
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
        stream.apply_history_page(
            issue72_page_with_sensitive_preview(38, "hover-preview-secret"),
            cx,
        );
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
    let preview_bounds = visual
        .debug_bounds("message-anchor-preview")
        .expect("hover displays a bounded preview");
    let message_list = visual
        .debug_bounds("conversation-message-list")
        .expect("message list");
    let transcript = visual
        .debug_bounds("conversation-scroll")
        .expect("transcript viewport");
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("composer shell");
    assert!(!preview_bounds.intersects(&message_list));
    assert!(!preview_bounds.intersects(&composer));
    assert!(preview_bounds.top() >= transcript.top());
    assert!(preview_bounds.bottom() <= transcript.bottom());
    assert!(preview_bounds.size.width <= px(180.0));
    assert!(f32::from(preview_bounds.right()) <= f32::from(rail.left()));
    let hover_preview = stream.read_with(&visual, |stream, _| {
        stream
            .message_anchor_projections()
            .into_iter()
            .find(|anchor| anchor.message_id == target)
            .expect("hovered anchor")
            .preview
    });
    assert!(hover_preview.contains("[凭据已隐藏]"));
    assert!(!hover_preview.contains("hover-preview-secret"));
    visual.simulate_mouse_move(preview_bounds.center(), None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .as_deref()
            .map(str::to_string)),
        Some(target.clone())
    );
    assert!(visual.debug_bounds("message-anchor-preview").is_some());
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
    let focused_preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("focus keeps the selected preview visible");
    visual.simulate_mouse_move(focused_preview.center(), None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .as_deref()
            .map(str::to_string)),
        Some(target.clone())
    );
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
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .clone()),
        None
    );
    assert!(visual.debug_bounds("message-anchor-preview").is_some());
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

#[gpui_kit::test]
async fn hover_anchor_preview_dismisses_when_pointer_leaves_rail_and_preview(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-hover-preview-exit");
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(656.0, cx);
        stream.apply_history_page(issue72_page(80), cx);
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(960.0), px(600.0)));
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(960.), px(600.)),
        |_, _| stream.clone().into_any_element(),
    );
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("long session rail");
    let transcript = visual
        .debug_bounds("conversation-scroll")
        .expect("transcript viewport");
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
    let preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("rail hover displays preview");
    let lane = visual
        .debug_bounds("message-anchor-preview-lane")
        .expect("narrow pane reserves a preview lane");
    assert!(lane.intersects(&preview));
    assert!(!rail.contains(&preview.center()));
    let blank_transcript_point = point(
        px(f32::from(transcript.right()) - 4.0),
        transcript.center().y,
    );
    assert!(transcript.contains(&blank_transcript_point));
    assert!(!rail.contains(&blank_transcript_point));
    assert!(!preview.contains(&blank_transcript_point));

    visual.simulate_mouse_move(blank_transcript_point, None, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        stream.read_with(&visual, |stream, _| stream.message_anchor_hovered.clone()),
        None,
        "leaving the rail for blank transcript space clears its hover selection"
    );
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .clone()),
        None,
        "blank transcript space is outside the preview hover target"
    );
    assert!(
        visual.debug_bounds("message-anchor-preview").is_none(),
        "preview is hidden once the pointer has left both the rail and preview"
    );

    visual.simulate_mouse_move(rail_point, None, Modifiers::default());
    cx.run_until_parked();
    let preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("returning to the rail displays its preview");
    let selected_on_reentry = stream
        .read_with(&visual, |stream, _| stream.message_anchor_hovered.clone())
        .expect("rail hover selects an anchor after reentry");
    visual.simulate_mouse_move(preview.center(), None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .clone()),
        Some(selected_on_reentry),
        "entering the preview transfers hover ownership without dismissing it"
    );
    assert!(visual.debug_bounds("message-anchor-preview").is_some());

    visual.simulate_mouse_move(blank_transcript_point, None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_preview_hovered
            .clone()),
        None,
        "leaving the preview clears its hover ownership"
    );
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream.message_anchor_hovered.clone()),
        None,
        "leaving the preview also clears the rail selection"
    );
    assert!(
        visual.debug_bounds("message-anchor-preview").is_none(),
        "the preview dismisses after the pointer leaves both hover targets"
    );
}

#[gpui_kit::test]
async fn keyboard_anchor_selection_displays_a_bounded_sanitized_preview(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-keyboard-preview");
    let target_index = 38;
    let secret = "private-preview-secret";
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            issue72_page_with_sensitive_preview(target_index, secret),
            cx,
        )
    });
    cx.run_until_parked();

    let target_id = format!("persisted-answer-{target_index}");
    let previous_id = format!("persisted-answer-{}", target_index - 1);
    stream.update(cx, |stream, cx| {
        stream.message_anchor_keyboard_id = Some(previous_id);
        cx.notify();
    });

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let focus = stream.read_with(&visual, |stream, _| stream.message_anchor_focus.clone());
    window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .expect("focus rail");
    let previous_preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("focus keeps the previous keyboard selection visible");
    visual.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("down").expect("Down keystroke"),
        is_held: false,
        prefer_character_input: false,
    });

    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_keyboard_id
            .clone()),
        Some(target_id.clone())
    );
    let preview = stream.read_with(&visual, |stream, _| {
        stream
            .message_anchor_projections()
            .into_iter()
            .find(|anchor| anchor.message_id == target_id)
            .expect("target preview")
            .preview
    });
    assert!(preview.contains("[凭据已隐藏]"));
    assert!(!preview.contains(secret));
    assert!(preview.chars().count() <= MESSAGE_ANCHOR_PREVIEW_LIMIT + 1);

    let preview_bounds = visual
        .debug_bounds("message-anchor-preview")
        .expect("keyboard selection displays the visible preview");
    assert_ne!(
        preview_bounds.top(),
        previous_preview.top(),
        "the visible preview follows the newly selected anchor"
    );
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("message anchor rail");
    let message_list = visual
        .debug_bounds("conversation-message-list")
        .expect("message list");
    let transcript = visual
        .debug_bounds("conversation-scroll")
        .expect("transcript viewport");
    let viewport = window
        .update(cx, |_, window, _| window.viewport_size())
        .expect("viewport size");
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("composer shell");
    assert!(preview_bounds.size.width > px(0.) && preview_bounds.size.height > px(0.));
    assert!(preview_bounds.size.width <= px(180.0));
    assert!(f32::from(preview_bounds.right()) <= f32::from(viewport.width));
    assert!(preview_bounds.top() >= transcript.top());
    assert!(preview_bounds.bottom() <= transcript.bottom());
    assert!(f32::from(preview_bounds.right()) <= f32::from(rail.left()));
    assert!(!preview_bounds.intersects(&message_list));
    assert!(!preview_bounds.intersects(&composer));
}

#[gpui_kit::test]
async fn keyboard_anchor_preview_uses_a_reserved_lane_in_a_narrow_pane(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue72-keyboard-preview-narrow");
    let target_index = 38;
    let secret = "narrow-preview-secret";
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(656.0, cx);
        stream.apply_history_page(
            issue72_page_with_sensitive_preview(target_index, secret),
            cx,
        );
    });
    cx.run_until_parked();

    let target_id = format!("persisted-answer-{target_index}");
    let previous_id = format!("persisted-answer-{}", target_index - 1);
    stream.update(cx, |stream, cx| {
        stream.message_anchor_keyboard_id = Some(previous_id);
        cx.notify();
    });

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(960.0), px(600.0)));
    visual.draw(
        point(px(0.), px(0.)),
        gpui_kit::size(px(960.0), px(600.0)),
        |_, _| stream.clone().into_any_element(),
    );
    let focus = stream.read_with(&visual, |stream, _| stream.message_anchor_focus.clone());
    window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .expect("focus rail");
    visual.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("down").expect("Down keystroke"),
        is_held: false,
        prefer_character_input: false,
    });

    assert_eq!(
        stream.read_with(&visual, |stream, _| stream
            .message_anchor_keyboard_id
            .clone()),
        Some(target_id.clone())
    );
    let preview = stream.read_with(&visual, |stream, _| {
        stream
            .message_anchor_projections()
            .into_iter()
            .find(|anchor| anchor.message_id == target_id)
            .expect("target preview")
            .preview
    });
    assert!(preview.contains("[凭据已隐藏]"));
    assert!(!preview.contains(secret));
    assert!(preview.chars().count() <= MESSAGE_ANCHOR_PREVIEW_LIMIT + 1);

    let preview_bounds = visual
        .debug_bounds("message-anchor-preview")
        .expect("keyboard selection displays the visible preview");
    let lane = visual
        .debug_bounds("message-anchor-preview-lane")
        .expect("narrow pane reserves a preview lane");
    let message_list = visual
        .debug_bounds("conversation-message-list")
        .expect("message list");
    let transcript = visual
        .debug_bounds("conversation-scroll")
        .expect("transcript viewport");
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("composer shell");
    let viewport = window
        .update(cx, |_, window, _| window.viewport_size())
        .expect("viewport size");
    assert!(lane.intersects(&preview_bounds));
    assert!(!preview_bounds.intersects(&message_list));
    assert!(!preview_bounds.intersects(&composer));
    assert!(preview_bounds.size.width <= px(180.0));
    assert!(f32::from(preview_bounds.right()) <= f32::from(viewport.width));
    assert!(preview_bounds.top() >= transcript.top());
    assert!(preview_bounds.bottom() <= transcript.bottom());
}
