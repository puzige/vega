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
) -> Result<vega_runtime::MicroCompactionRequest, Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(90_000, 20_000, true)?;
    Ok(vega_runtime::MicroCompactionRequest {
        context: ContextCompactionRequest {
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
        },
        replay_frame: None,
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
        .compact(
            request(&installed, immediate)?.context,
            CancellationToken::new(),
        )
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
        request.context.system_prompt = "large system context ".repeat(12_000);
        request.context.estimate = estimate_wire_context(&request.wire_messages(), &[])?;
        assert!(
            request.context.estimate.input_tokens >= request.context.budget.precompute_tokens()?
        );
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

#[tokio::test]
async fn micro_accepts_partial_retained_prefix_and_disjoint_history_with_a_live_owner() -> TestResult
{
    use crate::types::{PartialCompactionDirection as Direction, PartialCompactionSelection};
    for disjoint in [false, true] {
        let (_directory, path, store) = fixture(40)?;
        store
            .conn()
            .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
        let mut row = messages::find(store.conn(), "owner")?.ok_or("owner missing")?;
        row.id = "later-task".into();
        row.seq = 99;
        row.role = "user".into();
        row.content = "Summarize the tail while keeping the earlier results".into();
        messages::insert(store.conn(), &row)?;
        row.id = "tail".into();
        row.seq = 100;
        row.role = "assistant".into();
        row.content = "TAIL_TO_SUMMARIZE".into();
        messages::insert(store.conn(), &row)?;
        let source = load_source(store.conn(), "thread")?;
        let provider = ScriptedSummaryProvider::new([
            SummaryReply::Text("SUMMARY_TAIL".into()),
            SummaryReply::Text("SUMMARY_TASK".into()),
        ]);
        let mut projected = history(&source)?;
        let mut selections = vec![("tail", Direction::From)];
        if disjoint {
            selections.push(("owner", Direction::UpTo));
        }
        for (message_id, direction) in selections {
            let mut context = request(&source, projected)?.context;
            context.trigger = ContextCompactionTrigger::Manual;
            context.source_owner_id = None;
            context.budget = ContextBudget::new(1_000_000, 20_000, true)?;
            context.target_tokens = context.budget.target_tokens()?;
            projected = ConversationCompactionHook::new(
                &provider,
                path.clone(),
                "thread",
                "model",
                None,
                None,
            )
            .with_partial_selection(PartialCompactionSelection {
                message_id: message_id.into(),
                source_fingerprint: source.fingerprint.clone(),
                direction,
            })
            .compact(context, CancellationToken::new())
            .await
            .map_err(|failure| *failure.error)?
            .messages;
        }
        let checkpoint =
            latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
        assert!(checkpoint.covered_through_seq >= 2);
        assert_eq!(checkpoint.covered_through_seq, 98);
        row.id = "followup".into();
        row.seq = 101;
        row.role = "user".into();
        row.content = "Continue the same task".into();
        messages::insert(store.conn(), &row)?;
        row.id = "live".into();
        row.seq = 102;
        row.role = "assistant".into();
        row.status = "streaming".into();
        row.content.clear();
        messages::insert(store.conn(), &row)?;
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "live-call",
                thread_id: "thread",
                message_id: "live",
                seq: 103,
                tool: "read",
                input_json: "{\"path\":\"live.txt\",\"offset\":null,\"limit\":null}",
                status: "success",
                created_at: 103,
            },
        )?;
        store.conn().execute("UPDATE tool_calls SET output_text='LIVE_RESULT', text_offset_bytes=0 WHERE id='live-call'", [])?;
        store.conn().execute("INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('live','live-call','live-response')", [])?;
        let source = load_source(store.conn(), "thread")?;
        let mut visible = source.clone();
        visible
            .messages
            .iter_mut()
            .find(|message| message.id == "live")
            .ok_or("live owner missing")?
            .status = "done".into();
        let original = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &visible,
            Some(&checkpoint),
            "__none__",
        )?;
        assert!(
            original
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some("call-1"))
        );
        let mut input = request(&source, original.clone())?;
        input.context.require_source_fence = false;
        input.context.source_owner_id = Some("live".into());
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
        let mut changed = input.clone();
        changed
            .context
            .messages
            .iter_mut()
            .find(|message| message.tool_call_id.as_deref() == Some("call-1"))
            .ok_or("retained result missing")?
            .content
            .push_str(" tampered");
        assert!(matches!(
            hook.micro_compact(changed, CancellationToken::new()).await,
            Err(VegaError::Context(ContextRuntimeError::SourceChanged))
        ));
        assert_eq!(rows(&store)?, 0);
        let result = hook
            .micro_compact(input, CancellationToken::new())
            .await?
            .ok_or("retained prefix not compacted")?;
        assert_eq!(
            result
                .targets
                .iter()
                .map(|target| target.call_id.as_str())
                .collect::<Vec<_>>(),
            (1..=36)
                .map(|round| format!("call-{round}"))
                .collect::<Vec<_>>()
        );
        let mut applied = original.clone();
        vega_runtime::apply_micro_compaction(&mut applied, &result.targets)?;
        assert_eq!(
            applied
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("live-call")),
            original
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("live-call"))
        );
        assert_eq!(rows(&store)?, 36);
        assert_eq!(
            latest_checkpoint(store.conn(), "thread", "model")?
                .ok_or("checkpoint lost")?
                .id,
            checkpoint.id
        );
        assert_eq!(provider.requests().len(), if disjoint { 2 } else { 1 });
    }
    Ok(())
}

#[tokio::test]
async fn micro_validates_live_screenshot_overlays_without_clearing_them() -> TestResult {
    for count in [6, 40] {
        let (_directory, path, store) = fixture(count)?;
        let screenshot_id = format!("call-{}", count + 1);
        insert_round(&store, count + 1)?;
        let receipt = serde_json::json!({
            "status": "delivered", "artifact_id": "synthetic-artifact",
            "delivery": "conversation_image_attachment", "saved_in_history": true,
            "read_only": true, "unchanged": false,
            "next_step": "use_delivered_image"
        })
        .to_string();
        store.conn().execute(
            "UPDATE tool_calls SET tool='ComputerScreenshot', input_json='{\"target_id\":\"synthetic-window\"}', output_text=?1 WHERE id=?2",
            (&receipt, &screenshot_id),
        )?;
        let source = load_source(store.conn(), "thread")?;
        let mut live = history(&source)?;
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1).write_to(&mut encoded, image::ImageFormat::Png)?;
        let screenshot = live
            .iter_mut()
            .find(|message| message.tool_call_id.as_deref() == Some(screenshot_id.as_str()))
            .ok_or("screenshot missing")?;
        screenshot.content =
            format!("{receipt}\nCurrent window image and valid observation target");
        screenshot
            .images
            .push(vega_runtime::ImageAttachment::from_bytes(
                encoded.into_inner(),
            )?);
        let original = live.clone();
        let mut input = request(&source, live.clone())?;
        input.context.require_source_fence = false;
        input.context.system_prompt = "large system context ".repeat(12_000);
        input.context.estimate = estimate_wire_context(&input.wire_messages(), &[])?;
        assert!(input.context.estimate.input_tokens >= input.context.budget.precompute_tokens()?);
        let provider = ScriptedSummaryProvider::new([]);
        let result =
            ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
                .micro_compact(input, CancellationToken::new())
                .await?;
        if count == 40 {
            let result = result.ok_or("unrelated results not compacted")?;
            assert_eq!(result.targets.len(), 35);
            assert!(
                !result
                    .targets
                    .iter()
                    .any(|target| target.call_id == screenshot_id)
            );
            vega_runtime::apply_micro_compaction(&mut live, &result.targets)?;
            assert_eq!(rows(&store)?, 35);
        } else {
            assert!(result.is_none());
            assert_eq!(rows(&store)?, 0);
            assert_eq!(live, original);
        }
        let preserved = live
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(screenshot_id.as_str()))
            .ok_or("screenshot lost")?;
        assert_eq!(
            Some(preserved),
            original
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some(screenshot_id.as_str()))
        );
        assert_eq!(preserved.images.len(), 1);
        assert!(preserved.content.contains("Current window image"));
        assert_eq!(
            load_source(store.conn(), "thread")?
                .tool_calls
                .last()
                .ok_or("stored screenshot missing")?
                .output_text
                .as_deref(),
            Some(receipt.as_str())
        );
        assert!(provider.requests().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn micro_persistence_gate_and_savings_use_the_framed_glm_wire() -> TestResult {
    let (_directory, path, store) = fixture(40)?;
    let source = load_source(store.conn(), "thread")?;
    let mut original = history(&source)?;
    original
        .iter_mut()
        .find(|message| message.role == ChatRole::Assistant)
        .ok_or("assistant missing")?
        .reasoning_content = Some("suppressed historical reasoning ".repeat(20_000));
    let mut input = request(&source, original.clone())?;
    input.replay_frame = Some(vega_runtime::ReplayFrame::new(
        original.len() + 1,
        vega_runtime::ReasoningReplayPolicy::GlmInterleaved,
    ));
    let before = estimate_wire_context(&input.wire_messages(), &[])?.input_tokens;
    input.context.budget = ContextBudget::new((before + 20_000) * 2 + 20_000, 20_000, true)?;
    input.context.target_tokens = input.context.budget.target_tokens()?;
    input.context.estimate = estimate_wire_context(&input.wire_messages(), &[])?;
    assert!(before < input.context.budget.precompute_tokens()?);
    assert!(
        estimate_chat_context("system", &original, &[])?.input_tokens
            >= input.context.budget.precompute_tokens()?
    );
    let provider = ScriptedSummaryProvider::new([]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    assert!(
        hook.micro_compact(input.clone(), CancellationToken::new())
            .await?
            .is_none()
    );
    assert_eq!(rows(&store)?, 0);
    assert_eq!(load_source(store.conn(), "thread")?, source);
    input.context.budget = ContextBudget::new(90_000, 20_000, true)?;
    input.context.target_tokens = input.context.budget.target_tokens()?;
    assert!(before >= input.context.budget.precompute_tokens()?);
    let result = hook
        .micro_compact(input.clone(), CancellationToken::new())
        .await?
        .ok_or("framed result not compacted")?;
    let mut after = input.wire_messages();
    vega_runtime::apply_micro_compaction(&mut after[1..], &result.targets)?;
    assert_eq!(
        result.estimated_savings,
        before - estimate_wire_context(&after, &[])?.input_tokens
    );
    assert!(result.estimated_savings >= vega_runtime::MICRO_MIN_SAVINGS);
    assert_eq!(rows(&store)?, 35);
    assert_eq!(
        after
            .iter()
            .find(|message| message.role == ChatRole::Assistant)
            .and_then(|message| message.reasoning_content.as_deref()),
        original
            .iter()
            .find(|message| message.role == ChatRole::Assistant)
            .and_then(|message| message.reasoning_content.as_deref())
    );
    assert!(provider.requests().is_empty());
    Ok(())
}
