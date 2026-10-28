use super::*;
use vega_runtime::{ContextCompactionUsageState, ReasoningReplayPolicy, ReplayFrame};

async fn publish(
    hook: &ConversationCompactionHook<'_>,
    request: ContextCompactionRequest,
) -> TestResult {
    let mut worker = hook
        .prepare_precompute(request, CancellationToken::new())
        .await?
        .ok_or("not prepared")?
        .launch()?;
    tokio::time::timeout(Duration::from_secs(10), worker.wait()).await??;
    Ok(())
}

fn current_input(
    store: &Store,
    mut request: ContextCompactionRequest,
) -> Result<ContextCompactionRequest, Box<dyn std::error::Error>> {
    let source = load_source(store.conn(), "thread")?;
    request.messages =
        crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
    request.source_version = source.source_version;
    request.source_fingerprint = Some(source.fingerprint);
    request.estimate =
        estimate_chat_context(&request.system_prompt, &request.messages, &request.tools)?;
    Ok(request)
}

#[tokio::test]
async fn ready_consumption_splices_append_reloads_and_never_rebills() -> TestResult {
    for mode in ["success", "unknown"] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        let provider = BackgroundProvider::new(mode, false);
        let hook =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
        publish(&hook, request.clone()).await?;
        append(&store)?;
        let before = load_source(store.conn(), "thread")?;
        let mut request = current_input(&store, request)?;
        request.trigger = ContextCompactionTrigger::Manual;
        let result = hook
            .compact_with_replay(
                request.clone(),
                Some(ReplayFrame::new(
                    request.messages.len() + 1,
                    ReasoningReplayPolicy::Unknown,
                )),
                CancellationToken::new(),
            )
            .await
            .map_err(|failure| *failure.error)?;
        assert!(result.usages.is_empty());
        assert_eq!(
            result.usage_state(),
            if mode == "success" {
                ContextCompactionUsageState::Known { priced: false }
            } else {
                ContextCompactionUsageState::Unknown
            }
        );
        assert_eq!(result.reactive_retained_groups, None);
        assert!(
            result
                .messages
                .iter()
                .any(|message| message.content == "Keep this later instruction.")
        );
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
        assert_eq!(count(&store, "token_usage")?, i64::from(mode == "success"));
        assert_eq!(count(&store, "context_checkpoints")?, 1);
        assert!(ready::recover(store.conn(), "thread", now_ms())?.is_none());
        assert_eq!(load_source(store.conn(), "thread")?, before);
        let checkpoint =
            latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
        let reloaded = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &before,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert!(
            hook.consume_precompute(&request, None, &CancellationToken::new())
                .await?
                .is_none()
        );
        assert_eq!(count(&store, "context_checkpoints")?, 1);
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn ready_consumption_uses_current_model_budget_and_compatible_preferences() -> TestResult {
    for case in [
        "fit",
        "budget",
        "preferences",
        "partial",
        "pending",
        "deeper",
        "system",
        "tools",
        "stale_request",
    ] {
        let (_directory, path, store) = source_fixture()?;
        let mut request = input(&store)?;
        let provider = BackgroundProvider::new("success", false);
        let hook =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
        publish(&hook, request.clone()).await?;
        store.conn().execute(
            "UPDATE threads SET model='current-model' WHERE id='thread'",
            [],
        )?;
        let mut hook =
            ConversationCompactionHook::new(&provider, path, "thread", "current-model", None, None);
        match case {
            "budget" => {
                request.budget = ContextBudget::new(128, 32, true)?;
                request.target_tokens = request.budget.target_tokens()?;
            }
            "preferences" => {
                hook = hook
                    .with_summary_requirements("Preserve a different emphasis".into())
                    .map_err(|failure| *failure.error)?;
            }
            "partial" => {
                hook = hook.with_partial_selection(crate::types::PartialCompactionSelection {
                    message_id: "task".into(),
                    source_fingerprint: request.source_fingerprint.clone().unwrap(),
                    direction: crate::types::PartialCompactionDirection::UpTo,
                });
            }
            "pending" => {
                store.conn().execute(
                    "UPDATE threads SET pending_model='next' WHERE id='thread'",
                    [],
                )?;
            }
            "deeper" => {
                request.trigger = ContextCompactionTrigger::InputOverflow {
                    token_gap: Some(100),
                    deeper: true,
                }
            }
            "system" => request.system_prompt = "large system ".repeat(70_000),
            "tools" => request.tools.push(ToolDefinition {
                name: "synthetic".into(),
                description: "large schema ".repeat(70_000),
                input_schema: serde_json::json!({"type":"object"}),
                strict: false,
            }),
            "stale_request" => append(&store)?,
            _ => {}
        }
        let result = hook
            .consume_precompute(&request, None, &CancellationToken::new())
            .await;
        if case == "stale_request" {
            assert!(matches!(
                result,
                Err(VegaError::Context(ContextRuntimeError::SourceChanged))
            ));
        } else if case == "fit" {
            let result = result?.ok_or("compatible model was not consumed")?;
            assert!(result.usages.is_empty());
            let checkpoint = latest_checkpoint(store.conn(), "thread", "current-model")?.unwrap();
            assert_eq!(checkpoint.model, "current-model");
        } else {
            assert!(result?.is_none(), "{case}");
        }
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1, "{case}");
        assert_eq!(
            count(&store, "context_checkpoints")?,
            i64::from(case == "fit")
        );
        assert_eq!(
            count(&store, "context_precompute_ready")?,
            i64::from(case != "fit")
        );
        assert_eq!(count(&store, "token_usage")?, 1);
    }
    Ok(())
}

fn append_live(store: &Store) -> TestResult {
    append(store)?;
    store.conn().execute_batch(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('live-owner','thread',4,'assistant','','streaming',4);
         INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes) VALUES('live-call','thread','live-owner',3,'read','{}','Latest result','success',4,0);
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('live-owner','live-call','live-response');"
    )?;
    Ok(())
}

#[tokio::test]
async fn ready_consumption_preserves_live_replay_and_checks_framed_reasoning_budget() -> TestResult
{
    for policy in [
        ReasoningReplayPolicy::DeepSeekTools,
        ReasoningReplayPolicy::GlmInterleaved,
    ] {
        for huge_reasoning in [false, true] {
            let (_directory, path, store) = source_fixture()?;
            let mut request = input(&store)?;
            let provider = BackgroundProvider::new("success", false);
            let hook =
                ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
            publish(&hook, request.clone()).await?;
            append_live(&store)?;
            let source = load_source(store.conn(), "thread")?;
            let mut visible = source.clone();
            visible.messages.last_mut().unwrap().status = "done".into();
            request.messages =
                crate::agent::pipeline::history_from_context_source(&visible, "__none__", None)?;
            let live = request
                .messages
                .iter_mut()
                .find(|message| message.tool_calls.iter().any(|call| call.id == "live-call"))
                .unwrap();
            live.reasoning_content = Some(if huge_reasoning {
                "live reasoning ".repeat(9000)
            } else {
                "Live reasoning".into()
            });
            let retained = live.clone();
            request.source_version = source.source_version;
            request.source_fingerprint = Some(source.fingerprint.clone());
            request.source_owner_id = Some("live-owner".into());
            request.require_source_fence = false;
            request.budget = ContextBudget::new(10_000, 1000, true)?;
            request.target_tokens = request.budget.target_tokens()?;
            let boundary = request
                .messages
                .iter()
                .position(|message| message.content == "Keep this later instruction.")
                .unwrap()
                + 1;
            let frame = ReplayFrame::new(boundary, policy);
            assert!(
                hook.consume_precompute(&request, None, &CancellationToken::new())
                    .await?
                    .is_none()
            );
            assert!(ready::recover(store.conn(), "thread", now_ms())?.is_some());
            let result = hook
                .consume_precompute(&request, Some(&frame), &CancellationToken::new())
                .await?;
            if huge_reasoning {
                assert!(result.is_none());
                assert_eq!(count(&store, "context_checkpoints")?, 0);
                assert!(ready::recover(store.conn(), "thread", now_ms())?.is_some());
            } else {
                let result = result.ok_or("live candidate not consumed")?;
                assert_eq!(
                    result.messages.iter().find(|message| message
                        .tool_calls
                        .iter()
                        .any(|call| call.id == "live-call")),
                    Some(&retained)
                );
                let wire = result.wire_messages(&request, Some(&frame))?;
                assert!(
                    estimate_wire_context(&wire, &request.tools)?.input_tokens
                        < request.budget.trigger_tokens()?
                );
                if policy == ReasoningReplayPolicy::GlmInterleaved {
                    let boundary = result
                        .replay_interleaved_user_boundary
                        .ok_or("no GLM boundary")?;
                    assert_eq!(
                        result.messages[boundary].content,
                        "Keep this later instruction."
                    );
                }
                let checkpoint = latest_checkpoint(store.conn(), "thread", "model")?.unwrap();
                let reloaded = crate::agent::pipeline::history_from_context_source_with_checkpoint(
                    &source,
                    Some(&checkpoint),
                    "__none__",
                )?;
                assert!(durable_messages_equal(
                    &result.messages.iter().collect::<Vec<_>>(),
                    &reloaded.iter().collect::<Vec<_>>()
                ));
                assert!(
                    reloaded
                        .iter()
                        .all(|message| message.reasoning_content.is_none())
                );
                assert_eq!(
                    checkpoint
                        .projection_plan
                        .as_ref()
                        .unwrap()
                        .source_owner_id
                        .as_deref(),
                    Some("live-owner")
                );
                assert_eq!(
                    checkpoint
                        .projection_plan
                        .as_ref()
                        .unwrap()
                        .task_message_id
                        .as_deref(),
                    Some("appended-task")
                );
            }
            assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
            assert_eq!(count(&store, "token_usage")?, 1);
        }
    }
    Ok(())
}

#[tokio::test]
async fn ready_consumption_rereads_credentials_and_honors_cancellation() -> TestResult {
    for case in ["reader", "secret", "cancel"] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        let provider = BackgroundProvider::new("success", false);
        let hook =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
        publish(&hook, request.clone()).await?;
        let reader: vega_runtime::CredentialReader = match case {
            "reader" => Arc::new(|| Err(())),
            "secret" => Arc::new(|| Ok(vec!["Continue the implementation.".into()])),
            _ => Arc::new(|| Ok(Vec::new())),
        };
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
            .with_credential_reader(Some(reader));
        let cancel = CancellationToken::new();
        if case == "cancel" {
            cancel.cancel();
        }
        let result = hook.consume_precompute(&request, None, &cancel).await;
        assert!(matches!(
            (case, result),
            ("reader", Err(VegaError::CredentialCheckUnavailable))
                | ("secret", Err(VegaError::CredentialExposureBlocked))
                | ("cancel", Err(VegaError::Cancelled))
        ));
        assert!(ready::recover(store.conn(), "thread", now_ms())?.is_some());
        assert_eq!(count(&store, "context_checkpoints")?, 0);
        assert_eq!(count(&store, "token_usage")?, 1);
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn incompatible_ready_preferences_fall_back_to_foreground_generation() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("success", false);
    let hook =
        ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
    publish(&hook, request.clone()).await?;
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_summary_requirements("Preserve the pending work".into())
        .map_err(|failure| *failure.error)?;
    let result = hook
        .compact_with_replay(request, None, CancellationToken::new())
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(provider.0.requests.lock().unwrap().len(), 2);
    assert_eq!(result.usages.len(), 1);
    assert!(result.settled_usage.is_none());
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_checkpoints")?, 1);
    Ok(())
}
