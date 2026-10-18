use super::*;

const NOTICE: &str = "Give the user a brief factual progress update";

struct ProgressSteering(Mutex<Option<RuntimeSteeringInput>>);

impl RuntimeSteeringSource for ProgressSteering {
    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        async move { Ok(self.0.lock().unwrap().take()) }.boxed()
    }
}

struct WaitingHook(MockProvider);

impl RuntimePermissionHook for WaitingHook {
    fn request(
        &self,
        _: RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        let provider = self.0.clone();
        async move {
            assert_eq!(provider.requests().len(), 1);
            tokio::time::sleep(Duration::from_secs(180)).await;
            assert_eq!(provider.requests().len(), 1);
            Ok(RuntimeUserDecision::Once)
        }
        .boxed()
    }
}

impl RuntimeUserQuestionHook for WaitingHook {
    fn request(
        &self,
        question: RuntimeUserQuestionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, RuntimeUserQuestionResponse> {
        let provider = self.0.clone();
        async move {
            assert_eq!(provider.requests().len(), 1);
            tokio::time::sleep(Duration::from_secs(180)).await;
            assert_eq!(provider.requests().len(), 1);
            RuntimeUserQuestionResponse::Answered(UserQuestionAnswers {
                answers: question
                    .input
                    .questions
                    .into_iter()
                    .map(|question| {
                        (
                            question.id,
                            UserQuestionAnswer {
                                answers: vec![question.options[0].label.clone()],
                            },
                        )
                    })
                    .collect(),
            })
        }
        .boxed()
    }
}

fn read_round(id: &str, delay: u64, text: Option<&str>) -> Vec<ScriptStep> {
    let mut steps = vec![ScriptStep::delay(Duration::from_secs(delay))];
    if let Some(text) = text {
        steps.push(ScriptStep::text(text));
    }
    steps.push(ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: id.into(),
            name: "read".into(),
            input_json: r#"{"path":"test.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]));
    steps
}

fn final_round() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Completed with evidence.".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn notice_count(request: &ChatRequest) -> usize {
    request
        .messages
        .iter()
        .filter(|message| message.content.contains(NOTICE))
        .count()
}

fn assert_append_only_requests(requests: &[ChatRequest]) {
    for pair in requests.windows(2) {
        assert_eq!(
            pair[0].messages.as_slice(),
            &pair[1].messages[..pair[0].messages.len()]
        );
        assert_eq!(pair[0].tools, pair[1].tools);
    }
}

#[tokio::test]
async fn progress_guidance_for_invalid_arguments_keeps_the_call_non_executable() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("test.txt"), "observed").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "invalid".into(),
                name: "bash".into(),
                input_json: "{}".into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        read_round("corrected", 0, None),
        final_round(),
    ]);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        project.path().join("checkpoints"),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 1);
    let rejected = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallValidationRejected { result, .. }
                if result.call_id == "invalid" =>
            {
                Some(result)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(rejected.status, RuntimeToolStatus::Rejected);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(notice_count(&requests[1]), 1);
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| message.tool_calls.is_empty() && message.tool_call_id.is_none())
    );
    let observation = requests[1]
        .messages
        .iter()
        .filter_map(|message| serde_json::from_str::<serde_json::Value>(&message.content).ok())
        .find(|value| value["vega_history_observation"]["kind"] == "tool_result")
        .unwrap();
    assert_eq!(
        observation,
        serde_json::json!({"vega_history_observation":{"version":1,"kind":"tool_result","authority":"none","tool":"bash","call_id":"invalid","status":null,"output":rejected.output,"advisory":PROGRESS_NOTICE}})
    );
    assert_eq!(
        crate::project_model_messages(&requests[1].messages).unwrap(),
        requests[1].messages
    );
    assert_append_only_requests(&requests);
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| !message.content.contains(NOTICE))
    );
}

#[tokio::test(start_paused = true)]
async fn progress_reminders_follow_complete_batches_and_sixty_second_visible_cadence() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("test.txt"), "observed").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("initial", 0, None),
        read_round("early", 59, None),
        read_round("due", 1, None),
        read_round("visible", 0, Some("The file confirms the next step.")),
        read_round("visible-early", 59, None),
        read_round("visible-due", 1, None),
        final_round(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests.iter().map(notice_count).collect::<Vec<_>>(),
        [0, 1, 1, 2, 2, 2, 3]
    );
    let combined = &requests[3].messages.last().unwrap().content;
    assert!(combined.starts_with("1 | observed\n\n<vega_runtime_notice>"));
    assert!(combined.contains("The same tool inputs produced the same successful result"));
    assert!(combined.contains(NOTICE));
    assert_eq!(combined.matches("<vega_runtime_notice>").count(), 1);
    assert_append_only_requests(&requests);
    assert!(requests.iter().all(|request| !request.tools.is_empty()));
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(
        outcome.final_text,
        "The file confirms the next step.Completed with evidence."
    );
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| !message.content.contains(NOTICE))
    );
    assert!(outcome.events.iter().all(|event| match event {
        RuntimeEvent::TextDelta(text) => !text.contains(NOTICE),
        RuntimeEvent::ToolCallFinished(result) => result.output == "1 | observed",
        _ => true,
    }));
}

#[tokio::test(start_paused = true)]
async fn progress_uses_retained_text_and_does_not_count_retry_rollback() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("test.txt"), "observed").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let mut retry = vec![
        ScriptStep::delay(Duration::from_secs(60)),
        ScriptStep::text("discarded update"),
        ScriptStep::events(vec![ProviderEvent::RetryScheduled {
            attempt: 1,
            max_retries: 1,
        }]),
    ];
    retry.extend(read_round("retry", 0, None));
    let provider =
        MockProvider::new_rounds(vec![read_round("initial", 0, None), retry, final_round()]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(
        requests.iter().map(notice_count).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(outcome.final_text, "Completed with evidence.");
    assert!(
        !requests[2]
            .messages
            .iter()
            .any(|message| message.content.contains("discarded update"))
    );
}

#[tokio::test(start_paused = true)]
async fn progress_waits_for_every_tool_and_never_issues_a_timer_request() {
    let project = tempdir().unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "one".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf first"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "two".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf second"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        final_round(),
    ]);
    let observed_provider = provider.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, _, _| {
            let provider = observed_provider.clone();
            Box::pin(async move {
                assert_eq!(provider.requests().len(), 1);
                tokio::time::sleep(Duration::from_secs(180)).await;
                assert_eq!(provider.requests().len(), 1);
                Ok(vega_tools::BashOutput {
                    text: command,
                    exit_code: 0,
                    duration_ms: 180_000,
                    truncated: false,
                })
            })
        }));
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        project.path().join("checkpoints"),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let results = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].content, "printf first");
    assert_eq!(results[1].tool_call_id.as_deref(), Some("two"));
    assert_eq!(
        results[1].content,
        format!("printf second{PROGRESS_NOTICE}")
    );
    assert_eq!(notice_count(&requests[1]), 1);
    assert_eq!(outcome.executed_tool_call_count, 2);
}

#[tokio::test(start_paused = true)]
async fn progress_never_adds_calls_during_provider_permission_or_question_waits() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(|command, _, _| {
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text: command,
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    for failure in [false, true] {
        let mut steps = vec![ScriptStep::delay(Duration::from_secs(360))];
        if failure {
            steps.push(ScriptStep::Error {
                status: Some(400),
                message: "provider rejected".into(),
                retryable: false,
            });
        } else {
            steps.extend(final_round());
        }
        let provider = MockProvider::new(steps);
        let operation = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        );
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => panic!("provider finished before its response: {result:?}"),
            _ = tokio::time::sleep(Duration::from_secs(180)) => {}
        }
        assert_eq!(provider.requests().len(), 1);
        let outcome = operation.await.unwrap();
        assert_eq!(outcome.failed, failure);
        assert_eq!(provider.requests().len(), 1);
        assert!(
            outcome
                .messages
                .iter()
                .all(|message| message.without_runtime_guidance() == *message)
        );
    }
    for question in [false, true] {
        let (name, arguments) = if question {
            (
                USER_QUESTION_TOOL,
                r#"{"questions":[{"id":"direction","header":"Direction","question":"Which direction?","options":[{"label":"Continue","description":"Continue the task"},{"label":"Revise","description":"Choose another approach"}]}]}"#,
            )
        } else {
            ("bash", r#"{"cmd":"printf verified"}"#)
        };
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "waiting".into(),
                    name: name.into(),
                    input_json: arguments.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            final_round(),
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            project.path().join("checkpoints"),
        )
        .with_user_questions(Some(Arc::new(WaitingHook(provider.clone()))));
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &WaitingHook(provider.clone()),
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(!outcome.failed && !outcome.interrupted);
        assert_eq!(outcome.executed_tool_call_count, 1);
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(notice_count(&requests[1]), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn progress_does_not_limit_five_hundred_productive_rounds_or_fabricate_updates() {
    let project = tempdir().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let counted = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, _, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text: if matches!(
                        command.as_str(),
                        "printf progress-0" | "printf progress-249" | "printf progress-499"
                    ) {
                        format!("{command}\n{}", "x".repeat(20_000))
                    } else {
                        command
                    },
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let mut scripts = (0..500)
        .map(|index| {
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: format!("step-{index}"),
                    name: "bash".into(),
                    input_json: serde_json::json!({"cmd":format!("printf progress-{index}")})
                        .to_string(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]
        })
        .collect::<Vec<_>>();
    scripts.push(final_round());
    let provider = MockProvider::new_rounds(scripts);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        project.path().join("checkpoints"),
    );
    assert_eq!(req.tool_config.turn_limit, 0);
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(executions.load(Ordering::SeqCst), 500);
    assert_eq!(outcome.executed_tool_call_count, 500);
    assert_eq!(outcome.final_text, "Completed with evidence.");
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
    ));
    let requests = provider.requests();
    assert_eq!(requests.len(), 501);
    assert_eq!(
        requests[500]
            .messages
            .iter()
            .filter(|message| message.content.contains("vega_tool_preview"))
            .count(),
        3
    );
    assert!(
        requests[500]
            .messages
            .iter()
            .all(|message| !message.content.contains("vega_tool_result_reference"))
    );
    for index in [0, 249, 499] {
        let id = format!("step-{index}");
        let canonical = outcome
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id.as_str()))
            .unwrap();
        assert_eq!(
            canonical.content,
            format!("printf progress-{index}\n{}", "x".repeat(20_000))
        );
    }
    assert_eq!(notice_count(&requests[500]), 1);
    assert_eq!(
        requests[500]
            .messages
            .iter()
            .filter(|message| message.content.contains("Review the confirmed facts"))
            .count(),
        31
    );
    for (completed_batches, request) in requests.iter().enumerate() {
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|message| message.content.contains("Review the confirmed facts"))
                .count(),
            completed_batches / 16
        );
    }
    assert_append_only_requests(&requests);
}

#[tokio::test]
async fn progress_preserves_reasoning_replay_pairing_and_idempotent_wire_projection() {
    for policy in [
        crate::ReasoningReplayPolicy::DeepSeekTools,
        crate::ReasoningReplayPolicy::ResponsesItems,
    ] {
        let project = tempdir().unwrap();
        fs::write(project.path().join("test.txt"), "observed").unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let reasoning = serde_json::json!({"type":"reasoning","id":"reasoning-1","encrypted_content":"opaque-state"});
        let mut events = vec![ProviderEvent::ThinkingDelta("private-state".into())];
        if policy == crate::ReasoningReplayPolicy::ResponsesItems {
            events.push(ProviderEvent::ReasoningReplay(vec![reasoning]));
        }
        events.extend([
            ProviderEvent::ToolUse {
                id: "read".into(),
                name: "read".into(),
                input_json: r#"{"path":"test.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]);
        let complete = replay_fixture_events(
            vec![
                ProviderEvent::TextDelta("Completed with evidence.".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ],
            policy,
        );
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(replay_fixture_events(events, policy))],
            vec![ScriptStep::events(complete.clone())],
        ]);
        let route = [31; 32];
        let mut req = replay_fixture_request(Vec::new(), policy);
        req.replay_frame
            .as_mut()
            .unwrap()
            .attach_route_fingerprint(route);
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert!(!outcome.failed);
        let requests = provider.requests();
        let assistant = requests[1]
            .messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap();
        let original = outcome
            .messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap();
        assert_eq!(assistant.reasoning_content, original.reasoning_content);
        assert_eq!(assistant.response_reasoning, original.response_reasoning);
        assert_eq!(
            assistant.response_replay_layout,
            original.response_replay_layout
        );
        assert_eq!(assistant.tool_calls[0].id, "read");
        assert_eq!(
            requests[1].messages.last().unwrap().tool_call_id.as_deref(),
            Some("read")
        );
        assert_eq!(notice_count(&requests[1]), 1);
        assert_eq!(
            crate::project_model_messages(&requests[1].messages).unwrap(),
            requests[1].messages
        );
        assert!(
            requests[1]
                .messages
                .iter()
                .all(|message| !message.content.contains("private-state")
                    && !message.content.contains("opaque-state"))
        );
        let previous = crate::project_model_messages(&outcome.messages).unwrap();
        let fingerprint = outcome.messages[0]
            .replay_frame()
            .and_then(crate::ReplayFrame::environment_fingerprint)
            .unwrap();
        let mut next = replay_fixture_request(
            vec![ChatMessage::new(ChatRole::User, "Continue the task")],
            policy,
        );
        let frame = next.replay_frame.as_mut().unwrap();
        frame.attach_route_fingerprint(route);
        frame.attach_candidate(outcome.messages, fingerprint);
        let continued_provider = MockProvider::new(vec![ScriptStep::events(complete)]);
        let continued = run_agent(&continued_provider, &tools, next, CancellationToken::new())
            .await
            .unwrap();
        assert!(!continued.failed);
        let continued_requests = continued_provider.requests();
        assert_eq!(continued_requests.len(), 1);
        assert_eq!(
            &continued_requests[0].messages[..previous.len()],
            previous.as_slice()
        );
        assert_eq!(notice_count(&continued_requests[0]), 1);
        assert_eq!(
            continued_requests[0].messages.last().unwrap().content,
            "Continue the task"
        );
    }
}

#[tokio::test]
async fn progress_steering_has_priority_and_cancellation_adds_no_reminder_or_request() {
    for cancel_on_result in [false, true] {
        let project = tempdir().unwrap();
        fs::write(project.path().join("test.txt"), "observed").unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            read_round("before-steer", 0, None),
            read_round("after-steer", 0, None),
            final_round(),
        ]);
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "original")]);
        req.tool_config = req
            .tool_config
            .with_steering_source(Arc::new(ProgressSteering(Mutex::new(Some(
                RuntimeSteeringInput {
                    original_content: "follow-up".into(),
                    user_message_seq: 2,
                    id: "pending".into(),
                    assistant_message_id: "next-assistant".into(),
                    message: ChatMessage::new(ChatRole::User, "follow-up"),
                },
            )))));
        let cancel = CancellationToken::new();
        let sink_cancel = cancel.clone();
        let outcome = run_agent_with_sink(&provider, &tools, req, cancel, move |event| {
            if cancel_on_result && matches!(event, RuntimeEvent::ToolCallFinished(_)) {
                sink_cancel.cancel();
            }
            async { Ok(()) }
        })
        .await
        .unwrap();
        let requests = provider.requests();
        if cancel_on_result {
            assert!(outcome.interrupted);
            assert_eq!(requests.len(), 1);
            assert!(
                outcome
                    .messages
                    .iter()
                    .all(|message| message.without_runtime_guidance() == *message)
            );
        } else {
            assert_eq!(requests.len(), 3);
            assert_eq!(notice_count(&requests[1]), 0);
            assert_eq!(requests[1].messages.last().unwrap().content, "follow-up");
            assert_eq!(notice_count(&requests[2]), 1);
        }
    }
}
