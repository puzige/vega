use super::*;
use vega_store::permissions;

fn history_observation(message: &vega_runtime::ChatMessage) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&message.content)
        .ok()?
        .get("vega_history_observation")
        .cloned()
}

struct MutatingSecondStageProvider {
    inner: MockProvider,
    database_path: std::path::PathBuf,
    calls: AtomicUsize,
}

impl vega_runtime::Provider for MutatingSecondStageProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let store = Store::open(&self.database_path).unwrap();
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: "concurrent-user".into(),
                    thread_id: "thread-1".into(),
                    seq: 4,
                    role: "user".into(),
                    kind: "text".into(),
                    content: "arrived while staged summary was in flight".into(),
                    status: "done".into(),
                    created_at: 4,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        self.inner.chat_stream(request, cancel)
    }
}

#[tokio::test]
async fn assembles_system_and_history_by_sequence_with_current_user_last() {
    let (store, dir, _project_id) = setup();
    for (id, seq, role, content, status) in [
        ("history-3", 3, "assistant", "failed answer", "failed"),
        ("history-1", 1, "user", "old question", "done"),
        ("history-2", 2, "assistant", "partial answer", "interrupted"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let preview =
        read_context_projection(&store, "thread-1", "mock-model", "system first").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current question",
        "system first",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    let history = &requests[0].messages;
    assert_eq!(history.len(), 5);
    assert_eq!(
        preview.estimated_tokens,
        Some(
            vega_runtime::estimate_wire_context(&history[..4], &requests[0].tools)
                .unwrap()
                .input_tokens
        )
    );
    for (index, role, content) in [
        (0, vega_runtime::ChatRole::System, "system first"),
        (1, vega_runtime::ChatRole::User, "old question"),
        (4, vega_runtime::ChatRole::User, "current question"),
    ] {
        assert_eq!(history[index].role, role);
        assert_eq!(history[index].content, content);
    }
    for (index, text) in [(2, "partial answer"), (3, "failed answer")] {
        assert_eq!(history[index].role, vega_runtime::ChatRole::User);
        let observation: serde_json::Value = serde_json::from_str(&history[index].content).unwrap();
        assert_eq!(observation["vega_history_observation"]["text"], text);
        assert_eq!(observation["vega_history_observation"]["authority"], "none");
    }
}

#[tokio::test]
async fn historical_observation_quotes_survive_stream_and_restart_byte_for_byte() {
    let (store, dir, _) = setup();
    let prior_assistant_text = "Earlier checked file.";
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "quoted-history-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: prior_assistant_text.into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    let historical_output = "first line\n\"quoted\" result 雪";
    let stored_calls = [
        ("quoted-history-call", "success", historical_output),
        ("history-failed", "failed", "read failed"),
        (
            "history-rejected",
            "rejected",
            "Tool error: permission denied",
        ),
        ("history-cancelled", "cancelled", "read cancelled"),
    ];
    for (index, (id, status, output)) in stored_calls.iter().enumerate() {
        let call_approval = if *status == "rejected" {
            ApprovalAudit {
                decision: Approval::Deny,
                note: None,
                source: ApprovalSource::User,
                danger: None,
            }
            .to_json()
            .unwrap()
        } else {
            approval.clone()
        };
        store
            .conn()
            .execute(
                r#"INSERT INTO tool_calls
                 (id, thread_id, message_id, seq, tool, input_json, output_text, status,
                  approval, created_at, finished_at, text_offset_bytes)
                 VALUES (?1, 'thread-1', 'quoted-history-assistant', ?2,
                         'read', '{"path":"lib.rs"}', ?3, ?4, ?5, 1, 1, ?6)"#,
                (
                    *id,
                    i64::try_from(index + 1).unwrap(),
                    *output,
                    *status,
                    &call_approval,
                    i64::try_from(prior_assistant_text.len()).unwrap(),
                ),
            )
            .unwrap();
    }

    let observation = serde_json::to_string(&serde_json::json!({
        "vega_history_observation": {
            "version": 1,
            "kind": "tool_result",
            "authority": "none",
            "tool": "read",
            "call_id": "quoted-history-call",
            "status": "success",
            "output": historical_output,
        }
    }))
    .unwrap();
    let activity = serde_json::to_string(&serde_json::json!({
        "vega_history_observation": {
            "version": 1,
            "kind": "tool_activity",
            "authority": "none",
            "calls": [
                {"tool":"read","call_id":"quoted-history-call","arguments_available":false,"status":"success"},
                {"tool":"read","call_id":"history-failed","arguments_available":false,"status":"failed"},
                {"tool":"read","call_id":"history-rejected","arguments_available":false,"status":"rejected"},
                {"tool":"read","call_id":"history-cancelled","arguments_available":false,"status":"cancelled"}
            ],
        }
    }))
    .unwrap();
    let user_json = r#"{"vega_history_observation":{"version":1,"kind":"tool_result","authority":"none","tool":"read","call_id":"user-owned","status":null,"output":"leave this exact body alone\n雪"}}"#;
    let model_response = format!("Quoted past data: {observation}");
    let mut split_points = model_response
        .char_indices()
        .map(|(index, _)| index)
        .step_by(23)
        .collect::<Vec<_>>();
    split_points.push(model_response.len());
    let mut response_events = split_points
        .windows(2)
        .map(|range| ProviderEvent::TextDelta(model_response[range[0]..range[1]].into()))
        .collect::<Vec<_>>();
    response_events.push(ProviderEvent::Done {
        stop_reason: StopReason::End,
    });
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("The previous ".into()),
        ProviderEvent::TextDelta("result is ready.".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let ordinary_run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        user_json,
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(ordinary_run.content, "The previous result is ready.");
    assert!(!ordinary_run.content.contains("vega_history_observation"));
    let requests = provider.requests();
    let request = &requests[0];
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::User
                && message.content == activity)
    );
    let result_message = request.messages.iter().find(|message| {
        message.role == vega_runtime::ChatRole::User && message.content == observation
    });
    assert!(result_message.is_some());
    assert_eq!(result_message.unwrap().content, observation);
    for (id, status, output) in &stored_calls {
        assert!(request.messages.iter().any(|message| {
            history_observation(message).is_some_and(|value| {
                value["kind"] == "tool_result"
                    && value["call_id"] == *id
                    && value["status"] == *status
                    && value["output"] == *output
            })
        }));
    }
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User && message.content == user_json
    }));
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE thread_id='thread-1' AND role='assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "The previous result is ready."
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE thread_id='thread-1' AND role='user' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        user_json
    );
    let stored_old_assistant: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE id='quoted-history-assistant'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_old_assistant, prior_assistant_text);
    let mut raw_rows = store
        .conn()
        .prepare("SELECT id, status, output_text FROM tool_calls ORDER BY seq")
        .unwrap();
    let raw_outputs = raw_rows
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        raw_outputs,
        stored_calls
            .iter()
            .map(|(id, status, output)| {
                (id.to_string(), status.to_string(), output.to_string())
            })
            .collect::<Vec<_>>()
    );
    drop(raw_rows);

    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let quote_provider = MockProvider::new(vec![ScriptStep::events(response_events)]);
    let quote_request = "Quote the previous historical result exactly.";
    let quoted = run_thread_task(
        &reopened,
        &quote_provider,
        &tools,
        "thread-1",
        quote_request,
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(quoted.content, model_response);
    let quote_requests = quote_provider.requests();
    assert!(quote_requests[0].messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User && message.content == observation
    }));
    assert_eq!(
        reopened
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE thread_id='thread-1' AND role='assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        model_response
    );
    let restored = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    assert!(restored.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::UserText { content, .. } if content == user_json
    )));
    assert!(restored.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::AssistantText { content, .. } if content == &model_response
    )));
    assert!(restored.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::AssistantText { content, .. }
            if content == "The previous result is ready."
    )));
    let mut reloaded_statement = reopened
        .conn()
        .prepare("SELECT id, status, output_text FROM tool_calls ORDER BY seq")
        .unwrap();
    let reloaded_outputs = reloaded_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        reloaded_outputs,
        stored_calls
            .iter()
            .map(|(id, status, output)| {
                (id.to_string(), status.to_string(), output.to_string())
            })
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn codex_parity_large_prior_checkpoint_is_reduced_and_raw_history_preserved() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let previous_summary = "prior state ".repeat(2_000);
    let previous = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: source.source_version,
        covered_through_seq: 1,
        covered_assistant_id: None,
        covered_assistant_text_bytes: 0,
        covered_assistant_tool_seq: 0,
        source_fingerprint: source.fingerprint.clone(),
        summary: previous_summary.clone(),
        estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: None,
        created_at: 1,
    };
    let previous = match vega_store::context_compaction::install_checkpoint(store.conn(), &previous)
        .unwrap()
    {
        vega_store::context_compaction::ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
        other => panic!("expected initial checkpoint, got {other:?}"),
    };
    // A new source revision permits replacing the predecessor checkpoint.
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('later-user', 'thread-1', 4, 'user', 'text', 'continue corrected task', 'done', 4)", []).unwrap();
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("new uncovered fact".into()),
        ProviderEvent::Usage {
            input: 300,
            output: 50,
            cache_read: Some(0),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(12_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!result.usages.is_empty());
    assert!(result.usages.iter().all(|usage| usage.usage.output == 50));
    assert!(provider.requests().iter().any(|request| {
        request
            .messages
            .iter()
            .any(|message| message.content.contains("prior state"))
    }));
    let latest =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
    assert_ne!(latest.id, previous.id);
    assert!(latest.summary.len() < previous_summary.len());
    let after = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(after.messages.len(), source.messages.len());
    assert_eq!(after.tool_calls.len(), source.tool_calls.len());
    assert!(
        after
            .messages
            .iter()
            .zip(source.messages.iter())
            .all(|(a, b)| a.id == b.id
                && a.seq == b.seq
                && a.content == b.content
                && a.status == b.status)
    );
}

fn seed_manual_compaction_history(store: &Store) {
    for (id, seq, role, content) in [
        ("manual-old-user", 1_i64, "user", "the original goal"),
        (
            "manual-old-assistant",
            2_i64,
            "assistant",
            "the first result",
        ),
        ("manual-current-user", 3_i64, "user", "the current question"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
}

fn seed_large_tool_result(
    store: &Store,
    tool: &str,
    input: &str,
    output: &str,
    approval_source: ApprovalSource,
) {
    for (id, seq, role) in [
        ("earlier-user", 1_i64, "user"),
        ("earlier-assistant", 2_i64, "assistant"),
        ("newest-user", 3_i64, "user"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: format!("message-{seq}"),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: approval_source,
        danger: None,
    }
    .to_json()
    .unwrap();
    store
        .conn()
        .execute(
            r#"INSERT INTO tool_calls
             (id, thread_id, message_id, seq, tool, input_json, output_text, status,
              approval, created_at, finished_at, text_offset_bytes)
             VALUES ('long-result', 'thread-1', 'earlier-assistant', 1, ?1, ?2, ?3,
                     'success', ?4, 2, 2, 9)"#,
            (tool, input, output, &approval),
        )
        .unwrap();
}

fn seed_large_external_tool_result(store: &Store, input: &str, output: &str) -> String {
    use sha2::{Digest, Sha256};

    let arguments: serde_json::Value = serde_json::from_str(input).unwrap();
    let preview = arguments
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            assert_eq!(key, "path");
            assert!(value.is_string());
            format!("{key}: string")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let identity = crate::types::McpCallIdentity {
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 1,
        exact_tool_name: "summary_fixture".into(),
        arguments_bytes: input.len() as u64,
        arguments_sha256: Sha256::digest(input.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        argument_preview: preview,
    };
    let call = crate::types::ToolCall {
        id: "long-result".into(),
        tool: identity.alias(),
        input_json: serde_json::json!({
            "server_id": identity.server_id,
            "config_revision": identity.config_revision,
            "tool": identity.exact_tool_name,
            "arguments_bytes": identity.arguments_bytes,
            "arguments_sha256": identity.arguments_sha256,
            "argument_preview": identity.argument_preview,
        })
        .to_string(),
    };
    assert_eq!(
        crate::types::McpCallIdentity::from_tool_call(&call),
        Some(identity)
    );
    let output = format!("[Untrusted external MCP result]\n{output}");
    assert!(output.len() <= 256 * 1024 + 64);
    seed_large_tool_result(
        store,
        &call.tool,
        &call.input_json,
        &output,
        ApprovalSource::User,
    );
    output
}

#[tokio::test]
async fn always_permission_and_rule_are_durable_before_second_write() {
    let (store, project_dir, _data_dir, project_id) = setup_external("confirm");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-first".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"first-secret"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "write-second".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"second-secret"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "write twice",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(project_dir.path().join("same.txt")).unwrap(),
        "second-secret"
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "write");
    assert_eq!(rules[0].pattern, "same.txt");
    let approvals = ["write-first", "write-second"].map(|id| {
        let json: String = store
            .conn()
            .query_row(
                "SELECT approval FROM tool_calls WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        ApprovalAudit::from_json(&json).unwrap()
    });
    assert_eq!(approvals[0].source, ApprovalSource::User);
    assert_eq!(approvals[0].decision, Approval::Always);
    assert_eq!(approvals[1].source, ApprovalSource::Rule);
    assert_eq!(approvals[1].decision, Approval::Always);
    assert!(run.events.iter().all(|event| {
        !format!("{event:?}").contains("first-secret")
            && !format!("{event:?}").contains("second-secret")
    }));
}

#[tokio::test]
async fn danger_readonly_always_rejects_and_persists_rule_atomically() {
    let (store, project_dir, data_dir, project_id) = setup_external("readonly");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "danger-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"rm -rf /"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "danger",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (status, approval_json): (String, String) = store
        .conn()
        .query_row(
            "SELECT status, approval FROM tool_calls WHERE id = 'danger-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "rejected");
    let approval = ApprovalAudit::from_json(&approval_json).unwrap();
    assert_eq!(approval.source, ApprovalSource::ReadOnly);
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(
        approval.danger.as_ref().map(|danger| danger.decision),
        Some(Approval::Always)
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "bash");
    assert_eq!(rules[0].pattern, "rm -rf /");
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Rejected
    )));
    assert_eq!(
        fs::read_dir(data_dir.path().join("checkpoints"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn write_edit_and_bash_execute_serially_with_strict_db_results() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("serial.txt"), "initial").unwrap();
    let serial_path = project_dir.path().join("serial.txt");
    let executions = Arc::new(AtomicUsize::new(0));
    let recorded = executions.clone();
    let tools = vega_tools::Tools::new(project_dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, full_access, _| {
            assert_eq!(command, "cat serial.txt");
            assert!(!full_access);
            let text = fs::read_to_string(&serial_path).unwrap();
            assert_eq!(text, "world");
            recorded.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(vega_tools::BashOutput {
                    text,
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    tools.read("serial.txt", None, None).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"serial.txt","content":"hello"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "edit-1".into(),
                name: "edit".into(),
                input_json: r#"{"path":"serial.txt","old_string":"hello","new_string":"world"}"#
                    .into(),
            },
            ProviderEvent::ToolUse {
                id: "bash-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"cat serial.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "serial tools",
        "system",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Deny { note: None },
        },
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(project_dir.path().join("serial.txt")).unwrap(),
        "world"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    let finished = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        finished,
        vec![
            ("write-1", ToolCallStatus::Success),
            ("edit-1", ToolCallStatus::Success),
            ("bash-1", ToolCallStatus::Success),
        ]
    );
    let rows = ["write-1", "edit-1", "bash-1"].map(|id| {
            store
                .conn()
                .query_row(
                    "SELECT status, output_text, exit_code, duration_ms, output_full_path FROM tool_calls WHERE id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i32>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .unwrap()
        });
    assert!(rows.iter().all(|row| row.0 == "success" && row.4.is_none()));
    assert!(vega_tools::WriteSuccessOutput::from_json(&rows[0].1).is_ok());
    assert!(vega_tools::EditSuccessOutput::from_json(&rows[1].1).is_ok());
    assert!(rows[2].1.contains("world"));
    assert_eq!(rows[2].2, Some(0));
    assert!(rows[2].3.is_some());
    assert!(data_dir.path().join("checkpoints").exists());
}

#[test]
fn strict_projection_validation_is_semantic_and_binds_results_and_danger() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let audit = tools
        .audit_write_json(r#"{"path":"bound.txt","content":"body"}"#)
        .unwrap();
    let canonical = audit.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    let reordered = format!(
        r#"{{"fingerprint_v1":{},"content_bytes":{},"path":{},"tool":{},"audit_version":{}}}"#,
        value["fingerprint_v1"],
        value["content_bytes"],
        value["path"],
        value["tool"],
        value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write", &canonical, &reordered
    ));

    let invalid = vega_tools::InvalidMutation::from_raw(
        vega_tools::MutationTool::Write,
        r#"{"path":"x","content":"secret","extra":true}"#,
        vega_tools::MutationErrorCode::UnexpectedField,
    )
    .unwrap();
    let invalid_json = invalid.audit().to_json().unwrap();
    let invalid_value: serde_json::Value = serde_json::from_str(&invalid_json).unwrap();
    let invalid_reordered = format!(
        r#"{{"validation_error_code":{},"raw_input_sha256":{},"raw_input_bytes":{},"tool":{},"audit_version":{}}}"#,
        invalid_value["validation_error_code"],
        invalid_value["raw_input_sha256"],
        invalid_value["raw_input_bytes"],
        invalid_value["tool"],
        invalid_value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write",
        &invalid_json,
        &invalid_reordered
    ));

    let ids = vega_tools::CheckpointIds::new("project", "thread", "call").unwrap();
    let success = vega_tools::WriteSuccessOutput {
        path: "bound.txt".to_string(),
        bytes_written: 4,
        checkpoint_ref: ids.checkpoint_ref(),
    };
    let success_json = success.to_json().unwrap();
    let auto = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "call",
            "write",
            &reordered,
            &success_json,
            RuntimeToolStatus::Success,
            &auto,
            None,
            None,
        )
        .is_ok()
    );
    for corrupt_output in [
        vega_tools::WriteSuccessOutput {
            path: "other.txt".to_string(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            bytes_written: 5,
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            checkpoint_ref: vega_tools::CheckpointIds::new("project", "thread", "other")
                .unwrap()
                .checkpoint_ref(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        "SECRET_RECOVERY_BODY".to_string(),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "call",
                "write",
                &canonical,
                &corrupt_output,
                RuntimeToolStatus::Success,
                &auto,
                None,
                None,
            )
            .is_err()
        );
    }

    let validation = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Validation,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "invalid",
            "write",
            &invalid_reordered,
            invalid.tool_result(),
            RuntimeToolStatus::Rejected,
            &validation,
            None,
            None,
        )
        .is_ok()
    );

    let dangerous = r#"{"cmd":"rm -rf /"}"#;
    let safe = r#"{"cmd":"printf safe"}"#;
    let wrong_danger = crate::types::DangerAudit {
        rule_id: "wrong".to_string(),
        decision: Approval::Once,
        note: None,
    };
    for (input, approval) in [
        (dangerous, auto.clone()),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger.clone()),
            },
        ),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Legacy,
                danger: None,
            },
        ),
        (
            safe,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger),
            },
        ),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "bash-call",
                "bash",
                input,
                "output",
                RuntimeToolStatus::Success,
                &approval,
                Some(0),
                Some(1),
            )
            .is_err()
        );
    }

    let recovery = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Recovery,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_ok()
    );
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{\"secret\":true}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_err()
    );

    let legacy_deny = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Legacy,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "legacy",
            "write",
            &canonical,
            &legacy_unavailable_output("write"),
            RuntimeToolStatus::Rejected,
            &legacy_deny,
            None,
            None,
        )
        .is_ok()
    );
}

#[tokio::test]
async fn file_backed_recovery_reuses_write_edit_and_unknown_without_execution() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("edit.txt"), "old").unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "old-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "interrupted".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let write_raw = r#"{"path":"new.txt","content":"recovery-secret"}"#;
    let edit_raw = r#"{"path":"edit.txt","old_string":"old","new_string":"recovery-new"}"#;
    let write_audit = tools
        .audit_write_json(write_raw)
        .unwrap()
        .to_json()
        .unwrap();
    let edit_audit = tools.audit_edit_json(edit_raw).unwrap().to_json().unwrap();
    for (seq, (id, tool, input, status)) in [
        (
            "recover-write",
            "write",
            write_audit.as_str(),
            "pending_approval",
        ),
        ("recover-edit", "edit", edit_audit.as_str(), "running"),
        ("recover-unknown", "future_tool", "{}", "pending_approval"),
    ]
    .into_iter()
    .enumerate()
    {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: "thread-1",
                message_id: "old-assistant",
                seq: i64::try_from(seq + 1).unwrap(),
                tool,
                input_json: input,
                status,
                created_at: 1,
            },
        )
        .unwrap();
    }
    let auto_json = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    }
    .to_json()
    .unwrap();
    tool_calls::update(
        store.conn(),
        "recover-edit",
        "running",
        Some(&auto_json),
        None,
        None,
    )
    .unwrap();
    let database_path = data_dir.path().join("vega.db");
    drop(store);
    let reopened = Store::open(&database_path).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "recover-write".into(),
                name: "write".into(),
                input_json: write_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-edit".into(),
                name: "edit".into(),
                input_json: edit_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-unknown".into(),
                name: "future_tool".into(),
                input_json: r#"{"secret":"must-not-survive"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "resume",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let reused = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if result.reused => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reused,
        vec![
            ("recover-write", ToolCallStatus::Rejected),
            ("recover-edit", ToolCallStatus::Cancelled),
            ("recover-unknown", ToolCallStatus::Rejected),
        ]
    );
    assert!(!project_dir.path().join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(project_dir.path().join("edit.txt")).unwrap(),
        "old"
    );
    let wire = format!("{:?}", provider.requests());
    assert!(!wire.contains("recovery-secret"));
    assert!(!wire.contains("recovery-new"));
    assert!(!wire.contains("must-not-survive"));
}

#[tokio::test]
async fn codex_parity_irreducible_suffix_makes_no_summary_call() {
    let (store, _dir, _) = setup();
    seed_manual_compaction_history(&store);
    store
        .conn()
        .execute(
            "UPDATE messages SET content=?1 WHERE id='manual-current-user'",
            [&"x".repeat(50_000)],
        )
        .unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("summary".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::ResultOverLimit { .. })
    ));
    assert!(
        provider.requests().is_empty(),
        "fixed suffix cannot fit, avoid billed work"
    );
}

#[tokio::test]
async fn codex_parity_historical_images_survive_compaction_and_reload() {
    let (store, dir, _) = setup();
    seed_manual_compaction_history(&store);
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(4, 3, image::Rgb([20, 40, 60]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    for id in ["manual-old-user", "manual-current-user"] {
        vega_store::image_attachments::insert(store.conn(), id, 0, png.get_ref()).unwrap();
    }
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("visual task continuation".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .messages
            .iter()
            .map(|m| m.images.len())
            .sum::<usize>(),
        2
    );
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .all(|m| m.images.is_empty())
    );
    assert!(
        provider.requests()[0].messages[1]
            .content
            .contains("attachment")
    );
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        reopened.conn(),
        "thread-1",
        "mock-model",
    )
    .unwrap();
    let history = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "none",
    )
    .unwrap();
    assert_eq!(history, result.messages);
    assert_eq!(source.images.len(), 2);
}

#[tokio::test]
async fn codex_parity_realistic_images_repeat_budget_omission_and_corruption() {
    let (store, dir, _) = setup();
    seed_manual_compaction_history(&store);
    let mut state = 17u32;
    let image = image::RgbImage::from_fn(270, 260, |_, _| {
        let mut pixel = [0; 3];
        for value in &mut pixel {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *value = state as u8;
        }
        image::Rgb(pixel)
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    assert!((210_000..220_000).contains(&png.get_ref().len()));
    for id in ["manual-old-user", "manual-current-user"] {
        vega_store::image_attachments::insert(store.conn(), id, 0, png.get_ref()).unwrap();
    }
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("visual task checkpoint".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let budget = vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap();
    let first = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        first.messages.iter().map(|m| m.images.len()).sum::<usize>(),
        2
    );
    assert_eq!(
        first.messages.last().unwrap().images[0].bytes(),
        png.get_ref()
    );
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('image-latest', 'thread-1', 4, 'user', 'text', 'latest visual task', 'done', 4)", []).unwrap();
    vega_store::image_attachments::insert(store.conn(), "image-latest", 0, png.get_ref()).unwrap();
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let second = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        second
            .messages
            .iter()
            .map(|m| m.images.len())
            .sum::<usize>(),
        2
    );
    let retained = second
        .messages
        .iter()
        .find(|m| !m.images.is_empty())
        .unwrap();
    assert!(retained.content.contains("manual-current-user"));
    assert!(retained.content.contains("untrusted data"));
    assert_eq!(retained.images[0].bytes(), png.get_ref());
    assert!(second.messages.iter().any(|m| {
        m.content.contains("1 earlier attachments")
            && m.content
                .contains("visual data is not in the current projection")
    }));
    assert_eq!(
        second.messages.last().unwrap().content,
        "latest visual task"
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        reopened.conn(),
        "thread-1",
        "mock-model",
    )
    .unwrap();
    let history = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "none",
    )
    .unwrap();
    assert_eq!(history, second.messages);
    let projected = read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    let tools = vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Execute);
    assert_eq!(
        projected.estimated_tokens,
        Some(
            vega_runtime::estimate_chat_context("system", &history, &tools)
                .unwrap()
                .input_tokens
        )
    );
    let mut corrupt = source.clone();
    corrupt.images[0].encoded = vec![1, 2, 3];
    assert!(
        super::super::pipeline::primary_history_from_context_source_with_checkpoint(
            &corrupt,
            checkpoint.as_ref(),
            "none"
        )
        .is_err(),
        "even omitted image corruption must fail closed"
    );
}

#[tokio::test]
async fn claude_direct_summary_preserves_roles_and_uses_20k_cap() {
    let (store, _dir, _) = setup();
    let output = "0123456789abcdef".repeat(14_000);
    let output = seed_large_external_tool_result(&store, "{}", &output);
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("direct continuation".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let calls = provider.requests();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].max_tokens, Some(20_000));
    assert!(calls[0].messages.iter().any(|message| {
        message.tool_call_id.is_none()
            && history_observation(message).is_some_and(|observation| {
                observation["kind"] == "tool_result"
                    && observation["call_id"] == "long-result"
                    && observation["output"] == output
            })
    }));
    assert_eq!(
        calls[0]
            .messages
            .iter()
            .flat_map(|m| m.tool_calls.iter())
            .count(),
        0
    );
    assert!(calls[0].tools.is_empty());
}

#[tokio::test]
async fn claude_failed_summary_rechecks_source_before_soft_fallback() {
    for overlarge in [false, true] {
        let (store, dir, _) = setup();
        seed_manual_compaction_history(&store);
        let mut steps = vec![ScriptStep::events(vec![ProviderEvent::Usage {
            input: 43,
            output: 7,
            cache_read: Some(0),
            cache_write: Some(0),
        }])];
        if overlarge {
            steps.push(ScriptStep::events(vec![
                ProviderEvent::TextDelta("x".repeat(64_000)),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]));
        } else {
            steps.push(ScriptStep::Error {
                status: Some(503),
                message: "temporary failure".into(),
                retryable: false,
            });
        }
        let provider = MutatingSecondStageProvider {
            inner: MockProvider::new(steps),
            database_path: dir.path().join("vega.db"),
            calls: AtomicUsize::new(0),
        };
        let failure = compact_thread_manually(
            &store,
            &provider,
            "thread-1",
            "mock-model",
            "system",
            vec![],
            vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                failure.error.as_ref(),
                VegaError::Context(vega_runtime::ContextRuntimeError::SourceChanged)
            ),
            "a failed request is not authority to continue stale context"
        );
        assert_eq!(failure.usages.len(), 1);
        assert_eq!(failure.usages[0].usage.output, 7);
        assert!(
            vega_store::context_compaction::latest_checkpoint(
                store.conn(),
                "thread-1",
                "mock-model"
            )
            .unwrap()
            .is_none()
        );
    }
}

#[tokio::test]
async fn claude_local_preflight_rejects_indivisible_history_without_provider_or_checkpoint() {
    let (store, _dir, _) = setup();
    seed_manual_compaction_history(&store);
    store
        .conn()
        .execute(
            "UPDATE messages SET content=?1 WHERE id='manual-old-user'",
            [&"x".repeat(80_000)],
        )
        .unwrap();
    for seq in 4..=10 {
        let role = if seq == 10 { "user" } else { "assistant" };
        store.conn().execute("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES (?1,'thread-1',?2,?3,'text','complete round','done',?2)",(format!("local-round-{seq}"),seq,role)).unwrap();
    }
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(400),
        message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
        retryable: false,
    }]);
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::SummaryInputOverLimit { .. })
    ));
    assert!(
        provider.requests().is_empty(),
        "an indivisible oversized goal must fail before any provider request"
    );
    assert!(failure.usages.is_empty());
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn claude_unrelated_provider_failures_never_trim_history() {
    for (status, diagnostic) in [
        (400, "bad request"),
        (401, "prompt too long"),
        (429, "prompt too long"),
        (503, "prompt too long"),
    ] {
        let (store, _dir, _) = setup();
        seed_manual_compaction_history(&store);
        let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let provider = MockProvider::new(vec![ScriptStep::Error {
            status: Some(status),
            message: diagnostic.into(),
            retryable: false,
        }]);
        compact_thread_manually(
            &store,
            &provider,
            "thread-1",
            "mock-model",
            "system",
            vec![],
            vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(provider.requests().len(), 1);
        assert!(
            provider.requests()[0]
                .messages
                .iter()
                .any(|m| m.content == "the original goal")
        );
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
            before
        );
    }
}
