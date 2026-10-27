use super::Fixture;
use crate::agent::events::{validate_runtime_approval_event, validate_runtime_terminal};
use crate::agent::pipeline::{approval_source_matches, validate_recovered_projection};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, DangerAudit, approval_audit_from_runtime,
    approval_audit_to_runtime,
};
use vega_runtime::{RuntimeToolResult, RuntimeToolStatus};
use vega_store::tool_calls;

#[test]
fn sandbox_readonly_audits_validate_live_bash_and_recovery_without_authorizing_other_tools() {
    let fixture = Fixture::new();
    let run = fixture.prepare(None).expect("accepted fixture run");
    for dangerous in [false, true] {
        let call = if dangerous {
            "danger-readonly"
        } else {
            "read-readonly"
        };
        let command = if dangerous {
            "rm -rf /"
        } else {
            "printf fixture"
        };
        let input = serde_json::json!({"command": command, "timeout": null}).to_string();
        let danger = vega_tools::danger::detect_danger(command)
            .expect("danger detector")
            .map(|danger| DangerAudit {
                rule_id: danger.rule_id.into(),
                decision: Approval::Deny,
                note: None,
            });
        assert_eq!(danger.is_some(), dangerous);
        let approval = ApprovalAudit {
            decision: if dangerous {
                Approval::Deny
            } else {
                Approval::Once
            },
            note: None,
            source: ApprovalSource::SandboxReadOnly,
            danger,
        };
        let encoded = approval.to_json().expect("strict readonly audit");
        assert_eq!(
            ApprovalAudit::from_json(&encoded).expect("decode"),
            approval
        );
        let runtime_approval = approval_audit_to_runtime(&approval);
        assert_eq!(approval_audit_from_runtime(&runtime_approval), approval);
        tool_calls::insert_pending(
            fixture.store.conn(),
            tool_calls::NewToolCall {
                id: call,
                thread_id: &fixture.thread,
                message_id: &run.assistant_message_id,
                seq: if dangerous { 2 } else { 1 },
                tool: "bash",
                input_json: &input,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .expect("pending call");
        let pending = tool_calls::find_state(fixture.store.conn(), call)
            .expect("state")
            .expect("pending");
        if dangerous {
            assert!(
                validate_runtime_approval_event(&pending, call, &runtime_approval, None).is_err()
            );
        } else {
            validate_runtime_approval_event(&pending, call, &runtime_approval, None)
                .expect("readonly approval");
            tool_calls::approve(fixture.store.conn(), call, &encoded, None, 2)
                .expect("persist approval");
            tool_calls::mark_running(fixture.store.conn(), call).expect("synthetic running record");
        }
        let state = tool_calls::find_state(fixture.store.conn(), call)
            .expect("state")
            .expect("retained");
        let result = RuntimeToolResult {
            call_id: call.into(),
            output: if dangerous {
                "Tool error: permission denied"
            } else {
                "fixture"
            }
            .into(),
            status: if dangerous {
                RuntimeToolStatus::Rejected
            } else {
                RuntimeToolStatus::Success
            },
            reused: false,
            exit_code: (!dangerous).then_some(0),
            duration_ms: (!dangerous).then_some(1),
            truncated: (!dangerous).then_some(false),
            approval: Some(runtime_approval),
            remember_rule: None,
        };
        validate_runtime_terminal("", &fixture.thread, call, &state, &result, &approval)
            .expect("real terminal validator");
        validate_recovered_projection(
            "",
            &fixture.thread,
            call,
            "bash",
            &input,
            &result.output,
            result.status,
            &approval,
            result.exit_code,
            result.duration_ms,
        )
        .expect("history recovery validator");
        for tool in [
            "read",
            "write",
            "edit",
            "glob",
            "grep",
            "mcp_fixture",
            "ComputerClick",
        ] {
            assert!(!approval_source_matches(
                tool,
                "{}",
                result.status,
                ApprovalSource::SandboxReadOnly,
                false
            ));
        }
        let mut forged = approval.clone();
        if let Some(danger) = &mut forged.danger {
            danger.rule_id = "wrong-danger-rule".into();
            assert!(forged.to_json().is_ok());
            assert!(
                validate_recovered_projection(
                    "",
                    &fixture.thread,
                    call,
                    "bash",
                    &input,
                    &result.output,
                    result.status,
                    &forged,
                    result.exit_code,
                    result.duration_ms
                )
                .is_err()
            );
            forged.danger.as_mut().expect("danger").decision = Approval::Always;
        } else {
            forged.decision = Approval::Always;
        }
        assert!(forged.to_json().is_err());
    }
}

#[test]
fn sandbox_readonly_codec_refuses_inconsistent_approvals() {
    let safe = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::SandboxReadOnly,
        danger: None,
    };
    let mut invalid = safe.clone();
    invalid.decision = Approval::Deny;
    assert!(invalid.to_json().is_err());
    invalid.decision = Approval::Always;
    assert!(invalid.to_json().is_err());
    invalid = safe;
    invalid.note = Some("synthetic note".into());
    assert!(invalid.to_json().is_err());
    for nested in [Approval::Once, Approval::Always] {
        let invalid = ApprovalAudit {
            decision: Approval::Deny,
            note: None,
            source: ApprovalSource::SandboxReadOnly,
            danger: Some(DangerAudit {
                rule_id: "synthetic-rule".into(),
                decision: nested,
                note: None,
            }),
        };
        assert!(invalid.to_json().is_err());
    }
    assert!(
        ApprovalAudit::from_json(
            r#"{"decision":"always","note":null,"source":"sandbox_readonly","danger":null}"#
        )
        .is_err()
    );
}
