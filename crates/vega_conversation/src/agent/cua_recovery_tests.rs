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
    live_store_at(std::path::Path::new(":memory:"))
}

fn live_store_at(path: &std::path::Path) -> Store {
    let store = Store::open(path).unwrap();
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

struct LateStopProvider {
    run: vega_runtime::computer_use::ComputerUseRun,
    screenshot: bool,
    requests: std::sync::atomic::AtomicUsize,
}

impl Provider for LateStopProvider {
    fn chat_stream(
        &self,
        _: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(!cancel.is_cancelled());
        let index = self
            .requests
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let (id, name, input) = match index {
            0 => (
                "directory",
                "ComputerObserve",
                json!({"target_id":null,"cursor":null}),
            ),
            1 => {
                let directory = self
                    .run
                    .control
                    .snapshot()
                    .observations
                    .into_iter()
                    .next()
                    .unwrap();
                let page: Value = serde_json::from_str(&directory.text).unwrap();
                (
                    "late",
                    if self.screenshot {
                        "ComputerScreenshot"
                    } else {
                        "ComputerObserve"
                    },
                    if self.screenshot {
                        json!({"target_id":page["windows"][0]["target_id"]})
                    } else {
                        json!({"target_id":page["windows"][0]["target_id"],"cursor":null})
                    },
                )
            }
            _ => panic!("late Stop must prevent another provider request"),
        };
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter([
                Ok(vega_runtime::ProviderEvent::ToolUse {
                    id: id.into(),
                    name: name.into(),
                    input_json: input.to_string(),
                }),
                Ok(vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::ToolUse,
                }),
            ])) as vega_runtime::EventStream)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopBoundary {
    CompletedObservation,
    BeforeScreenshotPublication,
    StaleScreenshotEpoch,
    AfterScreenshotPublication,
}

async fn late_stop_round_trip(boundary: StopBoundary) {
    let directory =
        std::env::temp_dir().join(format!("vega-cua-recovery-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory).unwrap();
    let database_path = directory.join("conversation.sqlite3");
    drop(live_store_at(&database_path));
    let driver = Driver::new([true, true], false);
    let run = configured_run(&driver).await;
    let root = CancellationToken::new();
    if matches!(
        boundary,
        StopBoundary::CompletedObservation | StopBoundary::BeforeScreenshotPublication
    ) {
        let stop = root.clone();
        driver.after_execution("late", move || stop.cancel());
    }
    let provider = LateStopProvider {
        run: run.clone(),
        screenshot: boundary != StopBoundary::CompletedObservation,
        requests: std::sync::atomic::AtomicUsize::new(0),
    };
    let request = AgentRequest {
        model: "scripted".into(),
        system_prompt: "Inspect the requested window.".into(),
        history: vec![vega_runtime::ChatMessage::new(
            vega_runtime::ChatRole::User,
            "Inspect the window.",
        )],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            "project".into(),
            "thread".into(),
            PathBuf::new(),
            Vec::new(),
        )
        .with_computer_use(Some(run.clone()))
        .with_turn_limit(4),
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
    let actor = PersistenceActor::start(PersistenceActorStart {
        database_path: database_path.clone(),
        project_id: "project".into(),
        thread_id: "thread".into(),
        message_id: "assistant".into(),
        model: "scripted".into(),
        is_plan: false,
        next_tool_seq: 1,
        config: PersistenceActorConfig::default(),
    })
    .await
    .unwrap();
    let (sender, receiver) = mpsc::channel(16);
    let tools = vega_tools::Tools::new(PathBuf::new());
    let runtime = vega_runtime::run_agent_with_sink(&provider, &tools, request, root.clone(), {
        let root = root.clone();
        move |event| {
            let sender = sender.clone();
            let root = root.clone();
            async move {
                if boundary == StopBoundary::StaleScreenshotEpoch
                    && let RuntimeEvent::ScreenshotDelivered { control, .. } = &event
                {
                    control.pause();
                    root.cancel();
                }
                let (ack, received) = oneshot::channel();
                sender
                    .send(RuntimeEnvelope {
                        event,
                        ack: Some(ack),
                    })
                    .await
                    .map_err(|_| persistence_actor_error("event channel closed"))?;
                received
                    .await
                    .map_err(|_| persistence_actor_error("event acknowledgement dropped"))?
            }
        }
    });
    let mut events = Vec::new();
    let mut text = String::new();
    let clock = crate::types::WorkClock::new(None);
    let diagnostics = super::super::diagnostics::DiagnosticsWriter::buffered();
    let mut sink = |event: &ConversationEvent| {
        if boundary == StopBoundary::AfterScreenshotPublication
            && matches!(event, ConversationEvent::ScreenshotDelivered { .. })
        {
            root.cancel();
        }
        Ok(())
    };
    let processor = process_runtime_events(
        receiver,
        &actor,
        RuntimeEventContext {
            message_id: "assistant",
            streamed_content: &mut text,
            events: &mut events,
            event_sink: &mut sink,
            cancel: root.clone(),
            run_started_at: std::time::Instant::now(),
            work_clock: &clock,
            diagnostics: &diagnostics,
            thread_id: "thread",
            run_id: "assistant",
            root_attempt_id: "attempt",
            subagent_receipts: None,
        },
    );
    let (outcome, persisted) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runtime, processor)
    })
    .await
    .unwrap();
    persisted.unwrap();
    let outcome = outcome.unwrap();
    assert!(root.is_cancelled() && outcome.interrupted && !outcome.failed);
    assert_eq!(
        provider.requests.load(std::sync::atomic::Ordering::Acquire),
        2
    );
    let published = boundary == StopBoundary::AfterScreenshotPublication;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::ScreenshotDelivered { .. }))
            .count(),
        usize::from(published)
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ConversationEvent::Error { .. }))
    );
    let terminal = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "late" => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap();
    let completed = boundary == StopBoundary::CompletedObservation || published;
    assert_eq!(
        terminal.status,
        if completed {
            RuntimeToolStatus::Success
        } else {
            RuntimeToolStatus::Cancelled
        }
    );
    if boundary == StopBoundary::CompletedObservation {
        assert!(terminal.output.starts_with("completed; observation_id="));
    } else {
        let output: Value = serde_json::from_str(&terminal.output).unwrap();
        if published {
            assert_eq!(output["status"], "delivered");
            assert_eq!(output["artifact_id"], "screenshot:late");
        } else {
            assert_eq!(output["code"], "cancelled");
            assert_eq!(output["execution_state"], "failed");
            assert!(output.get("status").is_none());
            assert_eq!(terminal.truncated, None);
        }
    }
    actor.close().await.unwrap();
    let store = Store::open(&database_path).unwrap();
    let rows = tool_calls::terminal_results(store.conn(), "thread").unwrap();
    assert_eq!(rows["late"].output, terminal.output);
    assert_eq!(
        rows["late"].status,
        if completed { "success" } else { "cancelled" }
    );
    assert_eq!(
        messages::find(store.conn(), "assistant")
            .unwrap()
            .unwrap()
            .status,
        "interrupted"
    );
    let page = crate::history::restart_history_page(&store, "thread", 20).unwrap();
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry,
                crate::history::HistoryEntry::AssistantScreenshot { .. }
            ))
            .count(),
        usize::from(published)
    );
    assert!(page.entries.iter().any(|entry| matches!(entry,
        crate::history::HistoryEntry::Tool { call_id, result: Some(crate::types::ToolCardResultProjection::ComputerUse { output, .. }), .. }
            if call_id == "late" && output == &terminal.output
    )));
    drop(store);
    let prepared = prepare_run_with_images_and_reasoning(
        database_path.clone(),
        "thread".into(),
        "Continue independent work.".into(),
        "Respond safely.".into(),
        "next-user".into(),
        "next-assistant".into(),
        PersistenceActorConfig::default(),
        false,
        None,
        None,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        prepared.request.completed_tool_results["late"]
            .result
            .status,
        terminal.status
    );
    assert_eq!(
        prepared.request.completed_tool_results["late"]
            .result
            .output,
        terminal.output
    );
    assert!(
        prepared
            .request
            .history
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("late"))
    );
    drop(prepared);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn late_stop_preserves_completed_observation_through_live_sink_reload_and_next_run() {
    late_stop_round_trip(StopBoundary::CompletedObservation).await;
}

#[tokio::test]
async fn late_stop_never_claims_an_unpublished_screenshot_was_delivered() {
    late_stop_round_trip(StopBoundary::BeforeScreenshotPublication).await;
}

#[tokio::test]
async fn revoked_screenshot_epoch_is_not_published_or_reported_as_pipeline_failure() {
    late_stop_round_trip(StopBoundary::StaleScreenshotEpoch).await;
}

#[tokio::test]
async fn late_stop_preserves_an_already_published_screenshot_through_reload_and_next_run() {
    late_stop_round_trip(StopBoundary::AfterScreenshotPublication).await;
}

#[test]
fn legacy_cancelled_completions_and_screenshot_receipts_remain_bounded_and_readable() {
    let directory =
        std::env::temp_dir().join(format!("vega-cua-legacy-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory).unwrap();
    let database_path = directory.join("conversation.sqlite3");
    let store = live_store_at(&database_path);
    let receipt = json!({"status":"delivered","artifact_id":"screenshot:legacy-image",
        "delivery":"conversation_image_attachment","saved_in_history":true,
        "read_only":true,"unchanged":false,"next_step":"continue_remaining_requested_work_or_finish"});
    let audit = approval_audit_to_runtime(&approval(Approval::Once));
    let mut next_seq = 1;
    for (id, tool, action, output) in [
        ("legacy-read", "ComputerObserve", "observe", "completed; observation_id=legacy; observation images and page data are ephemeral and are not retained in history.".into()),
        ("legacy-directory", "ComputerObserve", "observe", "completed; observation_id=legacy; window directory is ephemeral and is not retained in history.".into()),
        ("legacy-image", "ComputerScreenshot", "screenshot", receipt.to_string()),
    ] {
        for event in [
            RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall { id:id.into(), name:tool.into(), input_json:audit_input(action) }),
            RuntimeEvent::ToolCallApproved { call_id:id.into(), audit:audit.clone(), remember_rule:None },
            RuntimeEvent::ToolCallRunning { call_id:id.into() },
        ] {
            persist_live_event(&store, &mut next_seq, "", &event).unwrap();
        }
        let terminal = vega_runtime::RuntimeToolResult {
            call_id:id.into(), output, status:RuntimeToolStatus::Cancelled, reused:false,
            exit_code:None, duration_ms:None, truncated:None, approval:Some(audit.clone()), remember_rule:None,
        };
        persist_live_event(&store, &mut next_seq, "", &RuntimeEvent::ToolCallFinished(terminal)).unwrap();
    }
    persist_live_event(&store, &mut next_seq, "", &RuntimeEvent::Interrupted).unwrap();
    let validate_legacy = |tool: &str, action: &str, output: &str, approval: &ApprovalAudit| {
        validate_recovered_projection(
            "project",
            "thread",
            "legacy",
            tool,
            &audit_input(action),
            output,
            RuntimeToolStatus::Cancelled,
            approval,
            None,
            None,
        )
    };
    let mut forged = receipt.clone();
    forged["extra"] = json!("untrusted");
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            &forged.to_string(),
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate_legacy(
            "ComputerObserve",
            "observe",
            &receipt.to_string(),
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            &receipt.to_string(),
            &approval(Approval::Deny)
        )
        .is_err()
    );
    let mut wrong_source = approval(Approval::Once);
    wrong_source.source = ApprovalSource::RunMode;
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            &receipt.to_string(),
            &wrong_source
        )
        .is_err()
    );
    let mut with_note = approval(Approval::Once);
    with_note.note = Some("not allowed".into());
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            &receipt.to_string(),
            &with_note
        )
        .is_err()
    );
    let mut oversized = receipt.clone();
    oversized["artifact_id"] = json!(format!("screenshot:{}", "x".repeat(2048)));
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            &oversized.to_string(),
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate_legacy(
            "ComputerObserve",
            "observe",
            "completed; observation_id=legacy; forged completion",
            &approval(Approval::Once)
        )
        .is_err()
    );
    assert!(
        validate_legacy(
            "ComputerScreenshot",
            "screenshot",
            r#"{"status":"cancelled","arbitrary":true}"#,
            &approval(Approval::Once)
        )
        .is_err()
    );
    drop(store);
    let reopened = Store::open(&database_path).unwrap();
    let page = crate::history::restart_history_page(&reopened, "thread", 20).unwrap();
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry,
                crate::history::HistoryEntry::Tool {
                    status: crate::types::ToolCallStatus::Cancelled,
                    result: Some(crate::types::ToolCardResultProjection::ComputerUse {
                        status: crate::types::ToolCallStatus::Cancelled,
                        ..
                    }),
                    ..
                }
            ))
            .count(),
        3
    );
    drop(reopened);
    let prepared = prepare_run_with_images_and_reasoning(
        database_path,
        "thread".into(),
        "Continue.".into(),
        "Respond safely.".into(),
        "next-user".into(),
        "next-assistant".into(),
        PersistenceActorConfig::default(),
        false,
        None,
        None,
        Vec::new(),
    )
    .unwrap();
    for id in ["legacy-read", "legacy-directory", "legacy-image"] {
        assert_eq!(
            prepared.request.completed_tool_results[id].result.status,
            RuntimeToolStatus::Cancelled
        );
    }
    drop(prepared);
    std::fs::remove_dir_all(directory).unwrap();
}
