use super::*;

const CANARY: &str = "canary-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";
const HEAD_CANARY: &str = "canary-head-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";
const TAIL_CANARY: &str = "canary-tail-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";
const OMITTED_CANARY: &str = "canary-omitted-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";

#[tokio::test]
async fn issue170_bounded_bash_output_is_redacted_before_events_storage_and_followup_request() {
    let (store, dir, _project_id) = setup();
    let mut raw_output =
        format!("stdout head-safe-before {HEAD_CANARY} head-safe-after\n").into_bytes();
    let filler_line = vec![b'x'; 4_096];
    for line_index in 0..2_198 {
        if line_index == 1_099 {
            raw_output
                .extend_from_slice(format!("stdout omitted-middle {OMITTED_CANARY}").as_bytes());
        } else {
            raw_output.extend_from_slice(&filler_line);
        }
        raw_output.push(b'\n');
    }
    raw_output.extend_from_slice(
        format!("stderr tail-safe-before {TAIL_CANARY} tail-safe-after\n").as_bytes(),
    );
    let visible_output_budget = 2 * vega_tools::BASH_MAX_BYTES_PER_SIDE;
    assert!(raw_output.len() > visible_output_budget);
    let collected = vega_tools::collect_bash_output_for_test(&raw_output);
    assert!(collected.truncated);
    assert!(collected.text.len() <= visible_output_budget);
    assert!(collected.text.contains(HEAD_CANARY));
    assert!(collected.text.contains(TAIL_CANARY));
    assert!(!collected.text.contains(OMITTED_CANARY));
    assert!(
        collected
            .text
            .contains(vega_tools::BASH_OUTPUT_MIDDLE_MARKER)
    );
    let (head, tail) = collected
        .text
        .split_once(vega_tools::BASH_OUTPUT_MIDDLE_MARKER)
        .expect("bounded collector marks the omitted middle");
    let near_side_byte_limit = vega_tools::BASH_MAX_BYTES_PER_SIDE - 16 * 1_024;
    assert!(head.len() >= near_side_byte_limit);
    assert!(tail.len() >= near_side_byte_limit);
    assert!(head.lines().count() < vega_tools::BASH_MAX_LINES_PER_SIDE);
    assert!(tail.lines().count() < vega_tools::BASH_MAX_LINES_PER_SIDE);
    assert!(collected.text.contains("head-safe-before"));
    assert!(collected.text.contains("head-safe-after"));
    assert!(collected.text.contains("tail-safe-before"));
    assert!(collected.text.contains("tail-safe-after"));
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            let collected = collected.clone();
            Box::pin(async move { Ok(collected) })
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
    let reader: vega_runtime::CredentialReader =
        Arc::new(|| Ok(vec![HEAD_CANARY.to_string(), TAIL_CANARY.to_string()]));
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
                && chunk.0.matches(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER).count() == 2
                && !chunk.0.contains(HEAD_CANARY)
                && !chunk.0.contains(TAIL_CANARY)
                && !chunk.0.contains(OMITTED_CANARY)
                && chunk.0.contains("head-safe-before")
                && chunk.0.contains("head-safe-after")
                && chunk.0.contains("tail-safe-before")
                && chunk.0.contains("tail-safe-after")
                && chunk.0.contains(vega_tools::BASH_OUTPUT_MIDDLE_MARKER)
    )));
    assert!(live_events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.output.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
                && result.output.matches(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER).count() == 2
                && !result.output.contains(HEAD_CANARY)
                && !result.output.contains(TAIL_CANARY)
                && !result.output.contains(OMITTED_CANARY)
                && result.output.contains("head-safe-before")
                && result.output.contains("head-safe-after")
                && result.output.contains("tail-safe-before")
                && result.output.contains("tail-safe-after")
                && result.output.contains(vega_tools::BASH_OUTPUT_MIDDLE_MARKER)
    )));
    let (stored, output_full_path): (String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT output_text, output_full_path FROM tool_calls WHERE id = 'credential-output-call'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(
        output_full_path.is_none(),
        "S5 output must not spill to a file"
    );
    assert!(stored.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    assert_eq!(
        stored
            .matches(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
            .count(),
        2
    );
    assert!(!stored.contains(HEAD_CANARY));
    assert!(!stored.contains(TAIL_CANARY));
    assert!(!stored.contains(OMITTED_CANARY));
    assert!(stored.contains("head-safe-before"));
    assert!(stored.contains("head-safe-after"));
    assert!(stored.contains("tail-safe-before"));
    assert!(stored.contains("tail-safe-after"));
    assert!(stored.contains(vega_tools::BASH_OUTPUT_MIDDLE_MARKER));
    assert!(provider.requests().iter().all(|request| {
        request.messages.iter().all(|message| {
            !message.content.contains(HEAD_CANARY)
                && !message.content.contains(TAIL_CANARY)
                && !message.content.contains(OMITTED_CANARY)
        })
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
