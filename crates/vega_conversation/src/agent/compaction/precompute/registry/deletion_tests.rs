use super::*;

#[test]
fn background_deletion_waits_for_precompute_cleanup_and_retries_without_losing_thread() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("cleanup.db");
    let store = vega_store::Store::open(&database).unwrap();
    store.migrate().unwrap();
    let thread = crate::threads::create_standalone_thread(&store, "model", "confirm").unwrap();
    let registry = ContextPrecomputeRegistry::global();
    let key = Key {
        database: store.database_identity(),
        thread: thread.id.clone(),
    };
    let epoch = Arc::new(CancellationToken::new());
    let flight = Arc::new(Flight {
        key: key.clone(),
        epoch: epoch.clone(),
        generation: "pending-cleanup".into(),
        cancel: CancellationToken::new(),
        owner: None,
        state: Mutex::new(FlightState::default()),
    });
    registry.inner.lock().unwrap().entries.insert(
        key,
        Entry {
            epoch,
            failures: 0,
            pending: Some(flight.clone()),
        },
    );
    assert!(matches!(
        crate::threads::persist_thread_deletion(&database, &thread.id),
        Err(crate::types::ConversationError::ContextCleanupPending)
    ));
    assert!(flight.cancel.is_cancelled());
    assert!(crate::threads::get_thread(&store, &thread.id).is_ok());
    flight.state.lock().unwrap().outcome = Some(Ok(()));
    crate::threads::persist_thread_deletion(&database, &thread.id).unwrap();
    assert!(crate::threads::get_thread(&store, &thread.id).is_err());
}
