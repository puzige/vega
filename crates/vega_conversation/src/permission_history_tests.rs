use super::*;

fn row(status: &str, source: ApprovalSource, output: &str) -> PageToolCall {
    PageToolCall {
        id: "call".into(),
        message_id: "message".into(),
        seq: 1,
        text_offset_bytes: None,
        codex_display: None,
        tool: "bash".into(),
        input_json: r#"{"cmd":"printf hello"}"#.into(),
        output_text: Some(output.into()),
        status: status.into(),
        approval: Some(
            ApprovalAudit {
                decision: if source == ApprovalSource::FullAccess {
                    Approval::Once
                } else {
                    Approval::Deny
                },
                note: None,
                source,
                danger: None,
            }
            .to_json()
            .unwrap(),
        ),
        exit_code: (status == "success").then_some(0),
        duration_ms: (status == "success").then_some(1),
    }
}

fn corrupt(call: &PageToolCall) -> bool {
    matches!(
        tool_entry(call),
        HistoryEntry::Tool {
            result: Some(ToolCardResultProjection::Corrupt),
            ..
        }
    )
}

#[test]
fn successful_user_output_may_equal_permission_feedback() {
    for output in [
        vega_runtime::PERMISSION_CANCELLED_OUTPUT,
        vega_runtime::PERMISSION_UNAVAILABLE_OUTPUT,
        vega_runtime::PERMISSION_TIMEOUT_OUTPUT,
        vega_runtime::PERMISSION_DENIED_OUTPUT,
        vega_runtime::DANGER_DENIED_OUTPUT,
    ] {
        assert!(!corrupt(&row(
            "success",
            ApprovalSource::FullAccess,
            output
        )));
    }
}

#[test]
fn new_failure_sources_cannot_authorize_success_or_mismatched_rejection() {
    for (source, output) in [
        (
            ApprovalSource::Cancelled,
            vega_runtime::PERMISSION_CANCELLED_OUTPUT,
        ),
        (
            ApprovalSource::Unavailable,
            vega_runtime::PERMISSION_UNAVAILABLE_OUTPUT,
        ),
    ] {
        assert!(!corrupt(&row("rejected", source, output)));
        assert!(corrupt(&row("success", source, output)));
        assert!(corrupt(&row(
            "rejected",
            source,
            vega_runtime::PERMISSION_TIMEOUT_OUTPUT
        )));
        let mut metadata = row("rejected", source, output);
        metadata.duration_ms = Some(1);
        assert!(corrupt(&metadata));
    }
    assert!(corrupt(&row(
        "rejected",
        ApprovalSource::Timeout,
        vega_runtime::PERMISSION_CANCELLED_OUTPUT
    )));
    assert!(!corrupt(&row(
        "rejected",
        ApprovalSource::Timeout,
        "Tool error: permission denied"
    )));
}
