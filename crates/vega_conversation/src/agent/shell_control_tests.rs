use super::*;
use serde_json::json;
use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeToolCall,
    RuntimeToolResult, RuntimeToolStatus,
};

fn call(tool: &str, input: serde_json::Value) -> RuntimeToolCall {
    RuntimeToolCall {
        id: ulid::Ulid::generate().to_string(),
        name: tool.into(),
        input_json: input.to_string(),
        description: None,
    }
}

#[test]
fn controls_accept_only_their_parameter_contract() {
    let handle = ulid::Ulid::generate().to_string();
    for input in [
        json!({"handle":handle}),
        json!({"handle":handle,"cursor":null,"wait_ms":null}),
        json!({"handle":handle,"cursor":u64::MAX,"wait_ms":300000}),
        json!({"handle":handle,"cursor":0,"wait_ms":0}),
    ] {
        assert!(events::validate_runtime_proposal(&call("bash_wait", input)).is_ok());
    }
    assert!(
        events::validate_runtime_proposal(&call("bash_cancel", json!({"handle":handle}))).is_ok()
    );
    for (tool, input) in [
        ("bash_wait", json!({"handle":"bad"})),
        ("bash_wait", json!({"handle":handle,"wait_ms":300001})),
        ("bash_wait", json!({"handle":handle,"cursor":-1})),
        ("bash_wait", json!({"handle":handle,"extra":true})),
        ("bash_cancel", json!({"handle":handle,"cursor":null})),
        ("bash_cancel", json!({"handle":handle,"wait_ms":0})),
        ("unknown_tool", json!({"handle":handle})),
    ] {
        assert!(
            events::validate_runtime_proposal(&call(tool, input)).is_err(),
            "{tool}"
        );
    }
    for tool in ["bash_wait", "bash_cancel"] {
        assert!(events::validate_runtime_proposal(&call(tool, json!({}))).is_ok());
        assert!(!shell_operations::valid_control_input(tool, "{}"));
        assert!(!shell_operations::valid_control_input(tool, "null"));
        let oversized = format!("{}{}", json!({"handle":handle}), " ".repeat(2048));
        assert!(!shell_operations::valid_control_input(tool, &oversized));
    }
    assert!(!shell_operations::valid_control_input(
        "unknown_tool",
        &json!({"handle":handle}).to_string()
    ));
}

fn receipt(state: &str, safe: bool, exit: Option<i32>) -> serde_json::Value {
    json!({"handle":ulid::Ulid::generate().to_string(),"operation_id":ulid::Ulid::generate().to_string(),"state":state,"exit_code":exit,"output":"","next_cursor":0,"retained_from":0,"output_gap":false,"truncated":false,"duration_ms":10,"error":if state=="outcome_unknown" {Some("group_continuity_lost")} else {None},"cleanup_safe":safe,"command_result":if state=="running" {None} else {Some(if state=="succeeded" {"exited"} else {state})},"exit_signal":null,"output_incomplete":!safe,"resources_closed":safe,"process_resolved":safe,"cleanup_reasons":if safe {vec![]} else {vec!["group_continuity_lost"]}})
}

fn lifecycle(tool: &str, state: &str, status: RuntimeToolStatus, safe: bool, exit: Option<i32>) {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        "/tmp/shell-control-regression",
        "regression",
        None,
    )
    .unwrap();
    let thread_id = ulid::Ulid::generate().to_string();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            reasoning_preferences: None,
            id: &thread_id,
            project_id: &project.id,
            title: "control regression",
            mode: "execute",
            permission_mode: "confirm",
            permission_revision: 0,
            full_access_grant: None,
            model: "test-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    let thread = vega_store::threads::find(store.conn(), &thread_id)
        .unwrap()
        .unwrap();
    let message = ulid::Ulid::generate().to_string();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: message.clone(),
            thread_id: thread.id.clone(),
            seq: 0,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let proposed = call(tool, json!({"handle":ulid::Ulid::generate().to_string()}));
    let mut seq = 0;
    let mut persist = |event| {
        let tx = vega_store::write_coordination::WriteTransaction::new(
            store.conn(),
            vega_store::write_coordination::WriteCategory::RuntimeEvent,
        )
        .unwrap();
        let result = events::persist_runtime_event(
            &store,
            &tx,
            &project.id,
            &thread.id,
            &message,
            "test-model",
            false,
            "",
            &mut seq,
            &event,
            None,
            None,
        );
        if result.is_ok() {
            tx.commit().unwrap();
        }
        result
    };
    persist(RuntimeEvent::ToolCallProposed(proposed.clone())).unwrap();
    let audit = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::ReadonlyTool,
        danger: None,
    };
    persist(RuntimeEvent::ToolCallApproved {
        call_id: proposed.id.clone(),
        audit: audit.clone(),
        remember_rule: None,
    })
    .unwrap();
    persist(RuntimeEvent::ToolCallRunning {
        call_id: proposed.id.clone(),
        bash_started_at: None,
    })
    .unwrap();
    let output = receipt(state, safe, exit).to_string();
    persist(RuntimeEvent::ToolCallFinished(RuntimeToolResult {
        call_id: proposed.id.clone(),
        output: output.clone(),
        status,
        reused: false,
        exit_code: exit,
        duration_ms: Some(10),
        truncated: Some(false),
        approval: Some(audit),
        remember_rule: None,
        credential_failure: None,
    }))
    .unwrap();
    let saved = tool_calls::find_state(store.conn(), &proposed.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.output_text.as_deref(), Some(output.as_str()));
    let recovered =
        terminal_history::recover_terminal_tools(store.conn(), &project.id, &thread.id).unwrap();
    assert!(recovered.contains_key(&proposed.id));
    assert_eq!(
        saved.status,
        match status {
            RuntimeToolStatus::Success => "success",
            RuntimeToolStatus::Cancelled => "cancelled",
            RuntimeToolStatus::Failed => "failed",
            RuntimeToolStatus::Rejected => "rejected",
        }
    );
}

#[test]
fn wait_running_receipt_is_persisted_without_claiming_completion() {
    lifecycle(
        "bash_wait",
        "running",
        RuntimeToolStatus::Success,
        false,
        None,
    );
}
#[test]
fn wait_completed_receipt_preserves_exit_status() {
    lifecycle(
        "bash_wait",
        "succeeded",
        RuntimeToolStatus::Success,
        true,
        Some(0),
    );
}
#[test]
fn cancel_receipt_preserves_cancelled_terminal() {
    lifecycle(
        "bash_cancel",
        "cancelled",
        RuntimeToolStatus::Cancelled,
        true,
        None,
    );
}
#[test]
fn cancel_unknown_receipt_remains_failed_and_unsafe() {
    lifecycle(
        "bash_cancel",
        "outcome_unknown",
        RuntimeToolStatus::Failed,
        false,
        None,
    );
}
#[test]
fn unknown_cleanup_cannot_be_promoted_to_success() {
    let output = receipt("outcome_unknown", false, None).to_string();
    assert!(shell_operations::valid_receipt(
        &output,
        RuntimeToolStatus::Failed,
        None,
        Some(10)
    ));
    assert!(!shell_operations::valid_receipt(
        &output,
        RuntimeToolStatus::Success,
        None,
        Some(10)
    ));
    let mut forged = receipt("outcome_unknown", false, None);
    forged["cleanup_safe"] = json!(true);
    assert!(!shell_operations::valid_receipt(
        &forged.to_string(),
        RuntimeToolStatus::Failed,
        None,
        Some(10)
    ));
}

#[test]
fn control_approval_rejects_wrong_source_and_invalid_projection() {
    let input = json!({"handle":ulid::Ulid::generate().to_string()}).to_string();
    for tool in ["bash_wait", "bash_cancel"] {
        let state = tool_calls::ToolCallState {
            thread_id: "thread".into(),
            tool: tool.into(),
            input_json: input.clone(),
            status: "pending_approval".into(),
            approval: None,
            output_text: None,
            exit_code: None,
            duration_ms: None,
            output_full_path: None,
        };
        for source in [
            RuntimeApprovalSource::User,
            RuntimeApprovalSource::FullAccess,
            RuntimeApprovalSource::Auto,
            RuntimeApprovalSource::Validation,
        ] {
            let audit = RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Once,
                note: None,
                source,
                danger: None,
            };
            assert!(events::validate_runtime_approval_event(&state, "call", &audit, None).is_err());
        }
        let valid = RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Once,
            note: None,
            source: RuntimeApprovalSource::ReadonlyTool,
            danger: None,
        };
        assert!(events::validate_runtime_approval_event(&state, "call", &valid, None).is_ok());
        let invalid = tool_calls::ToolCallState {
            input_json: "{}".into(),
            ..state
        };
        assert!(events::validate_runtime_approval_event(&invalid, "call", &valid, None).is_err());
        let deny = ApprovalAudit {
            decision: Approval::Deny,
            note: None,
            source: ApprovalSource::Validation,
            danger: None,
        };
        assert_eq!(
            shell_operations::validate_projection(
                tool,
                "{}",
                "Tool error: invalid shell session input",
                RuntimeToolStatus::Rejected,
                &deny,
                None,
                None
            ),
            Some(true)
        );
        assert_eq!(
            shell_operations::validate_projection(
                tool,
                "{}",
                "Tool error: invalid shell session input",
                RuntimeToolStatus::Success,
                &deny,
                None,
                None
            ),
            Some(false)
        );
    }
}
