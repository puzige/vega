use super::Fixture;
use crate::agent::events::{validate_reused_terminal, validate_runtime_terminal};
use crate::agent::pipeline::{is_safe_bash_failure, validate_recovered_projection};
use crate::types::{ApprovalAudit, approval_audit_to_runtime};
use vega_runtime::{RuntimeToolResult, RuntimeToolStatus};
use vega_store::tool_calls;
use vega_tools::BashErrorCode;

#[test]
fn incomplete_bash_output_survives_terminal_validation_storage_and_recovery() {
    let fixture = Fixture::new();
    let run = fixture.prepare(None).expect("real accepted run");
    let state = fixture.running_bash(&run.assistant_message_id);
    let approval = ApprovalAudit::from_json(state.approval.as_deref().expect("approval"))
        .expect("valid approval");
    let output = BashErrorCode::OutputIncomplete.tool_output();
    assert_eq!(
        output,
        "Tool error: bash failed (output_incomplete). The shell exited, but its output did not close. Background work may still be running. Verify it before retrying."
    );
    let mut result = RuntimeToolResult {
        call_id: "unsettled".into(),
        output,
        status: RuntimeToolStatus::Failed,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: Some(approval_audit_to_runtime(&approval)),
        remember_rule: None,
    };
    validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
        .expect("new failure must be accepted by the live persistence validator");
    assert_eq!(
        validate_recovered_projection(
            "",
            &fixture.thread,
            "unsettled",
            "bash",
            &state.input_json,
            &result.output,
            result.status,
            &approval,
            result.exit_code,
            result.duration_ms,
        )
        .expect("recoverable exact failure"),
        state.input_json
    );

    let original = result.output.clone();
    for forged in [
        format!("{original} synthetic-secret"),
        "Tool error: bash failed (output_incomplete)".into(),
        "Tool error: bash failed (future_error)".into(),
    ] {
        result.output = forged;
        assert!(
            validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
                .is_err()
        );
    }
    result.output = original;
    result.status = RuntimeToolStatus::Cancelled;
    assert!(
        validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
            .is_err()
    );
    result.status = RuntimeToolStatus::Success;
    assert!(
        validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
            .is_err()
    );
    result.status = RuntimeToolStatus::Failed;
    result.exit_code = Some(0);
    assert!(
        validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
            .is_err()
    );
    result.exit_code = None;
    result.duration_ms = Some(0);
    assert!(
        validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
            .is_err()
    );
    result.duration_ms = None;
    result.truncated = Some(false);
    assert!(
        validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
            .is_err()
    );
    result.truncated = None;
    validate_runtime_terminal("", &fixture.thread, "unsettled", &state, &result, &approval)
        .expect("valid failure remains accepted");
    tool_calls::finish(
        fixture.store.conn(),
        tool_calls::FinishToolCall {
            id: "unsettled",
            status: "failed",
            output_text: &result.output,
            exit_code: None,
            duration_ms: None,
            finished_at: 3,
        },
    )
    .expect("persist exact failure");
    let stored = tool_calls::find_state(fixture.store.conn(), "unsettled")
        .expect("read terminal")
        .expect("retained terminal");
    assert_eq!(stored.status, "failed");
    assert_eq!(stored.output_text.as_deref(), Some(result.output.as_str()));
    assert_eq!(stored.approval, state.approval);
    result.reused = true;
    validate_reused_terminal("", &fixture.thread, &stored, &result)
        .expect("failed result remains recoverable without rerunning Bash");
}

#[test]
fn bash_failure_vocabulary_remains_exact_and_preserves_existing_results() {
    for (code, wire) in [
        (BashErrorCode::ScopeMismatch, "scope_mismatch"),
        (BashErrorCode::HardlinkPreflight, "hardlink_preflight"),
        (BashErrorCode::SandboxUnavailable, "sandbox_unavailable"),
        (BashErrorCode::TempUnavailable, "temp_unavailable"),
        (BashErrorCode::CleanupFailed, "cleanup_failed"),
        (BashErrorCode::SpawnFailed, "spawn_failed"),
        (
            BashErrorCode::ProcessControlFailed,
            "process_control_failed",
        ),
        (BashErrorCode::OutputFailed, "output_failed"),
        (BashErrorCode::TimedOut, "timed_out"),
    ] {
        let expected = format!("Tool error: bash failed ({wire})");
        assert_eq!(code.tool_output(), expected);
        assert!(is_safe_bash_failure(&expected));
        assert!(!is_safe_bash_failure(&format!("{expected}\nextra data")));
    }
    let denied = BashErrorCode::ScopeDenied.tool_output();
    assert!(denied.contains("not started"));
    assert!(denied.contains("new call"));
    assert!(is_safe_bash_failure(&denied));
    assert!(!is_safe_bash_failure(&format!("{denied}\nextra data")));
    for code in [BashErrorCode::InvalidInput, BashErrorCode::Cancelled] {
        assert!(!is_safe_bash_failure(&code.tool_output()));
    }
    assert!(!is_safe_bash_failure(
        "Tool error: bash failed (credential_protection_required)"
    ));
}
