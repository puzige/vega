use super::*;

#[test]
fn run_modes_advertise_exact_three_or_six_strict_tools() {
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let tools = tool_definitions(mode);
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["read", "glob", "grep"]
        );
        assert!(
            tools
                .iter()
                .all(|tool| tool.input_schema["additionalProperties"] == false)
        );
    }
    let tools = tool_definitions(RuntimeRunMode::Execute);
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["read", "glob", "grep", "write", "edit", "bash"]
    );
    assert!(
        tools
            .iter()
            .all(|tool| tool.input_schema["additionalProperties"] == false)
    );
}

#[tokio::test]
async fn permission_wait_is_first_wins_fail_closed_and_cancels_child_token() {
    let target = RuntimePermissionTarget {
        call_id: "call".to_string(),
        tool: RuntimeMutatingTool::Write,
        exact_pattern: "file.txt".to_string(),
        display_target: "file.txt".to_string(),
    };
    let prompt = RuntimePermissionPrompt {
        target,
        danger: None,
    };

    let captured = Arc::new(Mutex::new(None));
    let (decision, cancelled) = wait_for_permission(
        &ProbeHook {
            fail: false,
            token: captured.clone(),
        },
        prompt.clone(),
        Duration::from_secs(60),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(decision, RuntimeUserDecision::Once);
    assert!(!cancelled);
    assert!(
        captured
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    );

    let captured = Arc::new(Mutex::new(None));
    let (decision, cancelled) = wait_for_permission(
        &ProbeHook {
            fail: true,
            token: captured.clone(),
        },
        prompt.clone(),
        Duration::from_secs(60),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(decision, RuntimeUserDecision::Timeout);
    assert!(!cancelled);
    assert!(
        captured
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    );

    let cancel = CancellationToken::new();
    cancel.cancel();
    let (decision, cancelled) = wait_for_permission(
        &ProbeHook {
            fail: false,
            token: Arc::new(Mutex::new(None)),
        },
        prompt,
        Duration::from_secs(60),
        &cancel,
    )
    .await;
    assert_eq!(decision, RuntimeUserDecision::Timeout);
    assert!(cancelled);
}

#[tokio::test]
async fn ask_valid_mutations_are_safe_run_mode_rejections_without_hook_or_execution() {
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        fs::write(project.path().join("note.txt"), "old").unwrap();
        let checkpoint = data.path().join("must-not-be-created");
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "edit-1".into(),
                    name: "edit".into(),
                    input_json:
                        r#"{"path":"note.txt","old_string":"old","new_string":"SECRET_NEW"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "bash-1".into(),
                    name: "bash".into(),
                    input_json: r#"{"cmd":"echo allowed-audit"}"#.into(),
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
        req.tool_config = tool_config(mode, RuntimePermissionMode::Confirm, checkpoint.clone());
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
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert_eq!(
            fs::read_to_string(project.path().join("note.txt")).unwrap(),
            "old"
        );
        assert!(!project.path().join("new.txt").exists());
        assert!(!checkpoint.exists());
        let rendered = format!("{:?}", outcome.events);
        assert!(!rendered.contains("SECRET_BODY"));
        assert!(!rendered.contains("SECRET_NEW"));
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(
                    event,
                    RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                        status: RuntimeToolStatus::Rejected,
                        approval: Some(RuntimeApprovalAudit {
                            source: RuntimeApprovalSource::RunMode,
                            ..
                        }),
                        ..
                    })
                ))
                .count(),
            3
        );
    }
}

#[tokio::test]
async fn invalid_write_is_atomic_validation_rejection_before_any_proposal() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bad".into(),
                name: "write".into(),
                input_json: r#"{"path":"x","content":"SECRET","extra":true}"#.into(),
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
    req.tool_config = tool_config(
        RuntimeRunMode::Ask,
        RuntimePermissionMode::Confirm,
        checkpoint,
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(
        matches!(outcome.events.first(), Some(RuntimeEvent::ToolCallValidationRejected { call, result }) if !call.input_json.contains("SECRET") && matches!(result.approval, Some(RuntimeApprovalAudit { source: RuntimeApprovalSource::Validation, .. })))
    );
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ToolCallProposed(_)))
    );
}

#[tokio::test]
async fn ask_valid_body_with_invalid_call_id_is_validation_not_run_mode() {
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("must-not-be-created");
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: String::new(),
                    name: "write".into(),
                    input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
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
        req.tool_config = tool_config(mode, RuntimePermissionMode::Confirm, checkpoint.clone());
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
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!checkpoint.exists());
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallValidationRejected { call, result }
                if !call.input_json.contains("SECRET_BODY")
                    && result.output.contains("checkpoint_id_invalid")
                    && matches!(result.approval, Some(RuntimeApprovalAudit {
                        source: RuntimeApprovalSource::Validation,
                        ..
                    }))
        )));
    }
}

#[tokio::test]
async fn execute_write_waits_for_once_and_provider_observes_only_safe_projection() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    let checkpoint_display = checkpoint.display().to_string();
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
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
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(project.path().join("new.txt")).unwrap(),
        "SECRET_BODY"
    );
    assert!(matches!(
        outcome
            .events
            .iter()
            .find(|event| matches!(event, RuntimeEvent::ToolCallFinished(_))),
        Some(RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Success,
            ..
        }))
    ));
    let requests = provider.requests();
    let wire = serde_json::to_string(&crate::openai::build_request_body(&requests[1])).unwrap();
    assert!(!wire.contains("SECRET_BODY"));
    assert!(!wire.contains(&checkpoint_display));
    assert!(!wire.contains("project-1"));
    assert!(!wire.contains("thread-1"));
    assert!(wire.contains("fingerprint_v1"));
    assert!(wire.contains("checkpoint_ref"));
}

#[tokio::test]
async fn permission_modes_rules_and_danger_ordering_reach_the_dispatcher() {
    let cases = [
        (
            RuntimePermissionMode::Confirm,
            false,
            RuntimeUserDecision::Deny { note: None },
            "printf denied",
            1,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::User,
        ),
        (
            RuntimePermissionMode::Auto,
            false,
            RuntimeUserDecision::Deny { note: None },
            "printf auto",
            0,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::Auto,
        ),
        (
            RuntimePermissionMode::ReadOnly,
            false,
            RuntimeUserDecision::Once,
            "printf readonly",
            0,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::ReadOnly,
        ),
        (
            RuntimePermissionMode::Confirm,
            true,
            RuntimeUserDecision::Deny { note: None },
            "printf ruled",
            0,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::Rule,
        ),
        (
            RuntimePermissionMode::Auto,
            false,
            RuntimeUserDecision::Deny { note: None },
            "rm -rf /",
            1,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::Danger,
        ),
        (
            RuntimePermissionMode::Auto,
            true,
            RuntimeUserDecision::Deny { note: None },
            "rm -rf /",
            1,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::Danger,
        ),
    ];
    for (mode, rule, decision, command, hook_calls, status, source) in cases {
        let (outcome, actual_calls) = run_bash_permission_case(mode, rule, decision, command).await;
        assert_eq!(actual_calls, hook_calls, "{command}");
        assert!(
            outcome.events.iter().any(|event| matches!(
                event,
                RuntimeEvent::ToolCallFinished(result)
                    if result.status == status
                        && result.approval.as_ref().is_some_and(|audit| audit.source == source)
            )),
            "{command}"
        );
    }

    let (outcome, calls) = run_bash_permission_case(
        RuntimePermissionMode::ReadOnly,
        false,
        RuntimeUserDecision::Always,
        "rm -rf /",
    )
    .await;
    assert_eq!(calls, 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Rejected,
            approval: Some(RuntimeApprovalAudit {
                source: RuntimeApprovalSource::ReadOnly,
                danger: Some(crate::RuntimeDangerAudit {
                    decision: RuntimeApprovalDecision::Always,
                    ..
                }),
                ..
            }),
            remember_rule: Some(_),
            ..
        })
    )));

    let (outcome, calls) = run_bash_permission_case(
        RuntimePermissionMode::Auto,
        false,
        RuntimeUserDecision::Once,
        "git push --force",
    )
    .await;
    assert_eq!(calls, 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Success,
            exit_code: Some(code),
            duration_ms: Some(_),
            approval: Some(RuntimeApprovalAudit {
                source: RuntimeApprovalSource::Danger,
                ..
            }),
            ..
        }) if *code != 0
    )));
}

#[tokio::test]
async fn dangerous_cancel_after_proposal_keeps_nested_timeout_audit() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "danger-cancel".into(),
            name: "bash".into(),
            input_json: r#"{"cmd":"rm -rf /"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        checkpoint,
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
            if matches!(event, RuntimeEvent::ToolCallProposed(_)) {
                sink_cancel.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Rejected,
            approval: Some(RuntimeApprovalAudit {
                source: RuntimeApprovalSource::Timeout,
                danger: Some(crate::RuntimeDangerAudit {
                    decision: RuntimeApprovalDecision::Deny,
                    ..
                }),
                ..
            }),
            ..
        })
    )));
}

#[tokio::test]
async fn running_bash_cancellation_waits_for_process_reap() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "bash-cancel".into(),
            name: "bash".into(),
            input_json: r#"{"cmd":"sleep 30 & wait"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        checkpoint,
    );
    let cancel = CancellationToken::new();
    let sink_cancel = cancel.clone();
    let started = Instant::now();
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
            if matches!(event, RuntimeEvent::ToolCallRunning { .. }) {
                let delayed = sink_cancel.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    delayed.cancel();
                });
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Cancelled,
            output,
            ..
        }) if output == "Tool error: bash failed (cancelled)"
    )));
}

#[tokio::test]
async fn always_rule_bypasses_the_second_write_in_the_same_turn() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-first".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"first"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "write-second".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"second"}"#.into(),
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
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        checkpoint,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedHook {
        calls: calls.clone(),
        decision: Some(RuntimeUserDecision::Always),
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
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.executed_tool_call_count, 2);
    assert_eq!(
        fs::read_to_string(project.path().join("same.txt")).unwrap(),
        "second"
    );
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallApproved {
            call_id,
            audit: RuntimeApprovalAudit {
                source: RuntimeApprovalSource::Rule,
                ..
            },
            remember_rule: None,
        } if call_id == "write-second"
    )));
}

#[tokio::test]
async fn permission_timeout_rejects_without_mutation() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"new.txt","content":"body"}"#.into(),
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
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        checkpoint,
    )
    .with_permission_timeout(Duration::from_millis(5));
    let hook = FixedHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: None,
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
    assert!(!project.path().join("new.txt").exists());
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Rejected,
            approval: Some(RuntimeApprovalAudit {
                source: RuntimeApprovalSource::Timeout,
                ..
            }),
            ..
        })
    )));
}
