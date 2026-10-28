use super::*;
use vega_store::{messages, threads, tool_calls};

mod generation_tests;
mod micro_tests;
mod reactive_tests;

struct SummaryProvider;

impl Provider for SummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(request.tools.is_empty());
        let instruction = request.messages.last().expect("summary instruction");
        assert!(
            instruction
                .content
                .ends_with("Continue implementation; no local tests.")
        );
        Box::pin(async {
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta(
                    "Implementation remains in progress.".into(),
                )),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        })
    }
}

struct DatabaseDirectory(PathBuf);

impl Drop for DatabaseDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn insert_round(store: &Store, round: i64) -> Result<(), Box<dyn std::error::Error>> {
    let id = format!("call-{round}");
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: &id,
            thread_id: "thread",
            message_id: "owner",
            seq: round + 2,
            tool: "read",
            input_json: "{\"path\":\"notes.txt\",\"offset\":null,\"limit\":null}",
            status: "success",
            created_at: round,
        },
    )?;
    store.conn().execute(
        "UPDATE tool_calls SET output_text=?1, text_offset_bytes=0 WHERE id=?2",
        (format!("result-{round}"), &id),
    )?;
    store.conn().execute(
        "INSERT INTO tool_call_rounds(message_id, call_id, response_id) VALUES('owner', ?1, ?2)",
        (&id, format!("response-{round}")),
    )?;
    Ok(())
}

fn source_fixture() -> Result<(DatabaseDirectory, PathBuf, Store), Box<dyn std::error::Error>> {
    let directory = DatabaseDirectory(
        std::env::temp_dir().join(format!("vega-compaction-{}", ulid::Ulid::generate())),
    );
    std::fs::create_dir(&directory.0)?;
    let path = directory.0.join("test.sqlite");
    let store = Store::open(&path)?;
    store.migrate()?;
    threads::create_standalone(
        store.conn(),
        threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Synthetic",
            mode: "execute",
            permission_mode: "read_only",
            permission_revision: 0,
            full_access_grant: None,
            model: "model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    for (id, seq, role, content, status) in [
        (
            "task",
            1,
            "user",
            "Continue implementation; no local tests.",
            "done",
        ),
        ("owner", 2, "assistant", "", "streaming"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
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
        )?;
    }
    insert_round(&store, 1)?;
    insert_round(&store, 2)?;
    Ok((directory, path, store))
}

#[tokio::test]
async fn repeated_partial_owner_compaction_preserves_task_and_reload()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, path, store) = source_fixture()?;
    let reasoning = FrozenReasoning {
        compatibility: None,
        provider: "development".into(),
        model: "model".into(),
        protocol: vega_runtime::ReasoningProtocol::ZhipuChatCompletions,
        replay_policy: vega_runtime::ReasoningReplayPolicy::GlmInterleaved,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    };
    let provider = SummaryProvider;
    let hook =
        ConversationCompactionHook::new(&provider, path, "thread", "model", Some(reasoning), None)
            .with_credential_reader(Some(Arc::new(|| Ok(Vec::new()))));
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    let mut predecessor = None;
    for round in [2, 3] {
        if round == 3 {
            insert_round(&store, round)?;
        }
        let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
        let mut visible = source.clone();
        visible
            .messages
            .iter_mut()
            .filter(|row| row.id == "owner")
            .for_each(|row| row.status = "done".into());
        let history = super::super::pipeline::history_from_context_source_with_checkpoint(
            &visible,
            predecessor.as_ref(),
            "__none__",
        )?;
        let estimate = estimate_chat_context("system", &history, &[])?;
        let request = ContextCompactionRequest {
            trigger: ContextCompactionTrigger::Threshold,
            system_prompt: "system".into(),
            messages: history,
            tools: Vec::new(),
            budget,
            estimate,
            target_tokens: budget.target_tokens()?,
            source_version: source.source_version,
            source_fingerprint: Some(source.fingerprint.clone()),
            require_source_fence: false,
            source_owner_id: Some("owner".into()),
        };
        let result = hook
            .compact(request, CancellationToken::new())
            .await
            .map_err(|failure| *failure.error)?;
        assert_eq!(
            result.replay_cold_prefix_messages,
            Some(result.messages.len())
        );
        assert_eq!(
            result.replay_interleaved_user_boundary,
            Some(result.messages.len())
        );
        assert!(vega_runtime::validate_compaction_replay(
            &result.messages,
            result.messages.len(),
            Some(result.messages.len()),
            true
        ));
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint absent")?;
        assert_eq!(checkpoint.covered_assistant_tool_seq, (round + 1) as u64);
        let plan = checkpoint.projection_plan.as_ref().ok_or("plan absent")?;
        assert_eq!(plan.schema, 1);
        assert_eq!(plan.task_message_id.as_deref(), Some("task"));
        assert_eq!(
            plan.continuation,
            vega_store::context_projection::ContinuationMode::ContinueExistingRun,
        );
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert!(
            reloaded
                .iter()
                .any(|message| message.content == format!("result-{round}"))
        );
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
        predecessor = Some(checkpoint);
    }
    Ok(())
}

#[tokio::test]
async fn full_compaction_covers_all_completed_rounds_and_reloads_future_tail()
-> Result<(), Box<dyn std::error::Error>> {
    use vega_store::context_projection::{ProjectionFragment, ProjectionStrategy};
    for full in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
        let mut visible = source.clone();
        visible
            .messages
            .iter_mut()
            .filter(|row| row.id == "owner")
            .for_each(|row| row.status = "done".into());
        let history =
            super::super::pipeline::history_from_context_source(&visible, "__none__", None)?;
        let budget = ContextBudget::new(200_000, 32_000, true)?;
        let result = ConversationCompactionHook::new(
            &SummaryProvider,
            path.clone(),
            "thread",
            "model",
            None,
            None,
        )
        .with_full_on_threshold(full)
        .with_credential_reader(Some(Arc::new(|| Ok(Vec::new()))))
        .compact(
            ContextCompactionRequest {
                trigger: ContextCompactionTrigger::Threshold,
                system_prompt: "system".into(),
                estimate: estimate_chat_context("system", &history, &[])?,
                messages: history,
                tools: Vec::new(),
                budget,
                target_tokens: budget.target_tokens()?,
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint.clone()),
                require_source_fence: false,
                source_owner_id: Some("owner".into()),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint absent")?;
        let plan = checkpoint.projection_plan.as_ref().ok_or("plan absent")?;
        assert_eq!(
            plan.strategy,
            if full {
                ProjectionStrategy::Full
            } else {
                ProjectionStrategy::Automatic
            }
        );
        assert_eq!(
            checkpoint.covered_assistant_tool_seq,
            if full { 4 } else { 3 }
        );
        assert_eq!(
            result
                .messages
                .iter()
                .any(|message| message.content == "result-2"),
            !full
        );
        assert_eq!(plan.task_message_id.as_deref(), Some("task"));
        if full {
            assert!(result.messages.iter().all(
                |message| message.role != ChatRole::Assistant && message.role != ChatRole::Tool
            ));
            for fragment in &plan.fragments {
                if let ProjectionFragment::Source { range } = fragment {
                    assert!(range.project(&source)?.messages.is_empty());
                }
            }
        }
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
        insert_round(&store, 3)?;
        let reopened = Store::open(path)?;
        let appended = vega_store::context_compaction::load_source(reopened.conn(), "thread")?;
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &appended,
            Some(&checkpoint),
            "__none__",
        )?;
        assert!(reloaded.iter().any(|message| message.content == "result-3"));
        assert!(!reloaded.iter().any(|message| message.content == "result-1"));
        assert_eq!(
            reloaded.iter().any(|message| message.content == "result-2"),
            !full
        );
        assert!(vega_runtime::validate_compaction_replay(
            &reloaded,
            reloaded.len(),
            None,
            false
        ));
    }
    Ok(())
}

#[tokio::test]
async fn full_compaction_rejects_unfinished_response_without_installing()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, path, store) = source_fixture()?;
    store.conn().execute(
        "UPDATE messages SET content='unfinished response' WHERE id='owner'",
        [],
    )?;
    let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
    let mut visible = source.clone();
    visible
        .messages
        .iter_mut()
        .filter(|row| row.id == "owner")
        .for_each(|row| row.status = "done".into());
    let history = super::super::pipeline::history_from_context_source(&visible, "__none__", None)?;
    let provider = OrderedSummaryProvider {
        steps: Default::default(),
    };
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    for require_source_fence in [false, true] {
        let failure =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
                .with_full_on_threshold(true)
                .with_credential_reader(Some(Arc::new(|| Ok(Vec::new()))))
                .compact(
                    ContextCompactionRequest {
                        trigger: ContextCompactionTrigger::Threshold,
                        system_prompt: "system".into(),
                        estimate: estimate_chat_context("system", &history, &[])?,
                        messages: history.clone(),
                        tools: Vec::new(),
                        budget,
                        target_tokens: budget.target_tokens()?,
                        source_version: source.source_version,
                        source_fingerprint: Some(source.fingerprint.clone()),
                        require_source_fence,
                        source_owner_id: Some("owner".into()),
                    },
                    CancellationToken::new(),
                )
                .await
                .expect_err("unfinished full must fail");
        if require_source_fence {
            assert!(
                matches!(
                    failure.error.as_ref(),
                    VegaError::Context(ContextRuntimeError::InvalidProjection)
                ),
                "unexpected fenced failure: {:?}",
                failure.error
            );
        } else {
            assert!(
                matches!(
                    failure.error.as_ref(),
                    VegaError::Context(ContextRuntimeError::SourceChanged)
                ),
                "unexpected live failure: {:?}",
                failure.error
            );
        }
        assert!(
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .is_none()
        );
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
    }
    Ok(())
}

fn checkpoint_request(
    source: &vega_store::context_compaction::ContextSource,
    tool_seq: u64,
    previous: Option<i64>,
) -> Result<NewContextCheckpoint, Box<dyn std::error::Error>> {
    let plan = projection::prefix_plan(
        source,
        1,
        Some(("owner", 0, tool_seq)),
        Some("owner"),
        false,
    )
    .map_err(|failure| *failure.error)?;
    Ok(NewContextCheckpoint {
        thread_id: "thread".into(),
        model: "model".into(),
        source_version: source.source_version,
        source_fingerprint: source.fingerprint.clone(),
        covered_through_seq: 1,
        covered_assistant_id: Some("owner".into()),
        covered_assistant_text_bytes: 0,
        covered_assistant_tool_seq: tool_seq,
        summary: format!("Work through call {tool_seq} remains in progress."),
        estimator_version: CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: previous,
        projection_plan: Some(plan),
        created_at: tool_seq as i64,
    })
}

#[test]
fn same_source_installs_are_idempotent_predecessor_fenced_and_cancellable()
-> Result<(), Box<dyn std::error::Error>> {
    use vega_store::context_compaction::{
        ContextCheckpointError, install_checkpoint, install_checkpoint_with_guard,
        latest_checkpoint, load_source,
    };
    let (_directory, _path, store) = source_fixture()?;
    let source = load_source(store.conn(), "thread")?;
    let first_request = checkpoint_request(&source, 3, None)?;
    let ContextCheckpointInstall::Applied(first) =
        install_checkpoint(store.conn(), &first_request)?
    else {
        return Err("first install rejected".into());
    };
    assert!(matches!(
        install_checkpoint(store.conn(), &first_request)?,
        ContextCheckpointInstall::AlreadyCommitted(existing) if existing == first
    ));
    let second_request = checkpoint_request(&source, 4, Some(first.id))?;
    assert!(matches!(
        install_checkpoint_with_guard(store.conn(), &second_request, || true),
        Err(ContextCheckpointError::Cancelled)
    ));
    assert_eq!(
        latest_checkpoint(store.conn(), "thread", "model")?,
        Some(first.clone())
    );
    let ContextCheckpointInstall::Applied(second) =
        install_checkpoint(store.conn(), &second_request)?
    else {
        return Err("same-source second install rejected".into());
    };
    assert!(second.id > first.id);
    assert!(matches!(
        install_checkpoint(store.conn(), &second_request)?,
        ContextCheckpointInstall::AlreadyCommitted(existing) if existing == second
    ));
    let mut stale = second_request.clone();
    stale.summary = "Another result may not overwrite the winner.".into();
    assert_eq!(
        install_checkpoint(store.conn(), &stale)?,
        ContextCheckpointInstall::Stale
    );
    assert_eq!(load_source(store.conn(), "thread")?, source);
    store.conn().execute(
        "UPDATE messages SET content = 'Changed task' WHERE id = 'task'",
        [],
    )?;
    let mut edited = second_request;
    edited.expected_previous_id = Some(second.id);
    assert_eq!(
        install_checkpoint(store.conn(), &edited)?,
        ContextCheckpointInstall::Stale
    );
    assert_eq!(
        latest_checkpoint(store.conn(), "thread", "model")?,
        Some(second)
    );
    Ok(())
}

#[test]
fn source_ranges_preserve_complete_rounds_and_reject_split_groups_or_utf8()
-> Result<(), Box<dyn std::error::Error>> {
    use vega_store::context_projection::{AssistantCursor, SourceCursor, SourceRange};
    let (_directory, _path, store) = source_fixture()?;
    store.conn().execute(
        "UPDATE messages SET content = 'αβtail' WHERE id = 'owner'",
        [],
    )?;
    store.conn().execute(
        "UPDATE tool_calls SET text_offset_bytes = 2 WHERE id = 'call-1'",
        [],
    )?;
    store.conn().execute(
        "UPDATE tool_calls SET text_offset_bytes = 4 WHERE id = 'call-2'",
        [],
    )?;
    let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
    let cursor = SourceCursor {
        message_id: "owner".into(),
        message_seq: 2,
        assistant: Some(AssistantCursor {
            text_bytes: 2,
            tool_seq: 3,
            tool_id: "call-1".into(),
            response_id: Some("response-1".into()),
        }),
    };
    let prefix = SourceRange {
        after: None,
        through: Some(cursor.clone()),
    };
    let suffix = SourceRange {
        after: Some(cursor.clone()),
        through: None,
    };
    prefix.validate_terminal(&source)?;
    let before = prefix.project(&source)?;
    let after = suffix.project(&source)?;
    assert_eq!(
        before.messages.last().ok_or("prefix owner absent")?.content,
        "α"
    );
    assert_eq!(before.tool_calls.len(), 1);
    assert_eq!(before.tool_calls[0].id, "call-1");
    assert_eq!(after.messages[0].content, "βtail");
    assert_eq!(after.tool_calls.len(), 1);
    assert_eq!(after.tool_calls[0].id, "call-2");
    assert_eq!(after.tool_calls[0].text_offset_bytes, Some(2));
    let mut invalid = cursor.clone();
    invalid
        .assistant
        .as_mut()
        .ok_or("cursor absent")?
        .text_bytes = 1;
    assert!(invalid.validate(&source).is_err());
    let mut invalid = cursor.clone();
    invalid.message_id = "missing".into();
    assert!(invalid.validate(&source).is_err());
    let mut same_response = source.clone();
    same_response.tool_calls[1].response_id = Some("response-1".into());
    assert!(cursor.validate(&same_response).is_err());
    let mut incomplete = source.clone();
    incomplete.tool_calls[0].status = "running".into();
    assert!(prefix.validate_terminal(&incomplete).is_err());
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread")?,
        source
    );
    Ok(())
}

#[test]
fn partial_ranges_keep_screenshot_artifacts_with_their_owning_calls()
-> Result<(), Box<dyn std::error::Error>> {
    use vega_store::context_projection::{AssistantCursor, SourceCursor, SourceRange};
    let (_directory, _path, store) = source_fixture()?;
    store.conn().execute(
        "UPDATE tool_calls SET tool='ComputerScreenshot', status='running' WHERE id='call-1'",
        [],
    )?;
    vega_store::computer_use_delivery::insert_screenshot(
        store.conn(),
        "thread",
        "owner",
        "call-1",
        b"synthetic screenshot audit bytes",
    )?;
    store.conn().execute(
        "UPDATE tool_calls SET status='success' WHERE id='call-1'",
        [],
    )?;
    let captured = vega_store::context_compaction::load_source(store.conn(), "thread")?;
    assert!(captured.images.is_empty());
    let attachments =
        vega_store::image_attachments::for_messages(store.conn(), "thread", &captured.messages)?;
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].artifact_call_id.as_deref(), Some("call-1"));
    let mut source = captured.clone();
    source.images = attachments.clone();
    let cursor = SourceCursor {
        message_id: "owner".into(),
        message_seq: 2,
        assistant: Some(AssistantCursor {
            text_bytes: 0,
            tool_seq: 3,
            tool_id: "call-1".into(),
            response_id: Some("response-1".into()),
        }),
    };
    let prefix = SourceRange {
        after: None,
        through: Some(cursor.clone()),
    }
    .project(&source)?;
    let suffix = SourceRange {
        after: Some(cursor),
        through: None,
    }
    .project(&source)?;
    assert_eq!(prefix.images, attachments);
    assert!(suffix.images.is_empty());
    let prefix_history =
        super::super::pipeline::history_from_context_source(&prefix, "__none__", None)?;
    let suffix_history =
        super::super::pipeline::history_from_context_source(&suffix, "__none__", None)?;
    assert!(
        prefix_history
            .iter()
            .all(|message| message.images.is_empty())
    );
    assert_eq!(
        suffix_history.last().ok_or("tool result absent")?.content,
        "result-2"
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread")?,
        captured,
    );
    assert_eq!(
        vega_store::image_attachments::for_messages(store.conn(), "thread", &captured.messages)?,
        attachments,
    );
    Ok(())
}

#[tokio::test]
async fn compaction_preserves_retained_live_reasoning_without_persisting_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (_directory, path, store) = source_fixture()?;
    store
        .conn()
        .execute("UPDATE messages SET status = 'done' WHERE id = 'owner'", [])?;
    for (id, seq, role, content, status) in [
        (
            "latest-task",
            5,
            "user",
            "Continue implementation; no local tests.",
            "done",
        ),
        ("live-owner", 6, "assistant", "", "streaming"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
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
        )?;
    }
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "live-call",
            thread_id: "thread",
            message_id: "live-owner",
            seq: 7,
            tool: "read",
            input_json: "{\"path\":\"notes.txt\",\"offset\":null,\"limit\":null}",
            status: "success",
            created_at: 7,
        },
    )?;
    store.conn().execute(
        "UPDATE tool_calls SET output_text = 'Latest result' WHERE id = 'live-call'",
        [],
    )?;
    store.conn().execute("INSERT INTO tool_call_rounds(message_id, call_id, response_id) VALUES('live-owner', 'live-call', 'live-response')", [])?;
    let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
    let mut visible = source.clone();
    visible
        .messages
        .iter_mut()
        .filter(|message| message.id == "live-owner")
        .for_each(|message| message.status = "done".into());
    let mut history = super::super::pipeline::history_from_context_source_with_checkpoint(
        &visible, None, "__none__",
    )?;
    let retained = history
        .iter_mut()
        .find(|message| message.tool_calls.iter().any(|call| call.id == "live-call"))
        .ok_or("live assistant absent")?;
    retained.reasoning_content = Some("Run-local provider reasoning".into());
    retained.response_reasoning = vec![serde_json::json!({"type":"reasoning", "id":"live-only"})];
    let retained = retained.clone();
    let provider = reactive_tests::ScriptedSummaryProvider::new([
        reactive_tests::SummaryReply::Text("FIRST_SUMMARY ".repeat(300)),
        reactive_tests::SummaryReply::Text("SECOND_SUMMARY".into()),
    ]);
    let reasoning = FrozenReasoning {
        compatibility: None,
        provider: "development".into(),
        model: "model".into(),
        protocol: vega_runtime::ReasoningProtocol::ZhipuChatCompletions,
        replay_policy: vega_runtime::ReasoningReplayPolicy::GlmInterleaved,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    };
    let hook =
        ConversationCompactionHook::new(&provider, path, "thread", "model", Some(reasoning), None)
            .with_credential_reader(Some(Arc::new(|| Ok(Vec::new()))));
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    let result = hook
        .compact(
            ContextCompactionRequest {
                trigger: ContextCompactionTrigger::Threshold,
                system_prompt: "system".into(),
                estimate: estimate_chat_context("system", &history, &[])?,
                messages: history,
                tools: Vec::new(),
                budget,
                target_tokens: budget.target_tokens()?,
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint.clone()),
                require_source_fence: false,
                source_owner_id: Some("live-owner".into()),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    let immediate = result
        .messages
        .iter()
        .find(|message| message.tool_calls.iter().any(|call| call.id == "live-call"))
        .ok_or("retained live assistant absent")?;
    assert_eq!(immediate, &retained);
    let repeated = hook
        .compact(
            ContextCompactionRequest {
                trigger: ContextCompactionTrigger::InputOverflow {
                    token_gap: Some(1_000),
                    deeper: false,
                },
                system_prompt: "system".into(),
                estimate: estimate_chat_context("system", &result.messages, &[])?,
                messages: result.messages.clone(),
                tools: Vec::new(),
                budget,
                target_tokens: budget.target_tokens()?,
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint.clone()),
                require_source_fence: false,
                source_owner_id: Some("live-owner".into()),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(
        repeated
            .messages
            .iter()
            .find(|message| message.tool_calls.iter().any(|call| call.id == "live-call")),
        Some(&retained)
    );
    assert_eq!(repeated.replay_cold_prefix_messages, Some(1));
    assert_eq!(repeated.replay_interleaved_user_boundary, Some(1));
    assert_eq!(result.replay_cold_prefix_messages, Some(1));
    assert_eq!(result.replay_interleaved_user_boundary, Some(1));
    assert!(vega_runtime::validate_compaction_replay(
        &result.messages,
        1,
        Some(1),
        true
    ));
    let checkpoint =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
            .ok_or("checkpoint absent")?;
    let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
        &source,
        Some(&checkpoint),
        "__none__",
    )?;
    assert!(durable_messages_equal(
        &repeated.messages.iter().collect::<Vec<_>>(),
        &reloaded.iter().collect::<Vec<_>>()
    ));
    assert!(
        estimate_chat_context("system", &repeated.messages, &[])?.input_tokens
            < estimate_chat_context("system", &result.messages, &[])?.input_tokens
    );
    assert_eq!(provider.requests().len(), 2);
    let cold = reloaded
        .iter()
        .find(|message| message.tool_calls.iter().any(|call| call.id == "live-call"))
        .ok_or("reloaded assistant absent")?;
    assert!(cold.reasoning_content.is_none());
    assert!(cold.response_reasoning.is_empty());
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread")?,
        source
    );
    Ok(())
}

#[tokio::test]
async fn selected_directions_install_and_reload_without_mutating_audit()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::types::{PartialCompactionDirection, PartialCompactionSelection};
    for direction in [
        PartialCompactionDirection::UpTo,
        PartialCompactionDirection::From,
    ] {
        let (_directory, path, store) = source_fixture()?;
        store
            .conn()
            .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
        let mut next = messages::find(store.conn(), "task")?.ok_or("task absent")?;
        next.id = "selected".into();
        next.seq = 5;
        next.content = "Continue implementation; no local tests.".into();
        messages::insert(store.conn(), &next)?;
        let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
        let budget = ContextBudget::new(200_000, 32_000, false)?;
        let history =
            super::super::pipeline::history_from_context_source(&source, "__none__", None)?;
        let hook =
            ConversationCompactionHook::new(&SummaryProvider, path, "thread", "model", None, None)
                .with_partial_selection(PartialCompactionSelection {
                    message_id: "selected".into(),
                    source_fingerprint: source.fingerprint.clone(),
                    direction,
                });
        let result = hook
            .compact(
                ContextCompactionRequest {
                    trigger: ContextCompactionTrigger::Manual,
                    system_prompt: "system".into(),
                    estimate: estimate_chat_context("system", &history, &[])?,
                    messages: history,
                    tools: Vec::new(),
                    budget,
                    target_tokens: budget.target_tokens()?,
                    source_version: source.source_version,
                    source_fingerprint: Some(source.fingerprint.clone()),
                    require_source_fence: true,
                    source_owner_id: None,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(|failure| *failure.error)?;
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint absent")?;
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        let summary = reloaded
            .iter()
            .position(|message| message.content.contains("Historical context summary"))
            .ok_or("summary absent")?;
        match direction {
            PartialCompactionDirection::UpTo => {
                assert_eq!(summary, 0);
                assert_eq!(
                    reloaded.last().ok_or("selected absent")?.content,
                    next.content
                );
                assert!(!reloaded.iter().any(|message| message.content == "result-1"));
            }
            PartialCompactionDirection::From => {
                assert_eq!(summary, reloaded.len() - 1);
                assert!(reloaded.iter().any(|message| message.content == "result-1"));
            }
        }
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
    }
    Ok(())
}

#[tokio::test]
async fn repeated_selected_ranges_preserve_disjoint_coverage_and_reload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::types::{PartialCompactionDirection, PartialCompactionSelection};
    for final_direction in [
        PartialCompactionDirection::From,
        PartialCompactionDirection::UpTo,
    ] {
        let (_directory, path, store) = source_fixture()?;
        store
            .conn()
            .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
        let mut row = messages::find(store.conn(), "task")?.ok_or("task absent")?;
        for (id, seq) in [("middle", 3), ("tail", 4)] {
            row.id = id.into();
            row.seq = seq;
            messages::insert(store.conn(), &row)?;
        }
        let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
        let budget = ContextBudget::new(200_000, 32_000, false)?;
        let mut history =
            super::super::pipeline::history_from_context_source(&source, "__none__", None)?;
        for (id, direction) in [
            ("tail", PartialCompactionDirection::From),
            ("middle", PartialCompactionDirection::From),
            ("owner", final_direction),
        ] {
            let hook = ConversationCompactionHook::new(
                &SummaryProvider,
                path.clone(),
                "thread",
                "model",
                None,
                None,
            )
            .with_partial_selection(PartialCompactionSelection {
                message_id: id.into(),
                source_fingerprint: source.fingerprint.clone(),
                direction,
            });
            let result = hook
                .compact(
                    ContextCompactionRequest {
                        trigger: ContextCompactionTrigger::Manual,
                        system_prompt: "system".into(),
                        estimate: estimate_chat_context("system", &history, &[])?,
                        messages: history,
                        tools: Vec::new(),
                        budget,
                        target_tokens: budget.target_tokens()?,
                        source_version: source.source_version,
                        source_fingerprint: Some(source.fingerprint.clone()),
                        require_source_fence: true,
                        source_owner_id: None,
                    },
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
            let checkpoint =
                vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                    .ok_or("checkpoint absent")?;
            checkpoint
                .projection_plan
                .as_ref()
                .ok_or("plan absent")?
                .validate_install(&source)?;
            let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
                &source,
                Some(&checkpoint),
                "__none__",
            )?;
            assert_eq!(result.messages, reloaded);
            assert_eq!(
                vega_store::context_compaction::load_source(store.conn(), "thread")?,
                source
            );
            history = reloaded;
        }
        let summaries = history
            .iter()
            .filter(|message| message.content.contains("Historical context summary"))
            .count();
        assert_eq!(
            summaries,
            if final_direction == PartialCompactionDirection::From {
                1
            } else {
                2
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn legacy_coverage_is_normalized_before_real_hook_selection()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::types::{PartialCompactionDirection, PartialCompactionSelection};
    for partial_owner in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        store
            .conn()
            .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
        let mut row = messages::find(store.conn(), "task")?.ok_or("task absent")?;
        row.id = "selected".into();
        row.seq = 3;
        messages::insert(store.conn(), &row)?;
        let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
        store.conn().execute(
            "INSERT INTO context_checkpoints (thread_id, model, source_version, covered_through_seq, source_fingerprint, summary, estimator_version, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq) VALUES ('thread', 'model', ?1, ?2, ?3, 'Legacy history', 'development', 1, ?4, 0, ?5)",
            (source.source_version as i64, if partial_owner { 1 } else { 2 }, &source.fingerprint, if partial_owner { Some("owner") } else { None }, if partial_owner { 3 } else { 0 }),
        )?;
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("legacy absent")?;
        let selection = PartialCompactionSelection {
            message_id: "selected".into(),
            source_fingerprint: source.fingerprint.clone(),
            direction: PartialCompactionDirection::From,
        };
        let planned = projection::partial_plan(&source, Some(&checkpoint), &selection)
            .map_err(|failure| *failure.error)?;
        assert_eq!(planned.summary_history.len(), 1);
        assert_eq!(planned.summary_history[0].content, row.content);
        assert!(planned.summary_history[0].tool_calls.is_empty());
        let covered_selection = PartialCompactionSelection {
            message_id: "task".into(),
            ..selection.clone()
        };
        assert!(projection::partial_plan(&source, Some(&checkpoint), &covered_selection).is_err());
        let history = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        let budget = ContextBudget::new(200_000, 32_000, false)?;
        let result =
            ConversationCompactionHook::new(&SummaryProvider, path, "thread", "model", None, None)
                .with_partial_selection(selection)
                .compact(
                    ContextCompactionRequest {
                        trigger: ContextCompactionTrigger::Manual,
                        system_prompt: "system".into(),
                        estimate: estimate_chat_context("system", &history, &[])?,
                        messages: history,
                        tools: Vec::new(),
                        budget,
                        target_tokens: budget.target_tokens()?,
                        source_version: source.source_version,
                        source_fingerprint: Some(source.fingerprint.clone()),
                        require_source_fence: true,
                        source_owner_id: None,
                    },
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint absent")?;
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
        assert!(
            reloaded
                .iter()
                .any(|message| message.content.contains("Legacy history"))
        );
        assert!(!reloaded.iter().any(|message| message.content == "result-1"));
        assert_eq!(
            reloaded.iter().any(|message| message.content == "result-2"),
            partial_owner
        );
    }
    Ok(())
}

struct OrderedSummaryProvider {
    steps: std::sync::Mutex<std::collections::VecDeque<(Vec<&'static str>, &'static str)>>,
}

impl Provider for OrderedSummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let (markers, summary) = self
            .steps
            .lock()
            .expect("steps lock")
            .pop_front()
            .expect("unexpected summary call");
        assert!(request.tools.is_empty());
        let history = request.messages[1..request.messages.len() - 1]
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let positions = markers
            .iter()
            .map(|marker| history.find(marker).expect("missing ordered source marker"))
            .collect::<Vec<_>>();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "unordered summary input: {history}"
        );
        Box::pin(async move {
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta(summary.into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        })
    }
}

#[tokio::test]
async fn consecutive_partial_summaries_preserve_input_and_projection_order()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::types::{PartialCompactionDirection as Direction, PartialCompactionSelection};
    let (_directory, path, store) = source_fixture()?;
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
    let mut row = messages::find(store.conn(), "task")?.ok_or("task absent")?;
    for (id, seq, marker) in [("middle", 3, "SOURCE_MIDDLE"), ("tail", 4, "SOURCE_TAIL")] {
        row.id = id.into();
        row.seq = seq;
        row.role = "assistant".into();
        row.content = marker.into();
        messages::insert(store.conn(), &row)?;
    }
    let provider = OrderedSummaryProvider {
        steps: std::sync::Mutex::new(std::collections::VecDeque::from([
            (vec!["SOURCE_TAIL"], "SUMMARY_TAIL"),
            (vec!["SOURCE_MIDDLE", "SUMMARY_TAIL"], "SUMMARY_MIDDLE_TAIL"),
            (
                vec!["Continue implementation; no local tests."],
                "SUMMARY_TASK",
            ),
            (
                vec!["result-1", "result-2", "SUMMARY_MIDDLE_TAIL"],
                "SUMMARY_OWNER_TAIL",
            ),
            (vec!["SUMMARY_TASK", "SUMMARY_OWNER_TAIL"], "FULL_SUMMARY"),
        ])),
    };
    let source = vega_store::context_compaction::load_source(store.conn(), "thread")?;
    let budget = ContextBudget::new(200_000, 32_000, false)?;
    let mut history =
        super::super::pipeline::history_from_context_source(&source, "__none__", None)?;
    for (id, direction, expected_order) in [
        (
            "tail",
            Direction::From,
            vec!["result-2", "SOURCE_MIDDLE", "SUMMARY_TAIL"],
        ),
        (
            "middle",
            Direction::From,
            vec!["result-2", "SUMMARY_MIDDLE_TAIL"],
        ),
        (
            "owner",
            Direction::UpTo,
            vec!["SUMMARY_TASK", "result-2", "SUMMARY_MIDDLE_TAIL"],
        ),
        (
            "owner",
            Direction::From,
            vec!["SUMMARY_TASK", "SUMMARY_OWNER_TAIL"],
        ),
    ] {
        let result =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
                .with_partial_selection(PartialCompactionSelection {
                    message_id: id.into(),
                    source_fingerprint: source.fingerprint.clone(),
                    direction,
                })
                .compact(
                    ContextCompactionRequest {
                        trigger: ContextCompactionTrigger::Manual,
                        system_prompt: "system".into(),
                        estimate: estimate_chat_context("system", &history, &[])?,
                        messages: history,
                        tools: Vec::new(),
                        budget,
                        target_tokens: budget.target_tokens()?,
                        source_version: source.source_version,
                        source_fingerprint: Some(source.fingerprint.clone()),
                        require_source_fence: true,
                        source_owner_id: None,
                    },
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
        let text = result
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let positions = expected_order
            .iter()
            .map(|marker| text.find(marker).expect("projection marker absent"))
            .collect::<Vec<_>>();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "unordered projection: {text}"
        );
        let checkpoint =
            vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint absent")?;
        let reloaded = super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread")?,
            source
        );
        history = result.messages;
    }
    let previous =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
            .ok_or("partial checkpoint absent")?;
    let template =
        projection::prefix_plan(&source, 4, None, None, true).map_err(|failure| *failure.error)?;
    let ordered = projection::ordered_prefix_plan(&source, Some(&previous), template)
        .map_err(|failure| *failure.error)?;
    let ordered_text = ordered
        .summary_history
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        ordered_text
            .find("SUMMARY_TASK")
            .ok_or("task summary absent")?
            < ordered_text
                .find("SUMMARY_OWNER_TAIL")
                .ok_or("tail summary absent")?
    );
    assert!(!ordered_text.contains("SOURCE_MIDDLE"));
    assert!(!ordered_text.contains("result-1"));
    assert_eq!(
        ordered.plan.covered,
        previous
            .projection_plan
            .as_ref()
            .ok_or("partial plan absent")?
            .covered
    );
    assert_eq!(
        ordered.plan.omitted,
        previous
            .projection_plan
            .as_ref()
            .ok_or("partial plan absent")?
            .omitted
    );
    let result = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_full_on_threshold(true)
        .compact(
            ContextCompactionRequest {
                trigger: ContextCompactionTrigger::Threshold,
                system_prompt: "system".into(),
                estimate: estimate_chat_context("system", &history, &[])?,
                messages: history,
                tools: Vec::new(),
                budget,
                target_tokens: budget.target_tokens()?,
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint.clone()),
                require_source_fence: true,
                source_owner_id: None,
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(result.messages.len(), 1);
    assert!(result.messages[0].content.contains("FULL_SUMMARY"));
    let checkpoint =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?
            .ok_or("full checkpoint absent")?;
    let plan = checkpoint
        .projection_plan
        .as_ref()
        .ok_or("full plan absent")?;
    assert_eq!(
        plan.strategy,
        vega_store::context_projection::ProjectionStrategy::Full
    );
    assert_eq!(
        result.messages,
        super::super::pipeline::history_from_context_source_with_checkpoint(
            &source,
            Some(&checkpoint),
            "__none__",
        )?
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread")?,
        source
    );
    assert!(provider.steps.lock().expect("steps lock").is_empty());
    Ok(())
}
