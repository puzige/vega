use super::*;

#[test]
fn issue426_grep_permission_projection_requires_valid_canonical_absolute_path() {
    for (input, expected) in [
        (
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"/external/note.txt"}),
            Some("/external/note.txt"),
        ),
        (
            serde_json::json!({"pattern":"PRIVATE_PATTERN_CANARY", "path":"/external/search"}),
            Some("/external/search"),
        ),
        (serde_json::json!({"pattern":"", "path":"/"}), None),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/.git/config"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/.GIT/config"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/.Git"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search", "extra":true}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"relative/search"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/../search"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/./search"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external//search"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search/"}),
            None,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/\u{0000}search"}),
            None,
        ),
        (serde_json::json!({"pattern":"needle", "path":null}), None),
        (serde_json::json!({"pattern":"needle"}), None),
        (serde_json::json!({"pattern":"needle", "path":7}), None),
        (
            serde_json::json!({"pattern":7, "path":"/external/search"}),
            None,
        ),
        (serde_json::json!({"path":"/external/search"}), None),
    ] {
        let call = ToolCall {
            id: "grep-call".into(),
            tool: "grep".into(),
            input_json: input.to_string(),
        };
        let projection = tool_card_input_projection(&call);
        assert_eq!(projection.tool(), Some("grep"));
        assert_eq!(projection.permission_target(), expected, "{input}");
        assert!(matches!(
            projection,
            ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Grep,
                display_target: None,
                ..
            }
        ));
        assert!(!format!("{projection:?}").contains("PRIVATE_PATTERN_CANARY"));
        let glob = tool_card_input_projection(&ToolCall {
            tool: "glob".into(),
            ..call
        });
        assert_eq!(glob.permission_target(), None);
    }
}

#[test]
fn issue426_grep_permission_projection_rejects_duplicate_fields() {
    for input in [
        r#"{"pattern":"needle","pattern":"other","path":"/external/search"}"#,
        r#"{"pattern":"needle","path":"relative","path":"/external/search"}"#,
        r#"{"pattern":"needle","path":"/external/search","path":"/external/search"}"#,
    ] {
        assert!(grep_permission_path(input).is_none());
        assert!(!grep_permission_target_matches(
            input,
            "/external/search",
            "/external/search"
        ));
        let projection = tool_card_input_projection(&ToolCall {
            id: "grep-call".into(),
            tool: "grep".into(),
            input_json: input.into(),
        });
        assert!(projection.permission_target().is_none());
    }
}

#[test]
fn issue426_invalid_grep_envelope_is_strict_and_content_free() {
    let valid = serde_json::json!({
        "audit_version":"grep_invalid_v1", "raw_input_bytes":u64::MAX,
        "raw_input_sha256":"a".repeat(64),
    });
    assert!(valid_grep_invalid_input(&valid.to_string()));
    let mut zero_bytes = valid.clone();
    zero_bytes["raw_input_bytes"] = 0.into();
    assert!(valid_grep_invalid_input(&zero_bytes.to_string()));
    let projection = tool_card_input_projection(&ToolCall {
        id: "invalid-grep".into(),
        tool: "grep".into(),
        input_json: valid.to_string(),
    });
    assert!(matches!(
        projection,
        ToolCardInputProjection::ReadOnly {
            tool: ReadOnlyToolKind::Grep,
            permission_path: None,
            display_target: None,
        }
    ));
    assert!(!format!("{projection:?}").contains(&"a".repeat(64)));
    for (field, value) in [
        ("audit_version", serde_json::json!("grep_invalid_v2")),
        ("audit_version", serde_json::json!(1)),
        ("raw_input_bytes", serde_json::json!("7")),
        ("raw_input_bytes", serde_json::json!(-1)),
        ("raw_input_bytes", serde_json::json!(7.0)),
        ("raw_input_bytes", serde_json::json!(null)),
        ("raw_input_sha256", serde_json::json!("A".repeat(64))),
        ("raw_input_sha256", serde_json::json!("g".repeat(64))),
        ("raw_input_sha256", serde_json::json!("a".repeat(63))),
        ("raw_input_sha256", serde_json::json!(7)),
        ("pattern", serde_json::json!("PRIVATE_PATTERN_CANARY")),
        ("path", serde_json::json!("/external/search")),
    ] {
        let mut input = valid.clone();
        input[field] = value;
        let input_json = input.to_string();
        assert!(grep_invalid_input_claim(&input_json));
        assert!(!valid_grep_invalid_input(&input_json));
        assert_eq!(
            tool_card_input_projection(&ToolCall {
                id: "invalid-grep".into(),
                tool: "grep".into(),
                input_json,
            }),
            ToolCardInputProjection::Corrupt
        );
    }
    for field in ["audit_version", "raw_input_bytes", "raw_input_sha256"] {
        let mut input = valid.clone();
        input.as_object_mut().unwrap().remove(field);
        assert!(!valid_grep_invalid_input(&input.to_string()));
    }
    let hash = "a".repeat(64);
    for input in [
        format!(
            r#"{{"audit_version":"grep_invalid_v1","audit_version":"grep_invalid_v1","raw_input_bytes":7,"raw_input_sha256":"{hash}"}}"#
        ),
        format!(
            r#"{{"audit_version":"grep_invalid_v1","raw_input_bytes":7,"raw_input_bytes":7,"raw_input_sha256":"{hash}"}}"#
        ),
        format!(
            r#"{{"audit_version":"grep_invalid_v1","raw_input_bytes":7,"raw_input_sha256":"{hash}","raw_input_sha256":"{hash}"}}"#
        ),
        format!(
            r#"{{"audit_version":"grep_invalid_v1","raw_input_bytes":18446744073709551616,"raw_input_sha256":"{hash}"}}"#
        ),
        r#"{"audit_version":"grep_invalid_v1""#.into(),
    ] {
        assert!(grep_invalid_input_claim(&input));
        assert!(!valid_grep_invalid_input(&input));
    }
}

#[test]
fn issue426_grep_result_projection_preserves_terminal_states_and_metadata_guards() {
    let input = tool_card_input_projection(&ToolCall {
        id: "grep-call".into(),
        tool: "grep".into(),
        input_json: serde_json::json!({"pattern":"needle", "path":"/external/search"}).to_string(),
    });
    for status in [
        ToolCallStatus::Success,
        ToolCallStatus::Failed,
        ToolCallStatus::Rejected,
        ToolCallStatus::Cancelled,
    ] {
        let result = ToolResult {
            status,
            output: "result".into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: if status == ToolCallStatus::Success {
                Some(false)
            } else {
                None
            },
            invalid: None,
        };
        assert!(
            matches!(tool_card_result_projection(Some(&input), &result), ToolCardResultProjection::ReadOnly { status: projected, .. } if projected == status)
        );
        let invalid = ToolResult {
            exit_code: Some(0),
            ..result
        };
        assert_eq!(
            tool_card_result_projection(Some(&input), &invalid),
            ToolCardResultProjection::Corrupt
        );
    }
}
