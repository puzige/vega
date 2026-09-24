use super::*;

#[tokio::test]
async fn retry_reuses_persisted_call_id_without_running_the_tool() {
    let (store, dir, _project_id) = setup();
    let database_path = dir.path().join("vega.db");
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let first = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
    run_thread_task(
        &store,
        &first,
        &tools,
        "thread-1",
        "First",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    drop(store);
    let store = Store::open(&database_path).unwrap();
    store.migrate().unwrap();
    fs::remove_file(dir.path().join("lib.rs")).unwrap();

    let retry = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
    let run = run_thread_task(
        &store,
        &retry,
        &tools,
        "thread-1",
        "Retry",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.reused
                && result.status == ToolCallStatus::Success
                && result.output.contains("persist me")
    )));
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE id = 'stable-call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn retry_reuses_failed_and_rejected_terminal_results_verbatim() {
    for (call_id, tool, input, status, approval, output, expected) in [
        (
            "failed-call",
            "read",
            r#"{"path":"missing.txt"}"#,
            "failed",
            "once",
            "original failed output",
            ToolCallStatus::Failed,
        ),
        (
            "rejected-call",
            "unknown",
            "{}",
            "rejected",
            r#"{"decision":"deny","note":null,"source":"run_mode","danger":null}"#,
            "Tool error: denied: unavailable tool",
            ToolCallStatus::Rejected,
        ),
        (
            "cancelled-call",
            "read",
            r#"{"path":"missing.txt"}"#,
            "cancelled",
            "once",
            "original cancelled output",
            ToolCallStatus::Cancelled,
        ),
    ] {
        let (store, dir, _project_id) = setup();
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: "prior-assistant".into(),
                thread_id: "thread-1".into(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: call_id,
                thread_id: "thread-1",
                message_id: "prior-assistant",
                seq: 1,
                tool,
                input_json: input,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .unwrap();
        tool_calls::update(
            store.conn(),
            call_id,
            status,
            Some(approval),
            Some(output),
            Some(2),
        )
        .unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: call_id.into(),
                    name: tool.into(),
                    input_json: input.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Retry",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.reused && result.status == expected && result.output == output
        )));
        let persisted: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, output_text FROM tool_calls WHERE id = ?1",
                [call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, (status.into(), output.into()));
    }
}

#[tokio::test]
async fn startup_recovery_survives_reopen_and_allows_a_new_turn() {
    let (store, dir, _project_id) = setup();
    let database_path = dir.path().join("vega.db");
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "stale-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: "partial".into(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    for (index, status) in [(1, "pending_approval"), (2, "approved"), (3, "running")] {
        let call_id = format!("stale-{status}");
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: &call_id,
                thread_id: "thread-1",
                message_id: "stale-assistant",
                seq: index,
                tool: "read",
                input_json: r#"{"path":"lib.rs"}"#,
                status,
                created_at: 1,
            },
        )
        .unwrap();
        if status != "pending_approval" {
            tool_calls::update(store.conn(), &call_id, status, Some("once"), None, None).unwrap();
        }
    }
    drop(store);
    let store = Store::open(&database_path).unwrap();
    store.migrate().unwrap();
    fs::remove_file(dir.path().join("lib.rs")).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "stale-pending_approval".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "stale-approved".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "stale-running".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("resumed".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();

    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Continue",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(run.content, "resumed");
    let stale_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id = 'stale-assistant'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stale_status, "interrupted");
    assert_eq!(
        messages::find(store.conn(), "stale-assistant")
            .unwrap()
            .unwrap()
            .content,
        "partial"
    );
    for (call_id, status, approval, expected_status) in [
        (
            "stale-pending_approval",
            "rejected",
            vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON,
            ToolCallStatus::Rejected,
        ),
        (
            "stale-approved",
            "cancelled",
            "once",
            ToolCallStatus::Cancelled,
        ),
        (
            "stale-running",
            "cancelled",
            "once",
            ToolCallStatus::Cancelled,
        ),
    ] {
        let persisted: (String, String, String, i64) = store
            .conn()
            .query_row(
                "SELECT status, approval, output_text, finished_at FROM tool_calls WHERE id = ?1",
                [call_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(persisted.0, status);
        assert_eq!(persisted.1, approval);
        assert!(persisted.2.contains("startup recovery"));
        assert!(persisted.3 > 0);
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { call_id: event_id, result }
                if event_id == call_id
                    && result.reused
                    && result.status == expected_status
                    && result.output == persisted.2
        )));
    }
    assert_eq!(provider.requests().len(), 2);
}

#[tokio::test]
async fn cross_thread_call_id_collision_fails_before_execution() {
    let (store, dir, project_id) = setup();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-2",
            project_id: &project_id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: "other-message".into(),
            thread_id: "thread-2".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "shared-call",
            thread_id: "thread-2",
            message_id: "other-message",
            seq: 1,
            tool: "read",
            input_json: r#"{"path":"lib.rs"}"#,
            status: "pending_approval",
            created_at: 1,
        },
    )
    .unwrap();
    tool_calls::update(
        store.conn(),
        "shared-call",
        "success",
        Some("once"),
        Some("other output"),
        Some(2),
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "shared-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();

    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Failed
                && !result.reused
                && result.output == vega_runtime::CALL_ID_CONFLICT_OUTPUT
    )));
    let persisted: (String, String, String) = store
        .conn()
        .query_row(
            "SELECT thread_id, status, output_text FROM tool_calls WHERE id = 'shared-call'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        persisted,
        ("thread-2".into(), "success".into(), "other output".into())
    );
    let done_assistants: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant' AND status = 'done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(done_assistants, 1);
}

#[tokio::test]
async fn same_thread_call_id_with_changed_input_cannot_overwrite_audit_row() {
    let (store, dir, _project_id) = setup();
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: "prior-message".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "changed-call",
            thread_id: "thread-1",
            message_id: "prior-message",
            seq: 1,
            tool: "read",
            input_json: r#"{"path":"original.txt"}"#,
            status: "pending_approval",
            created_at: 1,
        },
    )
    .unwrap();
    tool_calls::update(
        store.conn(),
        "changed-call",
        "failed",
        Some("once"),
        Some("original audit output"),
        Some(2),
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "changed-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();

    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read changed input",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Failed
                && result.output == vega_runtime::CALL_ID_CONFLICT_OUTPUT
    )));
    let persisted: (String, String, String) = store
        .conn()
        .query_row(
            "SELECT input_json, status, output_text FROM tool_calls WHERE id = 'changed-call'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        persisted,
        (
            r#"{"path":"original.txt"}"#.into(),
            "failed".into(),
            "original audit output".into()
        )
    );
}

#[tokio::test]
async fn invalid_write_is_atomically_audited_without_execution() {
    let (store, dir, _project_id) = setup();
    let data = tempdir().unwrap();
    let checkpoint_root = data.path().join("checkpoints");
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
    let run = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Write",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        PersistenceActorConfig {
            checkpoint_root: Some(checkpoint_root),
            ..PersistenceActorConfig::default()
        },
    )
    .await
    .unwrap();
    assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Rejected && result.output.contains("invalid write input")
        )));
    let status: (String, String) = store
        .conn()
        .query_row(
            "SELECT status, approval FROM tool_calls WHERE id = 'write-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let approval = ApprovalAudit::from_json(&status.1).unwrap();
    assert_eq!(status.0, "rejected");
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Validation);
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Failed
                || matches!(result.status, ToolCallStatus::Approved | ToolCallStatus::Running)
    )));
}

#[tokio::test]
async fn issue90_invalid_bash_round_trips_through_real_store_and_history() {
    let (store, project, data, _project_id) = setup_external("confirm");
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bad-bash".into(),
                name: "bash".into(),
                input_json: r#"{"command":"printf SECRET_BASH > issue90-marker"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let permission_calls = Arc::new(AtomicUsize::new(0));
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Check",
        "System",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: permission_calls.clone(),
            decision: PermissionDecision::Once,
        },
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(permission_calls.load(Ordering::SeqCst), 0);
    assert!(!project.path().join("issue90-marker").exists());
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallProposed { call } if call.id == "bad-bash"
    )));
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { call_id, result }
            if call_id == "bad-bash"
                && result.status == ToolCallStatus::Rejected
                && result.output == vega_runtime::BASH_INVALID_INPUT_OUTPUT
                && result.invalid.is_some()
    )));
    let row: (String, String, String, String, Option<i32>, Option<i64>) = store
        .conn()
        .query_row(
            "SELECT input_json, status, approval, output_text, exit_code, duration_ms FROM tool_calls WHERE id = 'bad-bash'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .unwrap();
    assert!(vega_runtime::InvalidBashAudit::from_json(&row.0).is_some());
    assert!(!row.0.contains("SECRET_BASH"));
    assert_eq!(row.1, "rejected");
    assert_eq!(row.3, vega_runtime::BASH_INVALID_INPUT_OUTPUT);
    assert!(row.4.is_none() && row.5.is_none());
    let approval = ApprovalAudit::from_json(&row.2).unwrap();
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Validation);
    let second = &provider.requests()[1];
    let feedback = second
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("bad-bash"))
        .unwrap();
    assert_eq!(feedback.content, vega_runtime::BASH_INVALID_INPUT_OUTPUT);
    assert!(
        !second
            .messages
            .iter()
            .any(|message| message.content.contains("SECRET_BASH"))
    );

    drop(store);
    let reopened = Store::open(data.path().join("vega.db")).unwrap();
    let history = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    assert!(history.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::Tool {
            call_id,
            status: ToolCallStatus::Rejected,
            input: None,
            result: Some(crate::types::ToolCardResultProjection::InvalidRejected {
                tool: crate::types::InvalidToolKind::Bash,
                code: crate::types::InvalidToolCode::InvalidInput,
                ..
            }),
            ..
        } if call_id == "bad-bash"
    )));
}

#[tokio::test]
#[ignore = "load-sensitive: asserts a wall-clock budget (<1000ms), fails under parallel test load; run with --ignored"]
async fn cancellation_is_persisted_as_interrupted_under_one_second() {
    let (store, dir, _project_id) = setup();
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
    let run = run_thread_task(
        &store, &provider, &tools, "thread-1", "Wait", "System", cancel,
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(run.interrupted);
    assert!(matches!(
        run.events.last(),
        Some(ConversationEvent::Interrupted { .. })
    ));
    let status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id = ?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "interrupted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_tool_persists_output_as_cancelled_and_starts_nothing_else() {
    let (store, dir, _project_id) = setup();
    store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode = 'full_access' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel::<()>();
    let connected_tx = Arc::new(Mutex::new(Some(connected_tx)));
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, full_access, cancel| {
            assert_eq!(command, "cat slow.txt");
            assert!(full_access);
            let started = connected_tx.lock().unwrap().take().unwrap();
            Box::pin(async move {
                started.send(()).unwrap();
                cancel.cancelled().await;
                Err(vega_tools::BashError::for_test(
                    vega_tools::BashErrorCode::Cancelled,
                ))
            })
        }));
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "slow-call".into(),
            name: "bash".into(),
            input_json: r#"{"cmd":"cat slow.txt"}"#.into(),
        },
        ProviderEvent::ToolUse {
            id: "must-not-start".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let mut connected_slot = Some(connected_rx);

    let run = run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read slowly",
        "System",
        cancel,
        |event| {
            if matches!(event, ConversationEvent::ToolCallApproved { .. })
                && let Some(connected) = connected_slot.take()
            {
                let trigger = trigger.clone();
                tokio::spawn(async move {
                    // Bounds the wait so a tool that never starts fails
                    // the test visibly instead of hanging silently.
                    tokio::time::timeout(Duration::from_secs(10), connected)
                        .await
                        .expect("mock tool starts")
                        .expect("mock start signal");
                    trigger.cancel();
                });
            }
            Ok(())
        },
    )
    .await
    .unwrap();

    assert!(run.interrupted);
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { call_id, result }
            if call_id == "slow-call"
                && result.status == ToolCallStatus::Cancelled
                && !result.output.is_empty()
    )));
    assert!(matches!(
        run.events.last(),
        Some(ConversationEvent::Interrupted { .. })
    ));
    let persisted: (String, String) = store
        .conn()
        .query_row(
            "SELECT status, output_text FROM tool_calls WHERE id = 'slow-call'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, "cancelled");
    assert!(!persisted.1.is_empty());
    let second_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE id = 'must-not-start'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(second_count, 0);
    let assistant_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id = ?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(assistant_status, "interrupted");
    assert_eq!(provider.requests().len(), 1);
}
