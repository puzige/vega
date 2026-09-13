use super::*;

fn click_composer_add(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) {
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-add")
        .expect("composer add button");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn frame_has(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

/// R57 P1: the `+` menu carries the permission-mode group, marks the current
/// value, and selecting a row emits exactly the existing settings request.
#[gpui_kit::test]
async fn r57_plus_menu_permission_group_marks_and_requests_exact_mode(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "r57-permission-menu");
    click_composer_add(window, cx);

    // All three permission rows and all three thread-mode commands are
    // reachable from the one menu.
    for selector in [
        "composer-action-permission-readonly",
        "composer-action-permission-confirm",
        "composer-action-permission-auto",
        "composer-action-mode-ask",
        "composer-action-mode-plan",
        "composer-action-mode-execute",
    ] {
        assert!(
            frame_has(window, selector, cx),
            "{selector} must be visible"
        );
    }

    // The fixture thread is `execute` + `confirm`: exactly those two rows are
    // marked, so the check reflects authoritative state rather than highlight.
    assert!(frame_has(
        window,
        "composer-action-permission-confirm-check",
        cx
    ));
    assert!(frame_has(window, "composer-action-mode-execute-check", cx));
    assert!(!frame_has(
        window,
        "composer-action-permission-auto-check",
        cx
    ));
    assert!(!frame_has(window, "composer-action-mode-ask-check", cx));

    // Mouse path: one click emits one permission-only request.
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-action-permission-auto")
        .expect("auto row");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        events.lock().expect("events").as_slice(),
        &[ThreadSettingsRequested {
            thread_id: "r57-permission-menu".into(),
            mode: None,
            permission_mode: Some(PermissionMode::Auto),
        }]
    );
    // The menu closes on selection and the draft stays untouched.
    assert!(!frame_has(window, "composer-action-permission-auto", cx));
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        ""
    );
}

/// R57 P1: keyboard navigation reaches the permission rows (the menu grew
/// from four rows to seven, so the index mapping is load-bearing).
#[gpui_kit::test]
async fn r57_plus_menu_keyboard_reaches_permission_rows_and_escape_closes(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "r57-permission-keys");
    click_composer_add(window, cx);

    // Rows: file(0) ask(1) plan(2) execute(3) readonly(4) confirm(5) auto(6).
    // Six downs from the initial highlight 0 land on `auto`.
    cx.simulate_keystrokes(window.into(), "down down down down down down");
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        events.lock().expect("events").as_slice(),
        &[ThreadSettingsRequested {
            thread_id: "r57-permission-keys".into(),
            mode: None,
            permission_mode: Some(PermissionMode::Auto),
        }]
    );

    // Escape closes without emitting anything.
    click_composer_add(window, cx);
    assert!(frame_has(window, "composer-action-permission-auto", cx));
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(!frame_has(window, "composer-action-permission-auto", cx));
    assert_eq!(events.lock().expect("events").len(), 1);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        ""
    );
}

/// R57 P1 regression guard: selecting the value the thread already has emits
/// nothing (the existing first-wins request contract), and the `+` menu still
/// carries all three thread modes.
#[gpui_kit::test]
async fn r57_plus_menu_thread_mode_rows_keep_the_existing_request_path(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "r57-mode-rows");
    click_composer_add(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-action-mode-plan")
        .expect("plan row");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        events.lock().expect("events").as_slice(),
        &[ThreadSettingsRequested {
            thread_id: "r57-mode-rows".into(),
            mode: Some(ThreadMode::Plan),
            permission_mode: None,
        }]
    );

    // Re-selecting the now-current mode is a no-op acknowledgement.
    stream.update(cx, |stream, cx| {
        let mut persisted = stream.thread.clone();
        persisted.mode = ThreadMode::Plan;
        stream.apply_thread(persisted, cx);
    });
    click_composer_add(window, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-action-mode-plan")
        .expect("plan row after apply");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert_eq!(events.lock().expect("events").len(), 1);
}

/// R57 P1: a slash query keeps the legacy filtered command list and never
/// offers permission rows, so the existing `/`-prefix flow is unchanged.
#[gpui_kit::test]
async fn r57_slash_query_stays_mode_only(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r57-slash-filter");
    focus_composer(window, &_stream, cx);
    window
        .update(cx, |_, _, cx| {
            _stream.update(cx, |stream, cx| {
                stream
                    .input
                    .update(cx, |input, cx| input.set_text("/p", cx));
            })
        })
        .expect("slash draft");
    cx.run_until_parked();
    assert!(frame_has(window, "composer-action-mode-plan", cx));
    assert!(!frame_has(window, "composer-action-mode-ask", cx));
    assert!(!frame_has(
        window,
        "composer-action-permission-readonly",
        cx
    ));
    assert!(!frame_has(window, "composer-action-permission-auto", cx));
}

#[gpui_kit::test]
async fn r11_composer_mode_ack_preserves_later_edits_and_terminal_precedence(
    cx: &mut TestAppContext,
) {
    let (window, stream, events) = open_controller_stream(cx, "actions-fence");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("/plan old draft", cx))
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(events.lock().expect("events").len(), 1);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/plan old draft",
        "no optimistic prefix deletion"
    );
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("later unsent draft", cx));
        let mut persisted = stream.thread.clone();
        persisted.mode = ThreadMode::Plan;
        stream.apply_thread(persisted, cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "later unsent draft"
    );
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        assert!(stream.actions.running);
        assert!(
            !stream.finish_composer_run(false, cx),
            "spawn failure release is not a cancellation"
        );
        assert!(!stream.actions.running);
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "finished-before-click".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "finished-before-click".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
        assert!(
            !stream.finish_composer_run(true, cx),
            "accepted successful terminal outranks a later token cancellation"
        );
        assert!(!stream.actions.running);
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "disconnected".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "disconnected".into(),
                delta: "retained disconnected partial".into(),
            },
            cx,
        );
        let before = stream.entries.len();
        assert!(!stream.finish_composer_run(false, cx));
        assert!(stream.active_agent_message.is_none());
        assert!(!stream.actions.running);
        assert_eq!(
            stream.entries.len(),
            before,
            "terminal presentation release retains partial message"
        );
    });
}

#[gpui_kit::test]
async fn r11_composer_marked_text_is_not_a_command(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler;
    let (window, stream, events) = open_controller_stream(cx, "actions-ime");
    focus_composer(window, &stream, cx);
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).composer_input();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "/plan", Some(5..5), window, cx)
            });
        })
        .expect("platform marked text");
    cx.run_until_parked();
    assert!(!stream.read_with(cx, |stream, _| stream.actions.visible()));
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));
    // Exercise the new scoped handlers directly while IME owns the text. Native
    // OS candidate windows are outside the headless test platform's scope.
    window
        .update(cx, |_, window, cx| {
            stream.update(cx, |stream, cx| {
                stream.accept_composer_action(&AcceptComposerAction, window, cx);
                stream.next_composer_action(&NextComposerAction, window, cx);
                stream.previous_composer_action(&PreviousComposerAction, window, cx);
                stream.close_composer_actions(&CloseComposerActions, window, cx);
                stream.next_composer_control(&NextComposerControl, window, cx);
                stream.open_composer_actions(window, cx);
                assert!(!stream.actions.visible());
                assert!(stream.input.read(cx).focus_handle(cx).is_focused(window));
            })
        })
        .expect("scoped IME guards");
    assert!(events.lock().expect("events").is_empty());
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/plan"
    );
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));
}
