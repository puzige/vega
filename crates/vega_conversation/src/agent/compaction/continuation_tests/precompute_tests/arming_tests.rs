use super::*;
use crate::types::ContextPrecomputeRegistry;

#[tokio::test]
async fn automatic_preparation_deduplicates_pending_and_ready_and_wait_cancellation_does_not_cancel_generation()
-> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::global();
    let admission = crate::thread_access::acquire(&store, "thread")?;
    let lease = registry
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("lease")?;
    let provider = BackgroundProvider::new("success", true);
    let hook =
        ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
            .with_thread_admission(admission.clone())
            .with_precompute_lease(Some(lease));
    hook.arm_precompute(request.clone(), CancellationToken::new())
        .await;
    tokio::time::timeout(Duration::from_secs(10), provider.0.started.notified()).await?;
    let pending = registry
        .pending(store.database_identity(), "thread", &admission)?
        .ok_or("pending")?;
    let generation = pending.generation().to_owned();
    hook.arm_precompute(request.clone(), CancellationToken::new())
        .await;
    assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    assert!(matches!(
        hook.consume_precompute_until(
            &request,
            None,
            &CancellationToken::new(),
            tokio::time::Instant::now() + Duration::from_millis(25),
        )
        .await,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    let cancel = CancellationToken::new();
    let waiter = hook.consume_precompute(&request, None, &cancel);
    tokio::pin!(waiter);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut waiter)
            .await
            .is_err()
    );
    cancel.cancel();
    assert!(matches!(waiter.await, Err(VegaError::Cancelled)));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), pending.wait())
            .await
            .is_err()
    );
    provider.0.proceed.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), pending.wait())
            .await?
            .is_ok()
    );
    hook.arm_precompute(request.clone(), CancellationToken::new())
        .await;
    assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?
            .ok_or("ready")?
            .generation,
        generation
    );
    let result = hook
        .consume_precompute(&request, None, &CancellationToken::new())
        .await?
        .ok_or("not consumed")?;
    assert!(result.usages.is_empty());
    assert!(result.settled_usage.is_some());
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_checkpoints")?, 1);
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    Ok(())
}

#[tokio::test]
async fn failed_pending_falls_back_only_after_exit_and_changed_source_never_installs() -> TestResult
{
    for changed in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        let mut request = input(&store)?;
        append(&store)?;
        let source = load_source(store.conn(), "thread")?;
        request.messages =
            crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
        request.source_version = source.source_version;
        request.source_fingerprint = Some(source.fingerprint);
        request.estimate =
            estimate_chat_context(&request.system_prompt, &request.messages, &request.tools)?;
        let registry = ContextPrecomputeRegistry::global();
        let admission = crate::thread_access::acquire(&store, "thread")?;
        let lease = registry
            .bind(store.database_identity(), "thread", &admission)?
            .ok_or("lease")?;
        let background = BackgroundProvider::new("failure", true);
        let background_hook = ConversationCompactionHook::new(
            &background,
            path.clone(),
            "thread",
            "model",
            None,
            None,
        )
        .with_thread_admission(admission.clone())
        .with_precompute_lease(Some(lease));
        background_hook
            .arm_precompute(request.clone(), CancellationToken::new())
            .await;
        tokio::time::timeout(Duration::from_secs(10), background.0.started.notified()).await?;
        let primary = BackgroundProvider::new("success", false);
        let foreground_hook =
            ConversationCompactionHook::new(&primary, path, "thread", "model", None, None)
                .with_thread_admission(admission);
        let cancel = CancellationToken::new();
        let waiting = foreground_hook.compact(request, cancel);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut waiting)
                .await
                .is_err()
        );
        assert!(primary.0.requests.lock().unwrap().is_empty());
        if changed {
            store
                .conn()
                .execute("UPDATE messages SET content='changed' WHERE id='task'", [])?;
        }
        background.0.proceed.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(10), waiting).await?;
        if changed {
            assert!(
                matches!(result, Err(failure) if matches!(*failure.error, VegaError::Context(ContextRuntimeError::SourceChanged)))
            );
            assert!(primary.0.requests.lock().unwrap().is_empty());
            assert_eq!(count(&store, "context_checkpoints")?, 0);
        } else {
            let result = result.map_err(|failure| *failure.error)?;
            assert_eq!(result.usages.len(), 1);
            assert!(result.settled_usage.is_none());
            assert_eq!(primary.0.requests.lock().unwrap().len(), 1);
            assert_eq!(count(&store, "context_checkpoints")?, 1);
        }
        assert_eq!(count(&store, "token_usage")?, 1);
        assert_eq!(count(&store, "context_precompute_stages")?, 1);
        assert_eq!(count(&store, "context_precompute_ready")?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn empty_current_owner_can_prepare_complete_history_but_not_edited_input() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let mut request = input(&store)?;
    append(&store)?;
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "new-owner".into(),
            thread_id: "thread".into(),
            seq: 4,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 4,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    let source = load_source(store.conn(), "thread")?;
    request.messages =
        crate::agent::pipeline::history_from_context_source(&source, "new-owner", None)?;
    request.require_source_fence = false;
    request.source_owner_id = Some("new-owner".into());
    let provider = BackgroundProvider::new("success", false);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    assert!(
        hook.prepare_precompute(request.clone(), CancellationToken::new())
            .await?
            .is_some()
    );
    request.messages[0].content = "not persisted".into();
    assert!(matches!(
        hook.prepare_precompute(request, CancellationToken::new())
            .await,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert!(provider.0.requests.lock().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn final_text_capture_requires_the_complete_durable_owner_suffix() -> TestResult {
    for mismatch in ["none", "changed", "missing"] {
        let (_directory, path, store) = source_fixture()?;
        let mut request = input(&store)?;
        store.conn().execute(
            "UPDATE messages SET content='Final complete response.' WHERE id='owner'",
            [],
        )?;
        let source = load_source(store.conn(), "thread")?;
        request.messages =
            crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
        request.require_source_fence = false;
        request.source_owner_id = Some("owner".into());
        if mismatch == "changed" {
            request.messages.last_mut().unwrap().content = "not persisted".into();
        } else if mismatch == "missing" {
            let removed = request.messages.pop().unwrap();
            assert_eq!(removed.content, "Final complete response.");
        }
        let provider = BackgroundProvider::new("success", false);
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
        let prepared = hook
            .prepare_precompute(request, CancellationToken::new())
            .await;
        if mismatch != "none" {
            assert!(matches!(
                prepared,
                Err(VegaError::Context(ContextRuntimeError::SourceChanged))
            ));
            assert!(provider.0.requests.lock().unwrap().is_empty());
        } else {
            let mut worker = prepared?.ok_or("not prepared")?.launch()?;
            tokio::time::timeout(Duration::from_secs(10), worker.wait()).await??;
            assert_eq!(count(&store, "context_precompute_ready")?, 1);
            assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
        }
    }
    Ok(())
}
