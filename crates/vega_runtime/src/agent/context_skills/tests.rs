use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn summary() -> ChatMessage {
    let mut message = ChatMessage::new(ChatRole::User, "historical summary");
    message.attach_context_skill_runs(vec!["recent".into(), "older".into()]);
    message
}

fn skill(name: &str) -> ActiveSkillSummary {
    ActiveSkillSummary {
        name: name.into(),
        source_label: "approved-source".into(),
        content_sha256: "a".repeat(64),
        source_scope: crate::skills::SourceScope::Project,
    }
}

#[tokio::test]
async fn history_skills_are_bounded_filtered_and_do_not_activate_execution() -> TestResult {
    let reader: Arc<crate::skills::HistoricalSkillReader> = Arc::new(|run| {
        if run == "older" {
            vec![(skill("same"), "old duplicate body".into())]
        } else {
            let mut items = vec![(skill("same"), "current body synthetic-private-value".into())];
            items.push((skill("oversized"), "x".repeat(20001)));
            for index in 0..6 {
                items.push((skill(&format!("bounded-{index}")), "z".repeat(20000)));
            }
            items
        }
    });
    let config = RuntimeToolConfig::default()
        .with_context_skill_reader(reader)
        .with_credential_reader(Arc::new(|| Ok(vec!["synthetic-private-value".into()])));
    let mut messages = vec![ChatMessage::new(ChatRole::System, "system"), summary()];
    let original = messages.clone();
    restore_context_skills(&mut messages, &config, &CancellationToken::new()).await?;
    assert_eq!(messages[1].without_runtime_metadata(), original[1]);
    assert!(config.active_skill_summaries()?.is_empty());
    let projected = crate::project_model_messages(&messages)?;
    assert_eq!(crate::project_model_messages(&projected)?, projected);
    let observations = projected[1]
        .content
        .lines()
        .skip(1)
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(observations.len(), 8);
    assert!(
        observations
            .iter()
            .all(|value| value["vega_history_observation"]["authority"] == "none")
    );
    assert!(!projected[1].content.contains("synthetic-private-value"));
    assert!(!projected[1].content.contains("old duplicate body"));
    assert!(projected[1].content.contains("compact_skill_reference"));
    let bodies = observations
        .iter()
        .filter_map(|value| value["vega_history_observation"]["content"].as_str())
        .collect::<Vec<_>>();
    assert!(bodies.iter().all(|body| body.len().div_ceil(4) <= 5000));
    assert!(
        bodies
            .iter()
            .map(|body| body.len().div_ceil(4))
            .sum::<usize>()
            <= 25000
    );
    let base = crate::estimate_wire_context(&original, &[])?.input_tokens;
    assert!(crate::estimate_wire_context(&messages, &[])?.input_tokens > base);
    super::super::context_files::fit_context_files(&mut messages, &[], base)?;
    assert_eq!(
        crate::estimate_wire_context(&messages, &[])?.input_tokens,
        base
    );
    let unavailable = RuntimeToolConfig::default()
        .with_context_skill_reader(Arc::new(|_| vec![(skill("private"), "body".into())]))
        .with_credential_reader(Arc::new(|| Err(())));
    assert!(
        restore_context_skills(&mut messages, &unavailable, &CancellationToken::new())
            .await
            .is_err()
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        restore_context_skills(&mut messages, &config, &cancel).await,
        Err(VegaError::Cancelled)
    ));
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
async fn real_warm_candidate_drops_skill_body_after_current_authority_revocation() -> TestResult {
    let directory = Directory(
        std::env::temp_dir().join(format!("vega-context-skills-{}", ulid::Ulid::generate())),
    );
    std::fs::create_dir(&directory.0)?;
    let tools = vega_tools::Tools::new(&directory.0)?;
    let revoked = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&revoked);
    let reader: Arc<crate::skills::HistoricalSkillReader> = Arc::new(move |run| {
        if observed.load(Ordering::SeqCst) || run != "recent" {
            Vec::new()
        } else {
            vec![(skill("historical"), "old frozen skill body".into())]
        }
    });
    let provider = CapturingProvider(Mutex::new(Vec::new()));
    let build = |history, frame| AgentRequest {
        model: "synthetic".into(),
        system_prompt: "system".into(),
        history,
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default().with_context_skill_reader(Arc::clone(&reader)),
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
    revoked.store(true, Ordering::SeqCst);
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
    assert!(wire(&requests[0]).contains("old frozen skill body"));
    assert!(wire(&requests[1]).contains("warm-only-marker"));
    assert!(!wire(&requests[1]).contains("old frozen skill body"));
    assert!(!wire(&requests[1]).contains("restored_skill"));
    Ok(())
}
