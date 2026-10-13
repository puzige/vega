use super::*;
use gpui_kit::{Bounds, Modifiers, VisualTestContext};
use vega_conversation::types::PendingMessage;

fn message(id: &str, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "issue299".into(),
        generation: "run-one".into(),
        behavior: PendingMessageBehavior::Queue,
        content: content.into(),
        images: Vec::new(),
    }
}

fn project(
    stream: &Entity<ConversationStream>,
    messages: Vec<PendingMessage>,
    cx: &mut TestAppContext,
) {
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages,
                ..Default::default()
            },
            cx,
        );
    });
    cx.run_until_parked();
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let target = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(target.center(), Modifiers::default());
    visual.run_until_parked();
}

fn capture_queue(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Arc<Mutex<Vec<PendingQueueRequested>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    cx.update(|cx| {
        cx.subscribe(stream, move |_, event: &PendingQueueRequested, _| {
            captured.lock().unwrap().push(event.clone())
        })
        .detach()
    });
    events
}

#[gpui_kit::test]
async fn issue299_queued_ack_preserves_a_retyped_identical_draft(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("same text", cx));
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| {
            input.clear(cx);
            input.set_text("same text", cx);
        });
        stream.accept_queued_submission("same text", cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "same text"
    );
    cx.simulate_keystrokes(window.into(), "enter");
    stream.update(cx, |stream, cx| {
        stream.accept_queued_submission("same text", cx)
    });
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).text().is_empty()));
}

#[gpui_kit::test]
async fn issue299_platform_replacement_and_ime_preserve_a_new_identical_draft(
    cx: &mut TestAppContext,
) {
    use gpui_kit::EntityInputHandler;
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("你好", cx));
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).composer_input();
            input.update(cx, |input, cx| {
                input.replace_text_in_range(Some(0..2), "", window, cx);
                input.replace_and_mark_text_in_range(None, "ni hao", Some(6..6), window, cx);
                input.replace_text_in_range(None, "你好", window, cx);
            });
            stream.update(cx, |stream, cx| stream.accept_queued_submission("你好", cx));
        })
        .unwrap();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "你好"
    );
    assert!(!stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));
}

#[gpui_kit::test]
async fn issue299_running_enter_follows_default_and_ack_preserves_later_draft(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let submitted = Arc::new(Mutex::new(Vec::<ComposerSubmitted>::new()));
    let captured = submitted.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
            captured.lock().unwrap().push(event.clone())
        })
        .detach()
    });
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("queued draft", cx));
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        submitted.lock().unwrap()[0].pending_behavior,
        Some(PendingMessageBehavior::Queue)
    );
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "queued draft"
    );
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("later draft", cx));
        stream.accept_queued_submission("queued draft", cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "later draft"
    );
    cx.update(|cx| cx.set_global(RunningMessageDefault(PendingMessageBehavior::Steer)));
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        submitted.lock().unwrap()[1].pending_behavior,
        Some(PendingMessageBehavior::Steer)
    );
    stream.update(cx, |stream, cx| {
        stream.reject_composer_submission(cx);
        stream.finish_composer_run(false, cx);
    });
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(submitted.lock().unwrap()[2].pending_behavior, None);
}

#[gpui_kit::test]
async fn issue299_running_send_focus_keeps_stop_adjacent_and_skips_pending_send(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let stopped = Arc::new(Mutex::new(Vec::new()));
    let captured = stopped.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerStopRequested, _| {
            captured.lock().unwrap().push(event.thread_id.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("retained draft", cx));
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).action_focus[1].is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).pending_ui.send_focus.is_focused(window));
        })
        .unwrap();
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    assert!(stream.read_with(cx, |stream, _| stream.composer_submit_pending));
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).action_focus[1].is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).model_focus.is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "tab enter");
    assert_eq!(*stopped.lock().unwrap(), ["issue299"]);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "retained draft"
    );
}

#[gpui_kit::test]
async fn issue299_pending_actions_keep_stable_ids_and_menu_restores_focus(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let events = capture_queue(&stream, cx);
    stream.update(cx, ConversationStream::begin_composer_run);
    project(
        &stream,
        vec![message("first", "First"), message("second", "Second")],
        cx,
    );
    click(window, "pending-steer-second", cx);
    assert_eq!(
        events.lock().unwrap()[0].action,
        PendingQueueAction::Steer("second".into())
    );
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.running_message_behavior(cx)),
        PendingMessageBehavior::Queue
    );
    project(&stream, vec![message("second", "Second")], cx);
    click(window, "pending-more-second", cx);
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.menu.is_none()));
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).pending_ui.items["second"].focus[2].is_focused(window))
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "space");
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.menu.is_some()));
    click(window, "pending-menu-toggle", cx);
    assert_eq!(
        events.lock().unwrap()[1].action,
        PendingQueueAction::ToggleDefault
    );
    project(&stream, vec![message("second", "Second")], cx);
    click(window, "pending-remove-second", cx);
    assert_eq!(
        events.lock().unwrap()[2].action,
        PendingQueueAction::Remove("second".into())
    );
}

#[gpui_kit::test]
async fn issue299_edit_requires_ack_and_preserves_a_new_draft_with_attachments(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let events = capture_queue(&stream, cx);
    let mut queued = message("edit", "Original queued message");
    queued.images.push(
        ImageAttachment::from_bytes(
            include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
        )
        .unwrap(),
    );
    project(&stream, vec![queued.clone()], cx);
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep draft", cx))
    });
    click(window, "pending-more-edit", cx);
    click(window, "pending-menu-edit", cx);
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "keep draft"
    );
    stream.update(cx, |stream, cx| stream.input.update(cx, TextInput::clear));
    click(window, "pending-menu-edit", cx);
    assert_eq!(
        events.lock().unwrap()[0].action,
        PendingQueueAction::Edit("edit".into())
    );
    assert!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().is_empty()
            && stream.attachments.is_empty())
    );
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("typed during removal", cx));
        stream.accept_pending_edit(queued.clone(), cx);
        stream.apply_pending_queue(PendingQueueProjection::default(), cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "typed during removal"
    );
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.recovered_edit.is_some()));
    stream.update(cx, |stream, cx| stream.input.update(cx, TextInput::clear));
    click(window, "pending-recover-edit", cx);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        queued.content
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.attachments.len()),
        1
    );
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.recovered_edit.is_none()));
}

#[gpui_kit::test]
async fn issue299_pending_rail_geometry_is_bounded_in_both_themes(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    stream.update(cx, ConversationStream::begin_composer_run);
    let mut first = message("first", &"Long queued content ".repeat(35));
    first.images.push(
        ImageAttachment::from_bytes(
            include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
        )
        .unwrap(),
    );
    project(
        &stream,
        vec![
            first,
            message("two", "Two"),
            message("three", "Three"),
            message("four", "Four"),
        ],
        cx,
    );
    for theme in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        cx.update(|cx| cx.set_global(theme));
        let shell = bounds(window, "composer-shell", cx);
        let rail = bounds(window, "pending-message-rail", cx);
        let viewport = bounds(window, "pending-message-scroll", cx);
        let thumbnail = bounds(window, "pending-thumbnail-first-0", cx);
        let text = bounds(window, "pending-content-first", cx);
        let steer = bounds(window, "pending-steer-first", cx);
        assert!((f32::from(rail.left() - shell.left()) - 16.0).abs() <= 1.0);
        assert!((f32::from(shell.right() - rail.right()) - 16.0).abs() <= 1.0);
        assert!((f32::from(rail.bottom() - shell.top()) - 8.0).abs() <= 1.0);
        assert!((f32::from(viewport.size.height) - 108.0).abs() <= 1.0);
        assert_eq!(thumbnail.size.width, px(24.0));
        assert_eq!(thumbnail.size.height, px(24.0));
        assert!(text.right() <= steer.left());
        assert!(bounds(window, "pending-more-first", cx).right() < shell.right());
        click(window, "pending-more-first", cx);
        let menu = bounds(window, "pending-message-menu", cx);
        assert!(menu.top() < shell.bottom());
        cx.simulate_keystrokes(window.into(), "escape");
    }
}

#[gpui_kit::test]
async fn issue299_durable_delivery_deduplicates_and_keeps_message_anchor_identity(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "issue299");
    project(&stream, vec![message("first", "Delivered once")], cx);
    let event = ConversationEvent::PendingMessageDelivered {
        pending_id: "first".into(),
        message_id: "durable-user".into(),
        seq: 37,
        content: "Delivered once".into(),
        images: Vec::new(),
    };
    stream.update(cx, |stream, cx| {
        stream.apply_event(event.clone(), cx);
        stream.apply_event(event, cx);
    });
    stream.read_with(cx, |stream, _| {
        assert!(stream.pending_queue.messages.is_empty());
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::User { .. }))
                .count(),
            1
        );
        assert_eq!(
            stream.entry_identities[0].message_id.as_deref(),
            Some("durable-user")
        );
        assert_eq!(stream.entry_identities[0].sequence, Some(37));
    });
}

#[gpui_kit::test]
async fn issue299_narrow_rail_keeps_max_attachment_actions_and_composer_hit_area(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "issue299".into();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root = stream.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::centered(
                        None,
                        gpui_kit::size(px(360.0), px(700.0)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| StreamHarness { stream: root }),
            )
        })
        .unwrap();
    let events = capture_queue(&stream, cx);
    stream.update(cx, ConversationStream::begin_composer_run);
    let mut queued = message("narrow", &"Long content ".repeat(80));
    let image = ImageAttachment::from_bytes(
        include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
    )
    .unwrap();
    queued.images = vec![image; 4];
    project(&stream, vec![queued], cx);
    let rail = bounds(window, "pending-message-rail", cx);
    for selector in [
        "pending-steer-narrow",
        "pending-remove-narrow",
        "pending-more-narrow",
    ] {
        let action = bounds(window, selector, cx);
        assert!(action.left() >= rail.left());
        assert!(action.right() <= rail.right());
        assert!(action.top() >= rail.top());
        assert!(action.bottom() <= rail.bottom() - px(Layout::PENDING_MESSAGE_OVERLAP));
    }
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("pending-thumbnail-narrow-1")
            .is_none()
    );
    click(window, "pending-more-narrow", cx);
    cx.simulate_keystrokes(window.into(), "tab");
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        events.lock().unwrap()[0].action,
        PendingQueueAction::ToggleDefault
    );
    project(&stream, vec![message("narrow", "Still queued")], cx);
    click(window, "pending-more-narrow", cx);
    let shell = bounds(window, "composer-shell", cx);
    let outside_position = gpui_kit::point(rail.left(), rail.top() - px(8.0));
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(outside_position, Modifiers::default());
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.menu.is_none()));
    let input_position = gpui_kit::point(shell.left() + px(24.0), shell.top() + px(20.0));
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(input_position, Modifiers::default());
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, _| stream.pending_ui.menu.is_none()));
    window
        .update(cx, |_, window, cx| {
            assert!(
                stream
                    .read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            )
        })
        .unwrap();
}

#[gpui_kit::test]
async fn issue299_paused_queue_requires_explicit_resume_and_codex_has_no_queue_controls(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let events = capture_queue(&stream, cx);
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages: vec![message("paused", "Ready to resume")],
                paused: true,
                ..Default::default()
            },
            cx,
        )
    });
    click(window, "pending-steer-paused", cx);
    assert!(events.lock().unwrap().is_empty());
    window
        .update(cx, |_, window, cx| {
            let resume_focus = stream.read(cx).pending_ui.resume_focus.clone();
            resume_focus.focus(window, cx);
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "space");
    assert_eq!(events.lock().unwrap()[0].action, PendingQueueAction::Resume);
    stream.update(cx, |stream, cx| {
        stream.thread.backend = TaskBackend::Codex;
        stream.begin_composer_run(cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("Codex follow-up", cx));
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("pending-message-rail").is_none());
    assert!(visual.debug_bounds("composer-running-send").is_none());
    assert!(visual.debug_bounds("pending-resume").is_none());
}

#[gpui_kit::test]
async fn issue299_failed_edit_keeps_queue_and_empty_draft_recoverable(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue299");
    let events = capture_queue(&stream, cx);
    project(&stream, vec![message("retry", "Retain on failure")], cx);
    click(window, "pending-more-retry", cx);
    click(window, "pending-menu-edit", cx);
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages: vec![message("retry", "Retain on failure")],
                error: Some("消息正在交付，请稍后重试".into()),
                ..Default::default()
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).text().is_empty()));
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.pending_queue.messages.len()),
        1
    );
    click(window, "pending-more-retry", cx);
    click(window, "pending-menu-edit", cx);
    assert_eq!(events.lock().unwrap().len(), 2);
    stream.update(cx, |stream, cx| {
        stream.accept_pending_edit(message("retry", "Retain on failure"), cx);
        stream.apply_pending_queue(
            PendingQueueProjection {
                error: Some("队列读取失败，请重试".into()),
                ..Default::default()
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "Retain on failure"
    );
}
