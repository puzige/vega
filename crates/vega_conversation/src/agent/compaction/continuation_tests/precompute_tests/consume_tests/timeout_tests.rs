use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vega_runtime::{AgentRequest, RuntimeEvent, RuntimeToolConfig};

struct ExpiredConsumption {
    provider: BackgroundProvider,
    path: PathBuf,
    reader: vega_runtime::CredentialReader,
}

impl ContextCompactionHook for ExpiredConsumption {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>>
    {
        self.compact_with_replay(request, None, cancel)
    }

    fn compact_with_replay<'a>(
        &'a self,
        request: ContextCompactionRequest,
        frame: Option<ReplayFrame>,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>>
    {
        Box::pin(async move {
            assert_eq!(request.trigger, ContextCompactionTrigger::Threshold);
            assert!(request.estimate.input_tokens < request.budget.blocking_tokens().unwrap());
            ConversationCompactionHook::new(
                &self.provider,
                self.path.clone(),
                "thread",
                "model",
                None,
                None,
            )
            .with_credential_reader(Some(self.reader.clone()))
            .consume_precompute_until(
                &request,
                frame.as_ref(),
                &cancel,
                tokio::time::Instant::now(),
            )
            .await
            .map_err(|error| failure(error, None))?
            .ok_or_else(|| failure(context_error(ContextRuntimeError::InvalidProjection), None))
        })
    }
}

#[derive(Default)]
struct PrimaryProvider(AtomicUsize);

impl Provider for PrimaryProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("Continued.".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        })
    }
}

fn mutate_after_capture(path: &std::path::Path, mutation: &str) {
    let store = Store::open(path).unwrap();
    match mutation {
        "unchanged" => {}
        "body" => {
            store
                .conn()
                .execute(
                    "UPDATE messages SET content='Changed task' WHERE id='task'",
                    [],
                )
                .unwrap();
        }
        "hidden_body" => {
            store
                .conn()
                .execute(
                    "UPDATE messages SET content='Changed hidden body' WHERE id='appended-task'",
                    [],
                )
                .unwrap();
        }
        "screenshot" => {
            store
                .conn()
                .execute(
                    "UPDATE image_attachments SET encoded=X'0908' WHERE message_id='live-owner'",
                    [],
                )
                .unwrap();
        }
        "model" => {
            store
                .conn()
                .execute(
                    "UPDATE threads SET model='changed-model' WHERE id='thread'",
                    [],
                )
                .unwrap();
        }
        "pending_model" => {
            store
                .conn()
                .execute(
                    "UPDATE threads SET pending_model='changed-model' WHERE id='thread'",
                    [],
                )
                .unwrap();
        }
        "predecessor" | "retained_predecessor" => {
            let transaction = store.conn().unchecked_transaction().unwrap();
            let prepared = vega_store::context_precompute::consume::prepare_in_transaction(
                &transaction,
                "thread",
                Some("live-owner"),
                now_ms(),
            )
            .unwrap()
            .unwrap();
            let checkpoint = prepared
                .checkpoint(
                    "model",
                    Some("live-owner"),
                    CONTEXT_ESTIMATOR_VERSION,
                    now_ms(),
                )
                .unwrap();
            transaction.commit().unwrap();
            if mutation == "predecessor" {
                assert!(matches!(
                    vega_store::context_compaction::install_checkpoint(store.conn(), &checkpoint)
                        .unwrap(),
                    ContextCheckpointInstall::Applied(_)
                ));
            } else {
                store.conn().execute(
                    "INSERT INTO context_checkpoints \
                     (thread_id,model,source_version,covered_through_seq,source_fingerprint,summary,estimator_version,expected_previous_id,created_at,covered_assistant_id,covered_assistant_text_bytes,covered_assistant_tool_seq,projection_plan) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                    (
                        checkpoint.thread_id.as_str(),
                        checkpoint.model.as_str(),
                        checkpoint.source_version as i64,
                        checkpoint.covered_through_seq as i64,
                        checkpoint.source_fingerprint.as_str(),
                        checkpoint.summary.as_str(),
                        checkpoint.estimator_version.as_str(),
                        checkpoint.expected_previous_id,
                        checkpoint.created_at,
                        checkpoint.covered_assistant_id.as_deref(),
                        checkpoint.covered_assistant_text_bytes as i64,
                        checkpoint.covered_assistant_tool_seq as i64,
                        checkpoint.projection_plan.as_ref().unwrap().encode().unwrap(),
                    ),
                ).unwrap();
                assert!(
                    ready::current_generation(store.conn(), "thread")
                        .unwrap()
                        .is_some()
                );
            }
        }
        _ => panic!("unknown mutation"),
    }
}

#[tokio::test]
async fn ready_timeout_revalidates_the_complete_snapshot_before_primary_recovery() -> TestResult {
    for mutation in [
        "unchanged",
        "body",
        "hidden_body",
        "screenshot",
        "model",
        "pending_model",
        "predecessor",
        "retained_predecessor",
    ] {
        let (_directory, path, store) = source_fixture()?;
        let mut request = input(&store)?;
        let provider = BackgroundProvider::new("success", false);
        let hook =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
        publish(&hook, request.clone()).await?;
        let candidate = ready::recover(store.conn(), "thread", now_ms())?.unwrap();
        append_live(&store)?;
        store.conn().execute_batch(
            "INSERT INTO historical_message_delivery(message_id,thread_id,model_content) VALUES('appended-task','thread','Keep this later instruction.');
             UPDATE tool_calls SET tool='ComputerScreenshot' WHERE id='live-call';
             INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('live-owner',0,X'0708');
             INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES('live-call','live-owner',0);"
        )?;
        let mut screenshot = [ChatMessage::assistant_with_tools(
            String::new(),
            vec![vega_runtime::ChatToolCall {
                id: "live-call".into(),
                name: "ComputerScreenshot".into(),
                input_json: serde_json::json!({"target_id":"synthetic-window"}).to_string(),
            }],
        )];
        vega_runtime::computer_use::redact_ephemeral_messages(&mut screenshot);
        let audit = &screenshot[0].tool_calls[0].input_json;
        assert!(
            vega_runtime::computer_use::ComputerUseAudit::valid_for_tool(
                "ComputerScreenshot",
                audit,
            )
        );
        let receipt = serde_json::json!({
            "status":"delivered",
            "artifact_id":"screenshot:live-call",
            "delivery":"conversation_image_attachment",
            "saved_in_history":true,
            "read_only":true,
            "unchanged":false,
            "next_step":"continue_remaining_requested_work_or_finish",
        })
        .to_string();
        store.conn().execute(
            "UPDATE tool_calls SET input_json=?1,output_text=?2 WHERE id='live-call'",
            [audit.as_str(), receipt.as_str()],
        )?;
        let source = load_source(store.conn(), "thread")?;
        let mut visible = source.clone();
        visible.messages.last_mut().unwrap().status = "done".into();
        request.messages =
            crate::agent::pipeline::history_from_context_source(&visible, "__none__", None)?;
        let config = RuntimeToolConfig::default();
        let tools = vega_runtime::tool_definitions(config.run_mode);
        let mut system = ChatMessage::new(ChatRole::System, request.system_prompt.clone());
        system.attach_replay_frame(ReplayFrame::new(
            request.messages.len() + 1,
            ReasoningReplayPolicy::Unknown,
        ));
        let mut wire = vec![system];
        wire.extend(request.messages.clone());
        let estimate = estimate_wire_context(&wire, &tools)?;
        let budget = ContextBudget::new(
            estimate.input_tokens + estimate.input_tokens / 10 + 1000,
            1000,
            true,
        )?;
        assert!(estimate.input_tokens >= budget.trigger_tokens()?);
        assert!(estimate.input_tokens < budget.blocking_tokens()?);
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        let mutation_path = path.clone();
        let reader: vega_runtime::CredentialReader = Arc::new(move || {
            assert_eq!(observed.fetch_add(1, Ordering::SeqCst), 0);
            mutate_after_capture(&mutation_path, mutation);
            Ok(Vec::new())
        });
        let agent_request = AgentRequest {
            model: "model".into(),
            system_prompt: request.system_prompt,
            history: request.messages,
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: config,
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: Some(budget),
            context_source_version: Some(source.source_version),
            context_source_fingerprint: Some(source.fingerprint.clone()),
            context_operation_id: Some("live-owner".into()),
            context_compaction_hook: Some(Arc::new(ExpiredConsumption {
                provider: provider.clone(),
                path,
                reader,
            })),
        };
        let primary = PrimaryProvider::default();
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let result =
            vega_runtime::run_agent(&primary, &tools, agent_request, CancellationToken::new())
                .await;
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "{mutation}: {:?}",
            result.as_ref().err()
        );
        if mutation == "unchanged" {
            let outcome = result?;
            assert!(!outcome.failed);
            assert!(outcome.events.iter().any(|event| matches!(
                event,
                RuntimeEvent::ContextCompactionStatusUpdated { status }
                    if status.failure == Some(vega_runtime::ContextCompactionStatusFailure::TimedOut)
            )));
        } else {
            assert!(
                matches!(
                    result,
                    Err(VegaError::Context(ContextRuntimeError::SourceChanged))
                ),
                "{mutation}"
            );
        }
        assert_eq!(
            primary.0.load(Ordering::SeqCst),
            usize::from(mutation == "unchanged"),
            "{mutation}"
        );
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1, "{mutation}");
        assert_eq!(
            count(&store, "context_checkpoints")?,
            i64::from(matches!(mutation, "predecessor" | "retained_predecessor")),
            "{mutation}"
        );
        assert_eq!(count(&store, "token_usage")?, 1, "{mutation}");
        assert_eq!(count(&store, "context_precompute_stages")?, 1, "{mutation}");
        assert_eq!(
            count(&store, "context_precompute_ready")?,
            i64::from(mutation != "predecessor"),
            "{mutation}"
        );
        assert_eq!(
            ready::current_generation(store.conn(), "thread")?,
            (mutation != "predecessor").then_some(candidate.generation.clone()),
            "{mutation}"
        );
        let retired: bool = store.conn().query_row(
            "SELECT retired FROM context_precompute_accounting WHERE generation=?1",
            [candidate.generation.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(retired, mutation == "predecessor", "{mutation}");
        if matches!(mutation, "hidden_body" | "screenshot") {
            let current = load_source(store.conn(), "thread")?;
            assert_eq!(current.source_version, source.source_version);
            assert_eq!(current.fingerprint, source.fingerprint);
            assert!(ready::recover(store.conn(), "thread", now_ms())?.is_some());
        }
    }
    Ok(())
}
