use super::*;
use crate::types::{SubagentAdmission, SubagentHost, SubagentSnapshot, SubagentState};
use std::sync::atomic::AtomicBool;
use tokio::sync::{Notify, Semaphore};
use vega_runtime::{ChatRequest, ChatRole, EventStream};

#[derive(Default)]
struct Host {
    agents: Mutex<Vec<SubagentSnapshot>>,
    events: Mutex<Vec<(String, ConversationEvent)>>,
    cancels: Mutex<Vec<CancellationToken>>,
    changed: Notify,
    reject: AtomicBool,
    hold_admission: AtomicBool,
}

impl SubagentHost for Host {
    fn admit(&self, admission: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        let cancel = admission.cancel.clone();
        self.cancels.lock().unwrap().push(admission.cancel);
        self.agents.lock().unwrap().push(admission.snapshot);
        let reject = self.reject.load(Ordering::SeqCst);
        let hold = self.hold_admission.load(Ordering::SeqCst);
        self.changed.notify_one();
        async move {
            if hold {
                cancel.cancelled().await;
                Err(VegaError::Cancelled)
            } else if reject {
                Err(VegaError::Cancelled)
            } else {
                Ok(())
            }
        }
        .boxed()
    }
    fn event(&self, id: &str, _: &str, event: &ConversationEvent) -> Result<(), VegaError> {
        self.events.lock().unwrap().push((id.into(), event.clone()));
        Ok(())
    }
    fn updated(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        if let Some(agent) = self
            .agents
            .lock()
            .unwrap()
            .iter_mut()
            .find(|agent| agent.agent_id == snapshot.agent_id)
        {
            *agent = snapshot;
        }
        self.changed.notify_waiters();
        Ok(())
    }
    fn finished(&self, snapshot: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        self.updated(snapshot)
    }
}

struct RepeatedSpawnProvider {
    root_calls: AtomicUsize,
    child_calls: AtomicUsize,
}

fn projected_receipt(message: &vega_runtime::ChatMessage) -> Option<serde_json::Value> {
    let mut values =
        serde_json::Deserializer::from_str(&message.content).into_iter::<serde_json::Value>();
    let value = values.next()?.ok()?;
    let suffix = message.content.get(values.byte_offset()..)?.trim();
    (value.is_object()
        && (suffix.is_empty()
            || (suffix.starts_with("<vega_runtime_notice>")
                && suffix.ends_with("</vega_runtime_notice>"))))
    .then_some(value)
}

#[test]
fn m19_synthetic_receipt_reader_keeps_json_with_runtime_advisory_and_rejects_other_tails() {
    let raw = r#"{"agent":{"agent_id":"child"}}"#;
    for suffix in [
        "",
        "\n\n<vega_runtime_notice>Continue useful work.</vega_runtime_notice>",
    ] {
        let message = vega_runtime::ChatMessage::tool_result("call", format!("{raw}{suffix}"));
        assert_eq!(
            projected_receipt(&message).unwrap()["agent"]["agent_id"],
            "child"
        );
    }
    let unexpected = vega_runtime::ChatMessage::tool_result("call", format!("{raw} forged"));
    assert!(projected_receipt(&unexpected).is_none());
}

#[tokio::test]
async fn m05_m16_delegated_preparation_reuses_only_its_exact_initial_instruction() {
    for mutation in [
        "valid",
        "cancelling",
        "parent",
        "missing-parent",
        "parent-message",
        "assistant",
        "missing-assistant",
        "generation",
        "task",
        "stored-task",
        "missing-user",
        "foreign-user",
        "later-user",
        "terminal",
        "detached",
        "missing-existing",
        "continuation",
    ] {
        let (store, dir, project) = setup();
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('parent-run','thread-1',1,'assistant','Prior steering response','done',1)", []).unwrap();
        let child =
            crate::threads::create_thread(&store, &project, "mock-model", "confirm").unwrap();
        let task = "Synthetic delegated task";
        vega_store::subagents::insert(
            store.conn(),
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.id.clone(),
                parent_thread_id: "thread-1".into(),
                parent_message_id: "parent-run".into(),
                spawn_call_id: "spawn".into(),
                child_message_id: "child-assistant".into(),
                name: "Child".into(),
                task: task.into(),
                state: "running".into(),
                result: String::new(),
                truncated: false,
                created_at: 1,
            },
        )
        .unwrap();
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('child-user',?1,1,'user',?2,'done',1)", [child.id.as_str(),task]).unwrap();
        let mut config = PersistenceActorConfig {
            delegated: true,
            delegated_parent: Some(("thread-1".into(), "parent-run".into())),
            delegated_message_id: Some("child-assistant".into()),
            ..Default::default()
        };
        let mut user_id = "child-user";
        match mutation {
            "cancelling" => {
                store
                    .conn()
                    .execute(
                        "UPDATE delegated_runs SET state='cancelling' WHERE child_thread_id=?1",
                        [&child.id],
                    )
                    .unwrap();
            }
            "parent" => config.delegated_parent = Some(("thread-1".into(), "other-run".into())),
            "missing-parent" => config.delegated_parent = None,
            "parent-message" => {
                store
                    .conn()
                    .execute("UPDATE messages SET role='user' WHERE id='parent-run'", [])
                    .unwrap();
            }
            "assistant" => config.delegated_message_id = Some("other-assistant".into()),
            "missing-assistant" => config.delegated_message_id = None,
            "generation" => {
                store.conn().execute("UPDATE delegated_runs SET child_message_id='other-generation' WHERE child_thread_id=?1", [&child.id]).unwrap();
            }
            "stored-task" => {
                store
                    .conn()
                    .execute(
                        "UPDATE messages SET content='Changed stored task' WHERE id='child-user'",
                        [],
                    )
                    .unwrap();
            }
            "missing-user" => user_id = "missing-user",
            "foreign-user" => {
                user_id = "foreign-user";
                store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('foreign-user','thread-1',2,'user',?1,'done',1)", [task]).unwrap();
            }
            "later-user" => {
                user_id = "later-user";
                store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('later-user',?1,2,'user',?2,'done',1)", [child.id.as_str(),task]).unwrap();
            }
            "terminal" => {
                store
                    .conn()
                    .execute(
                        "UPDATE delegated_runs SET state='completed' WHERE child_thread_id=?1",
                        [&child.id],
                    )
                    .unwrap();
            }
            "detached" => {
                store
                    .conn()
                    .execute(
                        "UPDATE threads SET parent_thread_id=NULL WHERE id=?1",
                        [&child.id],
                    )
                    .unwrap();
            }
            "continuation" => {
                config.credential_continuation_id = Some("foreign-continuation".into())
            }
            _ => {}
        }
        let fingerprint =
            vega_store::credential_privacy::source_fingerprint(store.conn(), &child.id).unwrap();
        let parent_fingerprint =
            vega_store::credential_privacy::source_fingerprint(store.conn(), "thread-1").unwrap();
        let before_thread = vega_store::threads::find(store.conn(), &child.id)
            .unwrap()
            .unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Child complete".into()),
            usage(2),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let persisted_user = (mutation != "missing-existing").then(|| user_id.into());
        let content = if mutation == "task" {
            "Changed task"
        } else {
            task
        };
        let result = entry::run_thread_task_with_automation_inner(
            &store,
            &provider,
            &tools,
            &child.id,
            content,
            "System",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            config.clone(),
            persisted_user,
            None,
            None,
            Vec::new(),
            Vec::new(),
        )
        .await;
        if matches!(mutation, "valid" | "cancelling") {
            let run = result.unwrap();
            assert!(!run.failed);
            assert_eq!(run.assistant_message_id, "child-assistant");
            let requests = provider.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0]
                    .messages
                    .iter()
                    .filter(|message| message.role == ChatRole::User && message.content == task)
                    .count(),
                1
            );
            let assistant = vega_store::messages::find(store.conn(), "child-assistant")
                .unwrap()
                .unwrap();
            assert_eq!(assistant.seq, 2);
            assert_eq!(assistant.status, "done");
            let after_first =
                vega_store::credential_privacy::source_fingerprint(store.conn(), &child.id)
                    .unwrap();
            assert!(
                entry::run_thread_task_with_automation_inner(
                    &store,
                    &provider,
                    &tools,
                    &child.id,
                    task,
                    "System",
                    CancellationToken::new(),
                    &RejectPermissionHook,
                    |_| Ok(()),
                    config,
                    Some(user_id.into()),
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                )
                .await
                .is_err()
            );
            assert_eq!(provider.requests().len(), 1);
            assert_eq!(
                vega_store::credential_privacy::source_fingerprint(store.conn(), &child.id)
                    .unwrap(),
                after_first
            );
        } else {
            assert!(result.is_err(), "mutation {mutation}");
            assert!(provider.requests().is_empty(), "mutation {mutation}");
            assert_eq!(
                vega_store::credential_privacy::source_fingerprint(store.conn(), &child.id)
                    .unwrap(),
                fingerprint,
                "mutation {mutation}"
            );
            assert_eq!(
                vega_store::credential_privacy::source_fingerprint(store.conn(), "thread-1")
                    .unwrap(),
                parent_fingerprint,
                "mutation {mutation}"
            );
            let after_thread = vega_store::threads::find(store.conn(), &child.id)
                .unwrap()
                .unwrap();
            assert_eq!(
                after_thread.parent_thread_id,
                before_thread.parent_thread_id
            );
            assert_eq!(after_thread.updated_at, before_thread.updated_at);
        }
        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='user'",
                [&child.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, if mutation == "later-user" { 2 } else { 1 });
    }
}

#[test]
fn m04_m19_subagent_proposals_and_approvals_accept_only_exact_authority_and_valid_input() {
    use vega_runtime::{RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource};

    let approval = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        source: RuntimeApprovalSource::ReadonlyTool,
        note: None,
        danger: None,
    };
    let state = |tool: &str, input: &str| tool_calls::ToolCallState {
        thread_id: "thread-1".into(),
        tool: tool.into(),
        input_json: input.into(),
        status: "pending_approval".into(),
        approval: None,
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    };
    for (tool, input) in [
        (
            "spawn_agent",
            r#"{"name":"Research","task":"Find evidence"}"#,
        ),
        ("list_agents", "{}"),
        ("wait_agent", r#"{"agent_ids":["child"],"timeout_ms":0}"#),
        ("cancel_agent", r#"{"agent_id":"child"}"#),
    ] {
        let call = vega_runtime::RuntimeToolCall {
            id: "call".into(),
            name: tool.into(),
            input_json: input.into(),
        };
        assert!(validate_runtime_proposal(&call).is_ok());
        let state = state(tool, input);
        assert!(validate_runtime_approval_event(&state, "call", &approval, None).is_ok());
        for altered in [
            RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Always,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                source: RuntimeApprovalSource::User,
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                note: Some("untrusted note".into()),
                ..approval.clone()
            },
            RuntimeApprovalAudit {
                danger: Some(vega_runtime::RuntimeDangerAudit {
                    rule_id: "synthetic".into(),
                    decision: RuntimeApprovalDecision::Once,
                    note: None,
                }),
                ..approval.clone()
            },
        ] {
            assert!(validate_runtime_approval_event(&state, "call", &altered, None).is_err());
        }
        let remember = vega_runtime::RuntimePermissionTarget {
            call_id: "call".into(),
            tool: RuntimeMutatingTool::Write,
            display_target: "/synthetic".into(),
            exact_pattern: "/synthetic".into(),
        };
        assert!(
            validate_runtime_approval_event(&state, "call", &approval, Some(&remember)).is_err()
        );
    }
    for (tool, input) in [
        ("spawn_agent", "{}"),
        ("wait_agent", "{}"),
        ("cancel_agent", "{}"),
        ("spawn_agent", r#"{"name":"Research"}"#),
        (
            "spawn_agent",
            r#"{"name":"Research","task":"Find","role":"admin"}"#,
        ),
        ("list_agents", r#"{"extra":true}"#),
        ("wait_agent", r#"{"agent_ids":[]}"#),
        ("cancel_agent", r#"{"agent_id":""}"#),
        ("spawn_agent", "[]"),
    ] {
        let call = vega_runtime::RuntimeToolCall {
            id: "invalid".into(),
            name: tool.into(),
            input_json: input.into(),
        };
        assert_eq!(validate_runtime_proposal(&call).is_ok(), input == "{}");
        assert!(
            validate_runtime_approval_event(&state(tool, input), "invalid", &approval, None)
                .is_err()
        );
    }
    let unknown = vega_runtime::RuntimeToolCall {
        id: "unknown".into(),
        name: "unknown_agent".into(),
        input_json: r#"{"name":"Research","task":"Find"}"#.into(),
    };
    assert!(validate_runtime_proposal(&unknown).is_err());
    assert!(
        validate_runtime_approval_event(&state("unknown_agent", "{}"), "unknown", &approval, None)
            .is_err()
    );
}

struct ReaderFailureHook(Arc<AtomicBool>);

impl vega_runtime::RuntimeSubagentHook for ReaderFailureHook {
    fn call(
        &self,
        _: String,
        _: String,
        _: serde_json::Value,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        let unavailable = self.0.clone();
        async move {
            unavailable.store(true, Ordering::SeqCst);
            Ok(serde_json::json!({"agents":[]}))
        }
        .boxed()
    }
}

#[tokio::test]
async fn m19_hook_completion_reader_failure_persists_a_recoverable_failed_terminal() {
    let (store, dir, project) = setup();
    let mut prepared = prepare_run_with_images_and_reasoning(
        store.database_path().unwrap().to_path_buf(),
        "thread-1".into(),
        "Inspect delegated work".into(),
        "System".into(),
        "reader-user".into(),
        "reader-assistant".into(),
        PersistenceActorConfig::default(),
        false,
        None,
        None,
        Vec::new(),
    )
    .unwrap();
    let unavailable = Arc::new(AtomicBool::new(false));
    let reader_state = unavailable.clone();
    prepared.request.tool_config = prepared
        .request
        .tool_config
        .with_subagents(Some(Arc::new(ReaderFailureHook(unavailable.clone()))))
        .with_credential_reader(Arc::new(move || {
            if reader_state.load(Ordering::SeqCst) {
                Err(())
            } else {
                Ok(Vec::new())
            }
        }))
        .with_turn_limit(1);
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "reader-tool".into(),
            name: "list_agents".into(),
            input_json: "{}".into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let mut sequence = prepared.next_tool_seq;
    let mut terminals = Vec::new();
    vega_runtime::run_agent_with_sink(
        &provider,
        &tools,
        prepared.request,
        CancellationToken::new(),
        |event| {
            let result = if matches!(
                event,
                RuntimeEvent::ToolCallProposed(_)
                    | RuntimeEvent::ToolCallApproved { .. }
                    | RuntimeEvent::ToolCallRunning { .. }
                    | RuntimeEvent::ToolCallFinished(_)
            ) {
                persist_runtime_event(
                    &store,
                    &project,
                    "thread-1",
                    "reader-assistant",
                    "mock-model",
                    false,
                    "",
                    &mut sequence,
                    &event,
                    None,
                )
            } else {
                Ok(())
            };
            if let RuntimeEvent::ToolCallFinished(terminal) = event {
                terminals.push(terminal);
            }
            async move { result }
        },
    )
    .await
    .unwrap();
    assert!(unavailable.load(Ordering::SeqCst));
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].status, RuntimeToolStatus::Failed);
    assert!(
        terminals[0]
            .output
            .starts_with("Tool error: tool 'subagent' failed:")
    );
    assert!(terminals[0].truncated.is_none());
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let terminal = tool_calls::terminal_results(reopened.conn(), "thread-1")
        .unwrap()
        .remove("reader-tool")
        .unwrap();
    assert_eq!(terminal.status, "failed");
    assert_eq!(terminal.output, terminals[0].output);
    let approval = ApprovalAudit::from_json(&terminal.approval).unwrap();
    assert_eq!(
        validate_recovered_projection(
            &project,
            "thread-1",
            "reader-tool",
            &terminal.tool,
            &terminal.input_json,
            &terminal.output,
            RuntimeToolStatus::Failed,
            &approval,
            terminal.exit_code,
            terminal.duration_ms
        )
        .unwrap(),
        "{}"
    );
}

#[tokio::test]
async fn m04_escaped_owner_secret_never_creates_child_metadata_or_provider_work() {
    let (store, dir, _) = setup();
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let encoded = "fixture-owned-secret"
        .bytes()
        .map(|byte| format!("\\u{byte:04x}"))
        .collect::<String>();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "encoded-spawn".into(),
                name: "spawn_agent".into(),
                input_json: format!("{{\"name\":\"A\",\"task\":\"{encoded}\"}}"),
            },
            usage(1),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Rejected unsafe input".into()),
            usage(2),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let config = PersistenceActorConfig::default()
        .with_subagent_host(host)
        .with_owner_credential_reader(Arc::new(|| Ok(vec!["fixture-owned-secret".into()])));
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(provider.requests().len(), 2);
    assert!(
        crate::subagents::children(&store, "thread-1")
            .unwrap()
            .is_empty()
    );
    let audit = vega_store::tool_calls::find_state(store.conn(), "encoded-spawn")
        .unwrap()
        .unwrap();
    assert_eq!(audit.input_json, "{}");
    assert_eq!(audit.status, "rejected");
}

impl Provider for RepeatedSpawnProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let parent = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::User)
            .is_some_and(|message| message.content == "parent");
        let events = if parent {
            let round = self.root_calls.fetch_add(1, Ordering::SeqCst);
            if round < 2 {
                vec![
                    ProviderEvent::ToolUse {
                        id: "repeated-spawn".into(),
                        name: "spawn_agent".into(),
                        input_json: r#"{"name":"Only child","task":"child-a"}"#.into(),
                    },
                    usage(11),
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                let values = request
                    .messages
                    .iter()
                    .filter(|message| message.role == ChatRole::Tool)
                    .filter_map(projected_receipt)
                    .collect::<Vec<_>>();
                let ids = values
                    .iter()
                    .filter_map(|value| {
                        value
                            .pointer("/agent/agent_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let done = values
                    .iter()
                    .rev()
                    .find_map(|value| value["agents"].as_array())
                    .is_some_and(|agents| {
                        agents.iter().all(|agent| {
                            matches!(
                                agent["state"].as_str(),
                                Some("completed" | "failed" | "interrupted")
                            )
                        })
                    });
                if ids.is_empty() || done {
                    vec![
                        ProviderEvent::TextDelta("Finished".into()),
                        usage(12),
                        ProviderEvent::Done {
                            stop_reason: StopReason::End,
                        },
                    ]
                } else {
                    vec![
                        ProviderEvent::ToolUse {
                            id: format!("wait-{round}"),
                            name: "wait_agent".into(),
                            input_json: serde_json::json!({"agent_ids":ids,"timeout_ms":60000})
                                .to_string(),
                        },
                        usage(12),
                        ProviderEvent::Done {
                            stop_reason: StopReason::ToolUse,
                        },
                    ]
                }
            }
        } else {
            self.child_calls.fetch_add(1, Ordering::SeqCst);
            vec![
                ProviderEvent::TextDelta("Child result".into()),
                usage(100),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]
        };
        async move { Ok(stream(events)) }.boxed()
    }
}

#[tokio::test]
async fn m16_repeated_completed_spawn_is_reused_and_creation_failure_rolls_back_reservation() {
    for reject_storage in [false, true] {
        let (store, dir, _) = setup();
        if reject_storage {
            store.conn().execute_batch("CREATE TRIGGER reject_delegation BEFORE INSERT ON delegated_runs BEGIN SELECT RAISE(ABORT,'synthetic delegation storage failure'); END;").unwrap();
        }
        let provider = RepeatedSpawnProvider {
            root_calls: AtomicUsize::new(0),
            child_calls: AtomicUsize::new(0),
        };
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let host = Arc::new(Host::default());
        let run = run_thread_task_with_pricing(
            &store,
            &provider,
            &tools,
            "thread-1",
            "parent",
            "System",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            PersistenceActorConfig::default().with_subagent_host(host),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!run.failed);
        assert_eq!(
            provider.child_calls.load(Ordering::SeqCst),
            usize::from(!reject_storage)
        );
        let children = crate::subagents::children(&store, "thread-1").unwrap();
        assert_eq!(children.len(), usize::from(!reject_storage));
        assert!(run.events.iter().any(|event| matches!(event,ConversationEvent::ToolCallFinished {call_id,result} if call_id == "repeated-spawn" && result.reused)));
        if reject_storage {
            assert_eq!(
                store
                    .conn()
                    .query_row("SELECT COUNT(*) FROM threads", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}

#[tokio::test]
async fn m07_m08_m16_stop_while_admission_ack_is_pending_never_starts_provider_or_reports_spawn_success()
 {
    let (store, dir, _) = setup();
    let provider = RepeatedSpawnProvider {
        root_calls: AtomicUsize::new(0),
        child_calls: AtomicUsize::new(0),
    };
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let host = Arc::new(Host::default());
    host.hold_admission.store(true, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let root_cancel = cancel.clone();
    let waiting = host.clone();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        cancel,
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host),
        None,
        None,
    );
    let stop = async move {
        waiting.changed.notified().await;
        assert_eq!(waiting.agents.lock().unwrap().len(), 1);
        root_cancel.cancel();
    };
    let (run, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(run, stop) })
            .await
            .unwrap();
    let run = run.unwrap();
    assert!(run.interrupted);
    assert_eq!(provider.root_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.child_calls.load(Ordering::SeqCst), 0);
    assert!(!run.events.iter().any(|event| matches!(event,ConversationEvent::ToolCallFinished {call_id,result} if call_id == "repeated-spawn" && result.status == ToolCallStatus::Success)));
    let children = crate::subagents::children(&store, "thread-1").unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].state, SubagentState::Interrupted);
    assert_eq!(
        vega_store::subagents::active_count(
            store.conn(),
            "thread-1",
            &children[0].parent_message_id
        )
        .unwrap(),
        0
    );
}

struct ParallelProvider {
    root_calls: AtomicUsize,
    child_calls: AtomicUsize,
    entered: Arc<Semaphore>,
    root_waiting: Arc<Semaphore>,
    release: Arc<Semaphore>,
    fail_second: bool,
    finish_early: bool,
    missing_child_cache: bool,
    parent_failure_gate: Option<Arc<Semaphore>>,
}

impl ParallelProvider {
    fn new() -> Self {
        Self {
            root_calls: AtomicUsize::new(0),
            child_calls: AtomicUsize::new(0),
            entered: Arc::new(Semaphore::new(0)),
            root_waiting: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
            fail_second: false,
            finish_early: false,
            missing_child_cache: false,
            parent_failure_gate: None,
        }
    }
}

fn stream(events: Vec<ProviderEvent>) -> EventStream {
    Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
}

fn usage(input: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output: 3,
        cache_read: Some(0),
        cache_write: Some(0),
    }
}

fn pricing() -> vega_token::PricingCatalog {
    vega_token::PricingCatalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "mock-model".into(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".into(),
            output_usd_per_million: "2".into(),
            cache_read_usd_per_million: "0.1".into(),
            cache_write_usd_per_million: "0".into(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }])
    .unwrap()
}

impl Provider for ParallelProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let user = request
            .messages
            .iter()
            .rev()
            .find(|message| {
                message.role == ChatRole::User
                    && matches!(message.content.as_str(), "parent" | "child-a" | "child-b")
            })
            .unwrap()
            .content
            .clone();
        if user == "parent" {
            let round = self.root_calls.fetch_add(1, Ordering::SeqCst);
            if round == 1
                && let Some(gate) = &self.parent_failure_gate
            {
                self.root_waiting.add_permits(1);
                let gate = gate.clone();
                return async move {
                    tokio::select! {biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), permit = gate.acquire() => permit.unwrap().forget()}
                    Err(VegaError::Provider {status:Some(400),message:"Synthetic parent failure after child admission".into(),retryable:false})
                }.boxed();
            }
            let events = match round {
                0 => vec![
                    ProviderEvent::ToolUse {
                        id: "spawn-a".into(),
                        name: "spawn_agent".into(),
                        input_json: r#"{"name":"A","task":"child-a"}"#.into(),
                    },
                    ProviderEvent::ToolUse {
                        id: "spawn-b".into(),
                        name: "spawn_agent".into(),
                        input_json: r#"{"name":"B","task":"child-b"}"#.into(),
                    },
                    usage(11),
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                1 if !self.finish_early => {
                    self.root_waiting.add_permits(1);
                    let ids = request
                        .messages
                        .iter()
                        .filter(|message| message.role == ChatRole::Tool)
                        .filter_map(projected_receipt)
                        .filter_map(|value| {
                            value
                                .pointer("/agent/agent_id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(ids.len(), 2);
                    vec![
                        ProviderEvent::ToolUse {
                            id: "wait-both".into(),
                            name: "wait_agent".into(),
                            input_json: serde_json::json!({"agent_ids":ids,"timeout_ms":60000})
                                .to_string(),
                        },
                        usage(12),
                        ProviderEvent::Done {
                            stop_reason: StopReason::ToolUse,
                        },
                    ]
                }
                _ if !self.finish_early => {
                    let pending = request
                        .messages
                        .iter()
                        .rev()
                        .filter(|message| message.role == ChatRole::Tool)
                        .find_map(|message| {
                            projected_receipt(message).filter(|value| value.get("agents").is_some())
                        })
                        .and_then(|value| {
                            value
                                .get("agents")
                                .and_then(serde_json::Value::as_array)
                                .cloned()
                        })
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|agent| {
                            matches!(agent["state"].as_str(), Some("running" | "cancelling"))
                        })
                        .filter_map(|agent| agent["agent_id"].as_str().map(str::to_owned))
                        .collect::<Vec<_>>();
                    if pending.is_empty() {
                        vec![
                            ProviderEvent::TextDelta("Aggregated".into()),
                            usage(13),
                            ProviderEvent::Done {
                                stop_reason: StopReason::End,
                            },
                        ]
                    } else {
                        vec![
                            ProviderEvent::ToolUse {
                                id: format!("wait-again-{round}"),
                                name: "wait_agent".into(),
                                input_json:
                                    serde_json::json!({"agent_ids":pending,"timeout_ms":60000})
                                        .to_string(),
                            },
                            usage(12),
                            ProviderEvent::Done {
                                stop_reason: StopReason::ToolUse,
                            },
                        ]
                    }
                }
                _ => vec![
                    ProviderEvent::TextDelta("Aggregated".into()),
                    usage(13),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ],
            };
            return async move { Ok(stream(events)) }.boxed();
        }
        assert!(matches!(user.as_str(), "child-a" | "child-b"));
        assert_eq!(request.model, "mock-model");
        assert!(
            !request
                .messages
                .iter()
                .any(|message| message.content == "parent")
        );
        assert!(
            !request
                .tools
                .iter()
                .any(|tool| tool.name == "spawn_agent" || tool.name.starts_with("Computer"))
        );
        self.child_calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        let release = self.release.clone();
        let fail = self.fail_second && user == "child-b";
        let missing_cache = self.missing_child_cache;
        async move {
            tokio::select! {biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), permit = release.acquire() => permit.unwrap().forget()}
            if fail {return Err(VegaError::Provider {status:None,message:"Synthetic child failure".into(),retryable:false});}
            Ok(stream(vec![ProviderEvent::ThinkingDelta(format!("Thinking {user}")),ProviderEvent::TextDelta(format!("Result {user}")),if missing_cache {ProviderEvent::Usage {input:100,output:3,cache_read:None,cache_write:None}} else {usage(100)},ProviderEvent::Done {stop_reason:StopReason::End}]))
        }.boxed()
    }
}

#[tokio::test]
async fn m01_m02_m05_m17_parallel_children_enter_before_release_and_persist_separately() {
    let (store, dir, _) = setup();
    crate::automation::command(
        &store,
        "thread-1",
        &crate::automation::parse_command("/goal --budget 200 Delegate work")
            .unwrap()
            .unwrap(),
        crate::automation::now_ms(),
    )
    .unwrap();
    let provider = ParallelProvider::new();
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let config = PersistenceActorConfig::default().with_subagent_host(host.clone());
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        Some(pricing()),
    );
    let drive = async {
        provider.entered.acquire_many(2).await.unwrap().forget();
        provider.root_waiting.acquire().await.unwrap().forget();
        assert_eq!(provider.child_calls.load(Ordering::SeqCst), 2);
        assert!(
            host.agents
                .lock()
                .unwrap()
                .iter()
                .all(|agent| agent.state == SubagentState::Running)
        );
        provider.release.add_permits(2);
    };
    let (run, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(run, drive) })
            .await
            .unwrap();
    let run = run.unwrap();
    assert!(!run.failed);
    assert_eq!(run.content, "Aggregated");
    let children = crate::subagents::children(&store, "thread-1").unwrap();
    assert_eq!(children.len(), 2);
    assert!(
        children
            .iter()
            .all(|agent| agent.state == SubagentState::Completed
                && agent.result.starts_with("Result child-"))
    );
    for child in &children {
        let page = crate::history::latest_history_page(&store, &child.agent_id, 20).unwrap();
        assert!(page.entries.iter().any(|entry| matches!(entry,crate::history::HistoryEntry::AssistantText {content,..} if content.starts_with("Result child-"))));
    }
    let own = vega_store::token_usage::aggregate_by_thread(store.conn(), "thread-1").unwrap();
    let combined =
        vega_store::token_usage::aggregate_task_by_thread(store.conn(), "thread-1").unwrap();
    assert_eq!(combined.input_tokens, own.input_tokens + 200);
    assert_eq!(combined.output_tokens, own.output_tokens + 6);
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::UsageUpdated { .. }))
            .count(),
        provider.root_calls.load(Ordering::SeqCst)
    );
    assert_eq!(
        host.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, event)| matches!(event, ConversationEvent::UsageUpdated { .. }))
            .count(),
        2
    );
    let summary =
        crate::summary::task_cost_summary(&store, "thread-1", &run.assistant_message_id, None)
            .unwrap();
    assert_eq!(summary.usage.unwrap().input, combined.input_tokens);
    assert_eq!(summary.usage.unwrap().cache_read, Some(0));
    assert!(
        matches!(summary.cost,crate::types::SummaryCost::Priced(crate::types::Microcents(total)) if total > 0)
    );
    let goal = crate::automation::load(&store, "thread-1")
        .unwrap()
        .remove(0);
    assert_eq!(
        goal.used_tokens,
        combined.input_tokens + combined.output_tokens
    );
    assert_eq!(goal.status, crate::types::AutomationStatus::BudgetLimited);
    assert!(goal.usage_complete);
    let child = &children[0];
    let later = vega_store::token_usage::insert(
        store.conn(),
        vega_store::token_usage::NewTokenUsage {
            thread_id: &child.agent_id,
            message_id: None,
            model: "mock-model",
            input_tokens: 500,
            output_tokens: 50,
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
    assert!(later > 0);
    let after =
        crate::summary::task_cost_summary(&store, "thread-1", &run.assistant_message_id, None)
            .unwrap();
    assert_eq!(after.usage.unwrap().input, combined.input_tokens);
    assert_eq!(
        vega_store::subagents::find(store.conn(), &child.agent_id)
            .unwrap()
            .unwrap()
            .result,
        child.result
    );
}

#[tokio::test]
async fn m17_child_missing_cache_is_unknown_in_parent_run_summary() {
    let (store, dir, _) = setup();
    let mut provider = ParallelProvider::new();
    provider.missing_child_cache = true;
    provider.release.add_permits(2);
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host),
        None,
        Some(pricing()),
    )
    .await
    .unwrap();
    let summary =
        crate::summary::task_cost_summary(&store, "thread-1", &run.assistant_message_id, None)
            .unwrap();
    let own = vega_store::token_usage::aggregate_by_message(
        store.conn(),
        "thread-1",
        &run.assistant_message_id,
    )
    .unwrap();
    assert_eq!(summary.usage.unwrap().input, own.input_tokens + 200);
    assert_eq!(summary.usage.unwrap().cache_read, None);
    assert_eq!(summary.usage.unwrap().cache_write, None);
    assert_eq!(summary.cache_hit_percent, None);
    assert_eq!(summary.cost, crate::types::SummaryCost::Unavailable);
    assert!(
        !vega_store::subagents::run_usage_complete(
            store.conn(),
            "thread-1",
            &run.assistant_message_id
        )
        .unwrap()
    );
}

#[tokio::test]
async fn m06_m15_child_failure_and_sibling_result_are_durable() {
    let (store, dir, _) = setup();
    let mut provider = ParallelProvider::new();
    provider.fail_second = true;
    provider.release.add_permits(2);
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!run.failed);
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let children = crate::subagents::children(&reopened, "thread-1").unwrap();
    assert!(
        children
            .iter()
            .any(|agent| agent.name == "A" && agent.state == SubagentState::Completed)
    );
    assert!(children.iter().any(|agent| agent.name == "B"
        && agent.state == SubagentState::Failed
        && !agent.result.is_empty()));
    let failed = children.iter().find(|agent| agent.name == "B").unwrap();
    assert_eq!(
        failed.result,
        "Subagent failed. Open the conversation for details."
    );
    let durable = vega_store::subagents::find(reopened.conn(), &failed.agent_id)
        .unwrap()
        .unwrap();
    assert_eq!(durable.state, "failed");
    assert_eq!(durable.result, failed.result);
    assert!(vega_store::subagents::has_unknown_usage(reopened.conn(), "thread-1").unwrap());
}

#[tokio::test]
async fn m07_m08_stop_one_child_then_parent_while_waiting_drains_every_owned_generation() {
    let (store, dir, _) = setup();
    let provider = ParallelProvider::new();
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let cancel = CancellationToken::new();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        cancel.clone(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host.clone()),
        None,
        None,
    );
    let drive = async {
        provider.entered.acquire_many(2).await.unwrap().forget();
        provider.root_waiting.acquire().await.unwrap().forget();
        let tokens = host.cancels.lock().unwrap().clone();
        tokens[0].cancel();
        assert!(!tokens[1].is_cancelled());
        loop {
            let changed = host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if host
                .agents
                .lock()
                .unwrap()
                .iter()
                .any(|agent| agent.state == SubagentState::Interrupted)
            {
                break;
            }
            changed.await;
        }
        assert!(!tokens[1].is_cancelled());
        cancel.cancel();
    };
    let (result, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(run, drive) })
            .await
            .unwrap();
    assert!(result.unwrap().interrupted);
    assert!(
        crate::subagents::children(&store, "thread-1")
            .unwrap()
            .iter()
            .all(|agent| agent.state == SubagentState::Interrupted
                && agent.result == "Interrupted before completion")
    );
    let active: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE status='streaming'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active, 0);
}

#[tokio::test]
async fn m08_parent_provider_failure_after_both_children_enter_cancels_and_drains_durably() {
    let (store, dir, _) = setup();
    let gate = Arc::new(Semaphore::new(0));
    let mut provider = ParallelProvider::new();
    provider.parent_failure_gate = Some(gate.clone());
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let cancel = CancellationToken::new();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "parent",
        "System",
        cancel.clone(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host.clone()),
        None,
        None,
    );
    let fail_parent = async {
        provider.entered.acquire_many(2).await.unwrap().forget();
        provider.root_waiting.acquire().await.unwrap().forget();
        assert_eq!(host.cancels.lock().unwrap().len(), 2);
        assert!(
            host.cancels
                .lock()
                .unwrap()
                .iter()
                .all(|cancel| !cancel.is_cancelled())
        );
        gate.add_permits(1);
    };
    let (run, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(run, fail_parent)
    })
    .await
    .unwrap();
    let run = run.unwrap();
    assert!(run.failed);
    assert!(!run.interrupted);
    assert!(!cancel.is_cancelled());
    assert_eq!(provider.child_calls.load(Ordering::SeqCst), 2);
    assert!(
        host.cancels
            .lock()
            .unwrap()
            .iter()
            .all(CancellationToken::is_cancelled)
    );
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let children = crate::subagents::children(&reopened, "thread-1").unwrap();
    assert_eq!(children.len(), 2);
    assert!(
        children
            .iter()
            .all(|agent| agent.state == SubagentState::Interrupted
                && agent.result == "Interrupted before completion")
    );
    for child in &children {
        assert!(!crate::subagents::is_live(&reopened, &child.agent_id));
        assert_eq!(
            vega_store::subagents::find(reopened.conn(), &child.agent_id)
                .unwrap()
                .unwrap()
                .state,
            "interrupted"
        );
    }
    assert_eq!(
        vega_store::messages::find(reopened.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    let live: i64 = reopened
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE status='streaming'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live, 0);
    assert_eq!(
        vega_store::subagents::active_count(reopened.conn(), "thread-1", &run.assistant_message_id)
            .unwrap(),
        0
    );
}

struct FollowupProvider {
    requests: Mutex<Vec<ChatRequest>>,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

impl Provider for FollowupProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request);
        self.entered.add_permits(1);
        let release = self.release.clone();
        async move {
            tokio::select! {biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), permit = release.acquire() => permit.unwrap().forget()}
            Ok(stream(vec![ProviderEvent::TextDelta("Independent follow-up result".into()),usage(71),ProviderEvent::Done {stop_reason:StopReason::End}]))
        }.boxed()
    }
}

#[tokio::test]
async fn m13_m14_m17_terminal_child_normal_native_followup_preserves_history_and_original_ownership()
 {
    let (store, dir, _) = setup();
    let original = RepeatedSpawnProvider {
        root_calls: AtomicUsize::new(0),
        child_calls: AtomicUsize::new(0),
    };
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let parent_cancel = CancellationToken::new();
    let parent = run_thread_task_with_pricing(
        &store,
        &original,
        &tools,
        "thread-1",
        "parent",
        "System",
        parent_cancel.clone(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host.clone()),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!parent.failed);
    let child = crate::subagents::children(&store, "thread-1")
        .unwrap()
        .remove(0);
    assert_eq!(child.state, SubagentState::Completed);
    assert_eq!(original.child_calls.load(Ordering::SeqCst), 1);
    let original_result = vega_store::subagents::find(store.conn(), &child.agent_id)
        .unwrap()
        .unwrap();
    let original_message = vega_store::messages::find(store.conn(), &child.message_id)
        .unwrap()
        .unwrap();
    assert_eq!(original_result.result, "Child result");
    assert!(!original_result.truncated);
    assert_eq!(original_message.content, "Child result");
    assert_eq!(original_message.status, "done");
    assert_eq!(
        host.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, event)| {
                id == &child.agent_id
                    && matches!(event, ConversationEvent::TextDelta { message_id, delta }
            if message_id == &child.message_id && delta == "Child result")
            })
            .count(),
        1
    );
    let original_count:i64 = store.conn().query_row("SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='assistant' AND content='Child result'", [&child.agent_id], |row| row.get(0)).unwrap();
    assert_eq!(original_count, 1);
    let original_usage = vega_store::token_usage::aggregate_task_by_message(
        store.conn(),
        "thread-1",
        &parent.assistant_message_id,
    )
    .unwrap();
    let provider = FollowupProvider {
        requests: Mutex::new(Vec::new()),
        entered: Arc::new(Semaphore::new(0)),
        release: Arc::new(Semaphore::new(0)),
    };
    let fresh_cancel = CancellationToken::new();
    let followup = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        &child.agent_id,
        "Follow up independently",
        "System",
        fresh_cancel.clone(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_subagent_host(host),
        None,
        None,
    );
    let cancel_old_parent = async {
        provider.entered.acquire().await.unwrap().forget();
        parent_cancel.cancel();
        assert!(!fresh_cancel.is_cancelled());
        provider.release.add_permits(1);
    };
    let (followup, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(followup, cancel_old_parent)
    })
    .await
    .unwrap();
    let followup = followup.unwrap();
    assert!(!followup.failed);
    assert!(!followup.interrupted);
    assert_eq!(followup.content, "Independent follow-up result");
    assert_ne!(followup.assistant_message_id, child.message_id);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let messages = &requests[0].messages;
    assert!(
        messages
            .iter()
            .any(|message| message.role == ChatRole::User && message.content == "child-a")
    );
    let observation = serde_json::json!({"vega_history_observation":{
        "version":1,"authority":"none","kind":"assistant_text","text":"Child result"
    }});
    assert_eq!(
        messages
            .iter()
            .filter(|message| {
                message.role == ChatRole::User
                    && serde_json::from_str::<serde_json::Value>(&message.content)
                        .is_ok_and(|value| value == observation)
            })
            .count(),
        1
    );
    assert!(!messages.iter().any(|message| message.role == ChatRole::Assistant && message.content == "Child result"));
    assert!(
        messages.iter().any(|message| message.role == ChatRole::User
            && message.content == "Follow up independently")
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.role == ChatRole::User && message.content == "parent")
    );
    assert!(
        !requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "spawn_agent")
    );
    drop(requests);
    let after = vega_store::subagents::find(store.conn(), &child.agent_id)
        .unwrap()
        .unwrap();
    assert_eq!(after.child_message_id, original_result.child_message_id);
    assert_eq!(after.result, original_result.result);
    assert_eq!(after.state, "completed");
    let preserved = vega_store::messages::find(store.conn(), &child.message_id)
        .unwrap()
        .unwrap();
    assert_eq!(preserved.content, original_message.content);
    assert_eq!(preserved.status, original_message.status);
    assert_eq!(
        vega_store::threads::find(store.conn(), &child.agent_id)
            .unwrap()
            .unwrap()
            .parent_thread_id
            .as_deref(),
        Some("thread-1")
    );
    assert_eq!(
        vega_store::token_usage::aggregate_task_by_message(
            store.conn(),
            "thread-1",
            &parent.assistant_message_id
        )
        .unwrap(),
        original_usage
    );
    let attribution:(Option<String>,Option<String>) = store.conn().query_row("SELECT delegated_parent_thread_id,delegated_parent_message_id FROM token_usage WHERE thread_id=?1 AND message_id=?2",[&child.agent_id,&followup.assistant_message_id],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!(attribution, (None, None));
    let own = vega_store::token_usage::aggregate_by_thread(store.conn(), &child.agent_id).unwrap();
    assert_eq!(own.input_tokens, 171);
    let runs: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='assistant'",
            [&child.agent_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(runs, 2);
}

#[tokio::test]
async fn m08_m16_natural_parent_end_and_rejected_admission_do_not_leave_live_work() {
    for reject in [false, true] {
        let (store, dir, _) = setup();
        let mut provider = ParallelProvider::new();
        provider.finish_early = true;
        let host = Arc::new(Host::default());
        host.reject.store(reject, Ordering::SeqCst);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let cancel = CancellationToken::new();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            run_thread_task_with_pricing(
                &store,
                &provider,
                &tools,
                "thread-1",
                "parent",
                "System",
                cancel.clone(),
                &RejectPermissionHook,
                |_| Ok(()),
                PersistenceActorConfig::default().with_subagent_host(host),
                None,
                None,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!result.failed);
        assert!(!result.interrupted);
        assert!(!cancel.is_cancelled());
        assert_eq!(result.content, "Aggregated");
        assert_eq!(provider.root_calls.load(Ordering::SeqCst), 2);
        let children = crate::subagents::children(&store, "thread-1").unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|agent| agent.state.is_terminal()));
        if reject {
            assert_eq!(provider.child_calls.load(Ordering::SeqCst), 0);
            assert!(
                children
                    .iter()
                    .all(|agent| agent.state == SubagentState::Interrupted)
            );
            assert_eq!(result.events.iter().filter(|event| matches!(event,
                ConversationEvent::ToolCallFinished { call_id, result }
                if matches!(call_id.as_str(), "spawn-a" | "spawn-b") && result.status == ToolCallStatus::Failed
            )).count(), 2);
        }
    }
}

struct OperationProvider {
    inner: ParallelProvider,
    edit: bool,
}
impl Provider for OperationProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let user = request
            .messages
            .iter()
            .rev()
            .find(|message| {
                message.role == ChatRole::User
                    && matches!(message.content.as_str(), "parent" | "child-a" | "child-b")
            })
            .unwrap()
            .content
            .clone();
        let call_id = format!("operation-{user}");
        let child_first = user != "parent"
            && !request.messages.iter().any(|message| {
                (message.role == ChatRole::Tool
                    && message.tool_call_id.as_deref() == Some(call_id.as_str()))
                    || (message.role == ChatRole::User
                        && serde_json::from_str::<serde_json::Value>(&message.content).is_ok_and(
                            |value| {
                                let observation = &value["vega_history_observation"];
                                observation["version"].as_u64() == Some(1)
                                    && observation["authority"].as_str() == Some("none")
                                    && observation["kind"].as_str() == Some("tool_result")
                                    && observation["call_id"].as_str() == Some(call_id.as_str())
                            },
                        ))
            });
        let source = self.inner.chat_stream(request, cancel);
        let edit = self.edit;
        async move {
            let original = source.await?;
            if !child_first {return Ok(original);}
            let input = if edit {serde_json::json!({"path":"lib.rs","old_string":"persist me","new_string":"changed"})} else {serde_json::json!({"path":format!("{user}.txt"),"content":"created"})};
            Ok(stream(vec![ProviderEvent::ToolUse {id:format!("operation-{user}"),name:if edit {"Edit"} else {"Write"}.into(),input_json:input.to_string()},usage(5),ProviderEvent::Done {stop_reason:StopReason::ToolUse}]))
        }.boxed()
    }
}

#[tokio::test]
async fn m09_m10_child_read_witness_and_permission_modes_remain_independent() {
    for mode in ["readonly", "confirm", "auto", "full_access"] {
        for edit in [false, true] {
            let (store, dir, _data_dir, _) = setup_external(mode);
            fs::write(dir.path().join("lib.rs"), "// TODO: persist me\n").unwrap();
            assert!(
                !store
                    .database_path()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("checkpoints")
                    .starts_with(dir.path())
            );
            let tools = vega_tools::Tools::new(dir.path()).unwrap();
            tools.read("lib.rs", None, None).unwrap();
            let before = fs::read(dir.path().join("lib.rs")).unwrap();
            let provider = OperationProvider {
                inner: ParallelProvider::new(),
                edit,
            };
            provider.inner.release.add_permits(16);
            let host = Arc::new(Host::default());
            let result = run_thread_task_with_pricing(
                &store,
                &provider,
                &tools,
                "thread-1",
                "parent",
                "System",
                CancellationToken::new(),
                &RejectPermissionHook,
                |_| Ok(()),
                PersistenceActorConfig::default().with_subagent_host(host.clone()),
                None,
                None,
            )
            .await
            .unwrap();
            assert!(!result.failed, "mode={mode}, edit={edit}");
            assert_eq!(
                fs::read(dir.path().join("lib.rs")).unwrap(),
                before,
                "mode={mode}, edit={edit}"
            );
            for name in ["child-a", "child-b"] {
                assert_eq!(
                    dir.path().join(format!("{name}.txt")).exists(),
                    !edit && matches!(mode, "auto" | "full_access"),
                    "mode={mode}, edit={edit}, child={name}"
                );
            }
            let children = crate::subagents::children(&store, "thread-1").unwrap();
            assert_eq!(children.len(), 2, "mode={mode}, edit={edit}");
            let events = host.events.lock().unwrap();
            let terminals = events
                .iter()
                .filter_map(|(child, event)| match event {
                    ConversationEvent::ToolCallFinished { call_id, result }
                        if call_id.starts_with("operation-") =>
                    {
                        Some((child.clone(), call_id.clone(), result.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                terminals.len(),
                2,
                "mode={mode}, edit={edit}, child_states={:?}, events={}",
                children.iter().map(|child| child.state).collect::<Vec<_>>(),
                events.len()
            );
            let proposals = events
                .iter()
                .filter_map(|(child, event)| match event {
                    ConversationEvent::ToolCallProposed { call }
                        if call.id.starts_with("operation-") =>
                    {
                        Some((child, call))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                proposals.len(),
                if edit { 0 } else { 2 },
                "mode={mode}, edit={edit}"
            );
            for (child_id, call) in proposals {
                let stored = tool_calls::find_state(store.conn(), &call.id)
                    .unwrap()
                    .unwrap();
                assert_eq!(&stored.thread_id, child_id, "mode={mode}, edit={edit}");
                assert_eq!(stored.tool, call.tool, "mode={mode}, edit={edit}");
                assert_eq!(
                    stored.input_json, call.input_json,
                    "mode={mode}, edit={edit}"
                );
            }
            for (child_id, call_id, terminal) in &terminals {
                let stored = tool_calls::find_state(store.conn(), call_id)
                    .unwrap()
                    .unwrap();
                let identity = tool_calls::find_identity(store.conn(), call_id)
                    .unwrap()
                    .unwrap();
                let child = children
                    .iter()
                    .find(|child| &child.agent_id == child_id)
                    .unwrap();
                assert_eq!(&stored.thread_id, child_id, "mode={mode}, edit={edit}");
                assert_eq!(
                    identity.message_id, child.message_id,
                    "mode={mode}, edit={edit}"
                );
                assert_eq!(
                    ToolCallStatus::parse(&stored.status),
                    Some(terminal.status),
                    "mode={mode}, edit={edit}"
                );
                assert_eq!(
                    stored.output_text.as_deref(),
                    Some(terminal.output.as_str()),
                    "mode={mode}, edit={edit}"
                );
                if edit {
                    let audit =
                        vega_tools::InvalidWriteEditAudit::from_json(&stored.input_json).unwrap();
                    assert_eq!(
                        audit.validation_error_code(),
                        vega_tools::MutationErrorCode::FileNotRead,
                        "mode={mode}, edit={edit}"
                    );
                }
            }
            if edit || matches!(mode, "readonly" | "confirm") {
                assert!(
                    terminals
                        .iter()
                        .all(|(_, _, result)| result.status != ToolCallStatus::Success),
                    "mode={mode}, edit={edit}"
                );
            } else {
                assert!(
                    terminals
                        .iter()
                        .all(|(_, _, result)| result.status == ToolCallStatus::Success),
                    "mode={mode}, edit={edit}"
                );
            }
        }
    }
}
