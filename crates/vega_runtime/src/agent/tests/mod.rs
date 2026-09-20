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
    req.context_budget = Some(ContextBudget::new(16_000, 1_000, true).unwrap());
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
    let tools = vega_tools::Tools::new(project.path()).unwrap();
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
