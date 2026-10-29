use super::*;

struct BorrowingProvider {
    inner: BackgroundProvider,
    snapshot: vega_runtime::model_compat::ResolvedModel,
}

impl Provider for BorrowingProvider {
    fn model_snapshot(&self) -> Option<vega_runtime::model_compat::ResolvedModel> {
        Some(self.snapshot.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert_eq!(request.tool_choice, Some(vega_runtime::ToolChoice::None));
        assert!(
            request
                .messages
                .iter()
                .any(|message| message.content.contains("Continue the implementation."))
        );
        assert!(
            !request
                .messages
                .iter()
                .any(|message| message.content.contains("older tool data"))
        );
        self.inner.0.requests.lock().unwrap().push(request);
        Box::pin(async {
            Ok(Box::pin(futures::stream::iter([
                Ok(ProviderEvent::Usage {
                    input: 25,
                    output: 5,
                    cache_read: None,
                    cache_write: None,
                }),
                Ok(ProviderEvent::TextDelta("BORROWED_FORK_SUMMARY".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as vega_runtime::EventStream)
        })
    }
}

#[tokio::test]
async fn foreground_summary_fork_borrows_ready_without_installing_or_rebilling_it() -> TestResult {
    use vega_runtime::model_compat::{
        ModelApi, ModelIdentity, ModelRoute, lookup_model_definition, resolve_model,
    };
    let (_directory, path, store) = source_fixture()?;
    store
        .conn()
        .execute("UPDATE threads SET model='gpt-4.1' WHERE id='thread'", [])?;
    let request = input(&store)?;
    let background = BackgroundProvider::new("success", false);
    let initial =
        ConversationCompactionHook::new(&background, path.clone(), "thread", "gpt-4.1", None, None);
    publish(&initial, request.clone()).await?;
    let candidate = ready::recover(store.conn(), "thread", now_ms())?.ok_or("ready missing")?;
    let definition = lookup_model_definition(
        &ModelRoute::new(ModelApi::Responses, "https://api.openai.com/v1")?,
        &ModelIdentity {
            provider: "openai".into(),
            model: "gpt-4.1".into(),
        },
    )?;
    let provider = BorrowingProvider {
        inner: background.clone(),
        snapshot: resolve_model(&definition, None)?,
    };
    let admission = crate::thread_access::acquire(&store, "thread")?;
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "gpt-4.1", None, None)
        .with_thread_admission(admission)
        .with_full_on_threshold(true);
    let (generated, _guard) = hook
        .generate_compaction(request, CancellationToken::new(), &mut None)
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(generated.checkpoint.summary, "BORROWED_FORK_SUMMARY");
    assert_eq!(generated.result.usages.len(), 1);
    assert_eq!(generated.result.usages[0].usage.input, 25);
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?,
        Some(candidate)
    );
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    assert_eq!(background.0.requests.lock().unwrap().len(), 2);
    Ok(())
}

#[tokio::test]
async fn borrowed_ready_preserves_candidate_source_and_accounting_for_later_consumption()
-> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("success", false);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    publish(&hook, request.clone()).await?;
    append(&store)?;
    let request = current_input(&store, request)?;
    let candidate = ready::recover(store.conn(), "thread", now_ms())?.ok_or("candidate missing")?;
    let source = load_source(store.conn(), "thread")?;
    for _ in 0..2 {
        let result = hook
            .borrow_precompute_until(
                &request,
                None,
                &CancellationToken::new(),
                tokio::time::Instant::now() + SUMMARY_OPERATION_TIMEOUT,
            )
            .await?
            .ok_or("borrow missing")?;
        assert!(
            result
                .messages
                .iter()
                .any(|message| message.content == "Keep this later instruction.")
        );
        assert!(result.usages.is_empty());
        assert_eq!(
            ready::recover(store.conn(), "thread", now_ms())?,
            Some(candidate.clone())
        );
        assert_eq!(count(&store, "context_checkpoints")?, 0);
        assert_eq!(count(&store, "token_usage")?, 1);
        assert_eq!(count(&store, "context_precompute_stages")?, 1);
        assert_eq!(load_source(store.conn(), "thread")?, source);
    }
    let installed = hook
        .consume_precompute(&request, None, &CancellationToken::new())
        .await?
        .ok_or("consume missing")?;
    assert!(installed.usages.is_empty());
    assert_eq!(count(&store, "context_checkpoints")?, 1);
    assert_eq!(count(&store, "token_usage")?, 1);
    assert!(ready::recover(store.conn(), "thread", now_ms())?.is_none());
    assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn cancelled_borrow_wait_leaves_the_background_generation_running() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("success", true);
    let admission = crate::thread_access::acquire(&store, "thread")?;
    let registry = crate::types::ContextPrecomputeRegistry::global();
    let lease = registry
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("lease missing")?;
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_thread_admission(admission);
    let prepared = hook
        .prepare_precompute(request.clone(), CancellationToken::new())
        .await?
        .ok_or("not prepared")?;
    let task = lease.try_launch(prepared)?.ok_or("not launched")?;
    tokio::time::timeout(Duration::from_secs(10), provider.0.started.notified()).await?;
    let cancel = CancellationToken::new();
    let waiting = hook.borrow_precompute_until(
        &request,
        None,
        &cancel,
        tokio::time::Instant::now() + SUMMARY_OPERATION_TIMEOUT,
    );
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut waiting)
            .await
            .is_err()
    );
    cancel.cancel();
    assert!(matches!(waiting.await, Err(VegaError::Cancelled)));
    assert!(lease.is_current()?);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), task.wait_for_exit())
            .await
            .is_err()
    );
    provider.0.proceed.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_ok()
    );
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?
            .ok_or("ready missing")?
            .generation,
        task.generation()
    );
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    Ok(())
}
