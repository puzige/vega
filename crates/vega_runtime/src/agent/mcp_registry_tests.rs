use super::mcp_registry::{McpDispatchFailure, McpDispatchOutput, McpToolDispatcher};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct CountingDispatcher(Arc<AtomicUsize>);

impl McpToolDispatcher for CountingDispatcher {
    fn call(
        &self,
        _: String,
        _: Value,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(McpDispatchOutput {
                text: "unexpected dispatch".into(),
                structured_content: None,
                is_error: false,
            })
        })
    }
}

fn candidate(cancel: CancellationToken, calls: Arc<AtomicUsize>) -> McpCandidate {
    McpCandidate::new(
        "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        1,
        "lookup".into(),
        ToolDefinition {
            name: "lookup".into(),
            description: "External lookup".into(),
            input_schema: serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
            strict: true,
        },
        Arc::new(CountingDispatcher(calls)),
        cancel,
    )
}

fn config(candidate: McpCandidate) -> RuntimeToolConfig {
    let mut config = RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        "project".into(),
        "thread".into(),
        std::env::temp_dir(),
        Vec::new(),
    );
    config.mcp_candidates.push(candidate);
    config
}

#[tokio::test]
async fn revocation_shrinks_definitions_and_digest_without_reviving_old_alias() -> TestResult {
    let revoked = CancellationToken::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let config = config(candidate(revoked.clone(), calls.clone()));
    let snapshot = RunCapabilitySnapshot::freeze(
        config.run_mode,
        config.permission_mode,
        config.mcp_candidates.clone(),
    )?;
    let alias = snapshot
        .current_definitions()
        .into_iter()
        .find(|definition| definition.name.starts_with("mcp_"))
        .ok_or("missing MCP alias")?
        .name;
    let before = config.replay_capability_digest()?;
    revoked.cancel();
    assert!(
        snapshot
            .current_definitions()
            .iter()
            .all(|item| item.name != alias)
    );
    assert!(
        snapshot
            .current_definitions()
            .iter()
            .any(|item| item.name == "read")
    );
    assert!(
        snapshot
            .mcp_tool(&alias)
            .is_some_and(|tool| tool.is_revoked())
    );
    assert_ne!(before, config.replay_capability_digest()?);
    let mut without = config.clone();
    without.mcp_candidates.clear();
    assert_eq!(
        without.replay_capability_digest()?,
        config.replay_capability_digest()?
    );
    assert!(
        RunCapabilitySnapshot::freeze(
            config.run_mode,
            config.permission_mode,
            config.mcp_candidates.clone()
        )
        .is_err()
    );
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let prepared = tools_exec::prepare_runtime_call(
        &tools,
        &config,
        &snapshot,
        RuntimeToolCall {
            id: "old-call".into(),
            name: alias.clone(),
            input_json: "{}".into(),
            description: None,
        },
    )?;
    match tools_exec::authorize_call(
        &prepared,
        &config,
        &HashSet::new(),
        &RejectPermissionHook,
        &CancellationToken::new(),
    )
    .await?
    {
        tools_exec::Authorization::Terminal(result) => {
            assert_eq!(result.status, RuntimeToolStatus::Rejected)
        }
        _ => panic!("revoked alias approved"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut replacement = candidate(CancellationToken::new(), calls);
    replacement.config_revision = 2;
    replacement.definition.description = "Replacement definition".into();
    let next =
        RunCapabilitySnapshot::freeze(config.run_mode, config.permission_mode, vec![replacement])?;
    assert!(
        next.current_definitions()
            .iter()
            .any(|item| item.name == alias && item.description.contains("Replacement definition"))
    );
    assert!(
        snapshot
            .current_definitions()
            .iter()
            .all(|item| item.name != alias)
    );
    Ok(())
}

struct RevokingHook {
    revoked: CancellationToken,
    requests: Mutex<Vec<crate::ContextCompactionRequest>>,
}

impl crate::ContextCompactionHook for RevokingHook {
    fn compact<'a>(
        &'a self,
        request: crate::ContextCompactionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'a, Result<crate::ContextCompactionResult, crate::ContextCompactionFailure>>
    {
        self.requests.lock().unwrap().push(request);
        Box::pin(async move {
            self.revoked.cancel();
            tokio::task::yield_now().await;
            Ok(crate::ContextCompactionResult {
                messages: vec![
                    ChatMessage::new(ChatRole::User, "Historical summary"),
                    ChatMessage::new(ChatRole::Assistant, "Retained response"),
                    ChatMessage::new(ChatRole::User, "Continue the task"),
                ],
                source_version: 2,
                source_fingerprint: Some("after-compaction".into()),
                replay_cold_prefix_messages: Some(2),
                replay_interleaved_user_boundary: None,
                reactive_retained_groups: Some(2),
                usages: Vec::new(),
                usage_complete: true,
                settled_usage: None,
            })
        })
    }
}

struct OverflowProvider(Mutex<Vec<ChatRequest>>);

impl Provider for OverflowProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.0.lock().unwrap();
        let first = requests.is_empty();
        requests.push(request);
        Box::pin(async move {
            if first {
                return Err(VegaError::Provider {
                    status: Some(400),
                    message: r#"{"error":{"code":"prompt_too_long"}}"#.into(),
                    retryable: false,
                });
            }
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(ProviderEvent::TextDelta("continued".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as crate::EventStream)
        })
    }
}

#[tokio::test]
async fn reactive_compaction_revocation_removes_schema_from_actual_continuation() -> TestResult {
    let revoked = CancellationToken::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = Arc::new(RevokingHook {
        revoked: revoked.clone(),
        requests: Mutex::new(Vec::new()),
    });
    let provider = OverflowProvider(Mutex::new(Vec::new()));
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let outcome = run_agent(
        &provider,
        &tools,
        AgentRequest {
            model: "synthetic".into(),
            system_prompt: "Continue without repeating tool calls".into(),
            history: vec![
                ChatMessage::new(ChatRole::User, "Original task"),
                ChatMessage::new(ChatRole::Assistant, "old context ".repeat(4_000)),
                ChatMessage::new(ChatRole::User, "Continue the task"),
            ],
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: config(candidate(revoked, calls.clone())),
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: Some(crate::ContextBudget::new(100_000, 10_000, true)?),
            context_source_version: Some(1),
            context_source_fingerprint: Some("before".into()),
            context_operation_id: Some("operation".into()),
            context_compaction_hook: Some(hook.clone()),
        },
        CancellationToken::new(),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let requests = provider.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name.starts_with("mcp_"))
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .all(|tool| !tool.name.starts_with("mcp_"))
    );
    assert!(requests[1].tools.iter().any(|tool| tool.name == "read"));
    let compactions = hook.requests.lock().unwrap();
    assert_eq!(compactions.len(), 1);
    assert!(matches!(
        compactions[0].trigger,
        crate::ContextCompactionTrigger::InputOverflow { .. }
    ));
    assert!(
        compactions[0]
            .tools
            .iter()
            .any(|tool| tool.name.starts_with("mcp_"))
    );
    Ok(())
}

struct PrecomputeHook(Mutex<Vec<crate::ContextCompactionRequest>>);

impl crate::ContextCompactionHook for PrecomputeHook {
    fn arm_precompute<'a>(
        &'a self,
        request: crate::ContextCompactionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {})
    }

    fn compact<'a>(
        &'a self,
        _: crate::ContextCompactionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'a, Result<crate::ContextCompactionResult, crate::ContextCompactionFailure>>
    {
        Box::pin(async { panic!("foreground compaction must not run") })
    }
}

#[tokio::test]
async fn preflight_await_revocation_refreshes_precompute_schema_and_estimate() -> TestResult {
    let revoked = CancellationToken::new();
    let config = config(candidate(revoked.clone(), Arc::new(AtomicUsize::new(0))));
    let native =
        RunCapabilitySnapshot::freeze(config.run_mode, config.permission_mode, Vec::new())?
            .current_definitions();
    let budget = crate::ContextBudget::new(100_000, 10_000, true)?;
    let history = |padding| {
        vec![
            ChatMessage::new(ChatRole::System, "system"),
            ChatMessage::new(ChatRole::User, "Original task"),
            ChatMessage::new(ChatRole::Assistant, "a".repeat(padding)),
            ChatMessage::new(ChatRole::User, "Continue"),
        ]
    };
    let (mut low, mut high) = (0, 300_000);
    while low < high {
        let middle = (low + high) / 2;
        if crate::estimate_wire_context(&history(middle), &native)?.input_tokens < 74_500 {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let mut messages = history(low);
    let current_estimate = crate::estimate_wire_context(&messages, &native)?;
    assert!(current_estimate.input_tokens >= budget.precompute_tokens()?);
    let frozen = RunCapabilitySnapshot::freeze(
        config.run_mode,
        config.permission_mode,
        config.mcp_candidates.clone(),
    )?;
    assert!(
        crate::estimate_wire_context(&messages, &frozen.current_definitions())?.input_tokens
            < budget.trigger_tokens()?
    );
    messages.remove(0);
    let hook = Arc::new(PrecomputeHook(Mutex::new(Vec::new())));
    let provider = OverflowProvider(Mutex::new(vec![ChatRequest {
        model: "prior".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: None,
        reasoning: None,
        model_snapshot: None,
        tool_choice: None,
    }]));
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let outcome = crate::run_agent_with_sink(
        &provider,
        &tools,
        AgentRequest {
            model: "synthetic".into(),
            system_prompt: "system".into(),
            history: messages,
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: config,
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: Some(budget),
            context_source_version: Some(1),
            context_source_fingerprint: Some("before".into()),
            context_operation_id: Some("operation".into()),
            context_compaction_hook: Some(hook.clone()),
        },
        CancellationToken::new(),
        move |event| {
            let revoked = revoked.clone();
            async move {
                if matches!(
                    event,
                    RuntimeEvent::ContextAccountingUpdated(ContextAccountingDecision {
                        stage: ContextAccountingStage::PrimaryPreflight,
                        ..
                    })
                ) {
                    tokio::task::yield_now().await;
                    revoked.cancel();
                }
                Ok(())
            }
        },
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let armed = hook.0.lock().unwrap();
    assert!(!armed.is_empty());
    for request in armed.iter() {
        assert!(
            request
                .tools
                .iter()
                .all(|tool| !tool.name.starts_with("mcp_"))
        );
        assert!(request.tools.iter().any(|tool| tool.name == "read"));
        let mut projected = vec![ChatMessage::new(
            ChatRole::System,
            request.system_prompt.clone(),
        )];
        projected.extend(request.messages.clone());
        assert_eq!(
            request.estimate,
            crate::estimate_wire_context(&projected, &request.tools)?
        );
    }
    assert_eq!(armed[0].estimate, current_estimate);
    Ok(())
}
