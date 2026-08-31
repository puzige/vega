use std::sync::Arc;

use super::{
    ConversationError, ConversationEvent, Microcents, ThreadMode, ThreadStatus, TokenUsage,
    from_runtime_event,
};

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
