use futures::future::BoxFuture;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRequest, ChatRole, ChatToolCall, ContextBudget,
    ContextCompactionFailure, ContextCompactionHook, ContextCompactionRequest,
    ContextCompactionResult, EventStream, MicroCompactionResult, MicroCompactionTarget, Provider,
    ProviderEvent, RuntimeToolConfig, StopReason, VegaError, apply_micro_compaction, run_agent,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct MicroHook {
    calls: Mutex<usize>,
    invalid: bool,
}

impl ContextCompactionHook for MicroHook {
    fn micro_compact<'a>(
        &'a self,
        request: vega_runtime::MicroCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<Option<MicroCompactionResult>, VegaError>> {
        *self.calls.lock().unwrap() += 1;
        let targets = (0..35)
            .map(|index| MicroCompactionTarget {
                call_id: format!("call-{index}"),
                status: "success".into(),
            })
            .collect::<Vec<_>>();
        let mut next = request.wire_messages();
        let before = vega_runtime::estimate_wire_context(&next, &request.context.tools)
            .unwrap()
            .input_tokens;
        apply_micro_compaction(&mut next[1..], &targets).unwrap();
        let after = vega_runtime::estimate_wire_context(&next, &request.context.tools)
            .unwrap()
            .input_tokens;
        let savings = before - after;
        assert!(savings >= 20_000);
        let invalid = self.invalid;
        Box::pin(async move {
            Ok(Some(MicroCompactionResult {
                source_version: 1,
                source_fingerprint: "after-micro".into(),
                targets,
                estimated_savings: if invalid { savings + 1 } else { savings },
            }))
        })
    }

    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        panic!("micro should avoid the summary request")
    }
}

struct RecordingProvider(Mutex<Vec<ChatRequest>>);
impl Provider for RecordingProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            let stream: EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("completed".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        })
    }
}

#[tokio::test]
async fn primary_preflight_applies_micro_without_summary_and_rejects_false_savings() -> TestResult {
    for invalid in [false, true] {
        let hook = Arc::new(MicroHook {
            calls: Mutex::new(0),
            invalid,
        });
        let provider = RecordingProvider(Mutex::new(Vec::new()));
        let mut history = vec![ChatMessage::new(ChatRole::User, "Original task")];
        for index in 0..40 {
            let id = format!("call-{index}");
            history.push(ChatMessage::assistant_with_tools(
                "",
                vec![ChatToolCall {
                    id: id.clone(),
                    name: "read".into(),
                    input_json: "{\"path\":\"notes.txt\",\"offset\":null,\"limit\":null}".into(),
                }],
            ));
            history.push(ChatMessage::tool_result(
                id,
                format!("RESULT_{index} {}", format!("detail-{index} ").repeat(1500)),
            ));
        }
        history.push(ChatMessage::new(ChatRole::User, "Continue"));
        let request = AgentRequest {
            model: "synthetic".into(),
            system_prompt: "system".into(),
            history,
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: RuntimeToolConfig::default(),
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: Some(ContextBudget::new(90_000, 20_000, true)?),
            context_source_version: Some(1),
            context_source_fingerprint: Some("before-micro".into()),
            context_operation_id: Some("owner".into()),
            context_compaction_hook: Some(hook.clone()),
        };
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let result = run_agent(&provider, &tools, request, CancellationToken::new()).await;
        assert_eq!(*hook.calls.lock().unwrap(), 1);
        let requests = provider.0.lock().unwrap();
        if invalid {
            assert!(matches!(
                result,
                Err(VegaError::Context(
                    vega_runtime::ContextRuntimeError::InvalidProjection
                ))
            ));
            assert!(requests.is_empty());
        } else {
            let result = result?;
            assert!(!result.failed);
            assert_eq!(requests.len(), 1);
            let inputs = requests[0]
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(inputs.contains("vega_compacted_tool_result"));
            assert!(!inputs.contains("detail-0 "));
            assert!(inputs.contains("detail-39 "));
        }
    }
    Ok(())
}

struct NoCompactionHook;

impl ContextCompactionHook for NoCompactionHook {
    fn micro_compact<'a>(
        &'a self,
        _request: vega_runtime::MicroCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<Option<MicroCompactionResult>, VegaError>> {
        panic!("suppressed reasoning must not trigger micro")
    }

    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        panic!("suppressed reasoning must not trigger summary")
    }
}

struct OneSteering(Mutex<Option<vega_runtime::RuntimeSteeringInput>>);

impl vega_runtime::RuntimeSteeringSource for OneSteering {
    fn take(&self) -> BoxFuture<'_, Result<Option<vega_runtime::RuntimeSteeringInput>, VegaError>> {
        let input = self.0.lock().unwrap().take();
        Box::pin(async { Ok(input) })
    }
}

struct GlmProvider(Mutex<Vec<ChatRequest>>);

impl Provider for GlmProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let mut requests = self.0.lock().unwrap();
        let first = requests.is_empty();
        assert!(requests.len() < 2);
        requests.push(request);
        Box::pin(async move {
            let mut events = Vec::new();
            if first {
                events.push(Ok(ProviderEvent::ChatReasoningContent(
                    vega_runtime::ChatReasoningPresence::NonEmpty("old reasoning ".repeat(20_000)),
                )));
            }
            events.push(Ok(ProviderEvent::TextDelta("completed".into())));
            events.push(Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }));
            let stream: EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

#[tokio::test]
async fn glm_steering_suppresses_prior_reasoning_before_micro_preflight() -> TestResult {
    let provider = GlmProvider(Mutex::new(Vec::new()));
    let mut reasoning = vega_runtime::FrozenReasoning::unknown("synthetic", "glm");
    reasoning.protocol = vega_runtime::ReasoningProtocol::ZhipuChatCompletions;
    reasoning.replay_policy = vega_runtime::ReasoningReplayPolicy::GlmInterleaved;
    let steering = Arc::new(OneSteering(Mutex::new(Some(
        vega_runtime::RuntimeSteeringInput {
            original_content: "Continue".into(),
            user_message_seq: 3,
            id: "steering".into(),
            assistant_message_id: "second-owner".into(),
            message: ChatMessage::new(ChatRole::User, "Continue"),
        },
    ))));
    let budget = ContextBudget::new(90_000, 20_000, true)?;
    let request = AgentRequest {
        model: "glm".into(),
        system_prompt: "system".into(),
        history: vec![ChatMessage::new(ChatRole::User, "Original task")],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default().with_steering_source(steering),
        pricing_catalog: None,
        reasoning: Some(reasoning),
        model_snapshot: None,
        replay_frame: None,
        context_budget: Some(budget),
        context_source_version: Some(1),
        context_source_fingerprint: Some("before".into()),
        context_operation_id: Some("first-owner".into()),
        context_compaction_hook: Some(Arc::new(NoCompactionHook)),
    };
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let result = run_agent(&provider, &tools, request, CancellationToken::new()).await?;
    assert!(!result.failed);
    let requests = provider.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let second = &requests[1];
    assert!(
        vega_runtime::estimate_wire_context(&second.messages, &second.tools)?.input_tokens
            < budget.precompute_tokens()?
    );
    let raw = result
        .messages
        .iter()
        .find(|message| message.role == ChatRole::Assistant)
        .ok_or("live assistant missing")?;
    assert_eq!(
        raw.reasoning_content.as_ref().map(String::len),
        Some("old reasoning ".len() * 20_000)
    );
    assert!(
        vega_runtime::estimate_chat_context(
            &result.messages[0].content,
            &result.messages[1..],
            &second.tools
        )?
        .input_tokens
            >= budget.precompute_tokens()?
    );
    assert!(
        second
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .all(|message| message.reasoning_content.is_none())
    );
    assert_eq!(
        second.messages.last().ok_or("steering missing")?.content,
        "Continue"
    );
    Ok(())
}
