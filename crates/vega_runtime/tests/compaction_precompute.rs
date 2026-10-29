use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRequest, ChatRole, ChatToolCall, ContextBudget,
    ContextCompactionFailure, ContextCompactionHook, ContextCompactionRequest,
    ContextCompactionResult, ContextCompactionTrigger, ContextRuntimeError, EventStream,
    MicroCompactionRequest, MicroCompactionResult, MicroCompactionTarget, Provider, ProviderEvent,
    RuntimeEvent, RuntimeSteeringInput, RuntimeSteeringSource, RuntimeToolConfig, StopReason,
    VegaError, apply_micro_compaction, estimate_wire_context, run_agent_with_sink,
};

#[path = "compaction_precompute/delegated_tests.rs"]
mod delegated_tests;
#[path = "compaction_precompute/skill_tests.rs"]
mod skill_tests;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Trace = Arc<Mutex<Vec<&'static str>>>;

struct RecordingHook {
    trace: Trace,
    armed: Mutex<Vec<ContextCompactionRequest>>,
    compacted: Mutex<Vec<ContextCompactionTrigger>>,
    fail_compaction: bool,
    micro: bool,
}

impl RecordingHook {
    fn new(trace: Trace, fail_compaction: bool, micro: bool) -> Arc<Self> {
        Arc::new(Self {
            trace,
            armed: Mutex::new(Vec::new()),
            compacted: Mutex::new(Vec::new()),
            fail_compaction,
            micro,
        })
    }
}

impl ContextCompactionHook for RecordingHook {
    fn arm_precompute<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("arm");
            self.armed.lock().unwrap().push(request);
        })
    }

    fn micro_compact<'a>(
        &'a self,
        request: MicroCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<Option<MicroCompactionResult>, VegaError>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("micro");
            if !self.micro {
                return Ok(None);
            }
            let targets = micro_targets();
            let mut next = request.wire_messages();
            let before = estimate_wire_context(&next, &request.context.tools)
                .map_err(|error| VegaError::Context(error.into()))?;
            apply_micro_compaction(&mut next[1..], &targets)?;
            let after = estimate_wire_context(&next, &request.context.tools)
                .map_err(|error| VegaError::Context(error.into()))?;
            Ok(Some(MicroCompactionResult {
                source_version: 2,
                source_fingerprint: "after-micro".into(),
                targets,
                estimated_savings: before.input_tokens - after.input_tokens,
            }))
        })
    }

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("compact");
            self.compacted.lock().unwrap().push(request.trigger);
            if self.fail_compaction {
                return Err(ContextCompactionFailure {
                    error: Box::new(VegaError::Context(ContextRuntimeError::SummaryTimedOut)),
                    usages: Vec::new(),
                    usage_complete: true,
                });
            }
            let latest_user = request
                .messages
                .iter()
                .rposition(|message| message.role == ChatRole::User)
                .unwrap();
            let mut messages = vec![
                ChatMessage::new(ChatRole::User, "Historical task summary"),
                ChatMessage::new(ChatRole::Assistant, "Retained response"),
            ];
            messages.extend_from_slice(&request.messages[latest_user..]);
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: Some("compacted-source".into()),
                replay_cold_prefix_messages: Some(2),
                replay_interleaved_user_boundary: None,
                reactive_retained_groups: Some(1),
                usages: Vec::new(),
                usage_complete: true,
                settled_usage: None,
            })
        })
    }
}

struct ScriptedProvider {
    trace: Trace,
    replies: Mutex<VecDeque<Vec<Result<ProviderEvent, VegaError>>>>,
}

impl ScriptedProvider {
    fn new(trace: Trace, replies: Vec<Vec<Result<ProviderEvent, VegaError>>>) -> Self {
        Self {
            trace,
            replies: Mutex::new(replies.into()),
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.trace.lock().unwrap().push("primary");
        let events = self.replies.lock().unwrap().pop_front().expect("reply");
        Box::pin(async move {
            let stream: EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

fn response(input: u64, stop_reason: StopReason) -> Vec<Result<ProviderEvent, VegaError>> {
    vec![
        Ok(ProviderEvent::Usage {
            input,
            output: 10,
            cache_read: None,
            cache_write: None,
        }),
        Ok(ProviderEvent::TextDelta("Completed response.".into())),
        Ok(ProviderEvent::Done { stop_reason }),
    ]
}

fn request(hook: Arc<RecordingHook>) -> AgentRequest {
    AgentRequest {
        model: "synthetic".into(),
        system_prompt: "system".into(),
        history: vec![ChatMessage::new(ChatRole::User, "Original task")],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: Some(ContextBudget::new(110_000, 10_000, true).unwrap()),
        context_source_version: Some(1),
        context_source_fingerprint: Some("original-source".into()),
        context_operation_id: Some("owner".into()),
        context_compaction_hook: Some(hook),
    }
}

fn fixture_wire(request: &AgentRequest) -> Vec<ChatMessage> {
    let mut system = ChatMessage::new(ChatRole::System, &request.system_prompt);
    system.attach_replay_frame(vega_runtime::ReplayFrame::new(
        request.history.len() + 1,
        vega_runtime::ReasoningReplayPolicy::Unknown,
    ));
    std::iter::once(system)
        .chain(request.history.iter().cloned())
        .collect()
}

fn sink_event(trace: &Trace, event: &RuntimeEvent) {
    let name = match event {
        RuntimeEvent::TextDelta(_) => "delta",
        RuntimeEvent::AssistantResponseCompleted { .. } => "complete",
        RuntimeEvent::ToolCallFinished(_) => "tool",
        RuntimeEvent::SteeringAccepted(_) => "steering",
        RuntimeEvent::Finished(_) => "finished",
        _ => return,
    };
    trace.lock().unwrap().push(name);
}

#[tokio::test]
async fn final_arming_requires_valid_usage_or_wire_threshold_and_successful_persistence_ack()
-> TestResult {
    for case in [
        "valid",
        "low",
        "invalid_usage",
        "length",
        "persistence",
        "disabled",
    ] {
        let trace = Trace::default();
        let hook = RecordingHook::new(trace.clone(), false, false);
        let mut request = request(hook.clone());
        if case == "disabled" {
            request.context_budget = request
                .context_budget
                .map(ContextBudget::without_automatic_compaction);
        }
        let mut events = response(
            if case == "low" { 1000 } else { 81_000 },
            if case == "length" {
                StopReason::Length
            } else {
                StopReason::End
            },
        );
        if case == "invalid_usage" {
            events[0] = Ok(ProviderEvent::Usage {
                input: 81_000,
                output: 10,
                cache_read: Some(81_001),
                cache_write: None,
            });
        }
        let provider = ScriptedProvider::new(trace.clone(), vec![events]);
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let result = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                sink_event(&trace, &event);
                let failed = case == "persistence" && matches!(event, RuntimeEvent::Finished(_));
                async move {
                    if failed {
                        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;
        if case == "persistence" {
            assert!(matches!(
                result,
                Err(VegaError::Context(ContextRuntimeError::SourceChanged))
            ));
        } else {
            assert!(!result?.failed);
        }
        let armed = hook.armed.lock().unwrap();
        if case == "valid" {
            assert_eq!(armed.len(), 1);
            assert!(armed[0].estimate.input_tokens >= 81_000);
            assert!(
                vega_runtime::estimate_chat_context(
                    &armed[0].system_prompt,
                    &armed[0].messages,
                    &armed[0].tools
                )?
                .input_tokens
                    < 80_000
            );
            assert_eq!(
                armed[0].messages.last().unwrap().content,
                "Completed response."
            );
            assert_eq!(
                trace.lock().unwrap().as_slice(),
                ["primary", "delta", "complete", "finished", "arm"]
            );
        } else {
            assert!(armed.is_empty(), "{case}");
        }
        assert!(hook.compacted.lock().unwrap().is_empty());
        assert!(provider.replies.lock().unwrap().is_empty());
    }
    Ok(())
}

fn micro_targets() -> Vec<MicroCompactionTarget> {
    (0..35)
        .map(|index| MicroCompactionTarget {
            call_id: format!("call-{index}"),
            status: "success".into(),
        })
        .collect()
}

#[tokio::test]
async fn preflight_arms_only_after_micro_and_never_for_the_initial_single_user() -> TestResult {
    for initial in [false, true] {
        let trace = Trace::default();
        let hook = RecordingHook::new(trace.clone(), false, !initial);
        let mut request = request(hook.clone());
        if initial {
            request.history[0].content = "u".repeat(320_000);
        } else {
            request
                .history
                .push(ChatMessage::new(ChatRole::Assistant, "a".repeat(280_000)));
            for index in 0..40 {
                let id = format!("call-{index}");
                request.history.push(ChatMessage::assistant_with_tools(
                    "",
                    vec![ChatToolCall {
                        id: id.clone(),
                        name: "read".into(),
                        input_json: "{\"path\":\"notes.txt\",\"offset\":null,\"limit\":null}"
                            .into(),
                    }],
                ));
                request
                    .history
                    .push(ChatMessage::tool_result(id, "detail ".repeat(1000)));
            }
            request
                .history
                .push(ChatMessage::new(ChatRole::User, "Continue"));
        }
        let definitions = vega_runtime::tool_definitions(request.tool_config.run_mode);
        let mut projected = fixture_wire(&request);
        if !initial {
            apply_micro_compaction(&mut projected[1..], &micro_targets())?;
        }
        let input = estimate_wire_context(&projected, &definitions)?.input_tokens;
        let budget = ContextBudget::new(input + 25_000, 10_000, true)?;
        assert!(input >= budget.precompute_tokens()?);
        assert!(input < budget.trigger_tokens()?);
        request.context_budget = Some(budget);
        let provider =
            ScriptedProvider::new(trace.clone(), vec![response(1000, StopReason::Length)]);
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                sink_event(&trace, &event);
                std::future::ready(Ok(()))
            },
        )
        .await?;
        assert!(!outcome.failed);
        let armed = hook.armed.lock().unwrap();
        if initial {
            assert!(armed.is_empty());
            assert_eq!(trace.lock().unwrap().first(), Some(&"micro"));
        } else {
            assert_eq!(armed.len(), 1);
            let captured = &armed[0];
            assert!(captured.estimate.input_tokens >= captured.budget.precompute_tokens()?);
            assert!(captured.estimate.input_tokens < captured.budget.trigger_tokens()?);
            assert_eq!(captured.source_version, 2);
            assert_eq!(captured.source_fingerprint.as_deref(), Some("after-micro"));
            assert!(
                captured
                    .messages
                    .iter()
                    .any(|message| message.content.contains("vega_compacted_tool_result"))
            );
            assert_eq!(&trace.lock().unwrap()[..3], ["micro", "arm", "primary"]);
        }
        assert!(hook.compacted.lock().unwrap().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn tool_response_arming_waits_for_every_terminal_tool_result() -> TestResult {
    let trace = Trace::default();
    let hook = RecordingHook::new(trace.clone(), false, false);
    let mut events = response(81_000, StopReason::ToolUse);
    events.insert(
        2,
        Ok(ProviderEvent::ToolUse {
            id: "call".into(),
            name: "unavailable_tool".into(),
            input_json: "{}".into(),
        }),
    );
    let provider =
        ScriptedProvider::new(trace.clone(), vec![events, response(1000, StopReason::End)]);
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        request(hook.clone()),
        CancellationToken::new(),
        |event| {
            sink_event(&trace, &event);
            std::future::ready(Ok(()))
        },
    )
    .await?;
    assert!(!outcome.failed);
    let armed = hook.armed.lock().unwrap();
    assert_eq!(armed.len(), 1);
    assert!(
        armed[0]
            .messages
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("call"))
    );
    let trace = trace.lock().unwrap();
    assert!(
        trace.iter().position(|value| *value == "tool").unwrap()
            < trace.iter().position(|value| *value == "arm").unwrap()
    );
    assert_eq!(trace.iter().filter(|value| **value == "primary").count(), 2);
    Ok(())
}

struct OneSteering(Mutex<Option<RuntimeSteeringInput>>);

impl RuntimeSteeringSource for OneSteering {
    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        let input = self.0.lock().unwrap().take();
        Box::pin(async { Ok(input) })
    }
}

#[tokio::test]
async fn automatic_and_reactive_attempts_suppress_arming_until_a_new_steering_owner() -> TestResult
{
    for case in ["automatic", "failure", "overflow", "steering"] {
        let trace = Trace::default();
        let hook = RecordingHook::new(trace.clone(), case == "failure", false);
        let mut request = request(hook.clone());
        request.history.extend([
            ChatMessage::new(
                ChatRole::Assistant,
                if case == "overflow" {
                    "a".repeat(40_000)
                } else {
                    "a".repeat(350_000)
                },
            ),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ]);
        let input = estimate_wire_context(
            &fixture_wire(&request),
            &vega_runtime::tool_definitions(request.tool_config.run_mode),
        )?;
        if case != "overflow" {
            let budget = ContextBudget::new(input.input_tokens + 16_000, 10_000, true)?;
            assert_eq!(budget.check(input)?, vega_runtime::ContextCheck::Triggered);
            request.context_budget = Some(budget);
        }
        let final_input = request.context_budget.unwrap().precompute_tokens()? + 1000;
        let mut replies = Vec::new();
        if case == "overflow" {
            replies.push(vec![Err(VegaError::Provider {
                status: Some(400),
                message: "{\"error\":{\"code\":\"prompt_too_long\"}}".into(),
                retryable: false,
            })]);
        }
        if case == "steering" {
            request.tool_config = request
                .tool_config
                .with_steering_source(Arc::new(OneSteering(Mutex::new(Some(
                    RuntimeSteeringInput {
                        original_content: "New task".into(),
                        user_message_seq: 4,
                        id: "new-user".into(),
                        assistant_message_id: "new-owner".into(),
                        message: ChatMessage::new(ChatRole::User, "New task"),
                    },
                )))));
            replies.push(response(1000, StopReason::End));
        }
        replies.push(response(final_input, StopReason::End));
        let provider = ScriptedProvider::new(trace.clone(), replies);
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                sink_event(&trace, &event);
                std::future::ready(Ok(()))
            },
        )
        .await?;
        assert!(!outcome.failed, "{case}");
        let compacted = hook.compacted.lock().unwrap();
        assert_eq!(compacted.len(), 1, "{case}");
        assert_eq!(
            matches!(compacted[0], ContextCompactionTrigger::InputOverflow { .. }),
            case == "overflow"
        );
        let armed = hook.armed.lock().unwrap();
        if case == "steering" {
            assert_eq!(armed.len(), 1);
            assert_eq!(armed[0].source_owner_id.as_deref(), Some("new-owner"));
        } else {
            assert!(armed.is_empty(), "{case}");
        }
        assert!(provider.replies.lock().unwrap().is_empty());
    }
    Ok(())
}
