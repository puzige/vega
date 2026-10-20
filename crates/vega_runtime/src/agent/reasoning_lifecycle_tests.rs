use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::computer_use::{ComputerUseRun, ControlPhase};
use crate::provider::EventStream;

struct ScriptedProvider {
    streams: Mutex<VecDeque<EventStream>>,
}

impl ScriptedProvider {
    fn new(streams: Vec<EventStream>) -> Self {
        Self {
            streams: Mutex::new(streams.into()),
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let stream = self.streams.lock().unwrap().pop_front().unwrap();
        Box::pin(async move { Ok(stream) })
    }
}

struct ObservedStream {
    receiver: mpsc::UnboundedReceiver<Result<ProviderEvent, VegaError>>,
    pending: mpsc::UnboundedSender<()>,
    reported_pending: bool,
}

impl Stream for ObservedStream {
    type Item = Result<ProviderEvent, VegaError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.receiver.poll_recv(context);
        if next.is_pending() && !self.reported_pending {
            self.reported_pending = true;
            let _ = self.pending.send(());
        } else if next.is_ready() {
            self.reported_pending = false;
        }
        next
    }
}

fn script(items: Vec<Result<ProviderEvent, VegaError>>) -> EventStream {
    Box::pin(futures::stream::iter(items))
}

fn done(stop_reason: StopReason) -> Result<ProviderEvent, VegaError> {
    Ok(ProviderEvent::Done { stop_reason })
}

fn request() -> AgentRequest {
    AgentRequest {
        model: "scripted".into(),
        system_prompt: String::new(),
        history: vec![ChatMessage::new(ChatRole::User, "continue")],
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default().with_turn_limit(4),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

async fn run(provider: &ScriptedProvider) -> AgentOutcome {
    let root = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(root.path()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        run_agent(provider, &tools, request(), CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap()
}

fn completions(events: &[RuntimeEvent]) -> Vec<(usize, &str)> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            RuntimeEvent::ReasoningCompleted { response_id } => Some((index, response_id.as_str())),
            _ => None,
        })
        .collect()
}

fn starts(events: &[RuntimeEvent]) -> Vec<(usize, &str, usize)> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            RuntimeEvent::AssistantResponseStarted {
                response_id,
                text_start_bytes,
            } => Some((index, response_id.as_str(), *text_start_bytes)),
            _ => None,
        })
        .collect()
}

fn assert_attempt_identity(events: &[RuntimeEvent]) {
    for (start_index, response_id, _) in starts(events) {
        assert!(!response_id.is_empty());
        assert!(events[start_index + 1..].iter().any(|event| {
            matches!(event, RuntimeEvent::DiagnosticAttempt(attempt)
                if attempt.phase == RuntimeDiagnosticPhase::PrimaryModel
                    && attempt.state == RuntimeDiagnosticState::Started
                    && attempt.attempt_id == response_id)
        }));
    }
}

#[tokio::test]
async fn drained_response_shapes_complete_once_without_fabricating_text_metadata() {
    for (prefix, expected_text, has_text_metadata) in [
        (
            vec![Ok(ProviderEvent::SummaryDelta(
                "Checking dependencies".into(),
            ))],
            "",
            false,
        ),
        (
            vec![Ok(ProviderEvent::ThinkingDelta("private detail".into()))],
            "",
            false,
        ),
        (vec![], "", false),
        (
            vec![Ok(ProviderEvent::TextDelta(" \n".into()))],
            " \n",
            false,
        ),
        (
            vec![Ok(ProviderEvent::TextDelta("answer".into()))],
            "answer",
            true,
        ),
    ] {
        let mut items = prefix;
        items.push(done(StopReason::End));
        let outcome = run(&ScriptedProvider::new(vec![script(items)])).await;
        assert!(!outcome.failed);
        assert!(!outcome.interrupted);
        assert_eq!(outcome.final_text, expected_text);
        assert_eq!(outcome.executed_tool_call_count, 0);
        let starts = starts(&outcome.events);
        let completions = completions(&outcome.events);
        assert_eq!(starts.len(), 1);
        assert_eq!(completions.len(), 1);
        assert_eq!(starts[0].1, completions[0].1);
        assert_eq!(starts[0].2, 0);
        assert!(starts[0].0 < completions[0].0);
        assert_attempt_identity(&outcome.events);
        for (index, event) in outcome.events.iter().enumerate() {
            match event {
                RuntimeEvent::TextDelta(_)
                | RuntimeEvent::ThinkingDelta(_)
                | RuntimeEvent::SummaryDelta(_) => {
                    assert!(starts[0].0 < index && index < completions[0].0);
                }
                RuntimeEvent::Finished(_) => assert!(completions[0].0 < index),
                RuntimeEvent::ToolRoundCompleted { .. } => panic!("unexpected tool round"),
                _ => {}
            }
        }
        let metadata = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::AssistantResponseCompleted {
                    response_id,
                    text_start_bytes,
                    text_end_bytes,
                    kind,
                } => Some((response_id, text_start_bytes, text_end_bytes, kind)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(metadata.len(), usize::from(has_text_metadata));
        if has_text_metadata {
            assert_eq!(metadata[0].0, starts[0].1);
            assert_eq!(*metadata[0].1, 0);
            assert_eq!(*metadata[0].2, expected_text.len());
            assert_eq!(*metadata[0].3, RuntimeAssistantResponseKind::Final);
        }
    }
}

#[tokio::test]
async fn silence_and_done_do_not_complete_a_held_open_stream_before_eof() {
    let root = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(root.path()).unwrap();
    let (sender, receiver) = mpsc::unbounded_channel();
    let (pending, mut observed_pending) = mpsc::unbounded_channel();
    let provider = ScriptedProvider::new(vec![Box::pin(ObservedStream {
        receiver,
        pending,
        reported_pending: false,
    })]);
    sender
        .send(Ok(ProviderEvent::ThinkingDelta("detail".into())))
        .unwrap();
    sender
        .send(Ok(ProviderEvent::SummaryDelta("Checking".into())))
        .unwrap();
    let delivered = Mutex::new(Vec::new());
    let running = run_agent_with_sink(
        &provider,
        &tools,
        request(),
        CancellationToken::new(),
        |event| {
            delivered.lock().unwrap().push(event);
            std::future::ready(Ok(()))
        },
    );
    tokio::pin!(running);
    for terminal_received in [false, true] {
        if terminal_received {
            sender.send(done(StopReason::End)).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                result = &mut running => panic!("stream finished while held open: {result:?}"),
                pending = observed_pending.recv() => pending.unwrap(),
            }
        })
        .await
        .unwrap();
        let events = delivered.lock().unwrap();
        assert_eq!(starts(&events).len(), 1);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::SummaryDelta(_)))
        );
        assert!(completions(&events).is_empty());
        assert!(!events.iter().any(|event| matches!(
            event,
            RuntimeEvent::AssistantResponseCompleted { .. } | RuntimeEvent::Finished(_)
        )));
    }
    drop(sender);
    let outcome = tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap();
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    let delivered = delivered.lock().unwrap();
    assert_eq!(completions(&delivered).len(), 1);
    assert_eq!(starts(&delivered)[0].1, completions(&delivered)[0].1);
    assert_eq!(completions(&outcome.events).len(), 1);
}

#[tokio::test]
async fn drained_response_completes_while_paused_but_durable_events_wait_for_resume() {
    let root = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(root.path()).unwrap();
    let computer_use = ComputerUseRun::new(false, false);
    let control = computer_use.control.clone();
    let mut changes = control.subscribe();
    let mut request = request();
    request.tool_config.run_mode = RuntimeRunMode::Execute;
    request.tool_config.permission_mode = RuntimePermissionMode::Confirm;
    request.tool_config = request.tool_config.with_computer_use(Some(computer_use));
    assert!(
        request
            .tool_config
            .computer_use
            .as_ref()
            .is_some_and(|run| run.control.same_instance(&control))
    );
    let (sender, receiver) = mpsc::unbounded_channel();
    let (pending, mut observed_pending) = mpsc::unbounded_channel();
    let provider = ScriptedProvider::new(vec![Box::pin(ObservedStream {
        receiver,
        pending,
        reported_pending: false,
    })]);
    sender
        .send(Ok(ProviderEvent::SummaryDelta("Checking".into())))
        .unwrap();
    sender
        .send(Ok(ProviderEvent::TextDelta("answer".into())))
        .unwrap();
    sender.send(done(StopReason::End)).unwrap();
    let delivered = Mutex::new(Vec::new());
    let (completed, mut observed_completion) = mpsc::unbounded_channel();
    let running = run_agent_with_sink(
        &provider,
        &tools,
        request,
        CancellationToken::new(),
        |event| {
            if let RuntimeEvent::ReasoningCompleted { response_id } = &event {
                completed.send(response_id.clone()).unwrap();
            }
            delivered.lock().unwrap().push(event);
            std::future::ready(Ok(()))
        },
    );
    tokio::pin!(running);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut running => panic!("stream finished before EOF: {result:?}"),
            pending = observed_pending.recv() => pending.unwrap(),
        }
    })
    .await
    .unwrap();
    let response_id = {
        let events = delivered.lock().unwrap();
        assert_eq!(starts(&events).len(), 1);
        assert!(completions(&events).is_empty());
        assert!(events.iter().any(|event| matches!(event,
            RuntimeEvent::TextDelta(text) if text == "answer")));
        starts(&events)[0].1.to_owned()
    };
    control.pause();
    let paused_epoch = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state = control.status();
            if state.phase == ControlPhase::Paused {
                break state.epoch;
            }
            tokio::select! {
                result = &mut running => panic!("stream finished before pause: {result:?}"),
                changed = changes.changed() => changed.unwrap(),
            }
        }
    })
    .await
    .unwrap();
    drop(sender);
    let completed_id = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut running => panic!("run finished while paused: {result:?}"),
            response_id = observed_completion.recv() => response_id.unwrap(),
        }
    })
    .await
    .unwrap();
    assert_eq!(completed_id, response_id);
    assert_eq!(control.status().phase, ControlPhase::Paused);
    {
        let events = delivered.lock().unwrap();
        assert_eq!(completions(&events).len(), 1);
        assert_eq!(completions(&events)[0].1, response_id);
        assert!(!events.iter().any(|event| matches!(
            event,
            RuntimeEvent::AssistantResponseCompleted { .. }
                | RuntimeEvent::ToolRoundCompleted { .. }
                | RuntimeEvent::Finished(_)
                | RuntimeEvent::Interrupted
                | RuntimeEvent::Error(_)
        )));
    }
    control.resume(paused_epoch).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap();
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, "answer");
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_attempt_identity(&outcome.events);
    let events = delivered.lock().unwrap();
    assert_eq!(completions(&events).len(), 1);
    assert_eq!(completions(&outcome.events).len(), 1);
    let durable_completion = events
        .iter()
        .position(|event| {
            matches!(event,
        RuntimeEvent::AssistantResponseCompleted {
            response_id: actual_id,
            text_start_bytes: 0,
            text_end_bytes: 6,
            kind: RuntimeAssistantResponseKind::Final,
        } if actual_id == &response_id)
        })
        .unwrap();
    let finished = events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::Finished(RuntimeFinishReason::End)))
        .unwrap();
    assert!(completions(&events)[0].0 < durable_completion);
    assert!(durable_completion < finished);
}

#[tokio::test]
async fn failed_or_cancelled_streams_do_not_complete_but_an_accepted_retry_completes_once() {
    for items in [
        vec![Ok(ProviderEvent::ThinkingDelta("unfinished".into()))],
        vec![Err(VegaError::Provider {
            status: None,
            message: "scripted failure".into(),
            retryable: false,
        })],
        vec![
            done(StopReason::End),
            Err(VegaError::Provider {
                status: None,
                message: "failure after terminal".into(),
                retryable: false,
            }),
        ],
        vec![
            Ok(ProviderEvent::ReasoningReplay(vec![serde_json::json!({
                "type": "reasoning"
            })])),
            done(StopReason::End),
        ],
    ] {
        let outcome = run(&ScriptedProvider::new(vec![script(items)])).await;
        assert!(outcome.failed);
        assert!(!outcome.interrupted);
        assert!(completions(&outcome.events).is_empty());
        assert!(
            outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Error(_)))
        );
        assert!(!outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::AssistantResponseCompleted { .. }
                | RuntimeEvent::ToolRoundCompleted { .. }
        )));
    }

    let root = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(root.path()).unwrap();
    let (sender, receiver) = mpsc::unbounded_channel();
    let (pending, mut observed_pending) = mpsc::unbounded_channel();
    let provider = ScriptedProvider::new(vec![Box::pin(ObservedStream {
        receiver,
        pending,
        reported_pending: false,
    })]);
    sender.send(done(StopReason::End)).unwrap();
    let cancel = CancellationToken::new();
    let running = run_agent(&provider, &tools, request(), cancel.clone());
    tokio::pin!(running);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut running => panic!("stream finished before cancellation: {result:?}"),
            pending = observed_pending.recv() => pending.unwrap(),
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap();
    assert!(outcome.interrupted);
    assert!(!outcome.failed);
    assert!(completions(&outcome.events).is_empty());
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Interrupted))
    );
    drop(sender);

    let outcome = run(&ScriptedProvider::new(vec![script(vec![
        Ok(ProviderEvent::TextDelta("discarded".into())),
        Ok(ProviderEvent::SummaryDelta("discarded summary".into())),
        Ok(ProviderEvent::RetryScheduled {
            attempt: 1,
            max_retries: 2,
        }),
        Ok(ProviderEvent::SummaryDelta("accepted summary".into())),
        Ok(ProviderEvent::TextDelta("accepted".into())),
        done(StopReason::End),
    ])]))
    .await;
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, "accepted");
    assert_eq!(starts(&outcome.events).len(), 1);
    let completions = completions(&outcome.events);
    assert_eq!(completions.len(), 1);
    assert_eq!(starts(&outcome.events)[0].1, completions[0].1);
    let retry = outcome
        .events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::ResponseRetrying { .. }))
        .unwrap();
    assert!(retry < completions[0].0);
    assert_attempt_identity(&outcome.events);
}

#[tokio::test]
async fn tool_responses_complete_before_reused_proposals_and_invalid_tool_rejections() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cached.txt"), "cached").unwrap();
    let tools = vega_tools::Tools::new(root.path()).unwrap();
    let input_json = serde_json::json!({ "file_path": "cached.txt" }).to_string();
    let provider = ScriptedProvider::new(vec![
        script(vec![
            Ok(ProviderEvent::SummaryDelta("Reading".into())),
            Ok(ProviderEvent::ToolUse {
                id: "cached-read".into(),
                name: "read".into(),
                input_json: input_json.clone(),
            }),
            done(StopReason::ToolUse),
        ]),
        script(vec![
            Ok(ProviderEvent::ThinkingDelta("next action".into())),
            Ok(ProviderEvent::ToolUse {
                id: "invalid-bash".into(),
                name: "bash".into(),
                input_json: "{}".into(),
            }),
            done(StopReason::ToolUse),
        ]),
        script(vec![done(StopReason::End)]),
    ]);
    let mut request = request();
    request.tool_config.run_mode = RuntimeRunMode::Execute;
    request.completed_tool_results.insert(
        "cached-read".into(),
        CompletedToolCall {
            tool: "read".into(),
            input_json,
            result: RuntimeToolResult {
                call_id: "cached-read".into(),
                output: "cached".into(),
                status: RuntimeToolStatus::Success,
                reused: false,
                exit_code: None,
                duration_ms: None,
                truncated: None,
                approval: None,
                remember_rule: None,
            },
        },
    );
    let outcome = tokio::time::timeout(
        Duration::from_secs(2),
        run_agent(&provider, &tools, request, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, "");
    assert_eq!(outcome.tool_call_count, 2);
    assert_eq!(outcome.executed_tool_call_count, 0);
    let starts = starts(&outcome.events);
    let completions = completions(&outcome.events);
    assert_eq!(starts.len(), 3);
    assert_eq!(completions.len(), 3);
    assert_attempt_identity(&outcome.events);
    for index in 0..3 {
        assert_eq!(starts[index].2, 0);
        assert_eq!(starts[index].1, completions[index].1);
        assert!(starts[index].0 < completions[index].0);
        for next in &starts[index + 1..] {
            assert_ne!(starts[index].1, next.1);
            assert!(completions[index].0 < next.0);
        }
    }
    let reused_proposal = outcome
        .events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::ToolCallProposed(call) if call.id == "cached-read"))
        .unwrap();
    let invalid_rejection = outcome
        .events
        .iter()
        .position(|event| {
            matches!(event, RuntimeEvent::ToolCallValidationRejected { call, result }
            if call.id == "invalid-bash" && result.status == RuntimeToolStatus::Rejected)
        })
        .unwrap();
    assert!(completions[0].0 < reused_proposal);
    assert!(completions[1].0 < invalid_rejection);
    assert!(outcome.events.iter().any(|event| matches!(event,
        RuntimeEvent::ToolCallFinished(result)
            if result.call_id == "cached-read" && result.reused
                && result.status == RuntimeToolStatus::Success && result.output == "cached")));
    for (index, event) in outcome.events.iter().enumerate() {
        if let RuntimeEvent::ToolRoundCompleted { response_id, .. } = event {
            let completion = completions
                .iter()
                .find(|(_, id)| *id == response_id)
                .unwrap();
            assert!(completion.0 < index);
        }
        assert!(!matches!(
            event,
            RuntimeEvent::AssistantResponseCompleted { .. } | RuntimeEvent::ToolCallRunning { .. }
        ));
    }
}
