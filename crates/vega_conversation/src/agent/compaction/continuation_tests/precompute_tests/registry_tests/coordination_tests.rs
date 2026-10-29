use super::*;

fn next_input(store: &Store) -> Result<ContextCompactionRequest, Box<dyn std::error::Error>> {
    for (original, id, seq, content) in [
        ("task", "next-task", 3, "Continue the remaining work."),
        ("owner", "next-owner", 4, "Another complete response."),
    ] {
        let mut row = messages::find(store.conn(), original)?.ok_or("source row")?;
        row.id = id.into();
        row.seq = seq;
        row.content = content.into();
        row.status = "done".into();
        messages::insert(store.conn(), &row)?;
    }
    let source = load_source(store.conn(), "thread")?;
    let checkpoint = latest_checkpoint(store.conn(), "thread", "model")?;
    let messages = crate::agent::pipeline::history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "__none__",
    )?;
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    Ok(ContextCompactionRequest {
        trigger: ContextCompactionTrigger::Threshold,
        system_prompt: "system".into(),
        estimate: estimate_chat_context("system", &messages, &[])?,
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens()?,
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint),
        require_source_fence: true,
        source_owner_id: None,
    })
}

#[tokio::test]
async fn foreground_install_cancels_its_generation_but_rearming_waits_for_real_exit() -> TestResult
{
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::global();
    let lease = bind(&registry, &store)?;
    let late_provider = BackgroundProvider::new("success", false);
    let late = prepare(&path, &request, &late_provider).await?;
    let state = Arc::new(RuntimeDrainState {
        failure: false,
        started: tokio::sync::Notify::new(),
        blocked: tokio::sync::Notify::new(),
        released: AtomicBool::new(false),
        gate: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
    });
    let release = ReleaseGate {
        gate: state.gate.clone(),
    };
    let provider = RuntimeDrainProvider(state.clone());
    let task = lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("task")?;
    tokio::time::timeout(Duration::from_secs(10), state.started.notified()).await?;
    tokio::time::timeout(Duration::from_secs(10), state.blocked.notified()).await?;
    let primary = BackgroundProvider::new("success", false);
    let hook =
        ConversationCompactionHook::new(&primary, path.clone(), "thread", "model", None, None)
            .with_full_on_threshold(true);
    let (generated, guard) = hook
        .generate_compaction(request, CancellationToken::new(), &mut None)
        .await
        .map_err(|failure| *failure.error)?;
    let result = hook
        .install_generated(generated, guard, CancellationToken::new())
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(result.usages.len(), 1);
    assert!(result.settled_usage.is_none());
    assert_eq!(primary.0.requests.lock().unwrap().len(), 1);
    assert_eq!(count(&store, "context_checkpoints")?, 1);
    assert!(ready::current_generation(store.conn(), "thread")?.is_none());
    assert!(lease.is_current()?);
    let mut late = late.launch()?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), late.wait()).await?,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert!(late_provider.0.requests.lock().unwrap().is_empty());
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let sealed: bool = store.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM context_precompute_accounting WHERE generation=?1 AND stage_count=1 AND usage_complete=1 AND retired=1)",
                [task.generation()], |row| row.get(0),
            ).map_err(VegaError::Store)?;
            if sealed { break Ok::<(), VegaError>(()); }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await??;
    assert!(!state.released.load(Ordering::Acquire));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), task.wait_for_exit())
            .await
            .is_err()
    );
    let admission = crate::thread_access::acquire(&store, "thread")?;
    assert_eq!(
        registry
            .pending(store.database_identity(), "thread", &admission)?
            .ok_or("pending")?
            .generation(),
        task.generation()
    );
    drop(admission);
    let next_request = next_input(&store)?;
    let next_provider = BackgroundProvider::new("success", true);
    assert!(
        lease
            .try_launch(prepare(&path, &next_request, &next_provider).await?)?
            .is_none()
    );
    assert!(next_provider.0.requests.lock().unwrap().is_empty());
    release.release();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .as_ref()
            .map_err(|error| error.as_ref()),
        Err(VegaError::Cancelled)
    ));
    assert!(state.released.load(Ordering::Acquire));
    assert!(lease.is_current()?);
    let next = lease
        .try_launch(prepare(&path, &next_request, &next_provider).await?)?
        .ok_or("next task")?;
    tokio::time::timeout(Duration::from_secs(10), next_provider.0.started.notified()).await?;
    let old_cleanup =
        registry.cancel_generation(store.database_identity(), "thread", task.generation())?;
    assert!(old_cleanup.is_finished()?);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), next.wait())
            .await
            .is_err()
    );
    next_provider.0.proceed.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), next.wait())
            .await?
            .is_ok()
    );
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?
            .ok_or("ready")?
            .generation,
        next.generation()
    );
    assert_eq!(count(&store, "token_usage")?, 2);
    assert_eq!(count(&store, "context_precompute_stages")?, 2);
    assert_eq!(count(&store, "context_checkpoints")?, 1);
    assert_eq!(next_provider.0.requests.lock().unwrap().len(), 1);
    Ok(())
}
