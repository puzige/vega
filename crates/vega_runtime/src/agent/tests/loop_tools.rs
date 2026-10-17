use super::*;

#[tokio::test]
async fn exact_same_turn_read_write_and_bash_calls_reuse_durable_results_once() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source").unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let recorded = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, full_access, _| {
            assert_eq!(command, "printf bash-ok");
            assert!(!full_access);
            recorded.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: "bash-ok".into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let read = || ProviderEvent::ToolUse {
        id: "read-1".into(),
        name: "read".into(),
        input_json: r#"{"path":"source.txt"}"#.into(),
    };
    let write = || ProviderEvent::ToolUse {
        id: "write-1".into(),
        name: "write".into(),
        input_json: r#"{"path":"new.txt","content":"body"}"#.into(),
    };
    let bash = || ProviderEvent::ToolUse {
        id: "bash-1".into(),
        name: "bash".into(),
        input_json: r#"{"cmd":"printf bash-ok"}"#.into(),
    };
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            read(),
            read(),
            write(),
            write(),
            bash(),
            bash(),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        checkpoint,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedHook {
        calls: calls.clone(),
        decision: Some(RuntimeUserDecision::Once),
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
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(outcome.executed_tool_call_count, 3);
    assert_eq!(
        fs::read_to_string(project.path().join("new.txt")).unwrap(),
        "body"
    );
    let reused = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.reused => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(reused.len(), 3);
    assert!(
        reused
            .iter()
            .all(|result| { result.approval.is_some() && result.truncated.is_none() })
    );
}

#[tokio::test]
async fn cancellation_at_proposal_or_running_ack_starts_no_mutation() {
    for cancel_on_running in [false, true] {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"new.txt","content":"must-not-write"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Auto,
            checkpoint.clone(),
        );
        let cancel = CancellationToken::new();
        let sink_cancel = cancel.clone();
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            cancel,
            &FixedHook {
                calls: Arc::new(AtomicUsize::new(0)),
                decision: Some(RuntimeUserDecision::Once),
            },
            move |event| {
                let should_cancel = if cancel_on_running {
                    matches!(event, RuntimeEvent::ToolCallRunning { .. })
                } else {
                    matches!(event, RuntimeEvent::ToolCallProposed(_))
                };
                if should_cancel {
                    sink_cancel.cancel();
                }
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert!(outcome.interrupted);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(!project.path().join("new.txt").exists());
        assert_eq!(fs::read_dir(&checkpoint).unwrap().count(), 0);
        let terminal = outcome.events.iter().find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        });
        if cancel_on_running {
            assert!(matches!(
                terminal,
                Some(RuntimeToolResult {
                    status: RuntimeToolStatus::Cancelled,
                    output,
                    ..
                }) if output == CANCELLED_BEFORE_EXECUTION_OUTPUT
            ));
        } else {
            assert!(matches!(
                terminal,
                Some(RuntimeToolResult {
                    status: RuntimeToolStatus::Rejected,
                    approval: Some(RuntimeApprovalAudit {
                        source: RuntimeApprovalSource::Timeout,
                        ..
                    }),
                    ..
                })
            ));
        }
    }
}

#[tokio::test]
async fn text_only_preserves_events_usage_and_visible_content() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("a".into()),
        ProviderEvent::ThinkingDelta("reason".into()),
        ProviderEvent::TextDelta("b".into()),
        ProviderEvent::Usage {
            input: 5,
            output: 2,
            cache_read: Some(1),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(vec![ChatMessage::new(ChatRole::User, "hello")]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "ab");
    assert!(!outcome.final_text.contains("reason"));
    assert_eq!(provider.requests().len(), 1);
    let semantic_events = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::DiagnosticAttempt(_) | RuntimeEvent::AssistantResponseStarted { .. } => {
                None
            }
            event => Some(event.clone()),
        })
        .collect::<Vec<_>>();
    assert!(matches!(&semantic_events[..], [
            RuntimeEvent::TextDelta(first),
            RuntimeEvent::ThinkingDelta(thinking),
            RuntimeEvent::TextDelta(second),
            RuntimeEvent::UsageUpdated { usage: RuntimeTokenUsage { input: 5, output: 2, cache_read: Some(1), cache_write: Some(0) }, cost_microcents: 0, pricing: None },
            RuntimeEvent::AssistantResponseCompleted { text_start_bytes: 0, text_end_bytes: 2, kind: RuntimeAssistantResponseKind::Final, .. },
            RuntimeEvent::Finished(RuntimeFinishReason::End),
        ] if first == "a" && thinking == "reason" && second == "b"));
}

#[tokio::test]
async fn tool_observe_round_uses_real_grep_and_converges() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "// TODO: wire loop\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("searching".into()),
            ProviderEvent::ToolUse {
                id: "call-grep".into(),
                name: "grep".into(),
                input_json: r#"{"pattern":"TODO"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 10,
                output: 2,
                cache_read: Some(1),
                cache_write: Some(0),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Found lib.rs TODO".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(vec![ChatMessage::new(ChatRole::User, "Find TODOs")]),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.final_text, "Found lib.rs TODO");
    assert_eq!(outcome.tool_call_count, 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallOutput { chunk, .. } if chunk.contains("lib.rs:1:// TODO")
    )));
    let approved = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallApproved { call_id, .. } if call_id == "call-grep"))
            .unwrap();
    let running = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "call-grep"))
            .unwrap();
    let succeeded = outcome
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                    call_id,
                    status: RuntimeToolStatus::Success,
                    ..
                }) if call_id == "call-grep"
            )
        })
        .unwrap();
    assert!(approved < running && running < succeeded);
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
    ));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].messages[0].role, ChatRole::System);
    assert!(requests[1].messages.iter().any(|message| {
        message.role == ChatRole::Tool
            && message.tool_call_id.as_deref() == Some("call-grep")
            && message.content.contains("lib.rs:1:// TODO")
    }));
}

#[tokio::test]
async fn one_turn_executes_read_glob_and_grep_serially() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "// TODO: all tools\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-1".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "glob-1".into(),
                name: "glob".into(),
                input_json: r#"{"pattern":"*.rs"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "grep-1".into(),
                name: "grep".into(),
                input_json: r#"{"pattern":"TODO"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let outputs: Vec<(&str, &str)> = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallOutput { call_id, chunk } => {
                Some((call_id.as_str(), chunk.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(outputs.len(), 3);
    assert_eq!(outputs[0], ("read-1", "1 | // TODO: all tools"));
    assert_eq!(outputs[1], ("glob-1", "lib.rs"));
    assert_eq!(outputs[2], ("grep-1", "lib.rs:1:// TODO: all tools"));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let follow_up = &requests[1].messages;
    let assistant = follow_up
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .unwrap();
    assert_eq!(
        assistant
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        vec!["read-1", "glob-1", "grep-1"]
    );
    assert_eq!(
        follow_up
            .iter()
            .filter(|message| message.role == ChatRole::Tool)
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["read-1", "glob-1", "grep-1"]
    );
}

#[tokio::test]
async fn bad_json_and_external_read_denial_return_errors_then_model_continues() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bad-json".into(),
                name: "read".into(),
                input_json: "{".into(),
            },
            ProviderEvent::ToolUse {
                id: "escape".into(),
                name: "read".into(),
                input_json: r#"{"path":"../outside"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Handled both errors.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "Handled both errors.");
    let failures: Vec<&RuntimeToolResult> = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result)
                if matches!(
                    result.status,
                    RuntimeToolStatus::Failed | RuntimeToolStatus::Rejected
                ) =>
            {
                Some(result)
            }
            _ => None,
        })
        .collect();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0].status, RuntimeToolStatus::Failed);
    assert_eq!(
        failures[0].output,
        r#"Tool error: read requires path identifying a regular text file. Expected {"path":"src/lib.rs","offset":null,"limit":null}"#
    );
    assert_eq!(failures[1].status, RuntimeToolStatus::Rejected);
    assert!(failures[1].output.contains("permission denied"));
}

#[tokio::test]
async fn provider_error_emits_error_without_message_finished() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(503),
        message: "unavailable".into(),
        retryable: false,
    }]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.failed);
    let semantic_events = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::DiagnosticAttempt(_) | RuntimeEvent::AssistantResponseStarted { .. } => {
                None
            }
            event => Some(event.clone()),
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        semantic_events.as_slice(),
        [RuntimeEvent::Error(error)]
            if matches!(
                error.as_ref(),
                VegaError::Provider {
                    status: Some(503),
                    message,
                    retryable: false,
                } if message == "unavailable"
            )
    ));
}

#[tokio::test]
async fn invalid_write_is_rejected_and_observed() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: "{}".into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallValidationRejected { result: RuntimeToolResult { status: RuntimeToolStatus::Rejected, output, .. }, .. }
                if output.contains("invalid write input")
        )));
}

#[tokio::test]
async fn persisted_call_id_is_observed_without_execution() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "done-1".into(),
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
    let mut req = request(Vec::new());
    req.completed_tool_results.insert(
        "done-1".into(),
        CompletedToolCall {
            tool: "read".into(),
            input_json: r#"{"path":"missing"}"#.into(),
            result: RuntimeToolResult {
                call_id: "done-1".into(),
                output: "persisted output".into(),
                status: RuntimeToolStatus::Success,
                reused: true,
                exit_code: None,
                duration_ms: None,
                truncated: None,
                approval: None,
                remember_rule: None,
            },
        },
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.tool_call_count, 1);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult { reused: true, output, .. })
            if output == "persisted output"
    )));
}

#[tokio::test]
async fn conflicting_persisted_call_id_is_not_silently_reused_or_executed() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "done-1".into(),
                name: "read".into(),
                input_json: r#"{"path":"different"}"#.into(),
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
    req.completed_tool_results.insert(
        "done-1".into(),
        CompletedToolCall {
            tool: "read".into(),
            input_json: r#"{"path":"original"}"#.into(),
            result: RuntimeToolResult {
                call_id: "done-1".into(),
                output: "persisted output".into(),
                status: RuntimeToolStatus::Success,
                reused: true,
                exit_code: None,
                duration_ms: None,
                truncated: None,
                approval: None,
                remember_rule: None,
            },
        },
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.tool_call_count, 1);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallConflict { result: RuntimeToolResult {
            status: RuntimeToolStatus::Failed,
            reused: false,
            output,
            ..
        }, .. } if output == CALL_ID_CONFLICT_OUTPUT
    )));
    assert!(provider.requests()[1].messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.content == CALL_ID_CONFLICT_OUTPUT
    }));
}

#[tokio::test]
#[ignore = "load-sensitive: asserts a wall-clock budget (<1000ms), fails under parallel test load; run with --ignored"]
async fn cancellation_stops_a_delayed_provider_under_one_second() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::delay(Duration::from_secs(30)),
        ScriptStep::text("late"),
    ]);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let outcome = run_agent(&provider, &tools, request(Vec::new()), cancel)
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(outcome.interrupted);
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Interrupted)
    ));
}

#[tokio::test]
async fn cancellation_before_start_makes_no_provider_request() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::text("never")]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = run_agent(&provider, &tools, request(Vec::new()), cancel)
        .await
        .unwrap();
    assert!(outcome.interrupted);
    assert!(provider.requests().is_empty());
    assert!(matches!(
        outcome.events.as_slice(),
        [RuntimeEvent::Interrupted]
    ));
}

#[tokio::test]
async fn default_unlimited_turn_limit_runs_all_tool_calls_in_one_turn() {
    // With the default `turn_limit` (0 = unlimited) a single turn that
    // proposes more than 100 tool calls executes every one of them: there is
    // no per-tool-call hard stop anymore (Issue #114).
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let proposals = 101usize;
    let calls: Vec<ProviderEvent> = (0..proposals)
        .map(|index| ProviderEvent::ToolUse {
            id: format!("call-{index}"),
            name: "read".into(),
            input_json: r#"{"path":"a.txt"}"#.into(),
        })
        .chain(std::iter::once(ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        }))
        .collect();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(calls)],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.tool_call_count, proposals);
    assert_eq!(outcome.executed_tool_call_count, proposals);
    let finished_calls = outcome
        .events
        .iter()
        .filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(_)))
        .count();
    assert_eq!(finished_calls, proposals);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallOutput { call_id, .. } if call_id == "call-100"
    )));
    assert!(!outcome.final_text.contains("limit"));
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
    ));
}

#[tokio::test]
async fn configured_turn_limit_soft_stops_before_the_next_request() {
    // A positive `turn_limit` caps provider round-trips: the (limit + 1)th
    // request is never issued and the run ends with a visible notice.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    // Every turn proposes one read and asks for another round, forever.
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "same-call".into(),
            name: "read".into(),
            input_json: r#"{"path":"a.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let mut req = request(Vec::new());
    req.tool_config = RuntimeToolConfig::default().with_turn_limit(2);
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 2);
    assert!(outcome.final_text.contains("Agent turn limit (2) reached"));
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::TurnLimit))
    ));
}

#[test]
fn tool_output_keeps_two_thousand_head_and_tail_lines() {
    let text = (0..4_005)
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let truncated = truncate_output_lines(&text);
    let lines: Vec<&str> = truncated.lines().collect();
    assert_eq!(lines.len(), 4_001);
    assert_eq!(lines[0], "line-0");
    assert_eq!(lines[1_999], "line-1999");
    assert_eq!(lines[2_000], OUTPUT_TRUNCATION_MARKER);
    assert_eq!(lines[2_001], "line-2005");
    assert_eq!(lines[4_000], "line-4004");
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_during_a_read_waits_for_it_then_skips_the_next_call() {
    let dir = tempdir().unwrap();
    let slow_content = "line\n".repeat(400_000);
    fs::write(dir.path().join("slow.txt"), slow_content).unwrap();
    fs::write(dir.path().join("second.txt"), "must not run\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "slow".into(),
            name: "read".into(),
            input_json: r#"{"path":"slow.txt"}"#.into(),
        },
        ProviderEvent::ToolUse {
            id: "second".into(),
            name: "read".into(),
            input_json: r#"{"path":"second.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let cancel = CancellationToken::new();
    let (_gate_scope, started, release) = register_readonly_execution_gate();
    let trigger = cancel.clone();
    // The worker has passed its cancellation check but cannot complete the real
    // read until released. No timer determines when cancellation is requested.
    let interrupt = async move {
        let observed = tokio::time::timeout(Duration::from_secs(5), started).await;
        trigger.cancel();
        // Release even if observing the start failed, before asserting below.
        let released = release.send(());
        (observed, released)
    };
    let (outcome, (observed, released)) = tokio::join!(
        run_agent(&provider, &tools, request(Vec::new()), cancel),
        interrupt,
    );
    observed
        .expect("read start within bound")
        .expect("read started");
    released.expect("read worker waiting for release");
    let outcome = outcome.unwrap();
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert!(outcome.interrupted);
    let approved = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallApproved { call_id, .. } if call_id == "slow"))
            .unwrap();
    let running = outcome
        .events
        .iter()
        .position(
            |event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "slow"),
        )
        .unwrap();
    let cancelled = outcome
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                RuntimeEvent::ToolCallFinished(result)
                    if result.call_id == "slow"
                        && result.status == RuntimeToolStatus::Cancelled
                        && !result.output.is_empty()
            )
        })
        .unwrap();
    let interrupted = outcome
        .events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::Interrupted))
        .unwrap();
    let RuntimeEvent::ToolCallFinished(result) = &outcome.events[cancelled] else {
        unreachable!("cancelled index identifies a finished read");
    };
    assert!(result.output.contains("1 | line"));
    assert!(!result.output.contains(CANCELLED_BEFORE_EXECUTION_OUTPUT));
    assert!(approved < running && running < cancelled && cancelled < interrupted);
    assert!(!outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallRunning { call_id } if call_id == "second"
    )));
}

#[tokio::test]
async fn repeated_call_id_counts_every_observation_and_executes_once() {
    // A repeated provider call id is observed on every proposal but executed
    // only once; there is no per-tool-call count gate anymore (Issue #114).
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let observations = 150usize;
    let calls: Vec<ProviderEvent> = (0..observations)
        .map(|_| ProviderEvent::ToolUse {
            id: "same-call".into(),
            name: "read".into(),
            input_json: r#"{"path":"a.txt"}"#.into(),
        })
        .chain(std::iter::once(ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        }))
        .collect();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(calls)],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);

    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.tool_call_count, observations);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(_)))
            .count(),
        observations
    );
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
    ));
}

#[tokio::test]
async fn repeated_call_id_across_rounds_cannot_loop_forever() {
    // A provider that always asks for another round is bounded by the
    // configured turn limit, not by a tool-call count.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "same-call".into(),
            name: "read".into(),
            input_json: r#"{"path":"a.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let mut req = request(Vec::new());
    req.tool_config = RuntimeToolConfig::default().with_turn_limit(3);

    let outcome = tokio::time::timeout(
        Duration::from_secs(2),
        run_agent(&provider, &tools, req, CancellationToken::new()),
    )
    .await
    .expect("configured turn limit must converge")
    .unwrap();

    assert_eq!(outcome.tool_call_count, 3);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(provider.requests().len(), 3);
    assert!(matches!(
        outcome.events.last(),
        Some(RuntimeEvent::Finished(RuntimeFinishReason::TurnLimit))
    ));
}

#[tokio::test]
async fn issue302_large_bash_output_is_bounded_for_fresh_and_reused_model_results() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let output = format!("{{\"value\":\"{}\"}}\n", "x".repeat(37_989)).repeat(69);
    assert!(output.len() > 2_600_000);
    assert_eq!(output.lines().count(), 69);
    let executed_output = output.clone();
    let executions = Arc::new(AtomicUsize::new(0));
    let recorded = executions.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            recorded.fetch_add(1, Ordering::SeqCst);
            let text = executed_output.clone();
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text,
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let call = || {
        ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "large-bash".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf fixture"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 131_289,
                output: 80,
                cache_read: Some(0),
                cache_write: Some(0),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])
    };
    let provider = MockProvider::new_rounds(vec![
        vec![call()],
        vec![call()],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "inspect fixture")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        data.path().join("checkpoints"),
    );
    req.context_budget = Some(ContextBudget::new(428_000, 128_000, true).unwrap());
    req.context_source_version = Some(1);
    let compactions = Arc::new(AtomicUsize::new(0));
    req.context_compaction_hook = Some(Arc::new(TailPreservingCompactionHook {
        calls: compactions.clone(),
        observed: Arc::new(Mutex::new(Vec::new())),
    }));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .expect("large Shell display output must fit as a bounded model projection");
    assert!(!outcome.failed);
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(compactions.load(Ordering::SeqCst), 0);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let projected = requests[1]
        .messages
        .iter()
        .find(|message| message.role == ChatRole::Tool)
        .unwrap();
    assert!(projected.content.len() <= 65_536);
    assert!(projected.content.contains("omitted"));
    assert!(projected.content.starts_with(&output[..100]));
    assert!(projected.content.ends_with(&output[output.len() - 100..]));
    let reused = requests[2]
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .collect::<Vec<_>>();
    assert_eq!(reused.len(), 2);
    assert_eq!(reused[0], projected);
    assert_eq!(reused[1], projected);
    let finished = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished.len(), 2);
    assert!(
        finished
            .iter()
            .all(|result| result.output == output && result.exit_code == Some(0))
    );
    assert!(!finished[0].reused);
    assert!(finished[1].reused);
    assert!(
        outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallOutput { chunk, .. } => Some(chunk),
                _ => None,
            })
            .all(|chunk| chunk == &output)
    );
}

#[tokio::test]
async fn issue302_reused_validation_result_redacts_before_shell_projection() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let baseline = "x".repeat(100_000);
    let boundary = ChatMessage::projected_tool_result("call", "bash", &baseline)
        .content
        .find("\n[Shell output shortened:")
        .unwrap();
    let secret = "fixture-owner-private-credential-abcdefghijklmnopqrstuvwxyz";
    let mut output = baseline;
    output.replace_range(boundary - 16..boundary - 16 + secret.len(), secret);
    let redacted = crate::redact_sensitive_credential_text(&output, &[secret.to_string()]);
    let expected = ChatMessage::projected_tool_result("prior-invalid-bash", "bash", &redacted);
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "prior-invalid-bash".into(),
                name: "bash".into(),
                input_json: "{}".into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(vec![]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        project.path().join("checkpoints"),
    )
    .with_credential_reader(Arc::new(move || Ok(vec![secret.to_string()])));
    req.completed_tool_results.insert(
        "prior-invalid-bash".into(),
        CompletedToolCall {
            tool: "bash".into(),
            input_json: InvalidBashAudit::from_raw("{}").unwrap().to_json().unwrap(),
            result: RuntimeToolResult {
                call_id: "prior-invalid-bash".into(),
                output,
                status: RuntimeToolStatus::Rejected,
                reused: false,
                exit_code: None,
                duration_ms: Some(0),
                truncated: Some(false),
                approval: None,
                remember_rule: None,
            },
        },
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.executed_tool_call_count, 0);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let feedback = requests[1]
        .messages
        .iter()
        .find(|message| {
            serde_json::from_str::<serde_json::Value>(&message.content)
                .ok()
                .is_some_and(|value| {
                    value["vega_history_observation"]["kind"] == "tool_result"
                        && value["vega_history_observation"]["call_id"] == "prior-invalid-bash"
                })
        })
        .unwrap();
    let observation: serde_json::Value = serde_json::from_str(&feedback.content).unwrap();
    assert_eq!(
        observation["vega_history_observation"]["status"],
        serde_json::Value::Null
    );
    assert_eq!(
        observation["vega_history_observation"]["output"],
        expected.content
    );
    assert!(feedback.tool_call_id.is_none());
    assert!(!expected.content.contains(&secret[..16]));
    assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallValidationRejected { result, .. } if result.output == redacted && result.reused && result.status == RuntimeToolStatus::Rejected)));
}
