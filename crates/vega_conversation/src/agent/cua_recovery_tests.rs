use super::*;
use serde_json::{Value, json};

fn audit_input(action: &str) -> String {
    json!({"kind":"computer_use","version":1,"action":action,"argument_bytes":2,
        "argument_digest":"0".repeat(64)})
    .to_string()
}

fn permission_output() -> String {
    json!({"code":"screen_recording_permission_missing","capability":"desktop","stage":"permissions",
        "execution_state":"not_dispatched","retryable":false,"next_step":"open_system_settings","field":null,
        "diagnostic":"executor=cua stage=permissions error_class=screen_recording_permission_missing: 请在系统设置中开启 Vega 的屏幕录制权限，然后重试。"}).to_string()
}

fn approval(decision: Approval) -> ApprovalAudit {
    ApprovalAudit {
        decision,
        source: ApprovalSource::ComputerUse,
        note: None,
        danger: None,
    }
}

fn validate(
    output: &str,
    status: RuntimeToolStatus,
    approval: &ApprovalAudit,
) -> Result<String, ConversationError> {
    validate_recovered_projection(
        "project",
        "thread",
        "call",
        "ComputerObserve",
        &audit_input("observe"),
        output,
        status,
        approval,
        None,
        None,
    )
}

#[test]
fn structured_and_historical_permission_errors_round_trip_through_store_and_audit() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Test",
            mode: "execute",
            permission_mode: "auto",
            model: "scripted",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant".into(),
            thread_id: "thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let outputs = [permission_output(), "Tool error: computer use not_dispatched: {\"code\":\"screen_recording_permission_missing\",\"retryable\":false,\"next_step\":\"open_system_settings\",\"field\":null}".into()];
    for (index, output) in outputs.iter().enumerate() {
        let id = format!("permission-{index}");
        let audit = approval(Approval::Deny);
        tool_calls::insert_pending(
            store.conn(),
            tool_calls::NewToolCall {
                id: &id,
                thread_id: "thread",
                message_id: "assistant",
                seq: index as i64 + 1,
                tool: "ComputerObserve",
                input_json: &audit_input("observe"),
                status: "pending_approval",
                created_at: 1,
            },
        )
        .unwrap();
        tool_calls::reject(
            store.conn(),
            &id,
            &audit.to_json().unwrap(),
            output,
            2,
            None,
        )
        .unwrap();
    }
    let terminal = tool_calls::terminal_results(store.conn(), "thread").unwrap();
    assert_eq!(terminal.len(), 2);
    for (id, row) in terminal {
        let audit = ApprovalAudit::from_json(&row.approval).unwrap();
        assert_eq!(row.status, "rejected");
        assert_eq!(audit.decision, Approval::Deny);
        assert_eq!(audit.source, ApprovalSource::ComputerUse);
        assert_eq!(
            validate_recovered_projection(
                "project",
                "thread",
                &id,
                &row.tool,
                &row.input_json,
                &row.output,
                RuntimeToolStatus::Rejected,
                &audit,
                row.exit_code,
                row.duration_ms
            )
            .unwrap(),
            audit_input("observe")
        );
        assert!(outputs.contains(&row.output));
    }
    let page = crate::history::latest_history_page(&store, "thread", 20).unwrap();
    let results = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            crate::history::HistoryEntry::Tool {
                result: Some(crate::types::ToolCardResultProjection::ComputerUse { output, .. }),
                ..
            } => Some(output),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|output| outputs.contains(output)));
}

#[test]
fn structured_errors_reject_forged_fields_sources_statuses_and_target_content() {
    let output = permission_output();
    let deny = approval(Approval::Deny);
    assert!(validate(&output, RuntimeToolStatus::Rejected, &deny).is_ok());
    for (key, value) in [
        ("diagnostic", json!("Private window text /sensitive/path")),
        ("code", json!("unbounded_upstream_exception")),
        ("capability", json!("shell")),
        ("stage", json!("arbitrary")),
        ("retryable", json!(true)),
        ("field", json!("private-window-id")),
        ("execution_state", json!("outcome_unknown")),
        ("extra", json!("payload")),
    ] {
        let mut forged: Value = serde_json::from_str(&output).unwrap();
        forged[key] = value;
        assert!(
            validate(&forged.to_string(), RuntimeToolStatus::Rejected, &deny).is_err(),
            "accepted forged {key}"
        );
    }
    let mut missing: Value = serde_json::from_str(&output).unwrap();
    missing.as_object_mut().unwrap().remove("field");
    assert!(validate(&missing.to_string(), RuntimeToolStatus::Rejected, &deny).is_err());
    let mut wrong_source = deny.clone();
    wrong_source.source = ApprovalSource::ReadonlyTool;
    assert!(validate(&output, RuntimeToolStatus::Rejected, &wrong_source).is_err());
    assert!(
        validate(
            &output,
            RuntimeToolStatus::Success,
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate(
            &output,
            RuntimeToolStatus::Rejected,
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(validate("{}", RuntimeToolStatus::Failed, &approval(Approval::Once)).is_err());
    assert!(
        validate(
            &"x".repeat(2049),
            RuntimeToolStatus::Failed,
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "call",
            "ComputerScreenshot",
            &audit_input("observe"),
            &output,
            RuntimeToolStatus::Rejected,
            &deny,
            None,
            None
        )
        .is_err()
    );
}

#[test]
fn superseded_receipts_and_guard_results_keep_exact_audit_boundaries() {
    let once = approval(Approval::Once);
    let superseded = vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT;
    assert!(validate(superseded, RuntimeToolStatus::Cancelled, &once).is_ok());
    let mut forged: Value = serde_json::from_str(superseded).unwrap();
    forged["executed"] = json!(true);
    assert!(validate(&forged.to_string(), RuntimeToolStatus::Cancelled, &once).is_err());
    assert!(validate(superseded, RuntimeToolStatus::Failed, &once).is_err());
    let guard = json!({"code":"no_progress","capability":"desktop","stage":"progress","execution_state":"failed",
        "retryable":false,"next_step":"stop_and_report","field":null,
        "diagnostic":"重复操作没有进展，已停止本轮电脑操作。",
        "guard":{"trigger":"repeated_directory","count":3,"threshold":3,"underlying_result":"succeeded","dispatch_started":true}});
    assert!(validate(&guard.to_string(), RuntimeToolStatus::Failed, &once).is_ok());
    let mut invalid_guard = guard.clone();
    invalid_guard["guard"]["threshold"] = json!(4);
    assert!(validate(&invalid_guard.to_string(), RuntimeToolStatus::Failed, &once).is_err());
    let mut frozen = guard;
    frozen["execution_state"] = json!("not_dispatched");
    assert!(
        validate(
            &frozen.to_string(),
            RuntimeToolStatus::Rejected,
            &approval(Approval::Deny)
        )
        .is_ok()
    );
    let receipt = json!({"status":"delivered","artifact_id":"screenshot:original","delivery":"conversation_image_attachment",
        "saved_in_history":true,"read_only":true,"unchanged":false,"next_step":"continue_remaining_requested_work_or_finish"});
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "call",
            "ComputerScreenshot",
            &audit_input("screenshot"),
            &receipt.to_string(),
            RuntimeToolStatus::Success,
            &once,
            None,
            None
        )
        .is_ok()
    );
    let old_budget = json!({"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,
        "next_step":"use_current_results_then_request_remaining_windows"});
    assert!(validate(&old_budget.to_string(), RuntimeToolStatus::Failed, &once).is_ok());
}
