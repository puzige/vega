use super::*;
use reactive_tests::{ScriptedSummaryProvider, SummaryReply};
use vega_store::context_compaction::{ContextSource, latest_checkpoint, load_source};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(count: i64) -> Result<(DatabaseDirectory, PathBuf, Store), Box<dyn std::error::Error>> {
    let (directory, path, store) = source_fixture()?;
    for round in 3..=count {
        insert_round(&store, round)?;
    }
    for round in 1..=count {
        store.conn().execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id=?2",
            (
                format!("RESULT_{round} {}", format!("detail-{round} ").repeat(1500)),
                format!("call-{round}"),
            ),
        )?;
    }
    store.conn().execute(
        "UPDATE tool_calls SET status='failed' WHERE id='call-1'",
        [],
    )?;
    Ok((directory, path, store))
}

fn history(source: &ContextSource) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
    let mut visible = source.clone();
    for message in &mut visible.messages {
        if message.status == "streaming" {
            message.status = "done".into();
        }
    }
    Ok(crate::agent::pipeline::history_from_context_source(
        &visible, "__none__", None,
    )?)
}

fn request(
    source: &ContextSource,
    messages: Vec<ChatMessage>,
) -> Result<ContextCompactionRequest, Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(90_000, 20_000, true)?;
    Ok(ContextCompactionRequest {
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
        source_owner_id: Some("owner".into()),
    })
}

fn rows(store: &Store) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(store.conn().query_row(
        "SELECT COUNT(*) FROM context_micro_compactions",
        [],
        |row| row.get(0),
    )?)
}

#[tokio::test]
async fn micro_keeps_five_results_persists_once_and_remains_compacted_after_summary() -> TestResult
{
    let (_directory, path, store) = fixture(40)?;
    let source = load_source(store.conn(), "thread")?;
    let original = history(&source)?;
    let before = estimate_chat_context("system", &original, &[])?.input_tokens;
    let provider = ScriptedSummaryProvider::new([SummaryReply::Text("AFTER_MICRO_SUMMARY".into())]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let result = hook
        .micro_compact(
            request(&source, original.clone())?,
            CancellationToken::new(),
        )
        .await?
        .ok_or("micro did not compact")?;
    assert_eq!(
        result
            .targets
            .iter()
            .map(|target| target.call_id.clone())
            .collect::<Vec<_>>(),
        (1..=35)
            .map(|round| format!("call-{round}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(result.targets[0].status, "failed");
    assert!(result.estimated_savings >= vega_runtime::MICRO_MIN_SAVINGS);
    assert_eq!(rows(&store)?, 35);
    let mut immediate = original.clone();
    vega_runtime::apply_micro_compaction(&mut immediate, &result.targets)?;
    assert_eq!(
        before - estimate_chat_context("system", &immediate, &[])?.input_tokens,
        result.estimated_savings
    );
    for call in 36..=40 {
        let id = format!("call-{call}");
        assert_eq!(
            immediate
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some(id.as_str())),
            original
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some(id.as_str()))
        );
    }
    let installed = load_source(store.conn(), "thread")?;
    assert_ne!(installed.fingerprint, source.fingerprint);
    assert_eq!(installed.fingerprint, result.source_fingerprint);
    assert_eq!(installed.messages, source.messages);
    assert_eq!(installed.images, source.images);
    for (before, after) in source.tool_calls.iter().zip(&installed.tool_calls) {
        let mut expected = before.clone();
        expected.micro_compacted = after.micro_compacted;
        assert_eq!(expected, *after);
    }
    assert!(durable_messages_equal(
        &immediate.iter().collect::<Vec<_>>(),
        &history(&installed)?.iter().collect::<Vec<_>>()
    ));
    assert!(
        hook.micro_compact(
            request(&installed, immediate.clone())?,
            CancellationToken::new()
        )
        .await?
        .is_none()
    );
    assert_eq!(rows(&store)?, 35);
    let summary = hook
        .compact(request(&installed, immediate)?, CancellationToken::new())
        .await
        .map_err(|failure| *failure.error)?;
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let input = requests[0]
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(input.contains("vega_compacted_tool_result"));
    assert!(!input.contains("detail-1 "));
    let checkpoint =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
    let mut visible = installed.clone();
    for message in &mut visible.messages {
        if message.status == "streaming" {
            message.status = "done".into();
        }
    }
    let reloaded = crate::agent::pipeline::history_from_context_source_with_checkpoint(
        &visible,
        Some(&checkpoint),
        "__none__",
    )?;
    assert!(durable_messages_equal(
        &summary.messages.iter().collect::<Vec<_>>(),
        &reloaded.iter().collect::<Vec<_>>()
    ));
    assert_eq!(load_source(store.conn(), "thread")?, installed);
    Ok(())
}

#[tokio::test]
async fn micro_noops_below_savings_gate_and_never_clears_unapproved_tool_kinds() -> TestResult {
    for ineligible in [false, true] {
        let (_directory, path, store) = fixture(if ineligible { 40 } else { 6 })?;
        if ineligible {
            store
                .conn()
                .execute("UPDATE tool_calls SET tool='write'", [])?;
        }
        let source = load_source(store.conn(), "thread")?;
        let mut request = request(&source, history(&source)?)?;
        request.system_prompt = "large system context ".repeat(12_000);
        request.estimate = estimate_chat_context(&request.system_prompt, &request.messages, &[])?;
        assert!(request.estimate.input_tokens >= request.budget.precompute_tokens()?);
        let provider = ScriptedSummaryProvider::new([]);
        let result =
            ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
                .micro_compact(request, CancellationToken::new())
                .await?;
        assert!(result.is_none());
        assert_eq!(rows(&store)?, 0);
        assert_eq!(load_source(store.conn(), "thread")?, source);
        assert!(provider.requests().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn micro_cancellation_and_source_changes_cannot_install_and_mutations_invalidate_marks()
-> TestResult {
    let (_directory, path, store) = fixture(40)?;
    let source = load_source(store.conn(), "thread")?;
    let provider = ScriptedSummaryProvider::new([]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        hook.micro_compact(request(&source, history(&source)?)?, cancel)
            .await,
        Err(VegaError::Cancelled)
    ));
    assert_eq!(rows(&store)?, 0);
    store.conn().execute(
        "UPDATE tool_calls SET output_text=output_text || ' edited' WHERE id='call-1'",
        [],
    )?;
    assert!(matches!(
        vega_store::context_micro::install(
            store.conn(),
            &source,
            "model",
            None,
            &["call-1".into()],
            1,
            || false
        ),
        Err(vega_store::context_compaction::ContextCheckpointError::Stale)
    ));
    let current = load_source(store.conn(), "thread")?;
    assert!(matches!(
        vega_store::context_micro::install(
            store.conn(),
            &current,
            "model",
            None,
            &["call-1".into()],
            1,
            || true
        ),
        Err(vega_store::context_compaction::ContextCheckpointError::Cancelled)
    ));
    assert_eq!(rows(&store)?, 0);
    vega_store::context_micro::install(
        store.conn(),
        &current,
        "model",
        None,
        &["call-1".into()],
        1,
        || false,
    )?;
    assert!(load_source(store.conn(), "thread")?.tool_calls[0].micro_compacted);
    store.conn().execute(
        "UPDATE tool_calls SET output_text='REDACTED_CHANGED' WHERE id='call-1'",
        [],
    )?;
    let changed = load_source(store.conn(), "thread")?;
    assert!(!changed.tool_calls[0].micro_compacted);
    assert!(
        history(&changed)?
            .iter()
            .any(|message| message.content == "REDACTED_CHANGED")
    );
    store
        .conn()
        .execute("DELETE FROM tool_calls WHERE id='call-1'", [])?;
    assert_eq!(rows(&store)?, 0);
    assert!(provider.requests().is_empty());
    Ok(())
}
