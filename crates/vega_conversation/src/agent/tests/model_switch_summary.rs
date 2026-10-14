use super::*;
use vega_store::context_compaction::{self, ContextCheckpointInstall, NewContextCheckpoint};

fn install_summary(
    store: &Store,
    model: &str,
    summary: &str,
    previous: Option<i64>,
) -> context_compaction::ContextCheckpoint {
    store
        .conn()
        .execute(
            "UPDATE threads SET model = ?1 WHERE id = 'thread-1'",
            [model],
        )
        .unwrap();
    let source = context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let new = NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: model.into(),
        source_version: source.source_version,
        covered_through_seq: source.source_version.saturating_sub(1),
        covered_assistant_id: None,
        covered_assistant_text_bytes: 0,
        covered_assistant_tool_seq: 0,
        source_fingerprint: source.fingerprint,
        summary: summary.into(),
        estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: previous,
        created_at: 1,
    };
    match context_compaction::install_checkpoint(store.conn(), &new).unwrap() {
        ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
        other => panic!("expected checkpoint install, got {other:?}"),
    }
}

fn final_provider() -> MockProvider {
    MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("continuation done".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])])
}

#[tokio::test]
async fn switch_model_reuses_goal_progress_without_replaying_large_covered_history_and_switches_back_to_latest()
 {
    let (store, dir, _) = setup();
    let raw = format!(
        "COVERED_RAW_OUTPUT{}",
        "large historical tool output ".repeat(30_000)
    );
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'TASK_GOAL implement project loading', 'done', 1), ('covered', 'thread-1', 2, 'assistant', 'text', ?1, 'done', 2), ('tail', 'thread-1', 3, 'user', 'text', 'UNCOVERED_CORRECTION preserve keyboard access', 'done', 3)", [&raw]).unwrap();
    let first = install_summary(
        &store,
        "gpt-model",
        "TASK_GOAL implement project loading; PROGRESS first stage finished; pending keyboard support",
        None,
    );
    let original_source = context_compaction::load_source(store.conn(), "thread-1").unwrap();
    store
        .conn()
        .execute(
            "UPDATE threads SET model = 'deepseek-model' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    context_compaction::save_settings(
        store.conn(),
        &context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "deepseek-model".into(),
            context_limit: Some(32_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let projection =
        read_context_projection(&store, "thread-1", "deepseek-model", "system").unwrap();
    assert!(projection.estimated_tokens.unwrap() < 32_000);
    assert_eq!(
        projection.settings.as_ref().unwrap().model,
        "deepseek-model"
    );
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = final_provider();
    run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "continue on DeepSeek",
        "current system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "deepseek-model");
    let contents = requests[0]
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(contents.contains("TASK_GOAL"));
    assert!(contents.contains("PROGRESS first stage finished"));
    assert!(contents.contains("UNCOVERED_CORRECTION"));
    assert!(!contents.contains("COVERED_RAW_OUTPUT"));
    assert!(!contents.contains("large historical tool output"));
    assert!(
        requests[0]
            .messages
            .iter()
            .filter(|m| m.content.contains("TASK_GOAL"))
            .all(|m| m.role != vega_runtime::ChatRole::System)
    );
    let current_source = context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(
        &current_source.messages[..3],
        original_source.messages.as_slice()
    );
    let second = install_summary(
        &store,
        "deepseek-model",
        "TASK_GOAL implement project loading; NEWEST_PROGRESS keyboard support completed; pending review",
        Some(first.id),
    );
    assert_ne!(first.id, second.id);
    store
        .conn()
        .execute(
            "UPDATE threads SET model = 'gpt-model' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    let switched_back = Store::open(dir.path().join("vega.db")).unwrap();
    let provider = final_provider();
    run_thread_task(
        &switched_back,
        &provider,
        &tools,
        "thread-1",
        "review latest work",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "gpt-model");
    let contents = requests[0]
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(contents.contains("NEWEST_PROGRESS keyboard support completed"));
    assert!(!contents.contains("PROGRESS first stage finished"));
    assert!(!contents.contains("COVERED_RAW_OUTPUT"));
}

#[tokio::test]
async fn smaller_model_recompacts_prior_summary_without_new_raw_history() {
    let (store, _dir, _) = setup();
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'COVERED_RAW_TASK', 'done', 1), ('continue', 'thread-1', 2, 'user', 'text', 'continue', 'done', 2)", []).unwrap();
    let prior_text = format!(
        "TASK_GOAL; PROGRESS_DONE; PENDING_WORK; {}",
        "historical detail ".repeat(800)
    );
    let prior = install_summary(&store, "gpt-model", &prior_text, None);
    store
        .conn()
        .execute(
            "UPDATE threads SET model = 'deepseek-model' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    let source_before = context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("TASK_GOAL; PROGRESS_DONE; PENDING_WORK".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let budget = vega_runtime::ContextBudget::new(16_000, 1_000, false).unwrap();
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "deepseek-model",
        "system",
        Vec::new(),
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let latest = context_compaction::latest_checkpoint(store.conn(), "thread-1", "gpt-model")
        .unwrap()
        .unwrap();
    assert_ne!(latest.id, prior.id);
    assert_eq!(latest.model, "deepseek-model");
    assert_eq!(latest.covered_through_seq, prior.covered_through_seq);
    assert!(latest.summary.len() < prior.summary.len());
    assert_eq!(
        context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        source_before
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let text = requests[0]
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("TASK_GOAL"));
    assert!(text.contains("PROGRESS_DONE"));
    assert!(text.contains("PENDING_WORK"));
    assert!(!text.contains("COVERED_RAW_TASK"));
}

#[tokio::test]
async fn smaller_target_model_automatically_compacts_then_sends_primary_within_budget() {
    let (store, dir, _) = setup();
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'COVERED_RAW_TASK', 'done', 1), ('tail', 'thread-1', 2, 'user', 'text', 'continue pending work', 'done', 2)", []).unwrap();
    let prior_text = format!(
        "TASK_GOAL; PROGRESS_DONE; PENDING_WORK; {}",
        "historical progress detail ".repeat(12_000)
    );
    let prior = install_summary(&store, "gpt-model", &prior_text, None);
    store
        .conn()
        .execute(
            "UPDATE threads SET model = 'deepseek-model' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    let initial_estimate = read_context_projection(&store, "thread-1", "deepseek-model", "system")
        .unwrap()
        .estimated_tokens
        .unwrap();
    let input_budget = initial_estimate * 115 / 100;
    let budget = vega_runtime::ContextBudget::new(input_budget + 1_000, 1_000, true).unwrap();
    assert!(initial_estimate >= budget.trigger_tokens().unwrap());
    assert!(initial_estimate < budget.input_budget());
    context_compaction::save_settings(
        store.conn(),
        &context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "deepseek-model".into(),
            context_limit: Some(input_budget + 1_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("TASK_GOAL; PROGRESS_DONE; PENDING_WORK".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("continued pending work".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "continue",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].tools.is_empty());
    assert!(!requests[1].tools.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| request.model == "deepseek-model")
    );
    for request in &requests {
        let text = request
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("TASK_GOAL"));
        assert!(text.contains("PROGRESS_DONE"));
        assert!(text.contains("PENDING_WORK"));
        assert!(!text.contains("COVERED_RAW_TASK"));
    }
    let primary = &requests[1];
    let estimate =
        vega_runtime::estimate_chat_context("", &primary.messages, &primary.tools).unwrap();
    assert!(estimate.input_tokens <= budget.input_budget());
    assert!(estimate.input_tokens <= budget.target_tokens().unwrap());
    assert!(
        !primary
            .messages
            .iter()
            .any(|m| m.content.contains("historical progress detail"))
    );
    let latest = context_compaction::latest_checkpoint(store.conn(), "thread-1", "gpt-model")
        .unwrap()
        .unwrap();
    assert_ne!(latest.id, prior.id);
    assert_eq!(latest.model, "deepseek-model");
}
