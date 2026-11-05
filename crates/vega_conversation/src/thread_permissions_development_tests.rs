use super::*;

struct Database(PathBuf);

impl Database {
    fn new() -> (Self, Store, ThreadPermissionService) {
        let root =
            std::env::temp_dir().join(format!("vega-permissions-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let database = Self(root.join("state.sqlite"));
        let store = Store::open(&database.0).unwrap();
        store.migrate().unwrap();
        let service = ThreadPermissionService::new(database.0.clone());
        (database, store, service)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn draft_save_reads_committed_state_while_another_connection_holds_writer() {
    let (database, store, service) = Database::new();
    let draft = crate::threads::draft_thread(None, "synthetic", "confirm");
    let change = service.prepare(&draft, PermissionMode::ReadOnly).unwrap();
    let writer = Store::open_existing(&database.0).unwrap();
    let tx = writer.immediate_transaction().unwrap();
    tx.execute(
        "UPDATE permission_contract_state SET reset_state='pending'",
        [],
    )
    .unwrap();

    let saved = service.save(&change).unwrap();
    assert_eq!(saved.permission_mode, PermissionMode::ReadOnly);
    assert_eq!(saved.permission_revision, draft.permission_revision + 1);
    assert_eq!(
        saved.draft_authority_epoch.as_deref(),
        Some(change.authority_epoch.as_str())
    );
    assert!(saved.full_access_grant.is_none());
    assert!(saved.default_permission.is_none());
    assert!(threads::find(store.conn(), &draft.id).unwrap().is_none());
    assert!(!writer.conn().is_autocommit());
    drop(tx);
    let _admission = crate::thread_access::acquire(&store, &draft.id).unwrap();
}

#[test]
fn draft_save_rejects_a_materialized_identity() {
    let (_database, store, service) = Database::new();
    let draft = crate::threads::draft_thread(None, "synthetic", "confirm");
    let change = service.prepare(&draft, PermissionMode::ReadOnly).unwrap();
    crate::threads::materialize_draft(&store, &draft).unwrap();
    assert!(matches!(
        service.save(&change),
        Err(ThreadPermissionError::Conflict)
    ));
    assert_eq!(
        service.read(&draft.id).unwrap().permission_mode,
        PermissionMode::Confirm
    );
}

#[test]
fn draft_save_rejects_reset_and_old_epoch() {
    let (_database, store, service) = Database::new();
    let draft = crate::threads::draft_thread(None, "synthetic", "confirm");
    let change = service.prepare(&draft, PermissionMode::ReadOnly).unwrap();
    store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state='pending'",
            [],
        )
        .unwrap();
    assert!(matches!(
        service.save(&change),
        Err(ThreadPermissionError::ResetPending)
    ));
    store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state='ready', authority_epoch=?1",
            [ulid::Ulid::generate().to_string()],
        )
        .unwrap();
    assert!(matches!(
        service.save(&change),
        Err(ThreadPermissionError::Conflict)
    ));
    assert!(threads::find(store.conn(), &draft.id).unwrap().is_none());
}

#[test]
fn draft_save_preserves_database_pause_and_thread_admission() {
    let (_database, store, service) = Database::new();
    let draft = crate::threads::draft_thread(None, "synthetic", "confirm");
    let change = service.prepare(&draft, PermissionMode::ReadOnly).unwrap();
    let admission = crate::thread_access::acquire(&store, &draft.id).unwrap();
    assert!(matches!(
        service.save(&change),
        Err(ThreadPermissionError::Unavailable)
    ));
    drop(admission);
    let pause = vega_store::thread_admission::try_pause_database(store.database_identity())
        .unwrap()
        .unwrap();
    assert!(matches!(
        service.save(&change),
        Err(ThreadPermissionError::ResetPending)
    ));
    drop(pause);
    assert_eq!(
        service.save(&change).unwrap().permission_mode,
        PermissionMode::ReadOnly
    );
}

#[test]
fn materialized_save_uses_revision_cas_and_rejects_stale_permissions() {
    let (_database, store, service) = Database::new();
    let thread = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let first = service.prepare(&thread, PermissionMode::ReadOnly).unwrap();
    let stale = service.prepare(&thread, PermissionMode::Auto).unwrap();
    let saved = service.save(&first).unwrap();
    assert_eq!(saved.permission_mode, PermissionMode::ReadOnly);
    assert_eq!(saved.permission_revision, thread.permission_revision + 1);
    assert!(matches!(
        service.save(&stale),
        Err(ThreadPermissionError::Conflict)
    ));
    let persisted = service.read(&thread.id).unwrap();
    assert_eq!(persisted.permission_mode, saved.permission_mode);
    assert_eq!(persisted.permission_revision, saved.permission_revision);
}
