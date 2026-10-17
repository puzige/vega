use super::*;
use crate::automation;
use crate::types::{AutomationCommand, AutomationControl, AutomationKind, AutomationStatus};
use serde_json::json;
use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeToolCall,
    RuntimeToolResult,
};

#[test]
fn automation_tool_audit_persists_only_valid_proposals_approvals_and_terminals() {
    let (store, _dir, project) = setup();
    store
        .conn()
        .execute_batch(
            "INSERT INTO messages (id,thread_id,seq,role,content,status,created_at)
         VALUES ('audit-assistant','thread-1',1,'assistant','','streaming',0);",
        )
        .unwrap();
    let mut seq = 1;
    let mut persist = |event: RuntimeEvent| {
        persist_runtime_event(
            &store,
            &project,
            "thread-1",
            "audit-assistant",
            "mock",
            false,
            "",
            &mut seq,
            &event,
            None,
        )
    };
    let approval = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::ReadonlyTool,
        danger: None,
    };
    for (name, input) in [
        ("get_goal", json!({})),
        (
            "update_goal",
            json!({"status":"complete","reason":"Verified outputs"}),
        ),
        ("schedule_wakeup", json!({"delay_seconds":60})),
    ] {
        for input_json in [
            "{".into(),
            "[]".into(),
            "null".into(),
            json!({"reason":"x".repeat(32 * 1024)}).to_string(),
        ] {
            assert!(
                persist(RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                    id: "invalid-proposal".into(),
                    name: name.into(),
                    input_json,
                }))
                .is_err()
            );
            assert!(
                tool_calls::find_state(store.conn(), "invalid-proposal")
                    .unwrap()
                    .is_none()
            );
        }
        persist(RuntimeEvent::ToolCallProposed(RuntimeToolCall {
            id: name.into(),
            name: name.into(),
            input_json: input.to_string(),
        }))
        .unwrap();
        assert!(
            persist(RuntimeEvent::ToolCallRunning {
                call_id: name.into()
            })
            .is_err()
        );
        for forged in [
            RuntimeApprovalAudit {
                source: RuntimeApprovalSource::User,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                source: RuntimeApprovalSource::Auto,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Always,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Deny,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                note: Some("unbound note".into()),
                ..approval.clone()
            },
        ] {
            assert!(
                persist(RuntimeEvent::ToolCallApproved {
                    call_id: name.into(),
                    audit: forged,
                    remember_rule: None,
                })
                .is_err()
            );
            assert_eq!(
                tool_calls::find_state(store.conn(), name)
                    .unwrap()
                    .unwrap()
                    .status,
                "pending_approval"
            );
        }
        persist(RuntimeEvent::ToolCallApproved {
            call_id: name.into(),
            audit: approval.clone(),
            remember_rule: None,
        })
        .unwrap();
        let result = RuntimeToolResult {
            call_id: name.into(),
            output: "{\"recorded\":true}".into(),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            approval: Some(approval.clone()),
            remember_rule: None,
        };
        assert!(persist(RuntimeEvent::ToolCallFinished(result.clone())).is_err());
        persist(RuntimeEvent::ToolCallRunning {
            call_id: name.into(),
        })
        .unwrap();
        for forged in [
            RuntimeToolResult {
                truncated: None,
                ..result.clone()
            },
            RuntimeToolResult {
                output: "not JSON".into(),
                ..result.clone()
            },
            RuntimeToolResult {
                exit_code: Some(0),
                ..result.clone()
            },
            RuntimeToolResult {
                duration_ms: Some(1),
                ..result.clone()
            },
            RuntimeToolResult {
                approval: Some(RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::User,
                    ..approval.clone()
                }),
                ..result.clone()
            },
        ] {
            assert!(persist(RuntimeEvent::ToolCallFinished(forged)).is_err());
            assert_eq!(
                tool_calls::find_state(store.conn(), name)
                    .unwrap()
                    .unwrap()
                    .status,
                "running"
            );
        }
        persist(RuntimeEvent::ToolCallFinished(result.clone())).unwrap();
        persist(RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            reused: true,
            truncated: None,
            ..result
        }))
        .unwrap();
        let stored = tool_calls::find_state(store.conn(), name).unwrap().unwrap();
        assert_eq!(stored.status, "success");
        assert_eq!(stored.output_text.as_deref(), Some("{\"recorded\":true}"));
        assert_eq!(
            ApprovalAudit::from_json(stored.approval.as_deref().unwrap()).unwrap(),
            approval_audit_from_runtime(&approval)
        );
        let cancelled_id = format!("cancelled-{name}");
        persist(RuntimeEvent::ToolCallProposed(RuntimeToolCall {
            id: cancelled_id.clone(),
            name: name.into(),
            input_json: input.to_string(),
        }))
        .unwrap();
        let denied = RuntimeToolResult {
            call_id: cancelled_id.clone(),
            output: "Tool error: permission denied".into(),
            status: RuntimeToolStatus::Rejected,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: Some(RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Deny,
                source: RuntimeApprovalSource::Timeout,
                note: None,
                danger: None,
            }),
            remember_rule: None,
        };
        for forged in [
            RuntimeToolResult {
                approval: Some(approval.clone()),
                ..denied.clone()
            },
            RuntimeToolResult {
                approval: Some(RuntimeApprovalAudit {
                    decision: RuntimeApprovalDecision::Deny,
                    source: RuntimeApprovalSource::User,
                    note: None,
                    danger: None,
                }),
                ..denied.clone()
            },
            RuntimeToolResult {
                output: "unexpected denial".into(),
                ..denied.clone()
            },
        ] {
            assert!(persist(RuntimeEvent::ToolCallFinished(forged)).is_err());
            assert_eq!(
                tool_calls::find_state(store.conn(), &cancelled_id)
                    .unwrap()
                    .unwrap()
                    .status,
                "pending_approval"
            );
        }
        persist(RuntimeEvent::ToolCallFinished(denied)).unwrap();
        assert_eq!(
            tool_calls::find_state(store.conn(), &cancelled_id)
                .unwrap()
                .unwrap()
                .status,
            "rejected"
        );
    }
    for name in ["get_goal", "unknown_automation_tool"] {
        assert!(
            persist(RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "invalid-proposal".into(),
                name: name.into(),
                input_json: "{\"extra\":true}".into(),
            }))
            .is_err()
        );
    }
    assert!(
        tool_calls::find_state(store.conn(), "invalid-proposal")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn malformed_automation_inputs_are_redacted_rejected_and_reopen_without_authority() {
    let (store, dir, _) = setup();
    command(&store, "/goal inspect safely");
    command(&store, "/loop inspect safely");
    let claim = automation::claim(
        &store,
        "thread-1",
        Some(AutomationKind::Loop),
        automation::now_ms(),
    )
    .unwrap()
    .unwrap();
    let mut calls = Vec::new();
    for name in ["get_goal", "update_goal", "schedule_wakeup"] {
        for input_json in [
            "{".into(),
            "[]".into(),
            "null".into(),
            json!({"reason":"x".repeat(32*1024)}).to_string(),
        ] {
            calls.push((name, input_json));
        }
    }
    calls.push(("get_goal", "{\"unexpected\":true}".into()));
    calls.push(("unknown_automation_tool", "{\"unexpected\":true}".into()));
    let mut events = calls
        .iter()
        .enumerate()
        .map(|(index, (name, input))| ProviderEvent::ToolUse {
            id: format!("invalid-{index}"),
            name: (*name).into(),
            input_json: input.clone(),
        })
        .collect::<Vec<_>>();
    events.push(ProviderEvent::Usage {
        input: 10,
        output: 2,
        cache_read: None,
        cache_write: None,
    });
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    let provider =
        MockProvider::new_rounds(vec![vec![ScriptStep::events(events)], round(None, true)]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Inspect safely",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_automation_claim(Some(claim)),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallApproved { .. } | ConversationEvent::ToolCallRunning { .. }
    )));
    for index in 0..calls.len() {
        let id = format!("invalid-{index}");
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallProposed {call} if call.id==id && call.input_json=="{}")));
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallFinished {call_id,result} if call_id==&id && result.status==ToolCallStatus::Rejected)));
        let state = tool_calls::find_state(store.conn(), &id).unwrap().unwrap();
        assert_eq!(state.status, "rejected");
        assert_eq!(state.input_json, "{}");
        assert_eq!(
            state.output_text.as_deref(),
            Some("Tool error: denied: unavailable tool")
        );
        let approval = ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
        assert_eq!(approval.source, ApprovalSource::RunMode);
        assert_eq!(approval.decision, Approval::Deny);
    }
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let next = MockProvider::new_rounds(vec![round(None, true)]);
    let run = run_thread_task(
        &reopened,
        &next,
        &tools,
        "thread-1",
        "Review rejection",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(next.requests().len(), 1);
    assert_eq!(
        store
            .conn()
            .query_row("SELECT count(*) FROM tool_calls", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        i64::try_from(calls.len()).unwrap()
    );
}

#[tokio::test]
async fn question_batch_rejects_automation_report_before_execution_and_replays() {
    let (store, dir, _) = setup();
    command(&store, "/goal finish only after user choice");
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id:"mixed-goal-report".into(), name:"update_goal".into(),
                input_json:json!({"status":"complete","reason":"Premature completion"}).to_string(),
            },
            ProviderEvent::ToolUse {
                id:"mixed-question".into(), name:vega_runtime::USER_QUESTION_TOOL.into(),
                input_json:r#"{"questions":[{"id":"approach","header":"Approach","question":"Which approach?","options":[{"label":"A","description":"Keep it"},{"label":"B","description":"Change it"}]}]}"#.into(),
            },
            ProviderEvent::Usage {input:10,output:2,cache_read:None,cache_write:None},
            ProviderEvent::Done {stop_reason:StopReason::ToolUse},
        ])],
        round(None,true),
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Choose before completing",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallApproved { .. } | ConversationEvent::ToolCallRunning { .. }
    )));
    for id in ["mixed-goal-report", "mixed-question"] {
        let state = tool_calls::find_state(store.conn(), id).unwrap().unwrap();
        assert_eq!(state.status, "rejected");
        assert_eq!(
            state.output_text.as_deref(),
            Some(vega_runtime::USER_QUESTION_BATCH_REJECTED)
        );
        let approval = ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
        assert_eq!(approval.source, ApprovalSource::Validation);
        assert_eq!(approval.decision, Approval::Deny);
    }
    let task = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_eq!(task.status, AutomationStatus::Active);
    assert_eq!(task.used_tokens, 24);
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let next = MockProvider::new_rounds(vec![round(None, true)]);
    let run = run_thread_task(
        &reopened,
        &next,
        &tools,
        "thread-1",
        "Review prior rejection",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(next.requests().len(), 1);
}

fn command(store: &Store, text: &str) {
    automation::command(
        store,
        "thread-1",
        &automation::parse_command(text).unwrap().unwrap(),
        automation::now_ms(),
    )
    .unwrap();
}

fn round(tool: Option<(&str, &str, serde_json::Value)>, tokens: bool) -> Vec<ScriptStep> {
    let mut events = vec![ProviderEvent::TextDelta("Verified current state. ".into())];
    let stop_reason = if let Some((id, name, arguments)) = tool {
        events.push(ProviderEvent::ToolUse {
            id: id.into(),
            name: name.into(),
            input_json: arguments.to_string(),
        });
        StopReason::ToolUse
    } else {
        StopReason::End
    };
    if tokens {
        events.push(ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: Some(5),
            cache_write: Some(3),
        });
    }
    events.push(ProviderEvent::Done { stop_reason });
    vec![ScriptStep::events(events)]
}

#[tokio::test]
async fn goal_tools_use_normal_lifecycle_and_meter_only_owned_provider_rows() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    command(&store, "/goal --budget 100 deliver verified changes");
    let provider = MockProvider::new_rounds(vec![
        round(Some(("goal-read", "get_goal", json!({}))), true),
        round(
            Some((
                "goal-report",
                "update_goal",
                json!({"status":"complete","reason":"Checked every requested artifact and its result"}),
            )),
            true,
        ),
        round(None, true),
    ]);
    let mut inserted_unrelated = false;
    let run = run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Continue the configured goal",
        "System",
        CancellationToken::new(),
        |event| {
            if matches!(event, ConversationEvent::UsageUpdated { .. }) && !inserted_unrelated {
                inserted_unrelated = true;
                vega_store::token_usage::insert(
                    store.conn(),
                    vega_store::token_usage::NewTokenUsage {
                        thread_id: "thread-1",
                        message_id: None,
                        model: "title-model",
                        input_tokens: 400,
                        output_tokens: 100,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                        cost_microcents: 0,
                        created_at: 1,
                        pricing_version: None,
                        pricing_profile: None,
                        call_started_at: None,
                    },
                )
                .unwrap();
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(!run.failed);
    let goal = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_eq!(goal.status, AutomationStatus::Complete);
    assert_eq!(goal.used_tokens, 36);
    assert!(goal.usage_complete);
    let all = vega_store::token_usage::aggregate_by_thread(store.conn(), "thread-1").unwrap();
    assert_eq!(all.input_tokens + all.output_tokens, 536);
    for id in ["goal-read", "goal-report"] {
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == id)));
        assert!(run.events.iter().any(
            |event| matches!(event, ConversationEvent::ToolCallRunning { call_id } if call_id == id)
        ));
        let row: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval FROM tool_calls WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, "success");
        let audit = ApprovalAudit::from_json(&row.1).unwrap();
        assert_eq!(audit.source, ApprovalSource::ReadonlyTool);
        assert_eq!(audit.decision, Approval::Once);
    }
    let read = run
        .events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if call_id == "goal-read" => {
                Some(serde_json::from_str::<serde_json::Value>(&result.output).unwrap())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(read["goal"]["used_tokens"], 12);
    assert_eq!(read["remaining_tokens"], 88);
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains("deliver verified changes"))
    );
    assert!(
        vega_store::automation::active_run(store.conn(), "thread-1")
            .unwrap()
            .is_none()
    );
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let review = MockProvider::new_rounds(vec![round(None, true)]);
    let run = run_thread_task(
        &reopened,
        &review,
        &tools,
        "thread-1",
        "Review the verified result",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(review.requests().len(), 1);
    assert_eq!(
        automation::load(&reopened, "thread-1").unwrap()[0].used_tokens,
        36
    );
}

#[tokio::test]
async fn missing_usage_and_cancelled_started_request_pause_budget_without_false_completion() {
    for cancel_after_report in [false, true] {
        let (store, dir, _) = setup();
        command(&store, "/goal --budget 100 finish");
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            round(
                Some((
                    "report",
                    "update_goal",
                    json!({"status":"complete","reason":"Checked outputs"}),
                )),
                true,
            ),
            round(None, false),
        ]);
        let cancel = CancellationToken::new();
        let cancellation = cancel.clone();
        let run = run_thread_task_with_sink(&store, &provider, &tools, "thread-1", "Continue", "System", cancel, |event| {
            if cancel_after_report && matches!(event, ConversationEvent::ToolCallFinished { call_id, .. } if call_id == "report") {
                cancellation.cancel();
            }
            Ok(())
        }).await.unwrap();
        let goal = automation::load(&store, "thread-1").unwrap().remove(0);
        assert_eq!(goal.used_tokens, 12);
        if cancel_after_report {
            assert!(run.interrupted);
            assert_eq!(goal.status, AutomationStatus::Paused);
            assert!(goal.usage_complete);
        } else {
            assert!(!run.failed);
            assert!(!goal.usage_complete);
            assert_eq!(goal.status, AutomationStatus::Paused);
            assert!(!goal.can_control());
            assert!(goal.reason.as_deref().unwrap().contains("用量"));
        }
        assert!(
            vega_store::automation::active_run(store.conn(), "thread-1")
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn multiple_steered_responses_share_one_goal_ledger() {
    let (store, dir, _) = setup();
    command(&store, "/goal --budget 20 inspect each request");
    crate::pending_messages::enqueue(
        &store,
        &crate::types::PendingMessage {
            id: "steer".into(),
            thread_id: "thread-1".into(),
            generation: "run".into(),
            behavior: crate::types::PendingMessageBehavior::Steer,
            content: "Also check the second item".into(),
            images: Vec::new(),
        },
    )
    .unwrap();
    let source = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "run".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![round(None, true), round(None, true)]);
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "First item",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_pending_input(None, Some(Arc::new(source))),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(provider.requests().len(), 2);
    let goal = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_eq!(goal.used_tokens, 24);
    assert_eq!(goal.status, AutomationStatus::BudgetLimited);
    let assistants: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE role='assistant'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(assistants, 2);
}

#[tokio::test]
async fn stale_preclaimed_entry_releases_ledger_and_never_calls_provider() {
    let (store, dir, _) = setup();
    command(&store, "/goal finish");
    let claim = automation::claim(
        &store,
        "thread-1",
        Some(AutomationKind::Goal),
        automation::now_ms(),
    )
    .unwrap()
    .unwrap();
    command(&store, "/goal pause");
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![round(None, true)]);
    let result = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Continue",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_automation_claim(Some(claim)),
        None,
        None,
        None,
        Vec::new(),
    )
    .await;
    assert!(result.is_err());
    assert!(provider.requests().is_empty());
    assert!(
        vega_store::automation::active_run(store.conn(), "thread-1")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        automation::load(&store, "thread-1").unwrap()[0].status,
        AutomationStatus::Paused
    );
}

#[tokio::test]
async fn loop_capability_is_only_available_on_its_tick_and_ordinary_runs_create_nothing() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let ordinary = MockProvider::new_rounds(vec![round(None, true)]);
    run_thread_task(
        &store,
        &ordinary,
        &tools,
        "thread-1",
        "Hello",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(automation::load(&store, "thread-1").unwrap().is_empty());
    assert!(
        !ordinary.requests()[0]
            .tools
            .iter()
            .any(|tool| tool.name == "get_goal")
    );
    command(&store, "/loop verify a timer");
    let user = MockProvider::new_rounds(vec![round(None, true)]);
    run_thread_task(
        &store,
        &user,
        &tools,
        "thread-1",
        "Ordinary question",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        !user.requests()[0]
            .tools
            .iter()
            .any(|tool| tool.name == "schedule_wakeup")
    );
    assert!(
        !user.requests()[0]
            .messages
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::System
                && message.content.contains("verify a timer"))
    );
    let claim = automation::claim(
        &store,
        "thread-1",
        Some(AutomationKind::Loop),
        automation::now_ms(),
    )
    .unwrap()
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        round(
            Some((
                "wakeup",
                "schedule_wakeup",
                json!({"delay_seconds":60,"noop":true}),
            )),
            true,
        ),
        round(None, true),
    ]);
    run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Run configured loop",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_automation_claim(Some(claim)),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(
        provider.requests()[0]
            .tools
            .iter()
            .any(|tool| tool.name == "schedule_wakeup")
    );
    let loop_task = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_eq!(loop_task.status, AutomationStatus::Active);
    assert!(loop_task.next_due_ms > automation::now_ms());
    automation::command(
        &store,
        "thread-1",
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Clear),
        automation::now_ms(),
    )
    .unwrap();
}

#[tokio::test]
async fn compaction_usage_is_persisted_with_claim_and_unknown_summary_blocks_budget() {
    let (store, dir, project_id) = setup();
    command(&store, "/goal --budget 100 verify accounting");
    let claim = automation::claim(&store, "thread-1", None, automation::now_ms())
        .unwrap()
        .unwrap();
    let binding = Arc::new(automation::AutomationBinding::new(
        store.database_path().unwrap().into(),
        claim.clone(),
    ));
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) VALUES ('assistant','thread-1',1,'assistant','text','','streaming',1)", []).unwrap();
    let actor = PersistenceActor::start(PersistenceActorStart {
        database_path: dir.path().join("vega.db"),
        project_id,
        thread_id: "thread-1".into(),
        message_id: "assistant".into(),
        model: "mock-model".into(),
        is_plan: false,
        next_tool_seq: 1,
        config: PersistenceActorConfig {
            automation_claim: Some(claim.clone()),
            automation_hook: Some(binding.clone()),
            ..Default::default()
        },
    })
    .await
    .unwrap();
    actor
        .event(
            RuntimeEvent::ContextCompactionUsageUpdated {
                usage: vega_runtime::ContextCompactionUsage {
                    usage: vega_runtime::RuntimeTokenUsage {
                        input: 7,
                        output: 3,
                        cache_read: Some(4),
                        cache_write: None,
                    },
                    cost_microcents: 0,
                    pricing: None,
                },
            },
            String::new(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::ContextCompactionStatusUpdated {
                status: vega_runtime::ContextCompactionStatusUpdate {
                    operation_key: "summary".into(),
                    generation: 1,
                    phase: vega_runtime::ContextCompactionPhase::Failed,
                    source_version: 1,
                    estimated_tokens: 40000,
                    input_budget: 49000,
                    target_tokens: 29400,
                    usage: vega_runtime::ContextCompactionUsageState::Unknown,
                    failure: Some(vega_runtime::ContextCompactionStatusFailure::Cancelled),
                },
            },
            String::new(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::UsageUpdated {
                usage: vega_runtime::RuntimeTokenUsage {
                    input: 10,
                    output: 2,
                    cache_read: None,
                    cache_write: None,
                },
                cost_microcents: 0,
                pricing: None,
            },
            String::new(),
            None,
        )
        .await
        .unwrap();
    actor.close().await.unwrap();
    assert!(!binding.usage_complete());
    automation::finish(
        &store,
        &claim,
        true,
        binding.usage_complete(),
        10,
        automation::now_ms(),
    )
    .unwrap();
    let goal = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_eq!(goal.used_tokens, 22);
    assert_eq!(goal.status, AutomationStatus::Paused);
    assert!(!goal.can_control());
    let rows: i64 = store
        .conn()
        .query_row("SELECT count(*) FROM token_usage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 2);
    automation::recover_startup(&store).unwrap();
    assert_eq!(
        automation::load(&store, "thread-1").unwrap()[0].used_tokens,
        22
    );
}

#[test]
fn replay_capability_fence_includes_goal_identity_revision_and_instruction() {
    let (store, dir, project_id) = setup();
    command(&store, "/goal first instruction");
    let claim = automation::claim(&store, "thread-1", None, automation::now_ms())
        .unwrap()
        .unwrap();
    let config = RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        project_id,
        "thread-1".into(),
        dir.path().join("checkpoints"),
        Vec::new(),
    );
    let digest = |claim: crate::types::AutomationRunClaim| {
        config
            .clone()
            .with_automation(Some(Arc::new(automation::AutomationBinding::new(
                store.database_path().unwrap().into(),
                claim,
            ))))
            .replay_capability_digest()
            .unwrap()
    };
    let original = digest(claim.clone());
    for field in 0..3 {
        let mut changed = claim.clone();
        match field {
            0 => changed.tasks[0].identity = "new-identity".into(),
            1 => changed.tasks[0].revision += 1,
            _ => changed.tasks[0].instruction = "second instruction".into(),
        }
        assert_ne!(original, digest(changed));
    }
}

#[tokio::test]
async fn automatic_title_provider_usage_is_separate_from_the_goal_run() {
    let (store, dir, _) = setup();
    command(&store, "/goal verify title isolation");
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let primary = MockProvider::new_rounds(vec![round(None, true)]);
    let title = Arc::new(MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Verified title".into()),
        ProviderEvent::Usage {
            input: 40,
            output: 5,
            cache_read: None,
            cache_write: None,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]));
    let (sender, _receiver) = std::sync::mpsc::channel();
    run_thread_task_with_images_and_reasoning(
        &store,
        &primary,
        &tools,
        "thread-1",
        "Inspect the title",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_automatic_title(Some(
            crate::types::AutomaticTitleRequest::new(
                "Inspect the title",
                title.clone(),
                CancellationToken::new(),
                sender,
            ),
        )),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    for _ in 0..200 {
        let total = vega_store::token_usage::aggregate_by_thread(store.conn(), "thread-1").unwrap();
        if total.row_count == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(title.requests().len(), 1);
    let total = vega_store::token_usage::aggregate_by_thread(store.conn(), "thread-1").unwrap();
    assert_eq!(total.input_tokens + total.output_tokens, 57);
    assert_eq!(
        automation::load(&store, "thread-1").unwrap()[0].used_tokens,
        12
    );
}

#[tokio::test]
async fn automation_report_credential_is_rejected_before_durable_tool_input_or_goal_report() {
    let (store, dir, _) = setup();
    command(&store, "/goal inspect safely");
    let secret = "synthetic-automation-credential-value";
    let provider = MockProvider::new_rounds(vec![
        round(
            Some((
                "report-secret",
                "update_goal",
                json!({"status":"complete","reason":secret}),
            )),
            true,
        ),
        round(None, true),
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let reader: vega_runtime::CredentialReader = Arc::new(move || Ok(vec![secret.into()]));
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Inspect safely",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_owner_credential_reader(reader),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(run.events.iter().any(|event|matches!(event,ConversationEvent::ToolCallFinished {call_id,result} if call_id=="report-secret" && result.status==ToolCallStatus::Rejected)));
    let row: (String, String) = store
        .conn()
        .query_row(
            "SELECT input_json,output_text FROM tool_calls WHERE id='report-secret'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(!row.0.contains(secret));
    assert!(!row.1.contains(secret));
    let task = automation::load(&store, "thread-1").unwrap().remove(0);
    assert_ne!(task.status, AutomationStatus::Complete);
    assert!(!task.reason.as_deref().unwrap_or_default().contains(secret));
}

#[tokio::test]
async fn failed_wakeup_tool_history_reopens_and_runs_again_with_strict_audit_validation() {
    let (store, dir, _) = setup();
    command(&store, "/loop 1m inspect");
    let claim = automation::claim(
        &store,
        "thread-1",
        Some(AutomationKind::Loop),
        automation::now_ms(),
    )
    .unwrap()
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let invalid = MockProvider::new_rounds(vec![
        round(
            Some((
                "invalid-wakeup",
                "schedule_wakeup",
                json!({"delay_seconds":60}),
            )),
            true,
        ),
        round(None, true),
    ]);
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &invalid,
        &tools,
        "thread-1",
        "Run the configured loop",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_automation_claim(Some(claim)),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(run.events.iter().any(|event|matches!(event,ConversationEvent::ToolCallFinished {call_id,result} if call_id=="invalid-wakeup" && result.status==ToolCallStatus::Failed && result.output.starts_with("Tool error: tool 'automation' failed:"))));
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let next = MockProvider::new_rounds(vec![round(None, true)]);
    let run = run_thread_task(
        &reopened,
        &next,
        &tools,
        "thread-1",
        "Review the prior result",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(next.requests().len(), 1);
    assert!(
        next.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains("固定间隔"))
    );
}
