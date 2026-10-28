use super::*;

fn blocked_provider() -> (RuntimeDrainProvider, ReleaseGate) {
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
    (RuntimeDrainProvider(state), release)
}

async fn started(provider: &RuntimeDrainProvider) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), provider.0.started.notified()).await?;
    tokio::time::timeout(Duration::from_secs(10), provider.0.blocked.notified()).await?;
    Ok(())
}

#[tokio::test]
async fn owner_close_waits_for_runtime_exit_and_does_not_revoke_another_windows_lease() -> TestResult
{
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::default();
    let first = registry.owner();
    let second = registry.owner();
    let admission = crate::thread_access::acquire(&store, "thread")?;
    let first_lease = first
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("first lease")?;
    let second_lease = second
        .bind(store.database_identity(), "thread", &admission)?
        .ok_or("second lease")?;
    drop(admission);
    let (provider, release) = blocked_provider();
    let task = first_lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("first task")?;
    started(&provider).await?;
    let drain = first.close()?;
    assert!(!first_lease.is_current()?);
    assert!(second_lease.is_current()?);
    let admission = crate::thread_access::acquire(&store, "thread")?;
    assert!(
        first
            .bind(store.database_identity(), "thread", &admission)?
            .is_none()
    );
    assert!(second_lease.verifies(store.database_identity(), "thread", &admission)?);
    drop(admission);
    let (entered_tx, entered) = tokio::sync::oneshot::channel();
    let mut shutdown = tokio::task::spawn_blocking(move || {
        let _ = entered_tx.send(());
        drain.wait_blocking()
    });
    entered.await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(250), &mut shutdown)
            .await
            .is_err()
    );
    assert!(!provider.0.released.load(Ordering::Acquire));
    release.release();
    tokio::time::timeout(Duration::from_secs(10), &mut shutdown).await???;
    assert!(provider.0.released.load(Ordering::Acquire));
    assert!(matches!(
        task.wait().await.as_ref().map_err(|error| error.as_ref()),
        Err(VegaError::Cancelled)
    ));
    assert_eq!(count(&store, "token_usage")?, 1);
    let replacement = BackgroundProvider::new("success", true);
    let next = second_lease
        .try_launch(prepare(&path, &request, &replacement).await?)?
        .ok_or("replacement task")?;
    tokio::time::timeout(Duration::from_secs(10), replacement.0.started.notified()).await?;
    assert!(first.close()?.is_finished()?);
    assert!(second_lease.is_current()?);
    replacement.0.proceed.notify_one();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), next.wait())
            .await?
            .is_ok()
    );
    assert_eq!(count(&store, "token_usage")?, 2);
    assert_eq!(count(&store, "context_precompute_ready")?, 1);
    second.close()?.wait_blocking()?;
    assert_eq!(count(&store, "context_precompute_ready")?, 1);
    Ok(())
}

#[tokio::test]
async fn model_mutations_revoke_old_leases_but_preserve_settled_ready() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::global();
    let provider = BackgroundProvider::new("success", false);
    let lease = bind(&registry, &store)?;
    let task = lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("task")?;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_ok()
    );
    let generation = task.generation().to_owned();
    for mutation in 0..3 {
        let lease = bind(&registry, &store)?;
        match mutation {
            0 => {
                crate::threads::set_thread_model(&store, "thread", "alternate")?;
            }
            1 => {
                crate::threads::stage_thread_model(&store, "thread", "model")?;
            }
            _ => {
                let admission = crate::thread_access::acquire(&store, "thread")?;
                crate::threads::promote_thread_model(&store, "thread", "model", &admission)?;
            }
        }
        assert!(!lease.is_current()?);
        let ready_generation: String = store.conn().query_row(
            "SELECT generation FROM context_precompute_ready WHERE thread_id='thread'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(ready_generation, generation);
        assert_eq!(count(&store, "token_usage")?, 1);
    }
    let old = bind(&registry, &store)?;
    let drain = crate::threads::cancel_context_precompute(&store, "thread")?;
    assert!(drain.is_finished()?);
    assert!(!old.is_current()?);
    assert!(bind(&registry, &store)?.is_current()?);
    assert_eq!(
        ready::recover(store.conn(), "thread", now_ms())?
            .ok_or("ready lost")?
            .generation,
        generation
    );
    Ok(())
}

#[tokio::test]
async fn deletion_does_not_remove_transcript_until_background_settlement_and_exit() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let registry = ContextPrecomputeRegistry::global();
    let lease = bind(&registry, &store)?;
    let (provider, release) = blocked_provider();
    let task = lease
        .try_launch(prepare(&path, &request, &provider).await?)?
        .ok_or("task")?;
    started(&provider).await?;
    let messages = count(&store, "messages")?;
    assert!(matches!(
        crate::threads::delete_thread(&store, "thread"),
        Err(crate::types::ConversationError::ContextCleanupPending)
    ));
    assert!(!lease.is_current()?);
    assert_eq!(count(&store, "threads")?, 1);
    assert_eq!(count(&store, "messages")?, messages);
    assert!(!provider.0.released.load(Ordering::Acquire));
    release.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task.wait())
            .await?
            .is_err()
    );
    assert_eq!(count(&store, "token_usage")?, 1);
    crate::threads::delete_thread(&store, "thread")?;
    assert_eq!(count(&store, "threads")?, 0);
    assert_eq!(count(&store, "messages")?, 0);
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_precompute_stages")?, 1);
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    Ok(())
}

#[tokio::test]
async fn reset_session_and_direct_reset_cannot_acknowledge_a_background_worker_still_exiting()
-> TestResult {
    for session_mode in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        let config_path = path.with_extension("toml");
        vega_store::config::AppConfig::default().save_to(&config_path)?;
        let original_config = std::fs::read(&config_path)?;
        let intent = crate::permission_reset::prepare_reset(&store, &config_path)
            .map_err(|error| format!("{error:?}"))?;
        let registry = ContextPrecomputeRegistry::global();
        let lease = bind(&registry, &store)?;
        let (provider, release) = blocked_provider();
        let task = lease
            .try_launch(prepare(&path, &request, &provider).await?)?
            .ok_or("task")?;
        started(&provider).await?;
        let replay = crate::agent::ReplaySessionRegistry::default();
        if session_mode {
            let mut session = crate::permission_reset::PermissionResetSession::begin(
                path.clone(),
                config_path.clone(),
                intent,
            )
            .map_err(|error| format!("{error:?}"))?;
            assert!(matches!(
                session.complete(&replay, Duration::from_millis(20)),
                Err(crate::types::PermissionResetError::Busy)
            ));
            assert!(!session.completed());
            assert_eq!(std::fs::read(&config_path)?, original_config);
            assert!(!provider.0.released.load(Ordering::Acquire));
            release.release();
            assert!(
                tokio::time::timeout(Duration::from_secs(10), task.wait())
                    .await?
                    .is_err()
            );
            session
                .complete(&replay, Duration::from_secs(10))
                .map_err(|error| format!("{error:?}"))?;
            assert!(session.completed());
        } else {
            let pause = crate::permission_reset::pause_admission(&store)
                .map_err(|error| format!("{error:?}"))?;
            assert!(matches!(
                crate::permission_reset::reset_quiescent(
                    &store,
                    &config_path,
                    &intent,
                    &pause,
                    &replay
                ),
                Err(crate::types::PermissionResetError::Busy)
            ));
            assert_eq!(std::fs::read(&config_path)?, original_config);
            assert!(!vega_store::permission_contracts::load_state(store.conn())?.reset_pending);
            assert!(!provider.0.released.load(Ordering::Acquire));
            release.release();
            assert!(
                tokio::time::timeout(Duration::from_secs(10), task.wait())
                    .await?
                    .is_err()
            );
            crate::permission_reset::reset_quiescent(
                &store,
                &config_path,
                &intent,
                &pause,
                &replay,
            )
            .map_err(|error| format!("{error:?}"))?;
        }
        assert!(!lease.is_current()?);
        assert_eq!(count(&store, "token_usage")?, 1);
        assert_eq!(count(&store, "context_precompute_stages")?, 1);
        assert_eq!(count(&store, "context_precompute_ready")?, 0);
        assert!(
            vega_store::permission_reset::load(store.conn())?
                .ok_or("receipt missing")?
                .completed
        );
    }
    Ok(())
}
