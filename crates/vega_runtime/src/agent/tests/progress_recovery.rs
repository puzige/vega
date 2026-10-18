use super::*;

const FAILURE: &str = "The same tool inputs produced the same unsuccessful evidence";
const REPEATED: &str = "The same tool inputs produced the same successful result";
const REVIEW: &str = "Review the confirmed facts";

fn call(id: &str, input: &str) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: "bash".into(),
        input_json: input.into(),
    }
}

fn batch(calls: Vec<ProviderEvent>) -> Vec<ScriptStep> {
    let mut events = calls;
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    vec![ScriptStep::events(events)]
}

fn complete() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Complete.".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn configured_request(project: &std::path::Path, history: Vec<ChatMessage>) -> AgentRequest {
    let mut req = request(history);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        project.join("checkpoints"),
    );
    req
}

fn count(request: &ChatRequest, marker: &str) -> usize {
    request
        .messages
        .iter()
        .filter(|message| message.content.contains(marker))
        .count()
}

fn assert_wire_prefixes(requests: &[ChatRequest]) {
    for pair in requests.windows(2) {
        assert_eq!(
            pair[0].messages.as_slice(),
            &pair[1].messages[..pair[0].messages.len()]
        );
        assert_eq!(pair[0].tools, pair[1].tools);
    }
    assert!(requests.iter().all(|request| !request.tools.is_empty()));
    for request in requests {
        assert_eq!(
            crate::project_model_messages(&request.messages).unwrap(),
            request.messages
        );
    }
}

#[tokio::test]
async fn interleaved_success_keeps_failure_evidence_and_never_replays_or_disables_tools() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let counted = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, _, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let failed = command == "fail";
                Ok(vega_tools::BashOutput {
                    text: if failed {
                        "same failure".into()
                    } else {
                        command
                    },
                    exit_code: if failed { 1 } else { 0 },
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let inputs = [
        r#"{"cmd":"fail","timeout_ms":1000}"#,
        r#"{"cmd":"read-a"}"#,
        r#"{"timeout_ms":1000,"cmd":"fail"}"#,
        r#"{"cmd":"read-b"}"#,
        r#"{"cmd":"fail","timeout_ms":1000}"#,
        r#"{"cmd":"fail","timeout_ms":1000}"#,
        r#"{"cmd":"fail","timeout_ms":1000}"#,
    ];
    let mut rounds = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| batch(vec![call(&format!("call-{index}"), input)]))
        .collect::<Vec<_>>();
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(project.path(), Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.final_text, "Complete.");
    assert_eq!(outcome.executed_tool_call_count, 7);
    assert_eq!(executions.load(Ordering::SeqCst), 7);
    let requests = provider.requests();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, FAILURE))
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 1, 1, 1]
    );
    assert_wire_prefixes(&requests);
    let failed = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.exit_code == Some(1) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 5);
    assert!(failed.iter().all(|result| result.output == "same failure"
        && result.exit_code == Some(1)
        && result.status == RuntimeToolStatus::Success));
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| !message.content.contains("vega_runtime_notice"))
    );
}

#[tokio::test]
async fn successful_polling_counts_batches_once_and_all_requested_calls_still_execute() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let counted = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: "pending".into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        batch(
            (0..3)
                .map(|index| call(&format!("first-{index}"), r#"{"cmd":"poll"}"#))
                .collect(),
        ),
        batch(vec![call("second", r#"{"cmd":"poll"}"#)]),
        batch(vec![call("third", r#"{"cmd":"poll"}"#)]),
        batch(vec![call("fourth", r#"{"cmd":"poll"}"#)]),
        complete(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(project.path(), Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 6);
    assert_eq!(executions.load(Ordering::SeqCst), 6);
    let requests = provider.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, REPEATED))
            .collect::<Vec<_>>(),
        [0, 0, 0, 1, 1]
    );
    assert_eq!(count(&requests[4], FAILURE), 0);
    assert_eq!(count(&requests[4], REVIEW), 0);
    assert_wire_prefixes(&requests);
    assert!(outcome.events.iter().all(|event| match event {
        RuntimeEvent::ToolCallFinished(result) =>
            result.output == "pending" && result.status == RuntimeToolStatus::Success,
        _ => true,
    }));
}

#[tokio::test]
async fn invalid_argument_recovery_stays_in_existing_observation_and_corrected_input_runs() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let counted = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, _, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text: command,
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let mut rounds = (0..3)
        .map(|index| batch(vec![call(&format!("invalid-{index}"), "{}")]))
        .collect::<Vec<_>>();
    rounds.push(batch(vec![call("corrected", r#"{"cmd":"valid"}"#)]));
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(project.path(), Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    let requests = provider.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, FAILURE))
            .collect::<Vec<_>>(),
        [0, 0, 0, 1, 1]
    );
    assert_eq!(requests[3].messages.len(), requests[0].messages.len() + 6);
    assert!(
        requests[3]
            .messages
            .iter()
            .all(|message| message.tool_calls.is_empty() && message.tool_call_id.is_none())
    );
    let rejections = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallValidationRejected { result, .. } => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rejections.len(), 3);
    for rejected in rejections {
        assert_eq!(rejected.status, RuntimeToolStatus::Rejected);
        let observation = requests[3]
            .messages
            .iter()
            .filter_map(|message| serde_json::from_str::<serde_json::Value>(&message.content).ok())
            .find(|value| {
                value["vega_history_observation"]["kind"] == "tool_result"
                    && value["vega_history_observation"]["call_id"] == rejected.call_id
            })
            .unwrap();
        assert_eq!(observation["vega_history_observation"]["authority"], "none");
        assert_eq!(
            observation["vega_history_observation"]["output"],
            rejected.output
        );
        if rejected.call_id == "invalid-2" {
            assert!(
                observation["vega_history_observation"]["advisory"]
                    .as_str()
                    .unwrap()
                    .contains(FAILURE)
            );
        }
    }
    assert_wire_prefixes(&requests);
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| !message.content.contains("vega_runtime_notice"))
    );
}

#[tokio::test]
async fn full_runtime_result_fingerprint_detects_changes_hidden_by_model_preview() {
    let project = tempdir().unwrap();
    let outputs = ["a", "a", "b", "b", "b"].map(|middle| {
        format!(
            "{}{}{}",
            "head".repeat(20_000),
            middle,
            "tail".repeat(20_000)
        )
    });
    let expected = outputs.clone();
    let executions = Arc::new(AtomicUsize::new(0));
    let counted = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            let text = outputs[counted.fetch_add(1, Ordering::SeqCst)].clone();
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text,
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let mut rounds = (0..5)
        .map(|index| {
            batch(vec![call(
                &format!("large-{index}"),
                r#"{"cmd":"inspect"}"#,
            )])
        })
        .collect::<Vec<_>>();
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(project.path(), Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 5);
    assert_eq!(executions.load(Ordering::SeqCst), 5);
    let requests = provider.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, REPEATED))
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 1]
    );
    let bodies = outcome
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 5);
    assert!(bodies.iter().all(|body| body.len() < expected[0].len()));
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
    let audited = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result.output.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        audited,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_wire_prefixes(&requests);
}

fn polling_tools(project: &std::path::Path, executions: Arc<AtomicUsize>) -> vega_tools::Tools {
    vega_tools::Tools::new(project)
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            executions.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: "pending".into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }))
}

fn polls(prefix: &str, count: usize) -> Vec<Vec<ScriptStep>> {
    (0..count)
        .map(|index| {
            batch(vec![call(
                &format!("{prefix}-{index}"),
                r#"{"cmd":"poll"}"#,
            )])
        })
        .collect()
}

struct SecondBoundarySteering(AtomicUsize);

impl RuntimeSteeringSource for SecondBoundarySteering {
    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        let second = self.0.fetch_add(1, Ordering::SeqCst) == 1;
        async move {
            Ok(second.then(|| RuntimeSteeringInput {
                original_content: "Use the new criteria".into(),
                user_message_seq: 2,
                id: "new-criteria".into(),
                assistant_message_id: "next-assistant".into(),
                message: ChatMessage::new(ChatRole::User, "Use the new criteria"),
            }))
        }
        .boxed()
    }
}

#[tokio::test]
async fn steering_resets_prior_evidence_without_rewriting_frozen_request_prefixes() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = polling_tools(project.path(), executions.clone());
    let mut rounds = polls("query", 5);
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let mut req = configured_request(project.path(), Vec::new());
    req.tool_config = req
        .tool_config
        .with_steering_source(Arc::new(SecondBoundarySteering(AtomicUsize::new(0))));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 5);
    assert_eq!(executions.load(Ordering::SeqCst), 5);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::SteeringAccepted(_)))
            .count(),
        1
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, REPEATED))
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 1]
    );
    assert_eq!(
        requests[2].messages.last().unwrap().content,
        "Use the new criteria"
    );
    assert_wire_prefixes(&requests);
}

#[tokio::test]
async fn successful_compaction_installs_fresh_evidence_window_and_preserves_execution() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = polling_tools(project.path(), executions.clone());
    let mut rounds = polls("before", 2);
    rounds.push(vec![ScriptStep::Error {
        status: Some(400),
        message:
            r#"responses request failed (HTTP 400): {"error":{"code":"context_length_exceeded"}}"#
                .into(),
        retryable: false,
    }]);
    rounds.extend(polls("after", 3));
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let compactions = Arc::new(AtomicUsize::new(0));
    let mut req = configured_request(
        project.path(),
        vec![
            ChatMessage::new(ChatRole::User, "Keep checking the result"),
            ChatMessage::new(ChatRole::Assistant, "Earlier observations ".repeat(1_000)),
            ChatMessage::new(ChatRole::User, "Continue"),
        ],
    );
    req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: compactions.clone(),
        messages: vec![ChatMessage::new(
            ChatRole::User,
            "Prior observations retained; continue checking",
        )],
        source_version: 1,
    }));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 5);
    assert_eq!(executions.load(Ordering::SeqCst), 5);
    assert_eq!(compactions.load(Ordering::SeqCst), 1);
    let requests = provider.requests();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, REPEATED))
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 0, 0, 1]
    );
    assert_eq!(
        requests[3].messages[1].content,
        "Prior observations retained; continue checking"
    );
    assert_wire_prefixes(&requests[..3]);
    assert_wire_prefixes(&requests[3..]);
}

#[tokio::test]
async fn stop_before_boundary_and_provider_failure_do_not_persist_evidence_into_a_new_run() {
    for failed in [false, true] {
        let project = tempdir().unwrap();
        let executions = Arc::new(AtomicUsize::new(0));
        let tools = polling_tools(project.path(), executions.clone());
        let mut rounds = polls("old", if failed { 2 } else { 3 });
        rounds.push(vec![ScriptStep::Error {
            status: Some(400),
            message: "provider rejected".into(),
            retryable: false,
        }]);
        let provider = MockProvider::new_rounds(rounds);
        let cancel = CancellationToken::new();
        let sink_cancel = cancel.clone();
        let outcome = run_agent_with_sink(&provider, &tools, configured_request(project.path(), Vec::new()), cancel, move |event| {
            if !failed && matches!(event, RuntimeEvent::ToolCallFinished(ref result) if result.call_id == "old-2") {
                sink_cancel.cancel();
            }
            async { Ok(()) }
        }).await.unwrap();
        assert_eq!(outcome.failed, failed);
        assert_eq!(outcome.interrupted, !failed);
        assert_eq!(provider.requests().len(), 3);
        assert!(
            provider
                .requests()
                .iter()
                .all(|request| count(request, REPEATED) == 0)
        );
        assert!(
            crate::project_model_messages(&outcome.messages)
                .unwrap()
                .iter()
                .all(|message| !message.content.contains(REPEATED))
        );
        let first_executions = if failed { 2 } else { 3 };
        assert_eq!(outcome.executed_tool_call_count, first_executions);
        let mut history = outcome
            .messages
            .into_iter()
            .filter(|message| message.role != ChatRole::System)
            .map(|message| message.without_runtime_guidance())
            .collect::<Vec<_>>();
        history.push(ChatMessage::new(ChatRole::User, "Continue"));
        let mut rounds = polls("resumed", 3);
        rounds.push(complete());
        let resumed_provider = MockProvider::new_rounds(rounds);
        let resumed = run_agent(
            &resumed_provider,
            &tools,
            configured_request(project.path(), history),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!resumed.failed && !resumed.interrupted);
        assert_eq!(resumed.executed_tool_call_count, 3);
        assert_eq!(executions.load(Ordering::SeqCst), first_executions + 3);
        let requests = resumed_provider.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests
                .iter()
                .map(|request| count(request, REPEATED))
                .collect::<Vec<_>>(),
            [0, 0, 0, 1]
        );
        assert_wire_prefixes(&requests);
    }
}

#[tokio::test]
async fn reused_results_preserve_execution_identity_and_distinct_truncation_evidence() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = polling_tools(project.path(), executions.clone());
    let mut rounds = (0..4)
        .map(|_| batch(vec![call("same-call-id", r#"{"cmd":"poll"}"#)]))
        .collect::<Vec<_>>();
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(project.path(), Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    let results = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 4);
    assert!(!results[0].reused);
    assert_eq!(results[0].truncated, Some(false));
    assert!(
        results[1..].iter().all(|result| result.reused
            && result.truncated.is_none()
            && result.output == "pending")
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, REPEATED))
            .collect::<Vec<_>>(),
        [0, 0, 0, 0, 1]
    );
    assert_wire_prefixes(&requests);
}

#[tokio::test]
async fn repeated_permission_denials_remain_denials_and_never_execute_or_add_prompts() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = polling_tools(project.path(), executions.clone());
    let mut rounds = polls("denied", 3);
    rounds.push(complete());
    let provider = MockProvider::new_rounds(rounds);
    let mut req = configured_request(project.path(), Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        project.path().join("checkpoints"),
    );
    let prompts = Arc::new(AtomicUsize::new(0));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &FixedHook {
            calls: prompts.clone(),
            decision: Some(RuntimeUserDecision::Deny { note: None }),
        },
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    assert_eq!(prompts.load(Ordering::SeqCst), 3);
    let denied = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(denied.len(), 3);
    assert!(denied.iter().all(|result| {
        result.status == RuntimeToolStatus::Rejected
            && result
                .approval
                .as_ref()
                .is_some_and(|approval| approval.decision == RuntimeApprovalDecision::Deny)
    }));
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .map(|request| count(request, FAILURE))
            .collect::<Vec<_>>(),
        [0, 0, 0, 1]
    );
    assert!(
        requests[3]
            .messages
            .last()
            .unwrap()
            .content
            .contains("Respect permission decisions and verify uncertain side effects")
    );
    assert_wire_prefixes(&requests);
}
