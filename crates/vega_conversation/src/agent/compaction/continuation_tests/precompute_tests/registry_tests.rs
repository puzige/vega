use super::*;
mod coordination_tests;
mod lifecycle_tests;
use crate::types::{ContextPrecomputeLease, ContextPrecomputeRegistry, PreparedContextPrecompute};
use std::sync::atomic::{AtomicBool, Ordering};

fn bind(
    registry: &ContextPrecomputeRegistry,
    store: &Store,
) -> Result<ContextPrecomputeLease, Box<dyn std::error::Error>> {
    let admission = crate::thread_access::acquire(store, "thread")?;
    Ok(registry
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("lease missing")?)
}

async fn prepare(
    path: &std::path::Path,
    request: &ContextCompactionRequest,
    provider: &dyn Provider,
) -> Result<PreparedContextPrecompute, Box<dyn std::error::Error>> {
    Ok(
        ConversationCompactionHook::new(provider, path.to_owned(), "thread", "model", None, None)
            .prepare_precompute(request.clone(), CancellationToken::new())
            .await?
            .ok_or("not prepared")?,
    )
}

#[tokio::test]
async fn registry_waiters_do_not_own_generation_and_database_keys_are_isolated() -> TestResult {
    let (_first_directory, first_path, first) = source_fixture()?;
    let (_second_directory, second_path, second) = source_fixture()?;
    let first_request = input(&first)?;
    let second_request = input(&second)?;
    let registry = ContextPrecomputeRegistry::default();
    let first_lease = bind(&registry, &first)?;
    let second_lease = bind(&registry, &second)?;
    let first_provider = BackgroundProvider::new("success", true);
    let second_provider = BackgroundProvider::new("success", false);
    let first_task = first_lease
        .try_launch(prepare(&first_path, &first_request, &first_provider).await?)?
        .ok_or("first task missing")?;
    let generation = first_task.generation().to_owned();
    tokio::time::timeout(Duration::from_secs(10), first_provider.0.started.notified()).await?;
    assert!(crate::thread_access::acquire(&first, "thread").is_ok());
    assert!(
        first_lease
            .try_launch(prepare(&first_path, &first_request, &first_provider).await?)?
            .is_none()
    );
    assert!(
        first_lease
            .try_launch(prepare(&second_path, &second_request, &second_provider).await?)
            .is_err()
    );
    let second_task = second_lease
        .try_launch(prepare(&second_path, &second_request, &second_provider).await?)?
        .ok_or("second task missing")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), second_task.wait())
            .await?
            .is_ok()
    );
    assert!(ready::recover(second.conn(), "thread", now_ms())?.is_some());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), first_task.wait())
            .await
            .is_err()
    );
    let remaining_waiter = first_task.clone();
    drop(first_task);
    first_provider.0.proceed.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), remaining_waiter.wait())
            .await?
            .is_ok()
    );
    assert!(remaining_waiter.wait().await.is_ok());
    assert_eq!(
        ready::recover(first.conn(), "thread", now_ms())?
            .ok_or("ready missing")?
            .generation,
        generation
    );
    for store in [&first, &second] {
        assert_eq!(count(store, "token_usage")?, 1);
        assert_eq!(count(store, "context_precompute_stages")?, 1);
        assert_eq!(count(store, "context_checkpoints")?, 0);
    }
    assert_eq!(first_provider.0.requests.lock().unwrap().len(), 1);
    assert_eq!(second_provider.0.requests.lock().unwrap().len(), 1);
    Ok(())
}

struct ReleaseGate {
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

impl ReleaseGate {
    fn release(&self) {
        let (lock, changed) = &*self.gate;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}

impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.release();
    }
}

struct RuntimeDrainState {
    failure: bool,
    started: tokio::sync::Notify,
    blocked: tokio::sync::Notify,
    released: AtomicBool,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

struct RuntimeDrainProvider(Arc<RuntimeDrainState>);

impl Provider for RuntimeDrainProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let state = self.0.clone();
        Some(Box::new(move || Ok(Arc::new(Self(state)))))
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(request.tools.is_empty());
        let state = self.0.clone();
        Box::pin(async move {
            let resource = state.clone();
            drop(tokio::task::spawn_blocking(move || {
                let (lock, changed) = &*resource.gate;
                let released = lock.lock().unwrap();
                resource.blocked.notify_one();
                let _released = changed.wait_while(released, |released| !*released).unwrap();
                resource.released.store(true, Ordering::Release);
            }));
            let events: vega_runtime::EventStream = Box::pin(
                futures::stream::iter([Ok(ProviderEvent::Usage {
                    input: 100,
                    output: 10,
                    cache_read: None,
                    cache_write: None,
                })])
                .chain(futures::stream::poll_fn(move |_| {
                    state.started.notify_one();
                    if state.failure {
                        std::task::Poll::Ready(Some(Err(VegaError::Provider {
                            status: Some(503),
                            message: "Synthetic unavailable".into(),
                            retryable: false,
                        })))
                    } else {
                        std::task::Poll::Pending
                    }
                })),
            );
            Ok(events)
        })
    }
}

#[tokio::test]
async fn invalidation_keeps_slot_until_accounting_and_background_runtime_exit() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let lease = bind(&registry, &store)?;
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
        .ok_or("task missing")?;
    tokio::time::timeout(Duration::from_secs(10), state.started.notified()).await?;
    tokio::time::timeout(Duration::from_secs(10), state.blocked.notified()).await?;
    let drain = registry.invalidate(store.database_identity(), "thread")?;
    assert!(!drain.is_finished()?);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), drain.wait())
            .await
            .is_err()
    );
    let replacement_lease = bind(&registry, &store)?;
    let replacement = BackgroundProvider::new("success", false);
    assert!(
        lease
            .try_launch(prepare(&path, &request, &replacement).await?)?
            .is_none()
    );
    assert!(
        replacement_lease
            .try_launch(prepare(&path, &request, &replacement).await?)?
            .is_none()
    );
    assert!(!state.released.load(Ordering::Acquire));
    release.release();
    tokio::time::timeout(Duration::from_secs(10), drain.wait()).await??;
    assert!(state.released.load(Ordering::Acquire));
    assert!(matches!(
        task.wait().await.as_ref().map_err(|error| error.as_ref()),
        Err(VegaError::Cancelled)
    ));
    let (stages, complete, retired): (u32, bool, bool) = store.conn().query_row(
        "SELECT stage_count,usage_complete,retired FROM context_precompute_accounting WHERE generation=?1",
        [task.generation()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((stages, complete, retired), (1, true, true));
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    assert!(
        lease
            .try_launch(prepare(&path, &request, &replacement).await?)?
            .is_none()
    );
    let replacement_task = replacement_lease
        .try_launch(prepare(&path, &request, &replacement).await?)?
        .ok_or("replacement blocked after drain")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), replacement_task.wait())
            .await?
            .is_ok()
    );
    assert_eq!(count(&store, "token_usage")?, 2);
    assert_eq!(count(&store, "context_precompute_ready")?, 1);
    assert_eq!(replacement.0.requests.lock().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn late_counted_failure_cannot_spend_the_replacement_epochs_failure_budget() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let lease = bind(&registry, &store)?;
    let state = Arc::new(RuntimeDrainState {
        failure: true,
        started: tokio::sync::Notify::new(),
        blocked: tokio::sync::Notify::new(),
        released: AtomicBool::new(false),
        gate: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
    });
    let release = ReleaseGate {
        gate: state.gate.clone(),
    };
    let provider = RuntimeDrainProvider(state.clone());
    let old = lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("old task missing")?;
    tokio::time::timeout(Duration::from_secs(10), state.started.notified()).await?;
    tokio::time::timeout(Duration::from_secs(10), state.blocked.notified()).await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let settled: bool = store.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM context_precompute_accounting WHERE generation=?1 AND stage_count=1 AND usage_complete=1 AND retired=1)",
                [old.generation()], |row| row.get(0),
            )?;
            if settled { return Ok::<_, Box<dyn std::error::Error>>(()); }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await??;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), old.wait())
            .await
            .is_err()
    );
    let drain = registry.invalidate(store.database_identity(), "thread")?;
    let fresh = bind(&registry, &store)?;
    assert!(!drain.is_finished()?);
    release.release();
    tokio::time::timeout(Duration::from_secs(10), drain.wait()).await??;
    for _ in 0..2 {
        assert!(matches!(
            old.wait().await.as_ref().map_err(|error| error.as_ref()),
            Err(VegaError::Provider {
                status: Some(503),
                ..
            })
        ));
    }
    let failure = BackgroundProvider::new("failure", false);
    for index in 0..3 {
        let task = fresh
            .try_launch(prepare(&path, &request, &failure).await?)?
            .ok_or("old failure spent new budget")?;
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(10), task.wait())
                .await?
                .as_ref()
                .map_err(|error| error.as_ref()),
            Err(VegaError::Provider {
                status: Some(503),
                ..
            })
        ));
        assert!(old.wait().await.is_err());
        assert_eq!(count(&store, "token_usage")?, index + 2);
    }
    assert!(
        fresh
            .try_launch(prepare(&path, &request, &failure).await?)?
            .is_none()
    );
    assert_eq!(failure.0.requests.lock().unwrap().len(), 3);
    assert_eq!(count(&store, "context_precompute_stages")?, 4);
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    Ok(())
}

#[tokio::test]
async fn overlapping_pauses_revoke_old_leases_and_close_never_reopens() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let lease = bind(&registry, &store)?;
    let provider = BackgroundProvider::new("success", false);
    let task = lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("task missing")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_ok()
    );
    let candidate = ready::recover(store.conn(), "thread", now_ms())?.ok_or("ready missing")?;
    let database_pause = registry.pause_database(store.database_identity())?;
    let thread_pause = registry.pause_thread(store.database_identity(), "thread")?;
    let second_database_pause = registry.pause_database(store.database_identity())?;
    assert!(database_pause.drain().is_finished()?);
    let admission = crate::thread_access::acquire(&store, "thread")?;
    for wrong_thread in ["other-thread", ""] {
        assert!(
            registry
                .bind(store.database_identity(), wrong_thread, &admission)
                .is_err()
        );
    }
    assert!(
        registry
            .bind(store.database_identity(), "thread", &admission)?
            .is_none()
    );
    drop(database_pause);
    drop(thread_pause);
    assert!(
        registry
            .bind(store.database_identity(), "thread", &admission)?
            .is_none()
    );
    drop(second_database_pause);
    let current = registry
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("pause did not release")?;
    drop(admission);
    assert!(
        lease
            .try_launch(prepare(&path, &request, &provider).await?)?
            .is_none()
    );
    let closed = registry.close()?;
    assert!(closed.is_finished()?);
    assert!(bind(&registry, &store).is_err());
    assert!(
        current
            .try_launch(prepare(&path, &request, &provider).await?)?
            .is_none()
    );
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?,
        Some(candidate)
    );
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn counted_precompute_failures_stop_at_three_and_success_resets_only_its_epoch() -> TestResult
{
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let lease = bind(&registry, &store)?;
    let failure = BackgroundProvider::new("failure", false);
    let success = BackgroundProvider::new("success", false);
    for (index, succeeds) in [false, false, true, false, false, false]
        .into_iter()
        .enumerate()
    {
        let task = lease
            .try_launch(
                prepare(&path, &request, if succeeds { &success } else { &failure }).await?,
            )?
            .ok_or("gate closed before three consecutive failures")?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), task.wait())
                .await?
                .is_ok(),
            succeeds,
        );
        assert_eq!(count(&store, "token_usage")?, index as i64 + 1);
    }
    assert!(
        lease
            .try_launch(prepare(&path, &request, &success).await?)?
            .is_none()
    );
    assert!(
        bind(&registry, &store)?
            .try_launch(prepare(&path, &request, &success).await?)?
            .is_none()
    );
    registry
        .invalidate(store.database_identity(), "thread")?
        .wait()
        .await?;
    let fresh = bind(&registry, &store)?;
    let task = fresh
        .try_launch(prepare(&path, &request, &success).await?)?
        .ok_or("fresh epoch blocked")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_ok()
    );
    assert_eq!(count(&store, "token_usage")?, 7);
    assert_eq!(count(&store, "context_precompute_ready")?, 1);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    Ok(())
}

#[tokio::test]
async fn cancellation_and_source_invalidation_do_not_trip_precompute_failure_gate() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let lease = bind(&registry, &store)?;
    let cancelled = BackgroundProvider::new("cancel", false);
    for _ in 0..4 {
        let task = lease
            .try_launch(prepare(&path, &request, &cancelled).await?)?
            .ok_or("cancellation counted as failure")?;
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(10), task.wait())
                .await?
                .as_ref()
                .map_err(|error| error.as_ref()),
            Err(VegaError::Cancelled)
        ));
    }
    let success = BackgroundProvider::new("success", false);
    for _ in 0..4 {
        let prepared = prepare(&path, &request, &success).await?;
        store.conn().execute(
            "UPDATE threads SET pending_model='next' WHERE id='thread'",
            [],
        )?;
        let task = lease
            .try_launch(prepared)?
            .ok_or("source change counted as failure")?;
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(10), task.wait())
                .await?
                .as_ref()
                .map_err(|error| error.as_ref()),
            Err(VegaError::Context(ContextRuntimeError::SourceChanged))
        ));
        store.conn().execute(
            "UPDATE threads SET pending_model=NULL WHERE id='thread'",
            [],
        )?;
    }
    let task = lease
        .try_launch(prepare(&path, &request, &success).await?)?
        .ok_or("uncounted outcomes exhausted gate")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_ok()
    );
    assert_eq!(count(&store, "token_usage")?, 5);
    assert_eq!(count(&store, "context_precompute_ready")?, 1);
    assert_eq!(cancelled.0.requests.lock().unwrap().len(), 4);
    assert_eq!(success.0.requests.lock().unwrap().len(), 1);
    Ok(())
}
