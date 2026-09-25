use std::sync::Arc;

use super::{
    ContextAccountingSource, ContextAccountingStage, ContextCompactionStatus,
    ContextCompactionStatusRecord, ContextCompactionUsageState, ConversationError,
    ConversationEvent, ConversationMeter, Microcents, ThreadMode, ThreadStatus, TokenUsage,
    UsagePricing, from_runtime_event,
};

#[test]
fn loaded_skill_audit_projects_only_safe_identity() {
    let audit = vega_runtime::skills::ActivationAudit {
        name: "reviewer".into(),
        source_label: Some("project:reviewed".into()),
        source_scope: Some(vega_runtime::skills::SourceScope::Project),
        content_sha256: Some("a".repeat(64)),
        origin: vega_runtime::skills::ActivationOrigin::ExplicitUser,
        status: "loaded",
    };
    let event = vega_runtime::RuntimeEvent::SkillActivation {
        audit: audit.clone(),
        binding: None,
        snapshot: None,
    };
    let Some(ConversationEvent::SkillActivated { message_id, skill }) =
        from_runtime_event("assistant-1", &event)
    else {
        panic!("loaded audit should become a safe active indicator");
    };
    assert_eq!(message_id, "assistant-1");
    assert_eq!(skill.name, "reviewer");
    assert_eq!(skill.source_label, "project:reviewed");
    assert_eq!(skill.content_sha256, "a".repeat(64));
    let failed = vega_runtime::RuntimeEvent::SkillActivation {
        audit: vega_runtime::skills::ActivationAudit {
            status: "unavailable",
            ..audit
        },
        binding: None,
        snapshot: None,
    };
    assert!(from_runtime_event("assistant-1", &failed).is_none());
}

#[test]
fn issue90_bash_validation_event_requires_exact_safe_audit_and_terminal_facts() {
    use super::{InvalidToolCode, InvalidToolKind};
    use vega_runtime::{
        RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeToolCall,
        RuntimeToolResult, RuntimeToolStatus,
    };

    let call = RuntimeToolCall {
        id: "bad-bash".into(),
        name: "bash".into(),
        input_json: vega_runtime::InvalidBashAudit::from_raw(
            r#"{"command":"SECRET_BASH_COMMAND"}"#,
        )
        .unwrap()
        .to_json()
        .unwrap(),
    };
    let result = RuntimeToolResult {
        call_id: call.id.clone(),
        output: vega_runtime::BASH_INVALID_INPUT_OUTPUT.into(),
        status: RuntimeToolStatus::Rejected,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: Some(RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note: None,
            source: RuntimeApprovalSource::Validation,
            danger: None,
        }),
        remember_rule: None,
    };
    let event = |call: RuntimeToolCall, result: RuntimeToolResult| {
        vega_runtime::RuntimeEvent::ToolCallValidationRejected { call, result }
    };
    assert!(matches!(
        from_runtime_event("assistant", &event(call.clone(), result.clone())),
        Some(ConversationEvent::ToolCallFinished { result, .. })
            if result.invalid.is_some_and(|invalid|
                invalid.tool() == InvalidToolKind::Bash
                    && invalid.code() == InvalidToolCode::InvalidInput)
    ));

    let mut wrong_id = result.clone();
    wrong_id.call_id = "different".into();
    let mut wrong_output = result.clone();
    wrong_output.output = "SECRET_FORGED_RESULT".into();
    let mut wrong_status = result.clone();
    wrong_status.status = RuntimeToolStatus::Failed;
    let mut wrong_metadata = result.clone();
    wrong_metadata.duration_ms = Some(1);
    let mut wrong_approval = result.clone();
    wrong_approval.approval.as_mut().unwrap().source = RuntimeApprovalSource::User;
    for forged in [
        wrong_id,
        wrong_output,
        wrong_status,
        wrong_metadata,
        wrong_approval,
    ] {
        assert!(from_runtime_event("assistant", &event(call.clone(), forged)).is_none());
    }
    assert!(
        from_runtime_event(
            "assistant",
            &event(
                RuntimeToolCall {
                    input_json: r#"{"command":"SECRET_BASH_COMMAND"}"#.into(),
                    ..call
                },
                result
            )
        )
        .is_none()
    );
}

#[test]
fn conversation_runtime_error_debug_and_display_redact_provider_payload() {
    const SENTINEL: &str = "VEGA_CONVERSATION_PROVIDER_SENTINEL";
    let error = ConversationError::Runtime(Arc::new(vega_runtime::VegaError::Provider {
        status: Some(503),
        message: SENTINEL.into(),
        retryable: true,
    }));
    assert!(!format!("{error:?}").contains(SENTINEL));
    assert!(!error.to_string().contains(SENTINEL));
    let ConversationError::Runtime(error) = error else {
        unreachable!()
    };
    assert!(matches!(
        error.as_ref(),
        vega_runtime::VegaError::Provider {
            status: Some(503),
            message,
            retryable: true,
        } if message == SENTINEL
    ));
}

#[test]
fn thread_mode_round_trips_the_ddl_vocabulary() {
    for (value, mode) in [
        ("ask", ThreadMode::Ask),
        ("plan", ThreadMode::Plan),
        ("execute", ThreadMode::Execute),
    ] {
        assert_eq!(ThreadMode::parse(value), Some(mode));
        assert_eq!(mode.as_str(), value);
    }
}

#[test]
fn thread_mode_rejects_unknown_strings() {
    assert_eq!(ThreadMode::parse("Ask"), None);
    assert_eq!(ThreadMode::parse(""), None);
    assert_eq!(ThreadMode::parse("yolo"), None);
}

#[test]
fn thread_status_round_trips_the_ddl_vocabulary() {
    for (value, status) in [
        ("active", ThreadStatus::Active),
        ("archived", ThreadStatus::Archived),
    ] {
        assert_eq!(ThreadStatus::parse(value), Some(status));
        assert_eq!(status.as_str(), value);
    }
    assert_eq!(ThreadStatus::parse("done"), None);
}

#[test]
fn converts_text_thinking_and_usage_runtime_events() {
    let message_id = "message-1";
    assert!(matches!(
        from_runtime_event(message_id, &vega_runtime::RuntimeEvent::TextDelta("hello".into())),
        Some(ConversationEvent::TextDelta { message_id, delta })
            if message_id == "message-1" && delta == "hello"
    ));
    assert!(matches!(
        from_runtime_event(message_id, &vega_runtime::RuntimeEvent::ThinkingDelta("why".into())),
        Some(ConversationEvent::ThinkingDelta { message_id, delta })
            if message_id == "message-1" && delta == "why"
    ));
    let usage = vega_runtime::RuntimeTokenUsage {
        input: 10,
        output: 4,
        cache_read: 3,
        cache_write: 2,
    };
    assert!(matches!(
        from_runtime_event(
            message_id,
            &vega_runtime::RuntimeEvent::UsageUpdated {
                usage,
                cost_microcents: 0,
                pricing: None
            }
        ),
        Some(ConversationEvent::UsageUpdated {
            usage: TokenUsage {
                input: 10,
                output: 4,
                cache_read: 3,
                cache_write: 2
            },
            cost: Microcents(0),
            ..
        })
    ));
}

#[test]
fn issue70_runtime_tool_running_reaches_the_safe_ui_event_boundary() {
    let projected = from_runtime_event(
        "assistant-running",
        &vega_runtime::RuntimeEvent::ToolCallRunning {
            call_id: "bash-running".into(),
        },
    );

    assert!(matches!(
        projected,
        Some(ConversationEvent::ToolCallRunning { call_id }) if call_id == "bash-running"
    ));
}

#[test]
fn issue73_mcp_safe_proposal_crosses_live_event_boundary_without_raw_arguments() {
    use super::{McpCallIdentity, ToolCardInputProjection, tool_card_input_projection};

    let identity = McpCallIdentity {
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 3,
        exact_tool_name: "echo".into(),
        arguments_bytes: 19,
        arguments_sha256: "a".repeat(64),
        argument_preview: "echo: string".into(),
    };
    let safe_input = serde_json::json!({
        "server_id": identity.server_id,
        "config_revision": identity.config_revision,
        "tool": identity.exact_tool_name,
        "arguments_bytes": identity.arguments_bytes,
        "arguments_sha256": identity.arguments_sha256,
        "argument_preview": identity.argument_preview,
    })
    .to_string();
    let alias = identity.alias();
    let call = vega_runtime::RuntimeToolCall {
        id: "mcp-live-call".into(),
        name: alias.clone(),
        input_json: safe_input.clone(),
    };
    let Some(ConversationEvent::ToolCallProposed { call: projected }) = from_runtime_event(
        "assistant",
        &vega_runtime::RuntimeEvent::ToolCallProposed(call.clone()),
    ) else {
        panic!("validated MCP proposal must reach the live permission card");
    };
    assert_eq!(projected.id, call.id);
    assert_eq!(projected.tool, alias);
    assert_eq!(projected.input_json, safe_input);
    assert!(matches!(
        tool_card_input_projection(&projected),
        ToolCardInputProjection::Mcp { .. }
    ));

    let mut with_raw_value: serde_json::Value =
        serde_json::from_str(&safe_input).expect("safe fixture JSON");
    with_raw_value["raw_value"] = serde_json::json!("PRIVATE_VALUE");

    for invalid in [
        vega_runtime::RuntimeToolCall {
            input_json: r#"{"echo":"PRIVATE_VALUE"}"#.into(),
            ..call.clone()
        },
        vega_runtime::RuntimeToolCall {
            name: "mcp_01ARZ3NDEKTSV4RRFFQ69G5FAV_wrong".into(),
            ..call.clone()
        },
        vega_runtime::RuntimeToolCall {
            input_json: with_raw_value.to_string(),
            ..call.clone()
        },
    ] {
        assert!(
            from_runtime_event(
                "assistant",
                &vega_runtime::RuntimeEvent::ToolCallProposed(invalid)
            )
            .is_none(),
            "raw, extra-field or mismatched MCP proposals must stay closed"
        );
    }
}

#[test]
fn converts_content_free_compaction_status_and_preserves_unknown_usage() {
    let event = vega_runtime::RuntimeEvent::ContextCompactionStatusUpdated {
        status: vega_runtime::ContextCompactionStatusUpdate {
            operation_key: "source-fingerprint".into(),
            generation: 7,
            phase: vega_runtime::ContextCompactionPhase::Succeeded,
            source_version: 11,
            estimated_tokens: 8_000,
            input_budget: 8_000,
            target_tokens: 6_000,
            usage: vega_runtime::ContextCompactionUsageState::Unknown,
            failure: None,
        },
    };
    let Some(ConversationEvent::ContextCompactionStatus { record }) =
        from_runtime_event("assistant-message", &event)
    else {
        panic!("status event must cross the conversation boundary");
    };
    assert_eq!(record.generation, 7);
    assert_eq!(record.status, ContextCompactionStatus::Succeeded);
    assert_eq!(record.source_version, Some(11));
    assert_eq!(record.usage, ContextCompactionUsageState::Unknown);
}

#[test]
fn issue91_runtime_accounting_crosses_facade_without_changing_usage() {
    let runtime = vega_runtime::RuntimeEvent::ContextAccountingUpdated(
        vega_runtime::ContextAccountingDecision {
            source: vega_runtime::ContextAccountingSource::UsageAnchored,
            stage: vega_runtime::ContextAccountingStage::PrimaryPreflight,
            provider_input_baseline: Some(85_032),
            incremental_estimate: 1_000,
            predicted_input: 86_032,
            input_budget: 300_000,
            trigger_tokens: 240_000,
            target_tokens: 180_000,
            revision: 2,
            covered_messages: 12,
        },
    );
    let Some(ConversationEvent::ContextAccounting { message_id, record }) =
        from_runtime_event("assistant-91", &runtime)
    else {
        panic!("accounting event missing");
    };
    assert_eq!(message_id, "assistant-91");
    assert_eq!(record.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(record.stage, ContextAccountingStage::PrimaryPreflight);
    assert_eq!(record.provider_input_baseline, Some(85_032));
    assert_eq!(record.incremental_estimate, 1_000);
    assert_eq!(record.predicted_input, 86_032);
    assert_eq!(record.revision, 2);
    assert_eq!(record.covered_messages, 12);
}

#[test]
fn unknown_summary_usage_keeps_later_primary_cost_unknown() {
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::ContextCompactionStatus {
        record: ContextCompactionStatusRecord {
            generation: 1,
            status: ContextCompactionStatus::Succeeded,
            updated_at: 0,
            estimated_tokens: Some(8_000),
            input_budget: Some(8_000),
            target_tokens: Some(6_000),
            source_version: Some(2),
            failure: None,
            usage: ContextCompactionUsageState::Unknown,
        },
    });
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "assistant".into(),
        seq: 1,
    });
    meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "assistant".into(),
        usage: TokenUsage {
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
        cost: Microcents(12),
        pricing: Some(UsagePricing {
            version: "test".into(),
            profile: "base".into(),
            call_started_at: 1,
        }),
    });
    assert_eq!(meter.snapshot().cost, None);
}

#[test]
fn converts_errors_without_losing_structured_fields() {
    let provider = vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Provider {
        status: Some(429),
        message: "rate limited".into(),
        retryable: true,
    }));
    assert!(matches!(
        from_runtime_event("message-1", &provider),
        Some(ConversationEvent::Error { error, .. })
            if matches!(
                error.as_ref(),
                vega_runtime::VegaError::Provider {
                    status: Some(429),
                    message,
                    retryable: true,
                } if message == "rate limited"
            )
    ));

    let tool = vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Tool {
        tool: "read".into(),
        message: "collision".into(),
    }));
    assert!(matches!(
        from_runtime_event("message-1", &tool),
        Some(ConversationEvent::Error { error, .. })
            if matches!(
                error.as_ref(),
                vega_runtime::VegaError::Tool { tool, message }
                    if tool == "read" && message == "collision"
            )
    ));

    let cancelled = vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Cancelled));
    assert!(matches!(
        from_runtime_event("message-1", &cancelled),
        Some(ConversationEvent::Error { error, .. })
            if matches!(error.as_ref(), vega_runtime::VegaError::Cancelled)
    ));
}

#[test]
fn issue114_finish_reasons_map_end_length_and_turn_limit() {
    use super::ConversationStopReason;

    for (runtime, expected) in [
        (
            vega_runtime::RuntimeFinishReason::End,
            ConversationStopReason::End,
        ),
        (
            vega_runtime::RuntimeFinishReason::Length,
            ConversationStopReason::Length,
        ),
        (
            vega_runtime::RuntimeFinishReason::TurnLimit,
            ConversationStopReason::TurnLimit,
        ),
    ] {
        let event = vega_runtime::RuntimeEvent::Finished(runtime);
        assert!(matches!(
            from_runtime_event("assistant-1", &event),
            Some(ConversationEvent::MessageFinished {
                message_id,
                stop_reason,
                ..
            })
                if message_id == "assistant-1" && stop_reason == expected
        ));
    }
}
