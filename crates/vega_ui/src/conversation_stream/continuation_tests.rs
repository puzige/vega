use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{TestAppContext, gpui};
use vega_conversation::types::{ConversationStopReason, ThreadStatus};

thread_local! {
    static FOCUSED_COPY: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

pub(super) fn record_focused_copy(id: u64) {
    FOCUSED_COPY.with(|focused| focused.set(Some(id)));
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
    });
}

fn click_message_copy(window: &mut Window, cx: &mut App, copy_id: u64) {
    window.hover(("message-with-copy", copy_id), cx);
    let button = window.find(("message-copy", copy_id));
    assert!(button.visible());
    assert!(button.bounds().size.width > px(0.) && button.bounds().size.height > px(0.));
    window.click(("message-copy", copy_id), cx);
}

#[derive(Clone, Copy)]
enum DeliveryPath {
    Composer,
    Queue,
    Echo,
}

fn image_attachment() -> ImageAttachment {
    let image = image::DynamicImage::new_rgba8(32, 24);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

#[derive(Debug)]
struct MeasuredGaps {
    width: f32,
    text_bottom: f32,
    last_image_bottom: Option<f32>,
}

fn assert_matching_gaps(ordinary: &MeasuredGaps, delivered: &MeasuredGaps) {
    assert_eq!(ordinary.width, delivered.width);
    assert!(
        (ordinary.text_bottom - delivered.text_bottom).abs() <= 0.5,
        "ordinary={ordinary:?}, delivered={delivered:?}"
    );
    match (ordinary.last_image_bottom, delivered.last_image_bottom) {
        (Some(ordinary_gap), Some(delivered_gap)) => assert!(
            (ordinary_gap - delivered_gap).abs() <= 0.5,
            "ordinary={ordinary:?}, delivered={delivered:?}"
        ),
        (None, None) => {}
        _ => panic!("image geometry missing: ordinary={ordinary:?}, delivered={delivered:?}"),
    }
}

fn measure_frame_gaps(
    window: &Window,
    cx: &App,
    list: &gpui_kit::ListState,
    group_id: gpui_kit::EntityId,
    image_ids: &[gpui_kit::EntityId],
) -> MeasuredGaps {
    let viewport = list.viewport_bounds();
    let header = window.find(("run-activity-toggle", group_id));
    assert!(header.visible());
    assert!(header.bounds().size.height > px(0.));
    assert_eq!(header.bounds().intersect(&viewport), header.bounds());
    assert!(header.bounds().bottom() <= window.viewport_size().height);
    let bubbles = window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            quad.background == gpui_kit::Background::from(theme(cx).colors.conversation_bubble)
        })
        .collect::<Vec<_>>();
    assert_eq!(bubbles.len(), 1);
    let bubble = &bubbles[0];
    assert!(bubble.bounds.size.width.as_f32() > 0.);
    assert!(bubble.bounds.size.height.as_f32() > 0.);
    assert_eq!(
        bubble.bounds.intersect(&bubble.content_mask.bounds),
        bubble.bounds
    );
    let bubble_bottom = bubble.bounds.bottom().as_f32() / window.scale_factor();
    let text_bottom = f32::from(header.bounds().top()) - bubble_bottom;
    assert!(text_bottom >= 0., "text overlaps header: {text_bottom}");
    assert!(
        text_bottom >= 48.,
        "normal bottom spacing must not shrink: {text_bottom}"
    );
    let mut image_bounds = Vec::<gpui_kit::Bounds<Pixels>>::new();
    for id in image_ids {
        let image = window.find(("user-image", *id));
        let bounds = image.bounds();
        assert!(image.visible());
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        assert_eq!(bounds.intersect(&viewport), bounds);
        assert!(bounds.top() >= px(bubble_bottom));
        assert!(bounds.bottom() <= header.bounds().top());
        for previous in &image_bounds {
            assert!(previous.top() <= bounds.top());
            assert!(
                previous.right() <= bounds.left()
                    || previous.bottom() <= bounds.top()
                    || bounds.right() <= previous.left(),
                "image thumbnails overlap: {previous:?}, {bounds:?}"
            );
        }
        image_bounds.push(bounds);
    }
    let last_image_bottom = image_bounds.last().map(|last| {
        assert!(
            image_bounds
                .iter()
                .all(|image| image.bottom() <= last.bottom())
        );
        let gap = f32::from(header.bounds().top() - last.bottom());
        assert!(gap >= 0., "image overlaps header: {gap}");
        gap
    });
    MeasuredGaps {
        width: f32::from(window.viewport_size().width),
        text_bottom,
        last_image_bottom,
    }
}

fn assert_scroll_round_trip(
    window: &mut Window,
    cx: &mut App,
    list: &gpui_kit::ListState,
    user_row: usize,
    copy_id: u64,
) {
    let before_offset = list.scroll_px_offset_for_scrollbar().y;
    let before_position = list.logical_scroll_top();
    let before_row = list.bounds_for_item(user_row).unwrap();
    let before_bubble = window.find(("message-with-copy", copy_id)).bounds();
    assert!(list.max_offset_for_scrollbar().y > px(80.));
    assert!(before_offset < px(-40.));
    window.scroll(
        ("message-with-copy", copy_id),
        gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(40.))),
        cx,
    );
    let moved_position = list.logical_scroll_top();
    assert!(
        moved_position.item_ix != before_position.item_ix
            || moved_position.offset_in_item != before_position.offset_in_item
    );
    assert!(
        (f32::from(list.scroll_px_offset_for_scrollbar().y - before_offset) - 40.).abs() <= 0.5
    );
    let moved_row = list.bounds_for_item(user_row).unwrap();
    let moved_bubble = window.find(("message-with-copy", copy_id));
    assert!(moved_bubble.visible());
    assert!((f32::from(moved_row.top() - before_row.top()) - 40.).abs() <= 0.5);
    assert!((f32::from(moved_bubble.bounds().top() - before_bubble.top()) - 40.).abs() <= 0.5);
    window.scroll(
        ("message-with-copy", copy_id),
        gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-40.))),
        cx,
    );
    let restored_position = list.logical_scroll_top();
    assert_eq!(restored_position.item_ix, before_position.item_ix);
    assert!(
        (f32::from(restored_position.offset_in_item - before_position.offset_in_item)).abs() <= 0.5
    );
    assert!((f32::from(list.scroll_px_offset_for_scrollbar().y - before_offset)).abs() <= 0.5);
    let restored_row = list.bounds_for_item(user_row).unwrap();
    let restored_bubble = window.find(("message-with-copy", copy_id));
    assert!(restored_bubble.visible());
    assert_eq!(restored_row.size, before_row.size);
    assert_eq!(restored_bubble.bounds().size, before_bubble.size);
    assert!((f32::from(restored_row.top() - before_row.top())).abs() <= 0.5);
    assert!((f32::from(restored_bubble.bounds().top() - before_bubble.top())).abs() <= 0.5);
}

fn measured_gap(
    cx: &mut TestAppContext,
    path: DeliveryPath,
    width: f32,
    content: &str,
    images: &[ImageAttachment],
    scrollable: bool,
) -> Vec<MeasuredGaps> {
    let window = cx.open_window(gpui_kit::size(px(width), px(900.)), |_, cx| {
        ConversationStream::new(thread(), cx)
    });
    window
        .update(cx, |view, _, cx| {
            if scrollable {
                view.apply_history_page(
                    history_page(vec![HistoryEntry::AssistantText {
                        seq: 0,
                        message_id: "overflow-history".into(),
                        content: "Earlier transcript paragraph.\n\n".repeat(80),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: Some(1000),
                    }]),
                    cx,
                );
            }
            match path {
                DeliveryPath::Composer => {
                    view.input
                        .update(cx, |input, cx| input.set_text(content, cx));
                    for (index, image) in images.iter().enumerate() {
                        let preview = attachments::ImagePreview::new(
                            image.clone(),
                            "attachment.png".into(),
                            cx,
                        );
                        view.attachments.push((index as u64, preview));
                    }
                    view.submit_message(cx);
                    view.accept_composer_submission(content, cx);
                }
                DeliveryPath::Queue | DeliveryPath::Echo => {
                    if matches!(path, DeliveryPath::Echo) {
                        start(view, "previous", 1, Instant::now(), cx);
                        view.apply_pending_queue(
                            PendingQueueProjection {
                                messages: vec![vega_conversation::types::PendingMessage {
                                    id: "user".into(),
                                    thread_id: "thread".into(),
                                    generation: "generation".into(),
                                    behavior: PendingMessageBehavior::Steer,
                                    content: content.into(),
                                    images: images.to_vec(),
                                }],
                                committed_generation: Some("generation".into()),
                                busy: true,
                                ..Default::default()
                            },
                            cx,
                        );
                        view.apply_event(continuation("previous", "user", "next"), cx);
                    }
                    view.apply_event(
                        ConversationEvent::PendingMessageDelivered {
                            pending_id: "user".into(),
                            message_id: "user".into(),
                            seq: 2,
                            content: content.into(),
                            images: images.to_vec(),
                        },
                        cx,
                    );
                }
            }
            start(view, "next", 3, Instant::now(), cx);
            assert_eq!(
                view.entries
                    .iter()
                    .filter(|entry| matches!(entry, StreamEntry::User { .. }))
                    .count(),
                1
            );
        })
        .unwrap();
    let (group_id, copy_id, image_ids, list, user_row) = window
        .update(cx, |view, _, _| {
            let copy_id = view
                .entries
                .iter()
                .find_map(|entry| match entry {
                    StreamEntry::User { copy, .. } => Some(copy.id),
                    _ => None,
                })
                .unwrap();
            let image_ids = view
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::UserImages { images } => Some(
                        images
                            .iter()
                            .map(|image| image.view.entity_id())
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>();
            assert_eq!(image_ids.len(), images.len());
            assert_eq!(
                image_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                images.len()
            );
            assert_eq!(
                view.entries
                    .iter()
                    .filter(|entry| matches!(entry, StreamEntry::UserImages { .. }))
                    .count(),
                usize::from(!images.is_empty())
            );
            let user_row = view
                .entries
                .iter()
                .position(|entry| matches!(entry, StreamEntry::User { .. }))
                .unwrap();
            (
                view.run_activity_groups["next"].entity_id(),
                copy_id,
                image_ids,
                view.list.clone(),
                user_row,
            )
        })
        .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(window.viewport_size().width, px(width));
        let header = window.find(("run-activity-toggle", group_id));
        let initial = measure_frame_gaps(window, cx, &list, group_id, &image_ids);
        let copy_bounds = window.find(("message-copy", copy_id)).bounds();
        window.hover(("run-activity-toggle", group_id), cx);
        let origin = cx.focus_handle();
        origin.focus(window, cx);
        let mut reached = false;
        for _ in 0..64 {
            FOCUSED_COPY.with(|focused| focused.set(None));
            window.focus_next(cx);
            window.press("f20", cx);
            if FOCUSED_COPY.with(|focused| focused.get()) == Some(copy_id) {
                assert!(window.focused(cx).is_some());
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "copy is keyboard reachable without activating other controls"
        );
        assert!(window.find(("message-copy", copy_id)).visible());
        for key in ["enter", "space"] {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()));
            window.press(key, cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(content)
            );
            assert_eq!(window.find(("message-copy", copy_id)).bounds(), copy_bounds);
        }
        origin.focus(window, cx);
        click_message_copy(window, cx, copy_id);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(content)
        );
        window.render_frame(cx);
        assert_eq!(window.find(("message-copy", copy_id)).bounds(), copy_bounds);
        assert_eq!(
            window.find(("run-activity-toggle", group_id)).bounds(),
            header.bounds()
        );
        let mut measurements = vec![initial];
        for resized_width in [width - 160., width - 320., width] {
            window.resize(gpui_kit::size(px(resized_width), px(900.)));
            window.bounds_changed(cx);
            window.render_frame(cx);
            assert_eq!(window.viewport_size().width, px(resized_width));
            measurements.push(measure_frame_gaps(window, cx, &list, group_id, &image_ids));
        }
        assert_matching_gaps(&measurements[0], measurements.last().unwrap());
        if scrollable {
            assert_scroll_round_trip(window, cx, &list, user_row, copy_id);
            let restored = measure_frame_gaps(window, cx, &list, group_id, &image_ids);
            assert_matching_gaps(measurements.last().unwrap(), &restored);
            measurements.push(restored);
        }
        window.remove_window();
        measurements
    })
}

#[gpui_kit::test]
fn direct_and_echo_delivery_match_composer_bubble_geometry_after_remeasurement(
    cx: &mut TestAppContext,
) {
    init(cx);
    for width in [960., 1400.] {
        for content in [
            "Same content".to_owned(),
            "Long wrapped content with 中文 and line breaks\n".repeat(8),
        ] {
            for images in [
                Vec::new(),
                vec![image_attachment()],
                vec![image_attachment(); 8],
            ] {
                let ordinary =
                    measured_gap(cx, DeliveryPath::Composer, width, &content, &images, false);
                for path in [DeliveryPath::Queue, DeliveryPath::Echo] {
                    let delivered = measured_gap(cx, path, width, &content, &images, false);
                    assert_eq!(ordinary.len(), delivered.len());
                    for (ordinary, delivered) in ordinary.iter().zip(&delivered) {
                        assert_matching_gaps(ordinary, delivered);
                    }
                }
            }
        }
        for images in [Vec::new(), vec![image_attachment(); 8]] {
            let ordinary = measured_gap(
                cx,
                DeliveryPath::Composer,
                width,
                "Scrollable target",
                &images,
                true,
            );
            for path in [DeliveryPath::Queue, DeliveryPath::Echo] {
                let delivered = measured_gap(cx, path, width, "Scrollable target", &images, true);
                assert_eq!(ordinary.len(), delivered.len());
                for (ordinary, delivered) in ordinary.iter().zip(&delivered) {
                    assert_matching_gaps(ordinary, delivered);
                }
            }
        }
    }
}

#[gpui_kit::test]
fn successor_failure_keeps_specific_error_and_predecessor_final_copy_separate(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            start(view, "first", 1, Instant::now(), cx);
            view.apply_event(
                ConversationEvent::AssistantResponseStarted {
                    message_id: "first".into(),
                    response_id: "response".into(),
                    text_start_bytes: 0,
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "first".into(),
                    delta: "Original final".into(),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: "first".into(),
                    response: vega_conversation::types::AssistantResponseMetadata {
                        response_id: "response".into(),
                        text_start_bytes: 0,
                        text_end_bytes: "Original final".len(),
                        kind: vega_conversation::types::AssistantResponseKind::Final,
                    },
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        let group = &stream.run_activity_groups["first"];
        assert!(!group.read(cx).expanded());
        let copy_id = stream
            .entries
            .iter()
            .find_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } if copy.original_text() == "Original final" => {
                    Some(copy.id)
                }
                _ => None,
            })
            .unwrap();
        click_message_copy(window, cx, copy_id);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Original final")
        );
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.apply_event(continuation("first", "user", "second"), cx);
            deliver(view, "user", 2, cx);
            start(view, "second", 3, Instant::now(), cx);
            view.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "second".into(),
                    delta: "Partial successor".into(),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::Error {
                    message_id: Some("second".into()),
                    error: Arc::new(vega_runtime::VegaError::Provider {
                        status: Some(429),
                        message: "rate limited".into(),
                        retryable: false,
                    }),
                    execution_duration_ms: Some(9000),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "first".into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: None,
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        assert_eq!(stream.run_failures.get("second"), Some(&RunFailureKind::ProviderRateLimited));
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, failure: Some(RunFailureKind::ProviderRateLimited), .. } if copy.original_text() == "Partial successor")));
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Original final")));
        let group = &stream.run_activity_groups["second"];
        assert!(window.find(("run-activity-toggle", group.entity_id())).label().unwrap().starts_with("Failed after 9s,"));
        let notice = window.find("run-error-second");
        assert_eq!(notice.label(), Some(RunFailureKind::ProviderRateLimited.message().as_str()));
        assert!(notice.visible());
        assert!(notice.bounds().size.height > px(0.));
    });
    let submissions = std::rc::Rc::new(std::cell::RefCell::new(Vec::<ComposerSubmitted>::new()));
    let _subscription = cx.update(|_, cx| {
        let submissions = submissions.clone();
        cx.subscribe(&view, move |_, event: &ComposerSubmitted, _| {
            submissions.borrow_mut().push(event.clone());
        })
    });
    let recovery_image = image_attachment();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.input.update(cx, |input, cx| {
                input.set_text("Continue after rate limit", cx)
            });
            view.attachments.push((
                0,
                attachments::ImagePreview::new(recovery_image.clone(), "recovery.png".into(), cx),
            ));
            assert!(!view.actions.running);
            assert!(view.can_submit_message(cx));
            view.submit_message(cx);
            assert!(view.composer_submit_pending);
            assert!(!view.can_submit_message(cx));
            view.submit_message(cx);
            assert_eq!(view.input.read(cx).text(), "Continue after rate limit");
            assert_eq!(view.attachments.len(), 1);
            assert_eq!(view.attachments[0].1.image, recovery_image);
            assert_eq!(view.submitted_attachments.len(), 1);
            assert_eq!(view.submitted_attachments[0].1.image, recovery_image);
            assert!(!view.entries.iter().any(|entry| matches!(entry, StreamEntry::User { copy, .. } if copy.original_text() == "Continue after rate limit")));
        })
    });
    let submissions = submissions.borrow();
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].thread_id, "thread");
    assert_eq!(submissions[0].content, "Continue after rate limit");
    assert_eq!(submissions[0].images, vec![recovery_image]);
    assert_eq!(submissions[0].pending_behavior, None);
    assert_eq!(submissions[0].skill_intent, None);
}

#[gpui_kit::test]
fn successor_accepts_text_after_late_terminal_and_finishes_with_its_own_final_copy(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
    let started_at = Instant::now();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            start(view, "previous", 1, started_at, cx);
            view.apply_event(continuation("previous", "user", "next"), cx);
            deliver(view, "user", 2, cx);
            start(view, "next", 3, started_at, cx);
            view.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "previous".into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::AssistantResponseStarted {
                    message_id: "next".into(),
                    response_id: "next-final".into(),
                    text_start_bytes: 0,
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "next".into(),
                    delta: "Successor final".into(),
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        assert_eq!(stream.active_agent_message.as_ref().map(|(id, _)| id.as_str()), Some("next"));
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Successor final")));
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: "next".into(),
                    response: vega_conversation::types::AssistantResponseMetadata {
                        response_id: "next-final".into(),
                        text_start_bytes: 0,
                        text_end_bytes: "Successor final".len(),
                        kind: vega_conversation::types::AssistantResponseKind::Final,
                    },
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "next".into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: Some(17000),
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        let previous = stream.run_activity_groups["previous"].entity_id();
        assert!(
            window
                .try_find(("run-activity-toggle", previous))
                .is_none_or(|header| header.bounds().size.height == px(0.))
        );
        let current = stream.run_activity_groups["next"].entity_id();
        assert!(
            window
                .find(("run-activity-toggle", current))
                .label()
                .unwrap()
                .starts_with("Worked for 17s,")
        );
        let copy_id = stream
            .entries
            .iter()
            .find_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. }
                    if copy.original_text() == "Successor final" =>
                {
                    Some(copy.id)
                }
                _ => None,
            })
            .unwrap();
        click_message_copy(window, cx, copy_id);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Successor final")
        );
    });
}

fn persisted_continuation_chain() -> vega_store::Store {
    let store = vega_store::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "History",
            mode: "ask",
            permission_mode: "readonly",
            model: "model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    let tx = store.immediate_transaction().unwrap();
    for (seq, id, role, content) in [
        (1, "first", "assistant", "First body"),
        (2, "user-one", "user", "First interjection"),
        (3, "middle", "assistant", "Middle body"),
        (4, "user-two", "user", "Second interjection"),
        (5, "last", "assistant", "Last body"),
    ] {
        vega_store::messages::insert(
            &tx,
            &vega_store::messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    for (user, previous) in [("user-one", "first"), ("user-two", "middle")] {
        vega_store::pending_messages::insert(
            &tx,
            &vega_store::pending_messages::PendingMessageRow {
                id: user.into(),
                thread_id: "thread".into(),
                ordinal: 0,
                behavior: "steer".into(),
                generation: "not-an-assistant-id".into(),
                payload: String::new(),
                created_at: 1,
            },
        )
        .unwrap();
        assert!(vega_store::pending_messages::mark_delivered(&tx, "thread", user).unwrap());
        tx.execute(
            "UPDATE pending_messages SET accepted_after_assistant_id = ?1 WHERE id = ?2",
            [previous, user],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    store
}

#[gpui_kit::test]
fn persisted_chain_survives_both_page_orders_and_replacing_history_window(cx: &mut TestAppContext) {
    init(cx);
    let store = persisted_continuation_chain();
    for newest_first in [false, true] {
        let (view, visual) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
        let sequences = if newest_first { [5, 3, 1] } else { [1, 3, 5] };
        for seq in sequences {
            let page = vega_conversation::history::history_page_after(&store, "thread", seq - 1, 1)
                .unwrap();
            visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    if newest_first {
                        view.apply_history_page(page, cx);
                    } else {
                        view.apply_newer_history_page(page, cx);
                    }
                })
            });
            visual.update(|window, cx| {
                window.render_frame(cx);
                let stream = view.read(cx);
                for id in ["first", "middle"] {
                    if let Some(group) = stream.run_activity_groups.get(id)
                        && let Some(header) =
                            window.try_find(("run-activity-toggle", group.entity_id()))
                    {
                        assert!(
                            header.bounds().size.height == px(0.)
                                || header
                                    .label()
                                    .is_some_and(|label| label.starts_with("Activity,"))
                        );
                    }
                }
            });
        }
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                let page = vega_conversation::history::history_page_containing_message(
                    &store, "thread", "last", 1,
                )
                .unwrap()
                .unwrap();
                assert!(view.replace_history_window(page, "last", cx));
                assert!(!view.run_activity_groups.contains_key("first"));
                let page = vega_conversation::history::history_page_before(
                    &store,
                    "thread",
                    vega_store::messages::PageCursor::Before(5),
                    4,
                )
                .unwrap();
                view.apply_history_page(page, cx);
            })
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
            let stream = view.read(cx);
            for expected in ["First body", "Middle body", "Last body"] {
                assert_eq!(stream.entries.iter().filter(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == expected)).count(), 1);
            }
            for id in ["first", "middle"] {
                let group = &stream.run_activity_groups[id];
                if let Some(header) = window.try_find(("run-activity-toggle", group.entity_id())) {
                    assert!(header.bounds().size.height == px(0.) || header.label().is_some_and(|label| label.starts_with("Activity,")));
                }
            }
            let last = stream.run_activity_groups["last"].entity_id();
            assert!(window.find(("run-activity-toggle", last)).label().unwrap().starts_with("Completed,"));
            window.remove_window();
        });
    }
}

fn history_page(entries: Vec<HistoryEntry>) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries,
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(3),
    }
}

#[gpui_kit::test]
fn loading_predecessor_page_after_successor_preserves_single_terminal_clock_across_routes(
    cx: &mut TestAppContext,
) {
    init(cx);
    for route in ["thread", "other-thread", "thread"] {
        let mut next_thread = thread();
        next_thread.id = route.into();
        let (view, visual) = cx.add_window_view(|_, cx| ConversationStream::new(next_thread, cx));
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.apply_history_page(
                    history_page(vec![
                        HistoryEntry::UserText {
                            seq: 2,
                            message_id: "user".into(),
                            content: "Continue".into(),
                        },
                        HistoryEntry::AssistantText {
                            seq: 3,
                            message_id: "next".into(),
                            content: "Final response".into(),
                            status: vega_conversation::history::AssistantStatus::Done,
                            execution_duration_ms: Some(14000),
                        },
                    ]),
                    cx,
                );
                view.apply_history_page(
                    history_page(vec![
                        HistoryEntry::RunContinuation {
                            previous_assistant_id: "previous".into(),
                            user_message_id: "user".into(),
                            next_assistant_id: "next".into(),
                        },
                        HistoryEntry::AssistantText {
                            seq: 1,
                            message_id: "previous".into(),
                            content: "Earlier response".into(),
                            status: vega_conversation::history::AssistantStatus::Done,
                            execution_duration_ms: None,
                        },
                    ]),
                    cx,
                );
            })
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
            let stream = view.read(cx);
            let previous = &stream.run_activity_groups["previous"];
            if let Some(header) = window.try_find(("run-activity-toggle", previous.entity_id())) {
                assert!(header.bounds().size.height == px(0.) || header.label().is_some_and(|label| label.starts_with("Activity,")));
            }
            let next = &stream.run_activity_groups["next"];
            assert!(window.find(("run-activity-toggle", next.entity_id())).label().unwrap().starts_with("Worked for 14s,"));
            for expected in ["Earlier response", "Final response"] {
                assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == expected)));
            }
            window.remove_window();
        });
    }
}

#[gpui_kit::test]
fn echo_does_not_discard_old_output_arriving_before_acceptance(cx: &mut TestAppContext) {
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            start(view, "previous", 1, Instant::now(), cx);
            view.apply_pending_queue(
                PendingQueueProjection {
                    messages: vec![vega_conversation::types::PendingMessage {
                        id: "user".into(),
                        thread_id: "thread".into(),
                        generation: "generation".into(),
                        behavior: PendingMessageBehavior::Steer,
                        content: "Interjection".into(),
                        images: Vec::new(),
                    }],
                    committed_generation: Some("generation".into()),
                    busy: true,
                    ..Default::default()
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "previous".into(),
                    delta: "Legitimate output after echo".into(),
                },
                cx,
            );
            view.apply_event(continuation("previous", "user", "next"), cx);
            deliver(view, "user", 2, cx);
            start(view, "next", 3, Instant::now(), cx);
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        let user = stream.entries.iter().position(|entry| matches!(entry, StreamEntry::User { .. })).unwrap();
        let old = stream.entries.iter().position(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Legitimate output after echo")).unwrap();
        let next = stream.entries.iter().position(|entry| matches!(entry, StreamEntry::RunActivity { group } if group == &stream.run_activity_groups["next"])).unwrap();
        assert!(user < old && old < next);
        assert_eq!(
            stream.entry_identities[user].message_id.as_deref(),
            Some("user")
        );
        assert_eq!(
            stream.entry_identities[old].message_id.as_deref(),
            Some("previous")
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .zip(&stream.entry_identities)
                .filter(|(entry, identity)| {
                    identity.message_id.as_deref() == Some("previous")
                        && matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Legitimate output after echo")
                })
                .count(),
            1
        );
        let viewport = stream.list.viewport_bounds();
        assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
        let user_row = stream.list.bounds_for_item(user).expect("user row must be laid out");
        let old_row = stream.list.bounds_for_item(old).expect("original output row must be laid out");
        let next_row = stream.list.bounds_for_item(next).expect("successor activity row must be laid out");
        for row in [user_row, old_row, next_row] {
            assert!(row.size.width > px(0.) && row.size.height > px(0.));
            assert_eq!(row.intersect(&viewport), row);
        }
        assert!(user_row.bottom() <= old_row.top());
        assert!(old_row.bottom() <= next_row.top());
        assert_eq!(stream.entries.iter().filter(|entry| matches!(entry, StreamEntry::User { .. })).count(), 1);
    });
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Continuation".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        model: "model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn start(
    view: &mut ConversationStream,
    id: &str,
    seq: u64,
    started_at: Instant,
    cx: &mut Context<ConversationStream>,
) {
    view.apply_event(
        ConversationEvent::MessageStarted {
            message_id: id.into(),
            seq,
        },
        cx,
    );
    view.apply_event(
        ConversationEvent::RunStarted {
            message_id: id.into(),
            started_at,
        },
        cx,
    );
}

fn assert_running_clock(label: &str, started_at: Instant, before: Instant, after: Instant) {
    let lower = before.duration_since(started_at).as_secs();
    let upper = after.duration_since(started_at).as_secs();
    assert!(
        (lower..=upper).any(|seconds| label.starts_with(&format!(
            "Working for {},",
            run_activity::format_run_clock(seconds * 1000)
        ))),
        "clock {label:?} outside {lower}..={upper}"
    );
}

fn continuation(previous: &str, user: &str, next: &str) -> ConversationEvent {
    ConversationEvent::RunContinued {
        previous_assistant_id: previous.into(),
        user_message_id: user.into(),
        next_assistant_id: next.into(),
    }
}

fn deliver(
    view: &mut ConversationStream,
    user: &str,
    seq: u64,
    cx: &mut Context<ConversationStream>,
) {
    view.apply_event(
        ConversationEvent::PendingMessageDelivered {
            pending_id: user.into(),
            message_id: user.into(),
            seq,
            content: "Continue with the original task".into(),
            images: Vec::new(),
        },
        cx,
    );
}

#[gpui_kit::test]
fn repeated_continuations_keep_one_clock_and_late_terminal_cannot_restore_old_clock(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (view, cx) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
    let started_at = Instant::now() - Duration::from_secs(12);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            start(view, "first", 1, started_at, cx);
            view.apply_event(
                ConversationEvent::ThinkingDelta {
                    message_id: "first".into(),
                    delta: "Inspect original task".into(),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: vega_conversation::types::ToolCall {
                        id: "read-call".into(),
                        tool: "read".into(),
                        input_json: r#"{"path":"input.txt"}"#.into(),
                    },
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: "read-call".into(),
                    approval: vega_conversation::types::Approval::Once,
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::ToolCallRunning {
                    call_id: "read-call".into(),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "read-call".into(),
                    result: vega_conversation::types::ToolResult {
                        status: vega_conversation::types::ToolCallStatus::Success,
                        output: "Original tool output".into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::AssistantResponseStarted {
                    message_id: "first".into(),
                    response_id: "old-final".into(),
                    text_start_bytes: 0,
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "first".into(),
                    delta: "Original assistant body".into(),
                },
                cx,
            );
            view.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: "first".into(),
                    response: vega_conversation::types::AssistantResponseMetadata {
                        response_id: "old-final".into(),
                        text_start_bytes: 0,
                        text_end_bytes: "Original assistant body".len(),
                        kind: vega_conversation::types::AssistantResponseKind::Final,
                    },
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(
            !view.read(cx).run_activity_groups["first"]
                .read(cx)
                .expanded()
        );
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.apply_event(continuation("first", "user", "second"), cx);
            deliver(view, "user", 2, cx);
            start(view, "second", 3, started_at, cx);
            view.apply_event(continuation("first", "user", "second"), cx);
            view.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "first".into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        let clock_before = Instant::now();
        window.render_frame(cx);
        let stream = view.read(cx);
        assert_eq!(stream.run_activity_groups.len(), 2);
        let first = &stream.run_activity_groups["first"];
        let second = &stream.run_activity_groups["second"];
        assert_ne!(first.entity_id(), second.entity_id());
        let old = window.find(("run-activity-toggle", first.entity_id()));
        assert!(old.label().unwrap().starts_with("Activity,"));
        let current = window.find(("run-activity-toggle", second.entity_id()));
        assert_running_clock(
            current.label().unwrap(),
            started_at,
            clock_before,
            Instant::now(),
        );
        assert!(old.bounds().bottom() <= current.bounds().top());
        let copy = stream
            .entries
            .iter()
            .find_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. }
                    if copy.original_text() == "Original assistant body" =>
                {
                    Some(copy.clone())
                }
                _ => None,
            })
            .expect("predecessor body retained at its original entry");
        let first_id = first.entity_id();
        let tool = stream
            .tool_cards
            .get("read-call")
            .expect("predecessor tool remains addressable");
        assert!(
            tool.read(cx)
                .matches_call(&vega_conversation::types::ToolCall {
                    id: "read-call".into(),
                    tool: "read".into(),
                    input_json: r#"{"path":"input.txt"}"#.into()
                })
        );
        let tool_id = tool.entity_id();
        let copy_id = copy.id;
        click_message_copy(window, cx, copy_id);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Original assistant body")
        );
        window.click(("run-activity-toggle", first_id), cx);
        window.render_frame(cx);
        assert!(
            window
                .find(("run-activity-toggle", first_id))
                .label()
                .unwrap()
                .contains("collapse run activity")
        );
        window.click(("tool-activity-toggle", tool_id), cx);
        window.render_frame(cx);
        let output = window.find("tool-output-0");
        assert_eq!(output.label(), Some("Original tool output"));
        assert!(output.visible());
        assert!(output.bounds().size.height > px(0.));
        window.click(("tool-activity-toggle", tool_id), cx);
        window.render_frame(cx);
        assert!(window.try_find("tool-output-0").is_none());
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.apply_event(continuation("second", "user-next", "third"), cx);
            deliver(view, "user-next", 4, cx);
            start(view, "third", 5, started_at, cx);
            view.apply_event(
                ConversationEvent::Interrupted {
                    message_id: "third".into(),
                    execution_duration_ms: Some(15000),
                },
                cx,
            );
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        let empty = &stream.run_activity_groups["second"];
        let old = window.try_find(("run-activity-toggle", empty.entity_id()));
        assert!(old.is_none_or(|entry| entry.bounds().size.height == px(0.)));
        let third = &stream.run_activity_groups["third"];
        assert!(
            window
                .find(("run-activity-toggle", third.entity_id()))
                .label()
                .unwrap()
                .starts_with("Stopped after 15s,")
        );
    });
}

#[gpui_kit::test]
fn independent_queue_preserves_previous_completion_and_starts_fresh_clock(cx: &mut TestAppContext) {
    init(cx);
    let second_started_at = Instant::now();
    let (view, cx) = cx.add_window_view(|_, cx| ConversationStream::new(thread(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            start(
                view,
                "first",
                1,
                Instant::now() - Duration::from_secs(12),
                cx,
            );
            view.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "first".into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: Some(12000),
                },
                cx,
            );
            deliver(view, "user", 2, cx);
            start(view, "second", 3, second_started_at, cx);
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let stream = view.read(cx);
        let first = &stream.run_activity_groups["first"];
        let second = &stream.run_activity_groups["second"];
        assert!(
            window
                .find(("run-activity-toggle", first.entity_id()))
                .label()
                .unwrap()
                .starts_with("Worked for 12s,")
        );
        assert_running_clock(
            window
                .find(("run-activity-toggle", second.entity_id()))
                .label()
                .unwrap(),
            second_started_at,
            second_started_at,
            Instant::now(),
        );
    });
}
