use super::validate_recovered_projection;
use crate::agent::events::{validate_rejected_remember, validate_runtime_terminal};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, DangerAudit,
    approval_audit_to_runtime,
};
use vega_runtime::{
    RuntimeFailureReason, RuntimeMutatingTool, RuntimePermissionTarget, RuntimeToolResult,
    RuntimeToolStatus, VegaError,
};
use vega_store::tool_calls::ToolCallState;

const INVALID_READ_OUTPUT: &str = r#"Tool error: read requires path identifying a regular text file. Expected {"path":"src/lib.rs","offset":null,"limit":null}"#;
const PROJECT_ID: &str = "read-audit-project";
const THREAD_ID: &str = "read-audit-thread";
const CALL_ID: &str = "read-audit-call";
const READ_TOOLS: [&str; 2] = ["read", "Read"];

struct ReadProjection {
    tool: &'static str,
    input_json: &'static str,
    output: &'static str,
    status: RuntimeToolStatus,
    approval: ApprovalAudit,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
}

impl ReadProjection {
    fn validation_rejection(tool: &'static str) -> Self {
        Self {
            tool,
            input_json: "{}",
            output: INVALID_READ_OUTPUT,
            status: RuntimeToolStatus::Rejected,
            approval: validation_audit(),
            exit_code: None,
            duration_ms: None,
        }
    }

    fn validate(&self) -> Result<String, ConversationError> {
        validate_recovered_projection(
            PROJECT_ID,
            THREAD_ID,
            CALL_ID,
            self.tool,
            self.input_json,
            self.output,
            self.status,
            &self.approval,
            self.exit_code,
            self.duration_ms,
        )
    }
}

fn validation_audit() -> ApprovalAudit {
    ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Validation,
        danger: None,
    }
}

fn pending_validation_rejection(tool: &str) -> (ToolCallState, RuntimeToolResult, ApprovalAudit) {
    let approval = validation_audit();
    let state = ToolCallState {
        thread_id: THREAD_ID.to_string(),
        tool: tool.to_string(),
        input_json: "{}".to_string(),
        status: "pending_approval".to_string(),
        approval: None,
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    };
    let result = RuntimeToolResult {
        call_id: CALL_ID.to_string(),
        output: INVALID_READ_OUTPUT.to_string(),
        status: RuntimeToolStatus::Rejected,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: Some(approval_audit_to_runtime(&approval)),
        remember_rule: None,
    };
    (state, result, approval)
}

#[test]
fn invalid_read_validation_projection_accepts_only_the_safe_rejection() {
    for tool in READ_TOOLS {
        let projection = ReadProjection::validation_rejection(tool);
        assert_eq!(
            projection
                .validate()
                .expect("safe read validation rejection"),
            "{}",
            "{tool}"
        );
    }
}

type ProjectionMutation = fn(&mut ReadProjection);

#[test]
fn invalid_read_validation_projection_rejects_single_invariant_forgeries() {
    let cases: &[(&str, ProjectionMutation)] = &[
        ("nonempty input", |projection| {
            projection.input_json = r#"{"path":"src/lib.rs","offset":null,"limit":null}"#;
        }),
        ("failed status", |projection| {
            projection.status = RuntimeToolStatus::Failed;
        }),
        ("success status", |projection| {
            projection.status = RuntimeToolStatus::Success;
        }),
        ("cancelled status", |projection| {
            projection.status = RuntimeToolStatus::Cancelled;
        }),
        ("wrong output", |projection| {
            projection.output = "Tool error: permission denied";
        }),
        ("once decision", |projection| {
            projection.approval.decision = Approval::Once;
        }),
        ("always decision", |projection| {
            projection.approval.decision = Approval::Always;
        }),
        ("readonly tool source", |projection| {
            projection.approval.source = ApprovalSource::ReadonlyTool;
        }),
        ("recovery source", |projection| {
            projection.approval.source = ApprovalSource::Recovery;
        }),
        ("timeout source", |projection| {
            projection.approval.source = ApprovalSource::Timeout;
        }),
        ("user source", |projection| {
            projection.approval.source = ApprovalSource::User;
        }),
        ("full access source", |projection| {
            projection.approval.source = ApprovalSource::FullAccess;
        }),
        ("note present", |projection| {
            projection.approval.note = Some(String::new());
        }),
        ("danger present", |projection| {
            projection.approval.danger = Some(DangerAudit {
                rule_id: "read-audit-danger".to_string(),
                decision: Approval::Deny,
                note: None,
            });
        }),
        ("exit code present", |projection| {
            projection.exit_code = Some(0);
        }),
        ("duration present", |projection| {
            projection.duration_ms = Some(0);
        }),
    ];
    for tool in READ_TOOLS {
        for (name, mutate) in cases {
            let mut projection = ReadProjection::validation_rejection(tool);
            mutate(&mut projection);
            assert!(projection.validate().is_err(), "{tool}: {name}");
        }
    }
}

#[test]
fn read_cancellation_and_recovery_cannot_claim_validation_output() {
    let cases = [
        (
            RuntimeToolStatus::Rejected,
            Approval::Deny,
            ApprovalSource::Timeout,
            "Tool error: permission denied",
        ),
        (
            RuntimeToolStatus::Rejected,
            Approval::Deny,
            ApprovalSource::Recovery,
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
        ),
        (
            RuntimeToolStatus::Cancelled,
            Approval::Once,
            ApprovalSource::ReadonlyTool,
            vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
        ),
        (
            RuntimeToolStatus::Cancelled,
            Approval::Once,
            ApprovalSource::ReadonlyTool,
            vega_store::recovery::RECOVERY_CANCELLED_OUTPUT,
        ),
    ];
    for tool in READ_TOOLS {
        for (status, decision, source, output) in cases {
            let mut projection = ReadProjection::validation_rejection(tool);
            projection.status = status;
            projection.approval.decision = decision;
            projection.approval.source = source;
            projection.output = output;
            assert_eq!(
                projection
                    .validate()
                    .expect("safe cancellation or recovery"),
                "{}",
                "{tool}: {status:?}/{source:?}"
            );
            projection.output = INVALID_READ_OUTPUT;
            assert!(
                projection.validate().is_err(),
                "{tool}: {status:?}/{source:?} claimed read validation output"
            );
        }
    }
}

#[test]
fn other_tools_cannot_use_the_read_validation_projection() {
    for tool in [
        "glob",
        "grep",
        "bash",
        "write",
        "edit",
        "load_skill",
        "READ",
    ] {
        let mut projection = ReadProjection::validation_rejection("read");
        projection.tool = tool;
        assert!(projection.validate().is_err(), "{tool}");
    }
}

#[test]
fn read_validation_terminal_requires_pending_to_rejected() {
    for tool in READ_TOOLS {
        let (state, result, approval) = pending_validation_rejection(tool);
        validate_runtime_terminal(PROJECT_ID, THREAD_ID, CALL_ID, &state, &result, &approval)
            .expect("pending read can be rejected without approval or running");
    }
}

#[test]
fn read_validation_terminal_rejects_illegal_single_transition_changes() {
    for tool in READ_TOOLS {
        let (state, result, approval) = pending_validation_rejection(tool);
        for status in [
            "approved",
            "running",
            "rejected",
            "success",
            "failed",
            "cancelled",
        ] {
            let mut changed_state = state.clone();
            changed_state.status = status.to_string();
            let error = validate_runtime_terminal(
                PROJECT_ID,
                THREAD_ID,
                CALL_ID,
                &changed_state,
                &result,
                &approval,
            )
            .expect_err("read validation rejection must start from pending approval");
            assert!(
                matches!(error, VegaError::RuntimeFailure(detail) if detail.reason == RuntimeFailureReason::State),
                "{tool}: {status} to rejected"
            );
        }
        for status in [
            RuntimeToolStatus::Failed,
            RuntimeToolStatus::Success,
            RuntimeToolStatus::Cancelled,
        ] {
            let mut changed_result = result.clone();
            changed_result.status = status;
            let error = validate_runtime_terminal(
                PROJECT_ID,
                THREAD_ID,
                CALL_ID,
                &state,
                &changed_result,
                &approval,
            )
            .expect_err("a non-running read cannot finish as an executed tool");
            assert!(
                matches!(error, VegaError::RuntimeFailure(detail) if detail.reason == RuntimeFailureReason::State),
                "{tool}: pending approval to {status:?}"
            );
        }
    }
}

#[test]
fn read_validation_terminal_has_no_truncation_or_remembered_rule() {
    for tool in READ_TOOLS {
        let (state, result, approval) = pending_validation_rejection(tool);
        for truncated in [false, true] {
            let mut changed_result = result.clone();
            changed_result.truncated = Some(truncated);
            let error = validate_runtime_terminal(
                PROJECT_ID,
                THREAD_ID,
                CALL_ID,
                &state,
                &changed_result,
                &approval,
            )
            .expect_err("unexecuted read must not report truncation metadata");
            assert!(
                matches!(error, VegaError::RuntimeFailure(detail) if detail.reason == RuntimeFailureReason::ApprovalAudit),
                "{tool}: truncated={truncated}"
            );
        }
        validate_rejected_remember(&state, &result, &approval)
            .expect("validation rejection without a remembered rule");
        let mut changed_result = result.clone();
        changed_result.remember_rule = Some(RuntimePermissionTarget {
            call_id: CALL_ID.to_string(),
            tool: RuntimeMutatingTool::Read,
            exact_pattern: "src/lib.rs".to_string(),
            display_target: "src/lib.rs".to_string(),
        });
        let error = validate_rejected_remember(&state, &changed_result, &approval)
            .expect_err("validation rejection must not insert a permission rule");
        assert!(
            matches!(error, VegaError::RuntimeFailure(detail) if detail.reason == RuntimeFailureReason::ApprovalAudit),
            "{tool}: remembered rule"
        );
    }
}
