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
