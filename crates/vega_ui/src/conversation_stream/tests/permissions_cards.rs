use super::*;

#[gpui::test]
async fn permission_queue_installs_matching_card_and_once_resolves(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-once", "printf ok"));
    let future = request_permission(&queue, "call-once", "printf ok");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));

    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(future.await, PermissionDecision::Once);
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn permission_request_first_waits_for_matching_proposal(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);

    // The runtime may enqueue the permission request after the proposal has
    // already been durably sent to the app channel, but GPUI can run the
    // queue listener first. The request must stay owned until the matching
    // ToolCallProposed event arrives.
    let future = request_permission(&queue, "call-request-first", "printf request-first");
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));

    propose(
        window,
        cx,
        bash_call("call-request-first", "printf request-first"),
    );
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    propose(
        window,
        cx,
        bash_call("call-request-first", "printf request-first"),
    );
    cx.run_until_parked();
    let permission_entries = window
        .update(cx, |stream, _, _| {
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Permission { .. }))
                .count()
        })
        .expect("stream window");
    assert_eq!(permission_entries, 1, "duplicate proposal keeps one card");

    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(future.await, PermissionDecision::Once);
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn terminal_for_old_card_does_not_clear_new_permission_request(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-old", "printf old"));
    let old_future = request_permission(&queue, "call-old", "printf old");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));

    // A newer queue request replaces the old responder before its listener
    // wake is processed. The old terminal must close only the old card.
    let new_future = request_permission(&queue, "call-new", "printf new");
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "call-old".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Rejected,
                        output: "Tool error: permission denied".into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
        })
        .expect("stream window");
    assert_eq!(old_future.await, PermissionDecision::Timeout);
    assert!(queue.has_pending(), "the newer request remains owned");

    cx.run_until_parked();
    propose(window, cx, bash_call("call-new", "printf new"));
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(new_future.await, PermissionDecision::Once);
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn terminal_first_request_is_failed_closed_and_late_proposal_stays_hidden(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    let future = request_permission(&queue, "call-terminal-first", "printf terminal-first");
    cx.run_until_parked();
    assert!(queue.has_pending());

    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "call-terminal-first".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Rejected,
                        output: "Tool error: permission denied".into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
        })
        .expect("stream window");
    propose(
        window,
        cx,
        bash_call("call-terminal-first", "printf terminal-first"),
    );
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn settings_close_deferred_permission_before_late_proposal(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    let future = request_permission(&queue, "call-settings-deferred", "printf settings-deferred");
    cx.run_until_parked();
    assert!(queue.has_pending());

    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));

    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    propose(
        window,
        cx,
        bash_call("call-settings-deferred", "printf settings-deferred"),
    );
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn cancel_clears_deferred_permission_before_late_proposal(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    let future = request_permission(&queue, "call-cancel-deferred", "printf cancel-deferred");
    cx.run_until_parked();
    assert!(queue.has_pending());

    window
        .update(cx, |stream, _, cx| stream.timeout_permission(cx))
        .expect("stream window");
    assert_eq!(future.await, PermissionDecision::Timeout);
    propose(
        window,
        cx,
        bash_call("call-cancel-deferred", "printf cancel-deferred"),
    );
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn permission_target_mismatch_times_out_and_corrupts_tool_card(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-mismatch", "printf safe"));
    let future = request_permission(&queue, "call-mismatch", "printf different");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
    let visible = window
        .update(cx, |stream, _, cx| {
            stream.tool_cards["call-mismatch"].read(cx).visible_text()
        })
        .expect("stream window");
    assert!(visible.contains("工具结果损坏"));
    assert!(!visible.contains("printf different"));
}

#[gpui::test]
async fn late_permission_requests_for_approved_terminal_or_corrupt_cards_timeout(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);

    propose(window, cx, bash_call("call-approved", "printf approved"));
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: "call-approved".into(),
                    approval: vega_conversation::types::Approval::Once,
                },
                cx,
            );
        })
        .expect("stream window");
    let future = request_permission(&queue, "call-approved", "printf approved");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));

    propose(
        window,
        cx,
        bash_call("call-terminal-late", "printf terminal"),
    );
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "call-terminal-late".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Rejected,
                        output: "Tool error: permission denied".into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
        })
        .expect("stream window");
    let future = request_permission(&queue, "call-terminal-late", "printf terminal");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));

    propose(
        window,
        cx,
        ToolCall {
            id: "call-corrupt".into(),
            tool: "bash".into(),
            input_json: r#"{"cmd":1}"#.into(),
        },
    );
    let future = request_permission(&queue, "call-corrupt", "printf corrupt");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
    let permission_entries = window
        .update(cx, |stream, _, _| {
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Permission { .. }))
                .count()
        })
        .expect("stream window");
    assert_eq!(permission_entries, 0);
}

#[gpui::test]
async fn settings_hidden_and_terminal_paths_fail_closed_without_rendering(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-settings", "printf settings"));
    let future = request_permission(&queue, "call-settings", "printf settings");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));

    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    propose(window, cx, bash_call("call-terminal", "printf terminal"));
    let future = request_permission(&queue, "call-terminal", "printf terminal");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "call-terminal".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Rejected,
                        output: "Tool error: permission denied".into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
        })
        .expect("stream window");
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));

    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    propose(window, cx, bash_call("call-hidden", "printf hidden"));
    let future = request_permission(&queue, "call-hidden", "printf hidden");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
}

#[gpui::test]
async fn window_release_drops_listener_and_active_card_fail_closed(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-window", "printf close"));
    let future = request_permission(&queue, "call-window", "printf close");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("stream window");
    cx.run_until_parked();
    assert_eq!(future.await, PermissionDecision::Timeout);
}

#[gpui::test]
async fn thread_switch_timeout_contract_removes_prompt_before_view_replacement(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("call-thread", "printf switch"));
    let future = request_permission(&queue, "call-thread", "printf switch");
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    window
        .update(cx, |stream, _, cx| stream.timeout_permission(cx))
        .expect("stream window");
    assert_eq!(future.await, PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
}
