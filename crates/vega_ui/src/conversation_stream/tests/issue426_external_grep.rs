use super::*;
use crate::tool_card::{ToolActivityCategory, ToolActivityState};
use gpui_kit::test::TestWindowExt;

fn grep_call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: "grep".into(),
        input_json: serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":path})
            .to_string(),
    }
}

fn read_request(id: &str, target: &str) -> PermissionRequest {
    PermissionRequest {
        call_id: id.into(),
        tool: "read".into(),
        display_target: target.into(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: None,
    }
}

#[gpui_kit::test]
async fn issue426_grep_read_queue_mounts_and_resolves_existing_actions(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for (target, keys, expected) in [
        ("/external/note.txt", "enter", PermissionDecision::Once),
        ("/external/search", "tab space", PermissionDecision::Always),
        (
            "/external/rejected",
            "tab tab space",
            PermissionDecision::Deny { note: None },
        ),
    ] {
        let (window, queue) = open_permission_stream(cx);
        let decision = queue.request(
            read_request("external-grep", target),
            CancellationToken::new(),
        );
        cx.run_until_parked();
        assert!(!has_active_permission(window, cx));
        propose(window, cx, grep_call("external-grep", target));
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        window
            .update(cx, |stream, _, cx| {
                let card = stream.tool_cards["external-grep"].read(cx);
                assert_eq!(card.permission_identity(), Some(("read", target)));
                assert_eq!(card.activity_category(), ToolActivityCategory::Search);
                assert!(!card.visible_text().contains("PRIVATE_PATTERN_CANARY"));
                let prompt = stream
                    .active_permission
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .visible_text();
                assert!(prompt.contains(target));
                assert!(!prompt.contains("PRIVATE_PATTERN_CANARY"));
                assert!(!prompt.contains("grep-dir:v1:"));
                assert_eq!(
                    stream
                        .entries
                        .iter()
                        .filter(|entry| matches!(entry, StreamEntry::Permission { .. }))
                        .count(),
                    1
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), keys);
        assert_eq!(decision.await.unwrap(), expected);
        cx.run_until_parked();
        assert!(!has_active_permission(window, cx));
        assert!(!queue.has_pending());
        window
            .update(cx, |stream, _, cx| {
                let denied = matches!(expected, PermissionDecision::Deny { .. });
                if !denied {
                    stream.apply_event(
                        ConversationEvent::ToolCallApproved {
                            call_id: "external-grep".into(),
                            approval: if expected == PermissionDecision::Always {
                                Approval::Always
                            } else {
                                Approval::Once
                            },
                        },
                        cx,
                    );
                    stream.apply_event(
                        ConversationEvent::ToolCallRunning {
                            call_id: "external-grep".into(),
                        },
                        cx,
                    );
                }
                stream.apply_event(
                    ConversationEvent::ToolCallFinished {
                        call_id: "external-grep".into(),
                        result: ToolResult {
                            status: if denied {
                                ToolCallStatus::Rejected
                            } else {
                                ToolCallStatus::Success
                            },
                            output: if denied {
                                "Tool error: permission denied".into()
                            } else {
                                "/external/note.txt:1:match".into()
                            },
                            reused: false,
                            exit_code: None,
                            duration_ms: None,
                            truncated: if denied { None } else { Some(false) },
                            invalid: None,
                        },
                    },
                    cx,
                );
                let card = stream.tool_cards["external-grep"].read(cx);
                assert!(card.permission_identity().is_none());
                assert_eq!(card.activity_category(), ToolActivityCategory::Search);
                assert!(!card.visible_text().contains("Tool result corrupted"));
                assert!(!card.visible_text().contains("PRIVATE_PATTERN_CANARY"));
            })
            .unwrap();
    }
}

#[gpui_kit::test]
async fn issue426_grep_timeout_and_cancel_keep_real_terminal_state(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for (cancel_pending, cancel_after_approval) in [(false, false), (true, false), (false, true)] {
        let (window, queue) = open_permission_stream(cx);
        let target = "/external/search";
        propose(window, cx, grep_call("cancel-grep", target));
        let cancel = CancellationToken::new();
        let decision = queue.request(read_request("cancel-grep", target), cancel.clone());
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        if cancel_after_approval {
            cx.simulate_keystrokes(window.into(), "enter");
            assert_eq!(decision.await.unwrap(), PermissionDecision::Once);
        } else {
            if cancel_pending {
                cancel.cancel();
            } else {
                window
                    .update(cx, |stream, _, cx| stream.timeout_permission(cx))
                    .unwrap();
            }
            assert_eq!(decision.await.unwrap(), PermissionDecision::Timeout);
        }
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!has_active_permission(window, cx));
        assert!(!queue.has_pending());
        window
            .update(cx, |stream, _, cx| {
                if cancel_after_approval {
                    stream.apply_event(
                        ConversationEvent::ToolCallApproved {
                            call_id: "cancel-grep".into(),
                            approval: Approval::Once,
                        },
                        cx,
                    );
                    stream.apply_event(
                        ConversationEvent::ToolCallRunning {
                            call_id: "cancel-grep".into(),
                        },
                        cx,
                    );
                    cancel.cancel();
                }
                stream.apply_event(
                    ConversationEvent::ToolCallFinished {
                        call_id: "cancel-grep".into(),
                        result: ToolResult {
                            status: if cancel_after_approval {
                                ToolCallStatus::Cancelled
                            } else {
                                ToolCallStatus::Rejected
                            },
                            output: if cancel_after_approval {
                                vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT.into()
                            } else {
                                "Tool error: permission denied".into()
                            },
                            reused: false,
                            exit_code: None,
                            duration_ms: None,
                            truncated: None,
                            invalid: None,
                        },
                    },
                    cx,
                );
                let card = stream.tool_cards["cancel-grep"].read(cx);
                assert_eq!(
                    card.activity_state(),
                    if cancel_after_approval {
                        ToolActivityState::Cancelled
                    } else {
                        ToolActivityState::Rejected
                    }
                );
                assert!(!card.visible_text().contains("Tool result corrupted"));
            })
            .unwrap();
    }
}

#[gpui_kit::test]
async fn issue426_grep_pending_to_cancelled_is_corrupt_without_relaxing_ui(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, _) = open_permission_stream(cx);
    propose(window, cx, grep_call("pending-cancel", "/external/search"));
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "pending-cancel".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Cancelled,
                        output: vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT.into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
            assert!(
                stream.tool_cards["pending-cancel"]
                    .read(cx)
                    .visible_text()
                    .contains("Tool result corrupted")
            );
        })
        .unwrap();
}

#[gpui_kit::test]
async fn issue426_invalid_grep_safe_card_keeps_failure_and_hides_audit_hash(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    let hash = "a".repeat(64);
    propose(
        window,
        cx,
        ToolCall {
            id: "invalid-grep".into(),
            tool: "grep".into(),
            input_json: serde_json::json!({
                "audit_version":"grep_invalid_v1", "raw_input_bytes":37, "raw_input_sha256":hash,
            })
            .to_string(),
        },
    );
    window
        .update(cx, |stream, _, cx| {
            assert!(
                stream.tool_cards["invalid-grep"]
                    .read(cx)
                    .permission_identity()
                    .is_none()
            );
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: "invalid-grep".into(),
                    approval: Approval::Once,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallRunning {
                    call_id: "invalid-grep".into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "invalid-grep".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Failed,
                        output: vega_runtime::GREP_INVALID_INPUT_OUTPUT.into(),
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        invalid: None,
                    },
                },
                cx,
            );
            let card = stream.tool_cards["invalid-grep"].read(cx);
            assert_eq!(card.activity_state(), ToolActivityState::Failed);
            assert_eq!(card.activity_category(), ToolActivityCategory::Search);
            let text = card.visible_text();
            assert!(!text.contains(&hash));
            assert!(!text.contains("raw_input_sha256"));
            assert!(!text.contains("PRIVATE_PATTERN_CANARY"));
            assert!(!text.contains("Tool result corrupted"));
        })
        .unwrap();
    assert!(!queue.has_pending());
}

#[gpui_kit::test]
async fn issue426_grep_wrong_identity_fails_closed_without_relaxing_glob(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for (tool, input, request_tool, request_target, expected_queued_mismatch) in [
        (
            "grep",
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"/external/search"}),
            "grep",
            "/external/search",
            false,
        ),
        (
            "grep",
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"/external/search"}),
            "read",
            "/external/other",
            true,
        ),
        (
            "grep",
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"/external/search"}),
            "read",
            "grep-dir:v1:/external/search",
            false,
        ),
        (
            "grep",
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"relative/search"}),
            "read",
            "relative/search",
            false,
        ),
        (
            "grep",
            serde_json::json!({"pattern":7, "path":"/external/search"}),
            "read",
            "/external/search",
            true,
        ),
        (
            "grep",
            serde_json::json!({"path":"/external/search"}),
            "read",
            "/external/search",
            true,
        ),
        (
            "glob",
            serde_json::json!({"pattern":"*", "path":"/external/search"}),
            "read",
            "/external/search",
            true,
        ),
    ] {
        let (window, queue) = open_permission_stream(cx);
        propose(
            window,
            cx,
            ToolCall {
                id: "wrong-grep".into(),
                tool: tool.into(),
                input_json: input.to_string(),
            },
        );
        let (original_state, original_identity, original_text) = window
            .update(cx, |stream, _, cx| {
                let card = stream.tool_cards["wrong-grep"].read(cx);
                (
                    card.activity_state(),
                    card.permission_identity()
                        .map(|(tool, target)| (tool.to_owned(), target.to_owned())),
                    card.visible_text(),
                )
            })
            .unwrap();
        let mut request = read_request("wrong-grep", request_target);
        request.tool = request_tool.into();
        let decision = queue.request(request, CancellationToken::new());
        assert_eq!(queue.has_pending(), expected_queued_mismatch);
        cx.run_until_parked();
        assert_eq!(decision.await.unwrap(), PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));
        assert!(!queue.has_pending());
        window
            .update(cx, |stream, _, cx| {
                assert!(stream.deferred_permission.is_none());
                assert!(
                    stream
                        .entries
                        .iter()
                        .all(|entry| !matches!(entry, StreamEntry::Permission { .. }))
                );
                let card = stream.tool_cards["wrong-grep"].read(cx);
                if expected_queued_mismatch {
                    assert!(card.visible_text().contains("Tool result corrupted"));
                } else {
                    assert_eq!(card.activity_state(), original_state);
                    assert_eq!(
                        card.permission_identity()
                            .map(|(tool, target)| (tool.to_owned(), target.to_owned())),
                        original_identity
                    );
                    assert_eq!(card.visible_text(), original_text);
                }
                assert!(!card.visible_text().contains("PRIVATE_PATTERN_CANARY"));
            })
            .unwrap();
    }
}

#[gpui_kit::test]
async fn issue426_grep_wrong_call_id_never_mounts_on_another_card(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, grep_call("owned-grep", "/external/search"));
    propose(window, cx, grep_call("other-grep", "/external/other"));
    let (original_state, original_text) = window
        .update(cx, |stream, _, cx| {
            let card = stream.tool_cards["owned-grep"].read(cx);
            (card.activity_state(), card.visible_text())
        })
        .unwrap();
    let decision = queue.request(
        read_request("other-grep", "/external/search"),
        CancellationToken::new(),
    );
    cx.run_until_parked();
    assert_eq!(decision.await.unwrap(), PermissionDecision::Timeout);
    assert!(!has_active_permission(window, cx));
    assert!(!queue.has_pending());
    window
        .update(cx, |stream, _, cx| {
            let owned_card = stream.tool_cards["owned-grep"].read(cx);
            assert_eq!(
                owned_card.permission_identity(),
                Some(("read", "/external/search"))
            );
            assert_eq!(owned_card.activity_state(), original_state);
            assert_eq!(owned_card.visible_text(), original_text);
            assert!(!owned_card.visible_text().contains("PRIVATE_PATTERN_CANARY"));
            let other_text = stream.tool_cards["other-grep"].read(cx).visible_text();
            assert!(other_text.contains("Tool result corrupted"));
            assert!(!other_text.contains("PRIVATE_PATTERN_CANARY"));
        })
        .unwrap();
    let decision = queue.request(
        read_request("owned-grep", "/external/search"),
        CancellationToken::new(),
    );
    cx.run_until_parked();
    assert!(has_active_permission(window, cx));
    assert!(queue.has_pending());
    window
        .update(cx, |stream, _, _| {
            assert_eq!(
                stream.active_permission_call_id.as_deref(),
                Some("owned-grep")
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(decision.await.unwrap(), PermissionDecision::Once);
    cx.run_until_parked();
    assert!(!has_active_permission(window, cx));
    assert!(!queue.has_pending());
}
