use std::sync::{Arc, Mutex};

use futures::{future::BoxFuture, stream};
use tokio_util::sync::CancellationToken;

use crate::{
    AgentOutcome, AgentRequest, ChatMessage, ChatRequest, ChatRole, ChatToolCall,
    ContextAccountingDecision, ContextAccountingSource, ContextAccountingStage, ContextBudget,
    ContextCompactionFailure, ContextCompactionHook, ContextCompactionRequest,
    ContextCompactionResult, ContextRuntimeError, EventStream, MICRO_KEEP_RECENT,
    MICRO_MIN_SAVINGS, MicroCompactionRequest, MicroCompactionResult, MicroCompactionTarget,
    Provider, ProviderEvent, ReasoningReplayPolicy, ReplayFrame, RuntimeEvent,
    RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig, RuntimeToolStatus, StopReason,
    VegaError, apply_micro_compaction, estimate_wire_context, project_model_messages, run_agent,
    tool_definitions,
};

const MODEL: &str = "micro-usage-model";
const SYSTEM: &str = "Inspect the available files.";
const LIVE_CALL_ID: &str = "live-glob";
const OLD_CALL_COUNT: usize = 40;
const SOURCE_VERSION: u64 = 7;
const GLOB_INPUT: &str =
    r#"{"pattern":"__vega_micro_usage_missing__/*.fixture","description":"Find fixture files"}"#;
const ASSISTANT_TEXT_BYTES: usize = 20_000;

#[derive(Clone, Copy)]
enum UsageMode {
    Input(u64),
    InvalidCache,
    Duplicate,
}

#[derive(Clone, Copy, Debug)]
enum MicroReply {
    None,
    Valid,
    StaleSource,
    EmptyFingerprint,
    WrongSavings,
    InsufficientSavings,
    InvalidTarget,
    DuplicateTarget,
    InvalidStatus,
    Error,
}

#[derive(Default)]
struct Trace {
    requests: Vec<ChatRequest>,
    micro_calls: Vec<(usize, MicroCompactionRequest)>,
    compact_calls: usize,
}

struct ScriptedProvider {
    trace: Arc<Mutex<Trace>>,
    usage: UsageMode,
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let request_index = {
            let mut trace = self.trace.lock().unwrap();
            let index = trace.requests.len();
            trace.requests.push(request);
            index
        };
        let usage = self.usage;
        Box::pin(async move {
            let events = match request_index {
                0 => {
                    let mut events = vec![
                        ProviderEvent::TextDelta("A".repeat(ASSISTANT_TEXT_BYTES)),
                        ProviderEvent::ToolUse {
                            id: LIVE_CALL_ID.into(),
                            name: "glob".into(),
                            input_json: GLOB_INPUT.into(),
                        },
                    ];
                    let input = match usage {
                        UsageMode::Input(input) => input,
                        UsageMode::InvalidCache | UsageMode::Duplicate => 1_000,
                    };
                    let usage_event = ProviderEvent::Usage {
                        input,
                        output: 67,
                        cache_read: Some(if matches!(usage, UsageMode::InvalidCache) {
                            input + 1
                        } else {
                            input / 2
                        }),
                        cache_write: Some(input / 4),
                    };
                    events.push(usage_event.clone());
                    if matches!(usage, UsageMode::Duplicate) {
                        events.push(usage_event);
                    }
                    events.push(ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    });
                    events
                }
                1 => vec![
                    ProviderEvent::TextDelta("done".into()),
                    ProviderEvent::Usage {
                        input: 2_000,
                        output: 1,
                        cache_read: None,
                        cache_write: None,
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ],
                _ => {
                    return Err(VegaError::Provider {
                        status: None,
                        message: "unexpected primary request".into(),
                        retryable: false,
                    });
                }
            };
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as EventStream)
        })
    }
}

struct RecordingHook {
    trace: Arc<Mutex<Trace>>,
    reply: MicroReply,
}

impl ContextCompactionHook for RecordingHook {
    fn micro_compact<'a>(
        &'a self,
        request: MicroCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<Option<MicroCompactionResult>, VegaError>> {
        Box::pin(async move {
            {
                let mut trace = self.trace.lock().unwrap();
                let count = trace.requests.len();
                trace.micro_calls.push((count, request.clone()));
            }
            if matches!(self.reply, MicroReply::None) {
                return Ok(None);
            }
            if matches!(self.reply, MicroReply::Error) {
                return Err(VegaError::Tool {
                    tool: "micro".into(),
                    message: "micro hook failed".into(),
                });
            }
            let target_count = if matches!(self.reply, MicroReply::InsufficientSavings) {
                1
            } else {
                OLD_CALL_COUNT + 1 - MICRO_KEEP_RECENT
            };
            let targets = (0..target_count)
                .map(|index| MicroCompactionTarget {
                    call_id: format!("historical-read-{index}"),
                    status: "success".into(),
                })
                .collect::<Vec<_>>();
            let before = estimate_wire_context(&request.wire_messages(), &request.context.tools)
                .unwrap()
                .input_tokens;
            let mut after_messages = request.wire_messages();
            apply_micro_compaction(&mut after_messages[1..], &targets).unwrap();
            let after = estimate_wire_context(&after_messages, &request.context.tools)
                .unwrap()
                .input_tokens;
            let savings = before.checked_sub(after).unwrap();
            if matches!(self.reply, MicroReply::InsufficientSavings) {
                assert!(savings > 0 && savings < MICRO_MIN_SAVINGS);
            } else {
                assert!(savings >= MICRO_MIN_SAVINGS);
            }
            let mut result = MicroCompactionResult {
                source_version: request.context.source_version,
                source_fingerprint: "updated-source".into(),
                targets,
                estimated_savings: savings,
            };
            match self.reply {
                MicroReply::StaleSource => result.source_version -= 1,
                MicroReply::EmptyFingerprint => result.source_fingerprint.clear(),
                MicroReply::WrongSavings => result.estimated_savings += 1,
                MicroReply::InvalidTarget => result.targets[0].call_id = "unknown-call".into(),
                MicroReply::DuplicateTarget => result.targets[1] = result.targets[0].clone(),
                MicroReply::InvalidStatus => result.targets[0].status = "pending".into(),
                MicroReply::None
                | MicroReply::Valid
                | MicroReply::InsufficientSavings
                | MicroReply::Error => {}
            }
            Ok(Some(result))
        })
    }

    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        Box::pin(async move {
            self.trace.lock().unwrap().compact_calls += 1;
            Err(ContextCompactionFailure::new(
                VegaError::Context(ContextRuntimeError::InvalidProjection),
                None,
            ))
        })
    }
}

fn budget() -> ContextBudget {
    ContextBudget::new(124_096, 4_096, true).unwrap()
}

fn history(result_bytes: usize) -> Vec<ChatMessage> {
    let mut history = Vec::new();
    for index in 0..OLD_CALL_COUNT {
        let id = format!("historical-read-{index}");
        let mut assistant = ChatMessage::assistant_with_tools(
            "Earlier investigation",
            vec![ChatToolCall {
                id: id.clone(),
                name: "read".into(),
                input_json: r#"{"path":"old.txt","offset":null,"limit":null}"#.into(),
            }],
        );
        assistant
            .mark_stored_tool_calls_with_statuses(vec!["success".into()])
            .unwrap();
        history.push(assistant);
        history.push(ChatMessage::tool_result(id, "H".repeat(result_bytes)));
    }
    history.push(ChatMessage::new(ChatRole::User, "Find the current files."));
    history
}

fn wire_history(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut system = ChatMessage::new(ChatRole::System, SYSTEM);
    system.attach_replay_frame(ReplayFrame::new(
        history.len() + 1,
        ReasoningReplayPolicy::StandardChat,
    ));
    std::iter::once(system)
        .chain(history.iter().cloned())
        .collect()
}

fn cold_history_below_precompute(margin: u64) -> Vec<ChatMessage> {
    let definitions = tool_definitions(RuntimeRunMode::Ask);
    let target = budget().precompute_tokens().unwrap() - margin;
    let (mut low, mut high) = (0, 8_000);
    while low < high {
        let middle = low + (high - low) / 2;
        let estimate = estimate_wire_context(&wire_history(&history(middle)), &definitions)
            .unwrap()
            .input_tokens;
        if estimate < target {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let history = history(low);
    let messages = wire_history(&history);
    let projected = project_model_messages(&messages).unwrap();
    assert_eq!(projected.len(), messages.len() + OLD_CALL_COUNT);
    let estimate = estimate_wire_context(&messages, &definitions).unwrap();
    assert!(estimate.input_tokens >= target);
    assert!(estimate.input_tokens < budget().precompute_tokens().unwrap());
    history
}

async fn run_case(
    usage: UsageMode,
    reply: MicroReply,
) -> (Result<AgentOutcome, VegaError>, Arc<Mutex<Trace>>) {
    run_case_with_margin(usage, reply, 3_000).await
}

async fn run_case_with_margin(
    usage: UsageMode,
    reply: MicroReply,
    margin: u64,
) -> (Result<AgentOutcome, VegaError>, Arc<Mutex<Trace>>) {
    let trace = Arc::new(Mutex::new(Trace::default()));
    let provider = ScriptedProvider {
        trace: trace.clone(),
        usage,
    };
    let hook = Arc::new(RecordingHook {
        trace: trace.clone(),
        reply,
    });
    let history = cold_history_below_precompute(margin);
    let replay_frame = ReplayFrame::new(history.len() + 1, ReasoningReplayPolicy::StandardChat);
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tools = vega_tools::Tools::new(&root).unwrap();
    let request = AgentRequest {
        history_safety: None,
        model: MODEL.into(),
        system_prompt: SYSTEM.into(),
        history,
        max_tokens: Some(4_096),
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Ask,
            RuntimePermissionMode::ReadOnly,
            "micro-project".into(),
            "micro-thread".into(),
            root.join("unused-micro-checkpoints"),
            Vec::new(),
        ),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: Some(replay_frame),
        context_budget: Some(budget()),
        context_source_version: Some(SOURCE_VERSION),
        context_source_fingerprint: Some("initial-source".into()),
        context_operation_id: Some("micro-run".into()),
        context_compaction_hook: Some(hook),
    };
    let outcome = run_agent(&provider, &tools, request, CancellationToken::new()).await;
    (outcome, trace)
}

fn assert_completed_tool_round(outcome: &AgentOutcome, requests: &[ChatRequest]) {
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.tool_call_count, 1);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(requests.len(), 2);
    let result = outcome.events.iter().find_map(|event| match event {
        RuntimeEvent::ToolCallFinished(result) | RuntimeEvent::ToolReadFinished { result, .. }
            if result.call_id == LIVE_CALL_ID =>
        {
            Some(result)
        }
        _ => None,
    });
    let result = result.expect("the real glob tool must finish");
    assert_eq!(result.status, RuntimeToolStatus::Success);
    assert!(!result.reused);
    assert!(result.output.is_empty());
    assert!(requests[1].messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some(LIVE_CALL_ID)
    }));
    assert!(outcome.final_text.ends_with("done"));
}

fn primary_decisions(outcome: &AgentOutcome) -> Vec<ContextAccountingDecision> {
    outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ContextAccountingUpdated(decision)
                if decision.stage == ContextAccountingStage::PrimaryPreflight =>
            {
                Some(*decision)
            }
            _ => None,
        })
        .collect()
}

fn raw_tokens(request: &ChatRequest) -> u64 {
    estimate_wire_context(&request.messages, &request.tools)
        .unwrap()
        .input_tokens
}

fn assert_micro_request(request: &MicroCompactionRequest) {
    assert!(request.context.estimate.input_tokens >= budget().precompute_tokens().unwrap());
    assert_eq!(request.context.source_version, SOURCE_VERSION);
    assert_eq!(
        request.context.source_fingerprint.as_deref(),
        Some("initial-source")
    );
    assert!(!request.context.require_source_fence);
    assert_eq!(
        request.context.source_owner_id.as_deref(),
        Some("micro-run")
    );
    assert!(request.context.messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some(LIVE_CALL_ID)
    }));
}

#[tokio::test]
async fn calibrated_prediction_below_precompute_skips_micro_and_sends_next_primary() {
    let input = 1_000;
    let (outcome, trace) = run_case(UsageMode::Input(input), MicroReply::None).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert_completed_tool_round(&outcome, &trace.requests);
    assert!(trace.micro_calls.is_empty());
    assert_eq!(trace.compact_calls, 0);
    let first = &trace.requests[0];
    let second = &trace.requests[1];
    let raw_history_len = 2 * OLD_CALL_COUNT + 2;
    assert_eq!(first.messages.len(), raw_history_len + OLD_CALL_COUNT);
    assert!(
        first
            .messages
            .iter()
            .any(|message| { message.content.contains("\"kind\":\"tool_activity\"") })
    );
    assert!(
        first
            .messages
            .iter()
            .any(|message| { message.content.contains("\"kind\":\"tool_result\"") })
    );
    let usages = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::UsageUpdated { usage, .. } => Some(usage),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(usages.len(), 2);
    assert_eq!(usages[0].input, input);
    assert_eq!(usages[0].output, 67);
    assert_eq!(usages[0].cache_read, Some(input / 2));
    assert_eq!(usages[0].cache_write, Some(input / 4));
    assert_eq!(
        &second.messages[..first.messages.len()],
        first.messages.as_slice()
    );
    assert_eq!(first.tools, second.tools);
    let first_raw = raw_tokens(first);
    let second_raw = raw_tokens(second);
    let threshold = budget().precompute_tokens().unwrap();
    assert!(first_raw < threshold);
    assert!(second_raw >= threshold);
    assert!(second_raw < budget().blocking_tokens().unwrap());
    let decisions = primary_decisions(&outcome);
    assert_eq!(decisions.len(), 2);
    let next = decisions[1];
    let tail = second_raw.checked_sub(first_raw).unwrap();
    assert_eq!(next.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(next.provider_input_baseline, Some(input));
    assert_eq!(next.covered_messages, first.messages.len());
    assert_eq!(next.incremental_estimate, tail);
    assert_eq!(next.predicted_input, input + tail);
    assert!(next.predicted_input < threshold);
}

#[tokio::test]
async fn calibrated_prediction_above_precompute_calls_micro_with_predicted_estimate() {
    let input = budget().precompute_tokens().unwrap() - 5_000;
    let (outcome, trace) = run_case(UsageMode::Input(input), MicroReply::None).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert_completed_tool_round(&outcome, &trace.requests);
    assert_eq!(trace.compact_calls, 0);
    assert_eq!(trace.micro_calls.len(), 1);
    assert_eq!(trace.micro_calls[0].0, 1);
    assert_micro_request(&trace.micro_calls[0].1);
    let decisions = primary_decisions(&outcome);
    assert_eq!(decisions.len(), 2);
    let next = decisions[1];
    assert_eq!(next.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(next.provider_input_baseline, Some(input));
    assert_eq!(
        next.incremental_estimate,
        raw_tokens(&trace.requests[1]) - raw_tokens(&trace.requests[0])
    );
    assert_eq!(next.predicted_input, input + next.incremental_estimate);
    assert_eq!(
        trace.micro_calls[0].1.context.estimate.input_tokens,
        next.predicted_input
    );
    assert!(next.predicted_input >= budget().precompute_tokens().unwrap());
    assert!(next.predicted_input < budget().trigger_tokens().unwrap());
}

#[tokio::test]
async fn provider_usage_can_trigger_micro_when_raw_remains_below_precompute() {
    let threshold = budget().precompute_tokens().unwrap();
    let input = threshold - 5_000;
    let (outcome, trace) =
        run_case_with_margin(UsageMode::Input(input), MicroReply::None, 12_000).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert_completed_tool_round(&outcome, &trace.requests);
    assert_eq!(trace.compact_calls, 0);
    assert_eq!(trace.micro_calls.len(), 1);
    assert_eq!(trace.micro_calls[0].0, 1);
    let request = &trace.micro_calls[0].1;
    assert_micro_request(request);
    let first_raw = raw_tokens(&trace.requests[0]);
    let second_raw = raw_tokens(&trace.requests[1]);
    assert!(first_raw < threshold);
    assert!(second_raw < threshold);
    let decisions = primary_decisions(&outcome);
    assert_eq!(decisions.len(), 2);
    let next = decisions[1];
    assert_eq!(next.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(next.provider_input_baseline, Some(input));
    assert_eq!(
        next.incremental_estimate,
        second_raw.checked_sub(first_raw).unwrap()
    );
    assert_eq!(next.predicted_input, input + next.incremental_estimate);
    assert_eq!(request.context.estimate.input_tokens, next.predicted_input);
    assert!(next.predicted_input >= threshold);
    assert!(next.predicted_input < budget().trigger_tokens().unwrap());
}

#[tokio::test]
async fn calibrated_prediction_at_precompute_calls_micro() {
    let threshold = budget().precompute_tokens().unwrap();
    let (probe, trace) = run_case(UsageMode::Input(1_000), MicroReply::None).await;
    let probe = probe.unwrap();
    let tail = {
        let trace = trace.lock().unwrap();
        assert_completed_tool_round(&probe, &trace.requests);
        assert!(trace.micro_calls.is_empty());
        let decisions = primary_decisions(&probe);
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[1].source, ContextAccountingSource::UsageAnchored);
        decisions[1].incremental_estimate
    };
    let input = threshold.checked_sub(tail).unwrap();
    assert!(input > 0);
    let (outcome, trace) = run_case(UsageMode::Input(input), MicroReply::None).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert_completed_tool_round(&outcome, &trace.requests);
    assert_eq!(trace.micro_calls.len(), 1);
    assert_eq!(trace.micro_calls[0].0, 1);
    assert_eq!(trace.compact_calls, 0);
    assert_micro_request(&trace.micro_calls[0].1);
    assert_eq!(
        trace.micro_calls[0].1.context.estimate.input_tokens,
        threshold
    );
    let decisions = primary_decisions(&outcome);
    assert_eq!(decisions.len(), 2);
    let next = decisions[1];
    assert_eq!(next.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(next.provider_input_baseline, Some(input));
    assert_eq!(next.incremental_estimate, tail);
    assert_eq!(next.predicted_input, threshold);
}

#[tokio::test]
async fn successful_micro_result_uses_raw_savings_and_invalidates_anchor() {
    let input = budget().precompute_tokens().unwrap() - 5_000;
    let (outcome, trace) = run_case(UsageMode::Input(input), MicroReply::Valid).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert_completed_tool_round(&outcome, &trace.requests);
    assert_eq!(trace.compact_calls, 0);
    assert_eq!(trace.micro_calls.len(), 1);
    assert_eq!(trace.micro_calls[0].0, 1);
    let request = &trace.micro_calls[0].1;
    assert_micro_request(request);
    let raw_before = estimate_wire_context(&request.wire_messages(), &request.context.tools)
        .unwrap()
        .input_tokens;
    let raw_after = raw_tokens(&trace.requests[1]);
    assert!(raw_before.checked_sub(raw_after).unwrap() >= MICRO_MIN_SAVINGS);
    let decisions = primary_decisions(&outcome);
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[1].source, ContextAccountingSource::Estimated);
    assert_eq!(decisions[1].provider_input_baseline, None);
    assert_eq!(decisions[1].predicted_input, raw_after);
}

#[tokio::test]
async fn untrusted_micro_results_and_hook_errors_stop_before_the_next_request() {
    let input = budget().precompute_tokens().unwrap() - 5_000;
    for reply in [
        MicroReply::StaleSource,
        MicroReply::EmptyFingerprint,
        MicroReply::WrongSavings,
        MicroReply::InsufficientSavings,
        MicroReply::InvalidTarget,
        MicroReply::DuplicateTarget,
        MicroReply::InvalidStatus,
        MicroReply::Error,
    ] {
        let (outcome, trace) = run_case(UsageMode::Input(input), reply).await;
        match reply {
            MicroReply::Error => assert!(matches!(outcome, Err(VegaError::Tool { .. }))),
            _ => assert!(matches!(
                outcome,
                Err(VegaError::Context(ContextRuntimeError::InvalidProjection))
            )),
        }
        let trace = trace.lock().unwrap();
        assert_eq!(trace.requests.len(), 1, "reply: {reply:?}");
        assert_eq!(trace.micro_calls.len(), 1, "reply: {reply:?}");
        assert_eq!(trace.micro_calls[0].0, 1);
        assert_eq!(trace.compact_calls, 0);
        assert_micro_request(&trace.micro_calls[0].1);
    }
}

#[tokio::test]
async fn invalid_usage_falls_back_to_raw_micro_gate() {
    for usage in [UsageMode::Input(0), UsageMode::InvalidCache] {
        let (outcome, trace) = run_case(usage, MicroReply::None).await;
        let outcome = outcome.unwrap();
        let trace = trace.lock().unwrap();
        assert_completed_tool_round(&outcome, &trace.requests);
        assert_eq!(trace.micro_calls.len(), 1);
        assert_eq!(trace.micro_calls[0].0, 1);
        assert_eq!(trace.compact_calls, 0);
        assert_micro_request(&trace.micro_calls[0].1);
        assert_eq!(
            trace.micro_calls[0].1.context.estimate.input_tokens,
            raw_tokens(&trace.requests[1])
        );
        let decisions = primary_decisions(&outcome);
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[1].source, ContextAccountingSource::Estimated);
        assert_eq!(decisions[1].provider_input_baseline, None);
        assert_eq!(decisions[1].predicted_input, raw_tokens(&trace.requests[1]));
        assert!(decisions[1].predicted_input >= budget().precompute_tokens().unwrap());
    }
}

#[tokio::test]
async fn duplicate_usage_fails_before_tool_execution_or_another_primary_request() {
    let (outcome, trace) = run_case(UsageMode::Duplicate, MicroReply::None).await;
    let outcome = outcome.unwrap();
    let trace = trace.lock().unwrap();
    assert!(outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(trace.requests.len(), 1);
    assert!(trace.micro_calls.is_empty());
    assert_eq!(trace.compact_calls, 0);
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Error(_)))
    );
}
