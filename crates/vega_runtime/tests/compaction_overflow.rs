use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentOutcome, AgentRequest, ChatMessage, ChatRequest, ChatRole, ContextBudget, ContextCheck,
    ContextCompactionFailure, ContextCompactionHook, ContextCompactionRequest,
    ContextCompactionResult, ContextCompactionTrigger, ContextRuntimeError, EventStream, Provider,
    ProviderEvent, ReasoningReplayPolicy, ReplayFrame, RuntimeEvent, RuntimeToolConfig,
    RuntimeToolStatus, StopReason, VegaError, estimate_wire_context, project_model_messages,
    run_agent, tool_definitions,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum ErrorSite {
    Creation,
    Stream,
}

enum Reply {
    Error(VegaError),
    Events(Vec<Result<ProviderEvent, VegaError>>),
}

struct ScriptedProvider {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<Vec<ChatMessage>>>,
}

impl ScriptedProvider {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<Vec<ChatMessage>> {
        assert!(self.replies.lock().unwrap().is_empty());
        self.requests.lock().unwrap().clone()
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request.messages);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected primary dispatch");
        Box::pin(async move {
            match reply {
                Reply::Error(error) => Err(error),
                Reply::Events(events) => {
                    let stream: EventStream = Box::pin(futures::stream::iter(events));
                    Ok(stream)
                }
            }
        })
    }
}

struct ScriptedHook {
    results: Mutex<VecDeque<ContextCompactionResult>>,
    requests: Mutex<Vec<ContextCompactionRequest>>,
}

impl ScriptedHook {
    fn new(results: impl IntoIterator<Item = ContextCompactionResult>) -> Arc<Self> {
        Arc::new(Self {
            results: Mutex::new(results.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<ContextCompactionRequest> {
        assert!(self.results.lock().unwrap().is_empty());
        self.requests.lock().unwrap().clone()
    }
}

impl ContextCompactionHook for ScriptedHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.requests.lock().unwrap().push(request);
        let result = self
            .results
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected compaction recovery");
        Box::pin(async move { Ok(result) })
    }
}

fn overflow_error(token_gap: Option<u64>) -> VegaError {
    let mut detail = serde_json::json!({"code": "prompt_too_long"});
    if let Some(gap) = token_gap {
        detail["requested_tokens"] = (100_000 + gap).into();
        detail["max_context_tokens"] = 100_000.into();
    }
    VegaError::Provider {
        status: Some(400),
        message: serde_json::json!({"error": detail}).to_string(),
        retryable: false,
    }
}

fn error_reply(site: ErrorSite, error: VegaError) -> Reply {
    match site {
        ErrorSite::Creation => Reply::Error(error),
        ErrorSite::Stream => Reply::Events(vec![Err(error)]),
    }
}

fn overflow(site: ErrorSite, gap: Option<u64>) -> Reply {
    error_reply(site, overflow_error(gap))
}

fn success() -> Reply {
    Reply::Events(vec![
        Ok(ProviderEvent::TextDelta("Recovered.".into())),
        Ok(ProviderEvent::Done {
            stop_reason: StopReason::End,
        }),
    ])
}

fn compacted(
    source_version: u64,
    retained: Option<usize>,
    padding: usize,
) -> ContextCompactionResult {
    ContextCompactionResult {
        messages: vec![
            ChatMessage::new(
                ChatRole::User,
                format!(
                    "Summary {source_version}: {}",
                    "summary context ".repeat(padding)
                ),
            ),
            ChatMessage::new(ChatRole::Assistant, "Retained response"),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ],
        source_version,
        source_fingerprint: Some(format!("source-{source_version}")),
        replay_cold_prefix_messages: Some(2),
        replay_interleaved_user_boundary: None,
        reactive_retained_groups: retained,
        usages: Vec::new(),
        usage_complete: true,
        settled_usage: None,
    }
}

fn request(hook: Arc<ScriptedHook>) -> AgentRequest {
    AgentRequest {
        model: "synthetic".into(),
        system_prompt: "Complete the task".into(),
        history: vec![
            ChatMessage::new(ChatRole::User, "Original task"),
            ChatMessage::new(ChatRole::Assistant, "old context ".repeat(4_000)),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: Some(ContextBudget::new(100_000, 10_000, true).unwrap()),
        context_source_version: Some(1),
        context_source_fingerprint: Some("initial".into()),
        context_operation_id: Some("synthetic-operation".into()),
        context_compaction_hook: Some(hook),
    }
}

async fn execute(
    provider: &ScriptedProvider,
    request: AgentRequest,
) -> Result<AgentOutcome, Box<dyn std::error::Error>> {
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    Ok(run_agent(provider, &tools, request, CancellationToken::new()).await?)
}

fn trigger(gap: Option<u64>, deeper: bool) -> ContextCompactionTrigger {
    ContextCompactionTrigger::InputOverflow {
        token_gap: gap,
        deeper,
    }
}

fn assert_triggers(requests: &[ContextCompactionRequest], expected: &[ContextCompactionTrigger]) {
    assert_eq!(
        requests
            .iter()
            .map(|request| request.trigger)
            .collect::<Vec<_>>(),
        expected
    );
}

fn assert_projection(actual: &[ChatMessage], expected: &ContextCompactionResult) {
    assert_message_contents(actual, &expected.messages);
}

fn assert_dispatched_projection(actual: &[ChatMessage], expected: &ContextCompactionResult) {
    let mut system = ChatMessage::new(ChatRole::System, "Complete the task");
    system.attach_replay_frame(ReplayFrame::new(
        expected.replay_cold_prefix_messages.unwrap() + 1,
        ReasoningReplayPolicy::Unknown,
    ));
    let mut messages = vec![system];
    messages.extend(expected.messages.clone());
    let projected = project_model_messages(&messages).unwrap();
    assert_message_contents(actual, &projected[1..]);
}

fn assert_message_contents(actual: &[ChatMessage], expected: &[ChatMessage]) {
    assert_eq!(
        actual
            .iter()
            .map(|message| (&message.role, &message.content))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|message| (&message.role, &message.content))
            .collect::<Vec<_>>()
    );
}

fn assert_overflow_failure(outcome: &AgentOutcome, gap: Option<u64>) {
    assert!(outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
    let errors = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::Error(error) => Some(error.context_input_overflow()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(errors, vec![Some(gap)]);
}

#[tokio::test]
async fn settled_compaction_usage_updates_status_without_emitting_another_bill() -> TestResult {
    use vega_runtime::{ContextCompactionPhase, ContextCompactionUsageState};
    for state in [
        ContextCompactionUsageState::Known { priced: true },
        ContextCompactionUsageState::Known { priced: false },
        ContextCompactionUsageState::Unknown,
    ] {
        let mut result = compacted(2, None, 60);
        result.usage_complete = state != ContextCompactionUsageState::Unknown;
        result.settled_usage = Some(state);
        let hook = ScriptedHook::new([result]);
        let provider = ScriptedProvider::new([overflow(ErrorSite::Creation, None), success()]);
        let outcome = execute(&provider, request(hook.clone())).await?;
        assert!(!outcome.failed);
        assert_eq!(outcome.final_text, "Recovered.");
        let successful = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ContextCompactionStatusUpdated { status }
                    if status.phase == ContextCompactionPhase::Succeeded =>
                {
                    Some(status.usage)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(successful, vec![state]);
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ContextCompactionUsageUpdated { .. }))
        );
        assert_eq!(provider.requests().len(), 2);
        assert_eq!(hook.requests().len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn adjacent_overflow_gets_exactly_one_deeper_pass_at_both_provider_boundaries() -> TestResult
{
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        for gap in [Some(37), None] {
            let fresh = compacted(2, Some(3), 600);
            let deeper = compacted(3, Some(2), 60);
            let hook = ScriptedHook::new([fresh.clone(), deeper.clone()]);
            let provider = ScriptedProvider::new([
                overflow(site, Some(400)),
                overflow(site, gap),
                overflow(site, Some(1)),
            ]);
            let outcome = execute(&provider, request(hook.clone())).await?;
            assert_overflow_failure(&outcome, Some(1));
            assert!(outcome.final_text.is_empty());
            let requests = provider.requests();
            assert_eq!(requests.len(), 3);
            assert_dispatched_projection(&requests[1][1..], &fresh);
            assert_dispatched_projection(&requests[2][1..], &deeper);
            assert_projection(&outcome.messages[1..], &deeper);
            let compactions = hook.requests();
            assert_triggers(
                &compactions,
                &[trigger(Some(400), false), trigger(gap, true)],
            );
            assert_eq!(compactions[0].source_version, 1);
            assert_eq!(compactions[1].source_version, fresh.source_version);
            assert_eq!(compactions[1].source_fingerprint, fresh.source_fingerprint);
            assert_projection(&compactions[1].messages, &fresh);
            assert!(
                estimate_wire_context(&requests[2], &compactions[1].tools)?.input_tokens
                    < estimate_wire_context(&requests[1], &compactions[0].tools)?.input_tokens
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn adjacent_deeper_pass_can_complete_the_primary_response() -> TestResult {
    for (first, second) in [
        (ErrorSite::Creation, ErrorSite::Stream),
        (ErrorSite::Stream, ErrorSite::Creation),
    ] {
        let fresh = compacted(2, Some(2), 600);
        let deeper = compacted(3, Some(1), 60);
        let hook = ScriptedHook::new([fresh.clone(), deeper.clone()]);
        let provider =
            ScriptedProvider::new([overflow(first, None), overflow(second, Some(9)), success()]);
        let outcome = execute(&provider, request(hook.clone())).await?;
        assert!(!outcome.failed);
        assert!(!outcome.interrupted);
        assert_eq!(outcome.final_text, "Recovered.");
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(
            outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Finished(_)))
        );
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Error(_)))
        );
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_dispatched_projection(&requests[1][1..], &fresh);
        assert_dispatched_projection(&requests[2][1..], &deeper);
        assert_eq!(outcome.messages.last().unwrap().content, "Recovered.");
        assert_triggers(
            &hook.requests(),
            &[trigger(None, false), trigger(Some(9), true)],
        );
    }
    Ok(())
}

#[tokio::test]
async fn retained_group_metadata_below_two_never_grants_a_deeper_pass() -> TestResult {
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        for retained in [None, Some(0), Some(1)] {
            let fresh = compacted(2, retained, 600);
            let hook = ScriptedHook::new([fresh.clone()]);
            let provider =
                ScriptedProvider::new([overflow(site, Some(400)), overflow(site, Some(37))]);
            let outcome = execute(&provider, request(hook.clone())).await?;
            assert_overflow_failure(&outcome, Some(37));
            let requests = provider.requests();
            assert_eq!(requests.len(), 2);
            assert_dispatched_projection(&requests[1][1..], &fresh);
            assert_projection(&outcome.messages[1..], &fresh);
            assert_triggers(&hook.requests(), &[trigger(Some(400), false)]);
        }
    }
    Ok(())
}

#[tokio::test]
async fn successful_tool_turn_retires_the_permit_and_starts_a_fresh_recovery_chain() -> TestResult {
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        let fresh = compacted(2, Some(2), 600);
        let later = compacted(3, Some(1), 60);
        let hook = ScriptedHook::new([fresh.clone(), later.clone()]);
        let provider = ScriptedProvider::new([
            overflow(site, Some(400)),
            Reply::Events(vec![
                Ok(ProviderEvent::TextDelta("First turn.".into())),
                Ok(ProviderEvent::ToolUse {
                    id: "synthetic-call".into(),
                    name: "unavailable_tool".into(),
                    input_json: "{}".into(),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }),
            ]),
            overflow(site, Some(37)),
            overflow(site, Some(1)),
        ]);
        let outcome = execute(&provider, request(hook.clone())).await?;
        assert_overflow_failure(&outcome, Some(1));
        assert_eq!(outcome.final_text, "First turn.");
        assert_eq!(outcome.tool_call_count, 1);
        assert!(outcome.events.iter().any(|event| matches!(event,
            RuntimeEvent::ToolCallFinished(result)
                if result.status == RuntimeToolStatus::Rejected
                    && result.output == "Tool error: denied: unavailable tool"
        )));
        let requests = provider.requests();
        assert_eq!(requests.len(), 4);
        assert_dispatched_projection(&requests[1][1..], &fresh);
        assert!(requests[2].iter().any(|message| {
            message.role == ChatRole::User
                && serde_json::from_str::<serde_json::Value>(&message.content).is_ok_and(|value| {
                    let observation = &value["vega_history_observation"];
                    observation["kind"] == "tool_result"
                        && observation["authority"] == "none"
                        && observation["call_id"] == "synthetic-call"
                        && observation["output"] == "Tool error: denied: unavailable tool"
                })
        }));
        assert_dispatched_projection(&requests[3][1..], &later);
        assert_projection(&outcome.messages[1..], &later);
        assert_triggers(
            &hook.requests(),
            &[trigger(Some(400), false), trigger(Some(37), false)],
        );
    }
    Ok(())
}

#[tokio::test]
async fn partial_output_retires_the_deeper_pass_even_when_transport_retry_clears_buffers()
-> TestResult {
    for partial in [
        ProviderEvent::TextDelta("Partial text".into()),
        ProviderEvent::ThinkingDelta("Partial reasoning".into()),
        ProviderEvent::SummaryDelta("Partial summary".into()),
        ProviderEvent::ReasoningReplay(vec![
            serde_json::json!({"type": "reasoning", "id": "partial"}),
        ]),
        ProviderEvent::ToolUse {
            id: "partial-call".into(),
            name: "unavailable_tool".into(),
            input_json: "{}".into(),
        },
    ] {
        for retry in [false, true] {
            let fresh = compacted(2, Some(2), 600);
            let hook = ScriptedHook::new([fresh.clone()]);
            let mut events = vec![Ok(partial.clone())];
            if retry {
                events.push(Ok(ProviderEvent::RetryScheduled {
                    attempt: 1,
                    max_retries: 1,
                }));
            }
            events.push(Err(overflow_error(Some(37))));
            let provider = ScriptedProvider::new([
                overflow(ErrorSite::Creation, Some(400)),
                Reply::Events(events),
            ]);
            let outcome = execute(&provider, request(hook.clone())).await?;
            assert_overflow_failure(&outcome, Some(37));
            assert_eq!(
                outcome.final_text,
                if !retry && matches!(partial, ProviderEvent::TextDelta(_)) {
                    "Partial text"
                } else {
                    ""
                }
            );
            assert_eq!(outcome.tool_call_count, 0);
            assert_eq!(
                outcome
                    .events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::ResponseRetrying { .. })),
                retry
            );
            if !retry {
                match &partial {
                    ProviderEvent::ThinkingDelta(text) => assert!(outcome.events.iter().any(|event|
                        matches!(event, RuntimeEvent::ThinkingDelta(actual) if actual == text)
                    )),
                    ProviderEvent::SummaryDelta(text) => assert!(outcome.events.iter().any(|event|
                        matches!(event, RuntimeEvent::SummaryDelta(actual) if actual == text)
                    )),
                    _ => {}
                }
            }
            assert_eq!(provider.requests().len(), 2);
            assert_projection(&outcome.messages[1..], &fresh);
            assert_triggers(&hook.requests(), &[trigger(Some(400), false)]);
        }
    }
    Ok(())
}

#[tokio::test]
async fn ordinary_errors_after_compaction_do_not_use_the_deeper_pass() -> TestResult {
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        let fresh = compacted(2, Some(2), 600);
        let hook = ScriptedHook::new([fresh.clone()]);
        let provider = ScriptedProvider::new([
            overflow(site, Some(400)),
            error_reply(
                site,
                VegaError::Provider {
                    status: Some(503),
                    message: "synthetic unavailable".into(),
                    retryable: false,
                },
            ),
        ]);
        let outcome = execute(&provider, request(hook.clone())).await?;
        assert!(outcome.failed);
        assert!(!outcome.interrupted);
        assert!(outcome.final_text.is_empty());
        assert_eq!(outcome.executed_tool_call_count, 0);
        let errors = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::Error(error) => Some(error.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            VegaError::Provider {
                status: Some(503),
                ..
            }
        ));
        assert_eq!(errors[0].context_input_overflow(), None);
        assert_eq!(provider.requests().len(), 2);
        assert_projection(&outcome.messages[1..], &fresh);
        assert_triggers(&hook.requests(), &[trigger(Some(400), false)]);
    }
    Ok(())
}

#[tokio::test]
async fn transport_retry_without_output_retires_the_adjacent_deeper_pass() -> TestResult {
    let fresh = compacted(2, Some(2), 600);
    let hook = ScriptedHook::new([fresh.clone()]);
    let provider = ScriptedProvider::new([
        overflow(ErrorSite::Creation, Some(400)),
        Reply::Events(vec![
            Ok(ProviderEvent::RetryScheduled {
                attempt: 1,
                max_retries: 1,
            }),
            Err(overflow_error(Some(37))),
        ]),
    ]);
    let outcome = execute(&provider, request(hook.clone())).await?;
    assert_overflow_failure(&outcome, Some(37));
    assert_eq!(provider.requests().len(), 2);
    assert_projection(&outcome.messages[1..], &fresh);
    assert_triggers(&hook.requests(), &[trigger(Some(400), false)]);
    Ok(())
}

#[tokio::test]
async fn threshold_compaction_cannot_grant_a_deeper_pass_even_with_retained_metadata() -> TestResult
{
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        let threshold = compacted(2, Some(3), 600);
        let fresh = compacted(3, Some(1), 60);
        let hook = ScriptedHook::new([threshold.clone(), fresh.clone()]);
        let mut request = request(hook.clone());
        let mut messages = vec![ChatMessage::new(
            ChatRole::System,
            request.system_prompt.clone(),
        )];
        messages.extend(request.history.clone());
        let estimate =
            estimate_wire_context(&messages, &tool_definitions(request.tool_config.run_mode))?;
        let budget = request
            .context_budget
            .unwrap()
            .with_policy_input_limit(estimate.input_tokens)?;
        assert_eq!(budget.check(estimate)?, ContextCheck::Triggered);
        request.context_budget = Some(budget);
        let provider = ScriptedProvider::new([overflow(site, Some(400)), overflow(site, Some(37))]);
        let outcome = execute(&provider, request).await?;
        assert_overflow_failure(&outcome, Some(37));
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_dispatched_projection(&requests[0][1..], &threshold);
        assert_dispatched_projection(&requests[1][1..], &fresh);
        assert_projection(&outcome.messages[1..], &fresh);
        assert_triggers(
            &hook.requests(),
            &[
                ContextCompactionTrigger::Threshold,
                trigger(Some(400), false),
            ],
        );
    }
    Ok(())
}

#[tokio::test]
async fn overflow_recovery_requires_history_automatic_compaction_and_a_hook() -> TestResult {
    for site in [ErrorSite::Creation, ErrorSite::Stream] {
        for gate in ["initial_input", "automatic_disabled", "missing_hook"] {
            let hook = ScriptedHook::new([]);
            let mut request = request(hook.clone());
            match gate {
                "initial_input" => {
                    request.history = vec![ChatMessage::new(ChatRole::User, "Original task")]
                }
                "automatic_disabled" => {
                    request.context_budget = Some(
                        request
                            .context_budget
                            .unwrap()
                            .without_automatic_compaction(),
                    )
                }
                "missing_hook" => request.context_compaction_hook = None,
                _ => unreachable!(),
            }
            let provider = ScriptedProvider::new([overflow(site, Some(400))]);
            let outcome = execute(&provider, request).await?;
            assert_overflow_failure(&outcome, Some(400));
            assert_eq!(provider.requests().len(), 1);
            assert!(hook.requests().is_empty());
        }
    }
    Ok(())
}

#[tokio::test]
async fn invalid_compaction_result_cannot_dispatch_an_eligible_retry() -> TestResult {
    let mut invalid = compacted(2, Some(2), 600);
    invalid.messages[0].role = ChatRole::System;
    let hook = ScriptedHook::new([invalid]);
    let provider = ScriptedProvider::new([overflow(ErrorSite::Creation, Some(400))]);
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let result = run_agent(
        &provider,
        &tools,
        request(hook.clone()),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(VegaError::Context(
            ContextRuntimeError::SystemMessageInResult
        ))
    ));
    assert_eq!(provider.requests().len(), 1);
    assert_triggers(&hook.requests(), &[trigger(Some(400), false)]);
    Ok(())
}
