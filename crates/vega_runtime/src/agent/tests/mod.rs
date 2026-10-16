mod cua_batch_projection;
mod cua_read_progress;
mod cua_usability;
mod steering;
use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tempfile::tempdir;

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
mod provider_overflow;
mod reasoning_budget;
mod response_phases;
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

// ─── pricing pipeline ────────────────────────────────────────

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

mod claude_compaction;
