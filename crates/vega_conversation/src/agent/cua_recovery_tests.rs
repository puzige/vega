use super::*;
use serde_json::{Value, json};
use vega_runtime::computer_use::recovery_test_support::{
    Driver, configured_run, fill_observation_text_budget, finite_error_outputs,
};

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

fn live_store() -> Store {
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
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    store
}

fn persist_live_event(
    store: &Store,
    next_seq: &mut i64,
    text: &str,
    event: &RuntimeEvent,
) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        "project",
        "thread",
        "assistant",
        "scripted",
        false,
        text,
        next_seq,
        event,
        None,
    )
}

#[test]
fn generated_finite_errors_pass_live_terminal_recovery_and_history_card_audits() {
    let store = live_store();
    let mut next_seq = 1;
    let outputs = finite_error_outputs();
    let mut changed_target = false;
    for (index, (tool, output, rejected)) in outputs.iter().enumerate() {
        let value: Value = serde_json::from_str(output).unwrap();
        if value["code"] == "target_expired"
            && value["diagnostic"]
                == vega_runtime::computer_use::ComputerUseError::TargetChanged.user_message()
        {
            changed_target = true;
            assert_ne!(
                value["diagnostic"],
                vega_runtime::computer_use::ComputerUseError::TargetExpired.user_message()
            );
        }
        let call_id = format!("finite-{index}");
        let audit = approval_audit_to_runtime(&approval(if *rejected {
            Approval::Deny
        } else {
            Approval::Once
        }));
        persist_live_event(
            &store,
            &mut next_seq,
            "",
            &RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall {
                id: call_id.clone(),
                name: tool.clone(),
                input_json: audit_input("observe"),
            }),
        )
        .unwrap();
        if !rejected {
            for event in [
                RuntimeEvent::ToolCallApproved {
                    call_id: call_id.clone(),
                    audit: audit.clone(),
                    remember_rule: None,
                },
                RuntimeEvent::ToolCallRunning {
                    call_id: call_id.clone(),
                },
            ] {
                persist_live_event(&store, &mut next_seq, "", &event).unwrap();
            }
        }
        let event = RuntimeEvent::ToolCallFinished(vega_runtime::RuntimeToolResult {
            call_id,
            output: output.clone(),
            status: if *rejected {
                RuntimeToolStatus::Rejected
            } else {
                RuntimeToolStatus::Failed
            },
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: Some(audit),
            remember_rule: None,
        });
        persist_live_event(&store, &mut next_seq, "", &event)
            .unwrap_or_else(|error| panic!("live audit rejected {output}: {error}"));
    }
    persist_live_event(
        &store,
        &mut next_seq,
        "",
        &RuntimeEvent::Finished(vega_runtime::RuntimeFinishReason::End),
    )
    .unwrap();
    assert!(changed_target);
    let terminal = tool_calls::terminal_results(store.conn(), "thread").unwrap();
    assert_eq!(terminal.len(), outputs.len());
    for (call_id, row) in terminal {
        let audit = ApprovalAudit::from_json(&row.approval).unwrap();
        assert_eq!(
            validate_recovered_projection(
                "project",
                "thread",
                &call_id,
                &row.tool,
                &row.input_json,
                &row.output,
                if row.status == "rejected" {
                    RuntimeToolStatus::Rejected
                } else {
                    RuntimeToolStatus::Failed
                },
                &audit,
                row.exit_code,
                row.duration_ms,
            )
            .unwrap(),
            audit_input("observe")
        );
    }
    let page = crate::history::latest_history_page(&store, "thread", 20).unwrap();
    let projected = page
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
    assert_eq!(projected.len(), outputs.len());
    assert!(
        projected
            .iter()
            .all(|output| outputs.iter().any(|(_, expected, _)| expected == *output))
    );
}

struct BudgetProvider {
    run: vega_runtime::computer_use::ComputerUseRun,
    requests: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

impl Provider for BudgetProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(!cancel.is_cancelled());
        let mut requests = self.requests.lock().unwrap();
        let input = match requests.len() {
            0 => Some(json!({"target_id":null,"cursor":null})),
            1 => {
                let observation = self
                    .run
                    .control
                    .snapshot()
                    .observations
                    .into_iter()
                    .next()
                    .unwrap();
                let directory: Value = serde_json::from_str(&observation.text).unwrap();
                Some(json!({"target_id":directory["windows"][0]["target_id"],"cursor":null}))
            }
            2 => {
                let results = request
                    .messages
                    .iter()
                    .filter(|message| {
                        message.role == vega_runtime::ChatRole::Tool
                            && message.tool_call_id.as_deref() == Some("budget")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(results.len(), 1);
                assert!(
                    request
                        .messages
                        .iter()
                        .flat_map(|message| &message.tool_calls)
                        .any(|call| call.id == "budget")
                );
                let error: Value = serde_json::from_str(&results[0].content).unwrap();
                assert_eq!(error["code"], "observation_budget_exceeded");
                assert_eq!(error["execution_state"], "failed");
                assert_eq!(error["executed"], true);
                assert_eq!(error["deferred"], true);
                None
            }
            _ => panic!("unexpected provider request"),
        };
        let step = if let Some(input) = input {
            vec![
                vega_runtime::ProviderEvent::ToolUse {
                    id: if requests.is_empty() {
                        "directory"
                    } else {
                        "budget"
                    }
                    .into(),
                    name: "ComputerObserve".into(),
                    input_json: input.to_string(),
                },
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::ToolUse,
                },
            ]
        } else {
            vec![
                vega_runtime::ProviderEvent::TextDelta("The read executed, but its observation exceeded the batch capacity. I can explain what remains.".into()),
                vega_runtime::ProviderEvent::Done { stop_reason: vega_runtime::StopReason::End },
            ]
        };
        requests.push(request);
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(step.into_iter().map(Ok)))
                as vega_runtime::EventStream)
        })
    }
}

#[tokio::test]
async fn executed_observation_budget_error_reaches_the_model_and_live_terminal_sink() {
    let driver = Driver::new([true, true], false);
    let run = configured_run(&driver).await;
    let provider = BudgetProvider {
        run: run.clone(),
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let store = live_store();
    let root = CancellationToken::new();
    let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
    let request = AgentRequest {
        model: "scripted".into(),
        system_prompt: "Inspect the requested window safely.".into(),
        history: vec![vega_runtime::ChatMessage::new(
            vega_runtime::ChatRole::User,
            "Inspect the window and explain what remains.",
        )],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            "project".into(),
            "thread".into(),
            std::path::PathBuf::new(),
            Vec::new(),
        )
        .with_computer_use(Some(run.clone()))
        .with_turn_limit(5),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    };
    let mut next_seq = 1;
    let mut budget_result = None;
    let mut streamed_text = String::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        vega_runtime::run_agent_with_sink(&provider, &tools, request, root.clone(), |event| {
            if let RuntimeEvent::TextDelta(delta) = &event {
                streamed_text.push_str(delta);
            }
            if matches!(&event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "budget") {
                fill_observation_text_budget(&run);
            }
            if let RuntimeEvent::ToolCallFinished(result) = &event
                && result.call_id == "budget"
            {
                assert_eq!(result.status, RuntimeToolStatus::Failed);
                assert_eq!(result.truncated, None);
                budget_result = Some(result.output.clone());
                let mut stale_success_metadata = result.clone();
                stale_success_metadata.truncated = Some(false);
                assert!(
                    persist_live_event(
                        &store,
                        &mut next_seq,
                        &streamed_text,
                        &RuntimeEvent::ToolCallFinished(stale_success_metadata)
                    )
                    .is_err()
                );
            }
            let persisted = persist_live_event(&store, &mut next_seq, &streamed_text, &event);
            std::future::ready(persisted)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!root.is_cancelled() && !outcome.interrupted && !outcome.failed);
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
    assert_eq!(
        outcome.final_text,
        "The read executed, but its observation exceeded the batch capacity. I can explain what remains."
    );
    assert_eq!(
        driver
            .calls()
            .iter()
            .filter(|call| call.as_str() == "get_window_state")
            .count(),
        1
    );
    let terminal = tool_calls::terminal_results(store.conn(), "thread").unwrap();
    let row = &terminal["budget"];
    assert_eq!(row.status, "failed");
    assert_eq!(Some(&row.output), budget_result.as_ref());
    assert_eq!(
        ApprovalAudit::from_json(&row.approval).unwrap(),
        approval(Approval::Once)
    );
    let page = crate::history::latest_history_page(&store, "thread", 20).unwrap();
    assert!(page.entries.iter().any(|entry| matches!(entry,
        crate::history::HistoryEntry::Tool {
            result: Some(crate::types::ToolCardResultProjection::ComputerUse { output, .. }), ..
        } if Some(output) == budget_result.as_ref()
    )));
}
