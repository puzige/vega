use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::loop_::reasoning_budget_violation;
use super::*;
use crate::{
    ContextBudget, ContextCompactionFailure, ContextCompactionHook, ContextCompactionRequest,
    ContextCompactionResult, ContextRuntimeError, MockProvider, ReasoningChoice,
    ReasoningDisabledWire, ReasoningProtocol, ScriptStep,
};

struct RecordingCompactionHook {
    calls: Arc<AtomicUsize>,
    messages: Vec<ChatMessage>,
    source_version: u64,
}

struct TailPreservingCompactionHook {
    calls: Arc<AtomicUsize>,
    observed: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ContextCompactionHook for TailPreservingCompactionHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut observed) = self.observed.lock() {
            *observed = request.messages.clone();
        }
        let last_user = request
            .messages
            .iter()
            .rposition(|message| message.role == ChatRole::User)
            .unwrap_or(request.messages.len());
        let mut messages = vec![ChatMessage::new(ChatRole::User, "summary")];
        messages.extend(request.messages[last_user..].iter().cloned());
        async move {
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}

impl ContextCompactionHook for RecordingCompactionHook {
    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let messages = self.messages.clone();
        let source_version = self.source_version;
        async move {
            Ok(ContextCompactionResult {
                messages,
                source_version,
                source_fingerprint: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}

mod loop_tools;
mod mcp_registry;
mod permission_flow;
mod skills;
mod usage_limits;

#[tokio::test]
async fn issue91_valid_primary_usage_prevents_premature_tool_round_compaction() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("large.txt"), "z".repeat(20_000)).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-anchor".into(),
                name: "read".into(),
                input_json: r#"{"path":"large.txt"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 500,
                output: 80,
                cache_read: 400,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(20_000))]);
    req.context_budget = Some(ContextBudget::new(11_000, 1_000, true).unwrap());
    req.context_source_version = Some(1);
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: calls.clone(),
        messages: vec![ChatMessage::new(ChatRole::User, "summary")],
        source_version: 2,
    }));
    let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
    let outcome = result.expect("anchored tool round must complete");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "usage anchor should avoid compaction"
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let raw_first = crate::estimate_wire_context(&requests[0].messages, &requests[0].tools)
        .unwrap()
        .input_tokens;
    let raw_second = crate::estimate_wire_context(&requests[1].messages, &requests[1].tools)
        .unwrap()
        .input_tokens;
    assert!(raw_first < 8_000);
    assert!(raw_second >= 8_000);
    let decisions = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ContextAccountingUpdated(decision)
                if decision.stage == ContextAccountingStage::PrimaryPreflight =>
            {
                Some(decision)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0].source, ContextAccountingSource::Estimated);
    assert_eq!(decisions[0].predicted_input, raw_first);
    assert_eq!(decisions[1].source, ContextAccountingSource::UsageAnchored);
    assert_eq!(decisions[1].provider_input_baseline, Some(500));
    assert_eq!(decisions[1].incremental_estimate, raw_second - raw_first);
    assert_eq!(decisions[1].predicted_input, 500 + raw_second - raw_first);
    assert!(decisions[1].predicted_input < decisions[1].trigger_tokens);
    assert_eq!(decisions[1].revision, 1);
    assert_eq!(decisions[1].covered_messages, requests[0].messages.len());
}

#[tokio::test]
async fn issue91_usage_anchor_still_rejects_a_genuinely_large_tool_tail() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("large.txt"),
        format!("{}\n", "z".repeat(1_000)).repeat(50),
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "read-large-tail".into(),
            name: "read".into(),
            input_json: r#"{"path":"large.txt"}"#.into(),
        },
        ProviderEvent::Usage {
            input: 500,
            output: 80,
            cache_read: 400,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(20_000))]);
    req.context_budget = Some(ContextBudget::new(11_000, 1_000, true).unwrap());
    req.context_source_version = Some(1);
    req.context_compaction_hook = Some(Arc::new(TailPreservingCompactionHook {
        calls: calls.clone(),
        observed: Arc::new(Mutex::new(Vec::new())),
    }));
    let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
    assert!(
        matches!(
            result,
            Err(VegaError::Context(
                ContextRuntimeError::ResultOverLimit { .. }
            ))
        ),
        "result={result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn issue91_missing_zero_or_inconsistent_usage_falls_back_before_next_round() {
    for usage in [None, Some((0, 0)), Some((100, 101))] {
        let project = tempdir().unwrap();
        fs::write(project.path().join("large.txt"), "z".repeat(20_000)).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let mut first = vec![ProviderEvent::ToolUse {
            id: "read-no-anchor".into(),
            name: "read".into(),
            input_json: r#"{"path":"large.txt"}"#.into(),
        }];
        if let Some((input, cache_read)) = usage {
            first.push(ProviderEvent::Usage {
                input,
                output: 10,
                cache_read,
                cache_write: 0,
            });
        }
        first.push(ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        });
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(first)],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(20_000))]);
        req.context_budget = Some(ContextBudget::new(11_000, 1_000, true).unwrap());
        req.context_source_version = Some(1);
        req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
            calls: calls.clone(),
            messages: vec![ChatMessage::new(ChatRole::User, "summary")],
            source_version: 2,
        }));
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let decisions = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ContextAccountingUpdated(decision)
                    if decision.stage == ContextAccountingStage::PrimaryPreflight =>
                {
                    Some(decision)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[1].source, ContextAccountingSource::Estimated);
        assert_eq!(decisions[1].provider_input_baseline, None);
        assert_eq!(decisions[1].revision, 1);
    }
}

#[tokio::test]
async fn issue91_unconfigured_run_keeps_legacy_request_without_estimator_bound() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let history = ChatMessage::new(ChatRole::User, "x".repeat(64 * 1024 * 1024 + 1));
    let outcome = run_agent(
        &provider,
        &tools,
        request(vec![history]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(provider.requests().len(), 1);
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ContextAccountingUpdated(_)))
    );
}

#[tokio::test]
async fn issue91_later_missing_usage_retires_anchor_with_monotonic_revision() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("a.txt"), "first").unwrap();
    fs::write(project.path().join("b.txt"), "second").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "one".into(),
                name: "read".into(),
                input_json: r#"{"path":"a.txt"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 500,
                output: 20,
                cache_read: 100,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "two".into(),
                name: "read".into(),
                input_json: r#"{"path":"b.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(20_000))]);
    req.context_budget = Some(ContextBudget::new(60_000, 1_000, true).unwrap());
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    assert_eq!(provider.requests().len(), 3);
    let decisions = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ContextAccountingUpdated(decision)
                if decision.stage == ContextAccountingStage::PrimaryPreflight =>
            {
                Some(decision)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(decisions.len(), 3);
    assert_eq!(
        decisions
            .iter()
            .map(|decision| decision.source)
            .collect::<Vec<_>>(),
        vec![
            ContextAccountingSource::Estimated,
            ContextAccountingSource::UsageAnchored,
            ContextAccountingSource::Estimated,
        ]
    );
    assert_eq!(
        decisions
            .iter()
            .map(|decision| decision.revision)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[tokio::test]
async fn issue91_duplicate_done_cannot_anchor_a_following_tool_round() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("large.txt"), "z".repeat(20_000)).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-duplicate-done".into(),
                name: "read".into(),
                input_json: r#"{"path":"large.txt"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 500,
                output: 10,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(20_000))]);
    req.context_budget = Some(ContextBudget::new(11_000, 1_000, true).unwrap());
    req.context_source_version = Some(1);
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: calls.clone(),
        messages: vec![ChatMessage::new(ChatRole::User, "summary")],
        source_version: 2,
    }));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ContextAccountingUpdated(ContextAccountingDecision {
            source: ContextAccountingSource::Estimated,
            revision: 1,
            ..
        })
    )));
}

struct FixedHook {
    calls: Arc<AtomicUsize>,
    decision: Option<RuntimeUserDecision>,
}

impl RuntimePermissionHook for FixedHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let decision = self.decision.clone();
        async move {
            match decision {
                Some(decision) => Ok(decision),
                None => futures::future::pending().await,
            }
        }
        .boxed()
    }
}

struct ProbeHook {
    fail: bool,
    token: Arc<Mutex<Option<CancellationToken>>>,
}

impl RuntimePermissionHook for ProbeHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        if let Ok(mut stored) = self.token.lock() {
            *stored = Some(cancel);
        }
        let fail = self.fail;
        async move {
            if fail {
                Err(VegaError::Tool {
                    tool: "permission".to_string(),
                    message: "closed".to_string(),
                })
            } else {
                Ok(RuntimeUserDecision::Once)
            }
        }
        .boxed()
    }
}

fn tool_config(
    run_mode: RuntimeRunMode,
    permission_mode: RuntimePermissionMode,
    checkpoint_root: PathBuf,
) -> RuntimeToolConfig {
    RuntimeToolConfig::new(
        run_mode,
        permission_mode,
        "project-1".to_string(),
        "thread-1".to_string(),
        checkpoint_root,
        Vec::new(),
    )
}

fn request(history: Vec<ChatMessage>) -> AgentRequest {
    AgentRequest {
        model: "mock".to_string(),
        system_prompt: "Be precise.".to_string(),
        history,
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

#[tokio::test]
async fn issue76_model_output_capacity_does_not_force_a_generation_cap() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "hello")]);
    req.context_budget = Some(ContextBudget::new(428_000, 128_000, true).unwrap());
    req.context_source_version = Some(1);
    run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].max_tokens, None);
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::System)
            .count(),
        1
    );
    let estimate = crate::estimate_wire_context(&requests[0].messages, &requests[0].tools).unwrap();
    let separate = crate::estimate_chat_context(
        "Be precise.",
        &[ChatMessage::new(ChatRole::User, "hello")],
        &requests[0].tools,
    )
    .unwrap();
    assert_eq!(estimate.input_tokens, separate.input_tokens);
}

#[tokio::test]
async fn issue76_explicit_generation_cap_is_bounded_by_model_output_capacity() {
    for (requested, expected) in [(512, 512), (200_000, 128_000)] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "hello")]);
        req.max_tokens = Some(requested);
        req.context_budget = Some(ContextBudget::new(428_000, 128_000, true).unwrap());
        run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].max_tokens, Some(expected));
    }
}

#[tokio::test]
async fn issue76_over_budget_has_zero_provider_requests_without_auto_compaction() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(4_000))]);
    req.context_budget = Some(ContextBudget::new(1_000, 100, false).unwrap());
    let error = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .expect_err("over-budget request must fail before provider");
    assert!(matches!(
        error,
        VegaError::Context(ContextRuntimeError::OverLimit { .. })
    ));
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn issue76_auto_compaction_precedes_tool_round_and_is_once_per_source() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-1".into(),
                name: "read".into(),
                input_json: r#"{"path":"missing"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = RecordingCompactionHook {
        calls: calls.clone(),
        messages: vec![ChatMessage::new(ChatRole::User, "historical summary")],
        source_version: 8,
    };
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "x".repeat(14_000))]);
    req.context_budget = Some(ContextBudget::new(5_000, 500, true).unwrap());
    req.context_source_version = Some(7);
    req.context_compaction_hook = Some(Arc::new(hook));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.requests().len(), 2);
    assert!(outcome.executed_tool_call_count <= 1);
    let second = &provider.requests()[1].messages;
    assert!(
        second
            .iter()
            .any(|message| { message.tool_calls.iter().any(|call| call.id == "read-1") })
    );
}

#[tokio::test]
async fn issue76_auto_compaction_triggers_after_tool_result_without_reexecution() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("large.txt"), "z".repeat(3_000)).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-after".into(),
                name: "read".into(),
                input_json: r#"{"path":"large.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let hook = TailPreservingCompactionHook {
        calls: calls.clone(),
        observed: observed.clone(),
    };
    let mut req = request(vec![
        ChatMessage::new(ChatRole::User, "old constraint ".repeat(2_200)),
        ChatMessage::new(ChatRole::Assistant, "old answer"),
        ChatMessage::new(ChatRole::User, "current goal"),
    ]);
    let initial_wire = std::iter::once(ChatMessage::new(
        ChatRole::System,
        req.system_prompt.clone(),
    ))
    .chain(req.history.iter().cloned())
    .collect::<Vec<_>>();
    let initial_tokens =
        crate::estimate_wire_context(&initial_wire, &tool_definitions(RuntimeRunMode::Ask))
            .unwrap()
            .input_tokens;
    // Trigger after the file result, never merely because the tool schema grew.
    let input_budget = (initial_tokens + 100) * 5 / 4 + 1;
    req.context_budget = Some(ContextBudget::new(input_budget + 1_000, 1_000, true).unwrap());
    req.context_source_version = Some(7);
    req.context_compaction_hook = Some(Arc::new(hook));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.executed_tool_call_count, 1);
    let observed = observed.lock().unwrap();
    let assistant = observed
        .iter()
        .find(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-after")
        })
        .expect("hook sees the live assistant tool group");
    assert_eq!(assistant.tool_calls[0].id, "read-after");
    assert!(observed.iter().any(|message| {
        message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some("read-after")
    }));
    assert_eq!(provider.requests().len(), 2);
}

async fn run_bash_permission_case(
    permission_mode: RuntimePermissionMode,
    exact_rule: bool,
    decision: RuntimeUserDecision,
    command: &str,
) -> (AgentOutcome, usize) {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let expected_command = command.to_owned();
    let executions = Arc::new(AtomicUsize::new(0));
    let recorded = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |actual, full_access, _| {
            assert_eq!(actual, expected_command);
            assert_eq!(
                full_access,
                permission_mode == RuntimePermissionMode::FullAccess
            );
            recorded.fetch_add(1, Ordering::SeqCst);
            let exit_code = if actual == "git push --force" { 1 } else { 0 };
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text: "fixture output".into(),
                    exit_code,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bash-case".into(),
                name: "bash".into(),
                input_json: serde_json::json!({ "cmd": command }).to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(Vec::new());
    let mut config = tool_config(RuntimeRunMode::Execute, permission_mode, checkpoint);
    if exact_rule {
        config.exact_rules.push(RuntimeExactRule {
            tool: RuntimeMutatingTool::Bash,
            pattern: command.to_string(),
        });
    }
    req.tool_config = config;
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedHook {
        calls: calls.clone(),
        decision: Some(decision),
    };
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let successful = outcome.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.status == RuntimeToolStatus::Success)).count();
    assert_eq!(executions.load(Ordering::SeqCst), successful);
    (outcome, calls.load(Ordering::SeqCst))
}

// ─── S7-T38 pricing pipeline ────────────────────────────────────────

fn priced_catalog() -> PricingCatalog {
    PricingCatalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "quote-model".to_string(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".to_string(),
            output_usd_per_million: "2".to_string(),
            cache_read_usd_per_million: "0.1".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        },
        max_standard_input_tokens: Some(2_000_000),
        schedule: None,
    }])
    .unwrap()
}

async fn assert_reasoning_follow_up(
    reasoning: FrozenReasoning,
    expected_reasoning_content: Option<&str>,
) {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("private chain".into()),
            ProviderEvent::ToolUse {
                id: "read-1".into(),
                name: "read".into(),
                input_json: r#"{"path":"source.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("done".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let mut req = request(Vec::new());
    req.reasoning = Some(reasoning.clone());
    req.tool_config = RuntimeToolConfig::new(
        RuntimeRunMode::Ask,
        RuntimePermissionMode::ReadOnly,
        "project-1".into(),
        "thread-1".into(),
        data.path().join("checkpoints"),
        Vec::new(),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].reasoning, Some(reasoning.clone()));
    assert_eq!(requests[1].reasoning, Some(reasoning));
    let assistant = requests[1]
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .expect("tool round assistant message");
    assert_eq!(
        assistant.reasoning_content.as_deref(),
        expected_reasoning_content
    );
}

#[tokio::test]
async fn non_preserving_reasoning_is_omitted_from_tool_round() {
    assert_reasoning_follow_up(
        FrozenReasoning {
            provider: "openai".into(),
            model: "mock".into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            choice: ReasoningChoice::Effort("low".into()),
            supports_disabled: false,
            preserve_reasoning_content: false,
            disabled_wire: None,
            declared_efforts: vec!["low".into()],
        },
        None,
    )
    .await;
}

#[tokio::test]
async fn preserving_reasoning_is_replayed_in_tool_round_memory_only() {
    assert_reasoning_follow_up(
        FrozenReasoning {
            provider: "zhipu".into(),
            model: "mock".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            choice: ReasoningChoice::Effort("low".into()),
            supports_disabled: false,
            preserve_reasoning_content: true,
            disabled_wire: None,
            declared_efforts: vec!["low".into()],
        },
        Some("private chain"),
    )
    .await;
}

#[tokio::test]
async fn unknown_reasoning_protocol_is_provider_default_and_omits_replay() {
    assert_reasoning_follow_up(FrozenReasoning::unknown("custom", "mock"), None).await;
}

#[tokio::test]
async fn unknown_reasoning_replay_claim_fails_before_provider_call() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![]);
    let mut req = request(Vec::new());
    let mut reasoning = FrozenReasoning::unknown("custom", "mock");
    reasoning.preserve_reasoning_content = true;
    req.reasoning = Some(reasoning);
    let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
    assert!(matches!(
        result,
        Err(VegaError::ReasoningSelectionInvalid { .. })
    ));
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn mismatched_disabled_wire_fails_before_provider_call() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![]);
    let mut req = request(Vec::new());
    req.reasoning = Some(FrozenReasoning {
        provider: "openai".into(),
        model: "mock".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: true,
        preserve_reasoning_content: false,
        disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
        declared_efforts: vec!["low".into()],
    });
    let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
    assert!(matches!(
        result,
        Err(VegaError::ReasoningSelectionInvalid { .. })
    ));
    assert!(provider.requests().is_empty());
}

#[test]
fn reasoning_budget_boundaries_are_byte_exact() {
    assert!(reasoning_budget_violation(REASONING_DELTA_MAX_BYTES, 0, 0).is_none());
    assert!(matches!(
        reasoning_budget_violation(REASONING_DELTA_MAX_BYTES + 1, 0, 0),
        Some((ReasoningBudgetScope::Delta, observed)) if observed == REASONING_DELTA_MAX_BYTES + 1
    ));
    assert!(matches!(
        reasoning_budget_violation(1, REASONING_TURN_MAX_BYTES, 0),
        Some((ReasoningBudgetScope::Turn, observed)) if observed == REASONING_TURN_MAX_BYTES + 1
    ));
    assert!(matches!(
        reasoning_budget_violation(1, 0, REASONING_RUN_MAX_BYTES),
        Some((ReasoningBudgetScope::Run, observed)) if observed == REASONING_RUN_MAX_BYTES + 1
    ));
}

mod claude_compaction;
