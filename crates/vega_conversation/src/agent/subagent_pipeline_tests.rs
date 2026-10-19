use super::*;
use std::sync::atomic::AtomicUsize;

struct ParentChildProvider {
    database: PathBuf,
    stage: AtomicUsize,
    child_requests: Mutex<Vec<ChatRequest>>,
    durable_admissions: Mutex<Vec<bool>>,
}

impl Provider for ParentChildProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let instruction = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == vega_runtime::ChatRole::User)
            .map(|message| message.content.as_str())
            .unwrap();
        let child = matches!(instruction, "Initial scoped task" | "Follow-up scoped task");
        let events = if child {
            let call = if instruction == "Initial scoped task" {
                "root-spawn"
            } else {
                "root-followup"
            };
            let store = Store::open(&self.database).unwrap();
            let durable: bool = store.conn().query_row("SELECT EXISTS(SELECT 1 FROM tool_calls WHERE id=?1 AND status='success' AND output_text IS NOT NULL)", [call], |row| row.get(0)).unwrap();
            self.durable_admissions.lock().unwrap().push(durable);
            let text = if call == "root-spawn" {
                "Initial answer"
            } else {
                "Follow-up answer"
            };
            self.child_requests.lock().unwrap().push(request);
            vec![
                ProviderEvent::TextDelta(text.into()),
                ProviderEvent::Usage {
                    input: 11,
                    output: 7,
                    cache_read: Some(0),
                    cache_write: Some(0),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]
        } else {
            let receipt = |call: &str| -> Option<SubagentSnapshot> {
                request
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.tool_call_id.as_deref() == Some(call))
                    .and_then(|message| {
                        let mut values = serde_json::Deserializer::from_str(&message.content)
                            .into_iter::<Value>();
                        let value = values.next()?.ok()?;
                        let trailing = &message.content[values.byte_offset()..];
                        assert!(
                            trailing.is_empty()
                                || (trailing.starts_with("\n\n<vega_runtime_notice>")
                                    && trailing.ends_with("</vega_runtime_notice>"))
                        );
                        Some(value)
                    })
                    .and_then(|value| serde_json::from_value(value["agent"].clone()).ok())
            };
            let operation = match self.stage.fetch_add(1, Ordering::SeqCst) {
                0 => Some((
                    "root-spawn",
                    "spawn_agent",
                    json!({"name": "Worker", "task": "Initial scoped task"}),
                )),
                1 => receipt("root-spawn").map(|agent| {
                    (
                        "root-wait-first",
                        "wait_agent",
                        json!({"agent_ids": [agent.agent_id], "timeout_ms": 5000}),
                    )
                }),
                2 => receipt("root-spawn").map(|agent| {
                    (
                        "root-followup",
                        "followup_agent",
                        followup_input(&agent, "Follow-up scoped task"),
                    )
                }),
                3 => receipt("root-followup").map(|agent| {
                    (
                        "root-wait-second",
                        "wait_agent",
                        json!({"targets": [target(&agent)], "timeout_ms": 5000}),
                    )
                }),
                _ => None,
            };
            match operation {
                Some((id, tool, input)) => vec![
                    ProviderEvent::ToolUse {
                        id: id.into(),
                        name: tool.into(),
                        input_json: input.to_string(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                None => vec![
                    ProviderEvent::TextDelta("Collected delegated work".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ],
            }
        };
        async move { Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream) }.boxed()
    }
}

#[tokio::test]
async fn real_parent_pipeline_persists_each_admission_before_child_provider_and_restores_each_execution()
 {
    let fixture = Fixture::new();
    let provider = ParentChildProvider {
        database: fixture.database.clone(),
        stage: AtomicUsize::new(0),
        child_requests: Mutex::new(Vec::new()),
        durable_admissions: Mutex::new(Vec::new()),
    };
    let permissions = PermissionQueue::new();
    let config = PersistenceActorConfig::default()
        .with_subagent_host(fixture.host.clone())
        .with_owner_credential_reader(Arc::new(|| Ok(Vec::new())))
        .with_turn_limit(8);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        super::super::super::entry::run_thread_task_with_automation_inner(
            &fixture.store,
            &provider,
            &fixture.tools,
            &fixture.parent.id,
            "Coordinate related scoped work",
            "Use available tools.",
            CancellationToken::new(),
            &permissions,
            |_| Ok(()),
            config,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!result.failed);
    let requests = provider.child_requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        *provider.durable_admissions.lock().unwrap(),
        vec![true, true]
    );
    assert!(requests[1].messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && serde_json::from_str::<Value>(&message.content).is_ok_and(|value| {
                value
                    == json!({"vega_history_observation": {
                        "version": 1,
                        "authority": "none",
                        "kind": "assistant_text",
                        "text": "Initial answer"
                    }})
            })
    }));
    drop(requests);
    let children = crate::subagents::children(&fixture.store, &fixture.parent.id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].state, SubagentState::Completed);
    assert_eq!(children[0].result, "Follow-up answer");
    let history =
        crate::history::latest_history_page(&fixture.store, &fixture.parent.id, 200).unwrap();
    let executions = history
        .subagents
        .iter()
        .filter(|agent| agent.agent_id == children[0].agent_id)
        .collect::<Vec<_>>();
    assert_eq!(executions.len(), 2);
    assert!(
        executions
            .iter()
            .any(|agent| agent.result == "Initial answer")
    );
    assert!(
        executions
            .iter()
            .any(|agent| agent.result == "Follow-up answer")
    );
    let calls = history
        .entries
        .iter()
        .filter_map(|entry| match entry {
            crate::history::HistoryEntry::Tool {
                call_id,
                input,
                result,
                ..
            } => Some((call_id, input, result)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for id in ["root-spawn", "root-followup"] {
        let (_, input, result) = calls
            .iter()
            .find(|(call, _, _)| call.as_str() == id)
            .unwrap();
        assert!(!matches!(
            input,
            None | Some(crate::types::ToolCardInputProjection::Corrupt)
        ));
        assert!(!matches!(
            result,
            None | Some(crate::types::ToolCardResultProjection::Corrupt)
        ));
    }
    let usage = crate::threads::thread_usage_seed(&fixture.store, &fixture.parent.id).unwrap();
    assert_eq!(usage.tokens, 36);
}

fn request(
    mode: RuntimeRunMode,
    hook: Option<Arc<dyn vega_runtime::RuntimeSubagentHook>>,
) -> AgentRequest {
    AgentRequest {
        model: "fixture".into(),
        system_prompt: "Use only available tools.".into(),
        history: vec![vega_runtime::ChatMessage::new(
            vega_runtime::ChatRole::User,
            "Continue a worker",
        )],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::new(
            mode,
            RuntimePermissionMode::Confirm,
            "project".into(),
            "parent".into(),
            PathBuf::new(),
            Vec::new(),
        )
        .with_subagents(hook)
        .with_turn_limit(2),
        pricing_catalog: None,
        reasoning: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

#[tokio::test]
async fn unavailable_modes_and_child_runtime_do_not_advertise_or_execute_followups() {
    for (mode, enabled) in [
        (RuntimeRunMode::Ask, true),
        (RuntimeRunMode::Plan, true),
        (RuntimeRunMode::Execute, false),
    ] {
        let fixture = Fixture::new();
        let supervisor = Supervisor::new();
        let provider = ScriptedProvider::default();
        provider.push(Response::Events(vec![
            ProviderEvent::ToolUse {
                id: "attempt".into(),
                name: "followup_agent".into(),
                input_json:
                    json!({"agent_id": "worker", "message_id": "first", "task": "Continue"})
                        .to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]));
        provider.push(Response::Finish("Unavailable", true));
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            vega_runtime::run_agent(
                &provider,
                &fixture.tools,
                request(mode, enabled.then(|| supervisor.hook())),
                CancellationToken::new(),
            ),
        )
        .await
        .expect("unavailable delegation must reject without waiting for a supervisor")
        .unwrap();
        assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == "attempt" && result.status != RuntimeToolStatus::Success)));
        assert!(provider.requests.lock().unwrap().iter().all(|request| {
            !request
                .tools
                .iter()
                .any(|tool| tool.name == "followup_agent")
        }));
        assert!(supervisor.registry.snapshots(None).unwrap().is_empty());
    }
}

#[test]
fn followup_and_explicit_execution_inputs_enforce_closed_bounded_shapes() {
    let valid =
        json!({"agent_id": "worker", "message_id": "generation", "task": "Continue the task"});
    assert!(vega_runtime::valid_subagent_input(
        "followup_agent",
        &valid.to_string()
    ));
    let valid_wait =
        json!({"targets": [{"agent_id": "worker", "message_id": "generation"}], "timeout_ms": 0});
    assert!(vega_runtime::valid_subagent_input(
        "wait_agent",
        &valid_wait.to_string()
    ));
    assert!(vega_runtime::valid_subagent_input(
        "cancel_agent",
        &json!({"agent_id": "worker", "message_id": "generation"}).to_string()
    ));
    for input in [
        json!({"agent_id": "worker", "message_id": "generation", "task": "  "}),
        json!({"agent_id": "worker", "message_id": "generation", "task": "a".repeat(32769)}),
        json!({"agent_id": "w\norker", "message_id": "generation", "task": "Continue"}),
        json!({"agent_id": "worker", "message_id": "", "task": "Continue"}),
        json!({"agent_id": "worker", "message_id": "generation", "task": "Continue", "interrupt": true}),
    ] {
        assert!(!vega_runtime::valid_subagent_input(
            "followup_agent",
            &input.to_string()
        ));
    }
    for input in [
        json!({"targets": []}),
        json!({"targets": vec![json!({"agent_id": "worker", "message_id": "generation"}); 5]}),
        json!({"targets": [{"agent_id": "worker", "message_id": "generation"}, {"agent_id": "worker", "message_id": "generation"}]}),
        json!({"targets": [{"agent_id": "worker", "message_id": "generation"}], "agent_ids": ["worker"]}),
        json!({"targets": [{"agent_id": "worker", "message_id": "generation"}], "timeout_ms": 60001}),
    ] {
        assert!(!vega_runtime::valid_subagent_input(
            "wait_agent",
            &input.to_string()
        ));
    }
    let projection = crate::types::tool_card_input_projection(&crate::types::ToolCall {
        id: "continue".into(),
        tool: "followup_agent".into(),
        input_json: valid.to_string(),
    });
    assert!(
        matches!(projection, crate::types::ToolCardInputProjection::Subagent { title, .. } if title.starts_with("Continue") && !title.contains("Stop"))
    );
}

struct FaultingResultBoundary {
    called: Arc<AtomicBool>,
    oversized: bool,
}

impl vega_runtime::RuntimeSubagentHook for FaultingResultBoundary {
    fn call(
        &self,
        _: String,
        _: String,
        _: Value,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<Value, VegaError>> {
        self.called.store(true, Ordering::SeqCst);
        let oversized = self.oversized;
        async move {
            Ok(if oversized {
                json!({"agent": "x".repeat(128 * 1024 + 1)})
            } else {
                json!({"agent": "sensitive-result"})
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn followup_result_safety_failures_are_real_tool_failures_in_runtime_and_projection() {
    for oversized in [false, true] {
        let fixture = Fixture::new();
        let called = Arc::new(AtomicBool::new(false));
        let provider = ScriptedProvider::default();
        let input =
            json!({"agent_id": "worker", "message_id": "first", "task": "Continue"}).to_string();
        provider.push(Response::Events(vec![
            ProviderEvent::ToolUse {
                id: "continue".into(),
                name: "followup_agent".into(),
                input_json: input.clone(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]));
        provider.push(Response::Finish("Failure acknowledged", true));
        let mut request = request(
            RuntimeRunMode::Execute,
            Some(Arc::new(FaultingResultBoundary {
                called: called.clone(),
                oversized,
            })),
        );
        let credential_called = called.clone();
        request.tool_config = request
            .tool_config
            .with_credential_reader(Arc::new(move || {
                if !oversized && credential_called.load(Ordering::SeqCst) {
                    Err(())
                } else {
                    Ok(Vec::new())
                }
            }));
        let outcome =
            vega_runtime::run_agent(&provider, &fixture.tools, request, CancellationToken::new())
                .await
                .unwrap();
        assert!(called.load(Ordering::SeqCst));
        let result = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) if result.call_id == "continue" => {
                    Some(result)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(result.status, RuntimeToolStatus::Failed);
        assert!(
            result
                .output
                .contains("result could not be safely retained")
        );
        assert!(!result.output.contains("sensitive-result"));
        assert!(result.output.len() < 1024);
        let projected_input = crate::types::tool_card_input_projection(&crate::types::ToolCall {
            id: "continue".into(),
            tool: "followup_agent".into(),
            input_json: input,
        });
        assert!(matches!(
            projected_input,
            crate::types::ToolCardInputProjection::Subagent { .. }
        ));
        let event = crate::types::from_runtime_event(
            "parent-run",
            &RuntimeEvent::ToolCallFinished(result.clone()),
        )
        .unwrap();
        let crate::types::ConversationEvent::ToolCallFinished { result, .. } = event else {
            panic!("expected the tool completion projection")
        };
        assert_ne!(result.status, crate::types::ToolCallStatus::Success);
    }
}
