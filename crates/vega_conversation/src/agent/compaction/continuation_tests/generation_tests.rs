use super::*;

mod fork_tests;
use reactive_tests::{ScriptedSummaryProvider, SummaryReply};
use vega_store::context_compaction::{ContextSource, latest_checkpoint, load_source};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn input(
    store: &Store,
) -> Result<(ContextSource, ContextCompactionRequest), Box<dyn std::error::Error>> {
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
    let source = load_source(store.conn(), "thread")?;
    let messages = crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    let request = ContextCompactionRequest {
        trigger: ContextCompactionTrigger::Threshold,
        system_prompt: "system".into(),
        estimate: estimate_chat_context("system", &messages, &[])?,
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens()?,
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint.clone()),
        require_source_fence: true,
        source_owner_id: None,
    };
    Ok((source, request))
}

fn checkpoints(store: &Store) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(store.conn().query_row(
        "SELECT COUNT(*) FROM context_checkpoints WHERE thread_id='thread'",
        [],
        |row| row.get(0),
    )?)
}

#[tokio::test]
async fn generated_summary_installs_only_explicitly_and_matches_reload() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let (source, request) = input(&store)?;
    let original = request.messages.clone();
    let provider = ScriptedSummaryProvider::new([SummaryReply::Text("GENERATED_SUMMARY".into())]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_full_on_threshold(true);
    let mut source_guard = None;
    let (generated, guard) = hook
        .generate_compaction(request, CancellationToken::new(), &mut source_guard)
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(checkpoints(&store)?, 0);
    assert!(latest_checkpoint(store.conn(), "thread", "model")?.is_none());
    assert_eq!(load_source(store.conn(), "thread")?, source);
    assert_eq!(
        crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?,
        original
    );
    assert_eq!(
        source_guard.as_ref().ok_or("guard missing")?.fingerprint,
        source.fingerprint
    );
    assert_eq!(generated.checkpoint.summary, "GENERATED_SUMMARY");
    assert_eq!(generated.result.usages.len(), 1);
    assert_eq!(generated.result.usages[0].usage.input, 100);
    assert_eq!(generated.result.usages[0].usage.output, 10);
    assert!(generated.result.usage_complete);
    assert!(crate::thread_access::acquire(&store, "thread").is_err());
    let expected = generated.result.clone();
    let result = hook
        .install_generated(generated, guard, CancellationToken::new())
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(checkpoints(&store)?, 1);
    let checkpoint =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
    assert_eq!(checkpoint.summary, "GENERATED_SUMMARY");
    let reloaded = crate::agent::pipeline::history_from_context_source_with_checkpoint(
        &source,
        Some(&checkpoint),
        "__none__",
    )?;
    assert_eq!(result.messages, expected.messages);
    assert_eq!(result.messages, reloaded);
    assert_eq!(result.usages, expected.usages);
    assert_eq!(
        result.source_fingerprint.as_deref(),
        Some(source.fingerprint.as_str())
    );
    assert_eq!(load_source(store.conn(), "thread")?, source);
    assert!(crate::thread_access::acquire(&store, "thread").is_ok());
    assert_eq!(provider.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn delayed_install_rejects_changed_authority_and_preserves_generated_usage() -> TestResult {
    for change in [
        "cancel",
        "deadline",
        "edit",
        "append",
        "model",
        "predecessor",
        "database",
    ] {
        let (_directory, path, store) = source_fixture()?;
        let (source, request) = input(&store)?;
        let provider = ScriptedSummaryProvider::new([SummaryReply::Text("STALE_SUMMARY".into())]);
        let mut hook =
            ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
                .with_full_on_threshold(true);
        let (generated, mut guard) = hook
            .generate_compaction(request, CancellationToken::new(), &mut None)
            .await
            .map_err(|failure| *failure.error)?;
        let expected_usages = generated.result.usages.clone();
        let cancel = CancellationToken::new();
        let mut expected_count = 0;
        let mut replacement = None;
        match change {
            "cancel" => cancel.cancel(),
            "deadline" => guard.deadline = tokio::time::Instant::now(),
            "edit" => {
                store.conn().execute(
                    "UPDATE messages SET content='Changed task' WHERE id='task'",
                    [],
                )?;
                assert_eq!(
                    load_source(store.conn(), "thread")?.source_version,
                    source.source_version
                );
            }
            "append" => {
                let mut row = messages::find(store.conn(), "task")?.ok_or("task missing")?;
                row.id = "appended-task".into();
                row.seq = 100;
                row.content = "Additional task".into();
                messages::insert(store.conn(), &row)?;
            }
            "model" => {
                store.conn().execute(
                    "UPDATE threads SET model='other-model' WHERE id='thread'",
                    [],
                )?;
            }
            "predecessor" => {
                let mut competing = generated.checkpoint.clone();
                competing.summary = "COMPETING_SUMMARY".into();
                assert!(matches!(
                    vega_store::context_compaction::install_checkpoint(store.conn(), &competing)?,
                    ContextCheckpointInstall::Applied(_)
                ));
                expected_count = 1;
            }
            "database" => {
                let (directory, path, other) = source_fixture()?;
                let (other_source, _) = input(&other)?;
                assert_eq!(other_source, source);
                hook.database_path = path;
                replacement = Some((directory, other));
            }
            _ => unreachable!(),
        }
        let current_source = load_source(store.conn(), "thread")?;
        let failure = hook
            .install_generated(generated, guard, cancel)
            .await
            .expect_err("changed authority must reject install");
        match change {
            "cancel" => assert!(matches!(*failure.error, VegaError::Cancelled)),
            "deadline" => assert!(matches!(
                *failure.error,
                VegaError::Context(ContextRuntimeError::SummaryTimedOut)
            )),
            _ => assert!(matches!(
                *failure.error,
                VegaError::Context(ContextRuntimeError::SourceChanged)
            )),
        }
        assert_eq!(failure.usages, expected_usages);
        assert!(failure.usage_complete);
        assert_eq!(checkpoints(&store)?, expected_count);
        assert_eq!(load_source(store.conn(), "thread")?, current_source);
        if change == "predecessor" {
            assert_eq!(
                latest_checkpoint(store.conn(), "thread", "model")?
                    .ok_or("competitor lost")?
                    .summary,
                "COMPETING_SUMMARY"
            );
        }
        if let Some((_, other)) = &replacement {
            assert_eq!(checkpoints(other)?, 0);
            assert_eq!(load_source(other.conn(), "thread")?, source);
        }
        assert!(crate::thread_access::acquire(&store, "thread").is_ok());
        assert_eq!(provider.requests().len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn discarding_generated_summary_releases_admission_without_installing() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let (source, request) = input(&store)?;
    let provider = ScriptedSummaryProvider::new([SummaryReply::Text("DISCARDED_SUMMARY".into())]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_full_on_threshold(true);
    let generated = hook
        .generate_compaction(request, CancellationToken::new(), &mut None)
        .await
        .map_err(|failure| *failure.error)?;
    assert!(crate::thread_access::acquire(&store, "thread").is_err());
    assert_eq!(generated.0.result.usages.len(), 1);
    drop(generated);
    assert!(crate::thread_access::acquire(&store, "thread").is_ok());
    assert_eq!(checkpoints(&store)?, 0);
    assert_eq!(load_source(store.conn(), "thread")?, source);
    assert_eq!(provider.requests().len(), 1);
    Ok(())
}
