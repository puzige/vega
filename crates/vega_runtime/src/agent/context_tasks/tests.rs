use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn summary() -> ChatMessage {
    let mut message = ChatMessage::new(ChatRole::User, "historical summary");
    message.attach_context_task_sources(vec!["source-call".into()]);
    message
}

#[tokio::test]
async fn task_status_is_filtered_optional_and_never_claims_liveness_or_result_delivery()
-> TestResult {
    let config = RuntimeToolConfig::default()
        .with_context_task_reader(Arc::new(|_| {
            Some(HistoricalTask {
                agent_id: "synthetic-private-value".into(),
                message_id: "execution".into(),
                state: "completed".into(),
                release_state: "held".into(),
            })
        }))
        .with_credential_reader(Arc::new(|| Ok(vec!["synthetic-private-value".into()])));
    let mut messages = vec![ChatMessage::new(ChatRole::System, "system"), summary()];
    let original = messages.clone();
    restore_context_tasks(&mut messages, &config, &CancellationToken::new()).await?;
    assert_eq!(messages[1].without_runtime_metadata(), original[1]);
    let projected = crate::project_model_messages(&messages)?;
    assert_eq!(crate::project_model_messages(&projected)?, projected);
    let value: Value =
        serde_json::from_str(projected[1].content.lines().last().ok_or("observation")?)?;
    let observation = &value["vega_history_observation"];
    assert_eq!(observation["authority"], "none");
    assert_eq!(observation["persisted_state"], "completed");
    assert_eq!(observation["release_state"], "held");
    assert_eq!(observation["worker_liveness"], "unknown");
    assert_eq!(observation["includes_result"], false);
    assert!(!projected[1].content.contains("synthetic-private-value"));
    let base = crate::estimate_wire_context(&original, &[])?.input_tokens;
    assert!(crate::estimate_wire_context(&messages, &[])?.input_tokens > base);
    super::super::context_files::fit_context_files(&mut messages, &[], base)?;
    assert_eq!(
        crate::estimate_wire_context(&messages, &[])?.input_tokens,
        base
    );
    assert_eq!(messages[1].without_runtime_metadata(), original[1]);
    let unavailable = config.clone().with_credential_reader(Arc::new(|| Err(())));
    assert!(
        restore_context_tasks(&mut messages, &unavailable, &CancellationToken::new())
            .await
            .is_err()
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        restore_context_tasks(&mut messages, &config, &cancel).await,
        Err(VegaError::Cancelled)
    ));
    let oversized = config.with_context_task_reader(Arc::new(|_| {
        Some(HistoricalTask {
            agent_id: "x".repeat(2049),
            message_id: "execution".into(),
            state: "running".into(),
            release_state: "held".into(),
        })
    }));
    restore_context_tasks(&mut messages, &oversized, &CancellationToken::new()).await?;
    assert_eq!(
        crate::estimate_wire_context(&messages, &[])?.input_tokens,
        base
    );
    Ok(())
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct CapturingProvider(Mutex<Vec<ChatRequest>>);
impl Provider for CapturingProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("continued".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as crate::EventStream)
        })
    }
}

#[tokio::test]
async fn warm_task_status_refreshes_current_execution_and_drops_deleted_sources() -> TestResult {
    for missing in [false, true] {
        let directory = Directory(
            std::env::temp_dir().join(format!("vega-context-tasks-{}", ulid::Ulid::generate())),
        );
        std::fs::create_dir(&directory.0)?;
        let tools = vega_tools::Tools::new(&directory.0)?;
        let state = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&state);
        let reader: Arc<HistoricalTaskReader> = Arc::new(move |_| {
            let state = observed.load(Ordering::SeqCst);
            (state != 2).then(|| HistoricalTask {
                agent_id: "child".into(),
                message_id: "exact-execution".into(),
                state: if state == 0 { "running" } else { "completed" }.into(),
                release_state: if state == 0 { "held" } else { "released" }.into(),
            })
        });
        let provider = CapturingProvider(Mutex::new(Vec::new()));
        let build = |history, frame| AgentRequest {
            model: "synthetic".into(),
            system_prompt: "system".into(),
            history,
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: RuntimeToolConfig::default().with_context_task_reader(Arc::clone(&reader)),
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: Some(frame),
            context_budget: Some(crate::ContextBudget::new(100000, 10000, true).unwrap()),
            context_source_version: Some(1),
            context_source_fingerprint: Some("source".into()),
            context_operation_id: Some("owner".into()),
            context_compaction_hook: None,
        };
        let mut frame = crate::ReplayFrame::new(2, crate::ReasoningReplayPolicy::Unknown);
        frame.attach_route_fingerprint([7; 32]);
        let first = run_agent(
            &provider,
            &tools,
            build(vec![summary()], frame),
            CancellationToken::new(),
        )
        .await?;
        assert!(!first.failed);
        let environment = first.messages[0]
            .replay_frame()
            .and_then(crate::ReplayFrame::environment_fingerprint)
            .ok_or("environment")?;
        let mut warm = first.messages;
        warm.push(ChatMessage::new(ChatRole::Assistant, "warm-only-marker"));
        state.store(if missing { 2 } else { 1 }, Ordering::SeqCst);
        let mut frame = crate::ReplayFrame::new(2, crate::ReasoningReplayPolicy::Unknown);
        frame.attach_route_fingerprint([7; 32]);
        frame.attach_candidate(warm, environment);
        let second = run_agent(
            &provider,
            &tools,
            build(
                vec![summary(), ChatMessage::new(ChatRole::User, "continue")],
                frame,
            ),
            CancellationToken::new(),
        )
        .await?;
        assert!(!second.failed);
        let requests = provider.0.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let wire = |request: &ChatRequest| {
            request
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(wire(&requests[0]).contains("running"));
        let current = wire(&requests[1]);
        assert!(current.contains("warm-only-marker"));
        assert!(!current.contains("running"));
        if missing {
            assert!(!current.contains("delegated_task_status"));
        } else {
            assert!(current.contains("completed"));
            assert!(current.contains("released"));
            assert!(current.contains("exact-execution"));
        }
        assert_eq!(second.executed_tool_call_count, 0);
    }
    Ok(())
}
