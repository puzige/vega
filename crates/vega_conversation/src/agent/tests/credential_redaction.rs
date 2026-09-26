use super::*;

const CANARY: &str = "canary-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";

#[tokio::test]
async fn issue170_tool_output_is_redacted_before_events_storage_and_followup_request() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(|_, _, _| {
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: format!(
                        "stdout: {CANARY}\nstderr: {CANARY}\nstructured: {{\"credential\":\"{CANARY}\"}}"
                    ),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "credential-output-call".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf synthetic-canary","timeout_ms":null}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Completed safely.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let reader: vega_runtime::CredentialReader = Arc::new(|| Ok(vec![CANARY.to_string()]));
    let mut live_events = Vec::new();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Run synthetic output test",
        "System",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Once,
        },
        |event| {
            live_events.push(event.clone());
            Ok(())
        },
        PersistenceActorConfig::default().with_owner_credential_reader(reader),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();

    assert!(!run.failed);
    assert!(live_events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallOutput { chunk, .. }
            if chunk.0.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
                && !chunk.0.contains(CANARY)
    )));
    assert!(live_events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.output.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
                && !result.output.contains(CANARY)
    )));
    let stored: String = store
        .conn()
        .query_row(
            "SELECT output_text FROM tool_calls WHERE id = 'credential-output-call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    assert!(!stored.contains(CANARY));
    assert!(provider.requests().iter().all(|request| {
        request
            .messages
            .iter()
            .all(|message| !message.content.contains(CANARY))
    }));
    assert!(provider.requests()[1].messages.iter().any(|message| {
        message
            .content
            .contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
    }));
}

#[tokio::test]
async fn issue170_legacy_credential_history_is_scrubbed_and_blocked_before_provider() {
    let (store, dir, _project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "legacy-user".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "older question".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let assistant_text = format!("old assistant saw {CANARY}");
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "legacy-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 2,
            role: "assistant".into(),
            kind: "text".into(),
            content: assistant_text.clone(),
            status: "done".into(),
            created_at: 2,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "legacy-tool-call",
            thread_id: "thread-1",
            message_id: "legacy-assistant",
            seq: 1,
            tool: "read",
            input_json: r#"{"path":"legacy.txt"}"#,
            status: "success",
            created_at: 2,
        },
    )
    .unwrap();
    tool_calls::update(
        store.conn(),
        "legacy-tool-call",
        "success",
        Some("once"),
        Some(CANARY),
        Some(3),
    )
    .unwrap();
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: source.source_version,
        covered_through_seq: 1,
        source_fingerprint: source.fingerprint,
        summary: format!("old summary repeated {CANARY}"),
        estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: None,
        created_at: 3,
    };
    assert!(matches!(
        vega_store::context_compaction::install_checkpoint(store.conn(), &checkpoint).unwrap(),
        vega_store::context_compaction::ContextCheckpointInstall::Applied(_)
    ));

    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let reader: vega_runtime::CredentialReader = Arc::new(|| Ok(vec![CANARY.to_string()]));
    let run = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Continue safely",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        PersistenceActorConfig::default().with_owner_credential_reader(reader),
    )
    .await
    .unwrap();

    assert!(run.failed);
    assert!(provider.requests().is_empty());
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::Error { error, .. }
            if matches!(error.as_ref(), VegaError::CredentialExposureBlocked)
    )));
    let assistant: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE id = 'legacy-assistant'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let tool_output: String = store
        .conn()
        .query_row(
            "SELECT output_text FROM tool_calls WHERE id = 'legacy-tool-call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let summary: String = store
        .conn()
        .query_row(
            "SELECT summary FROM context_checkpoints WHERE thread_id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for cleaned in [assistant, tool_output, summary] {
        assert!(!cleaned.contains(CANARY));
        assert!(cleaned.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }
}
