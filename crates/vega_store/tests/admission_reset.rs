use vega_store::Store;
use vega_store::thread_admission::{try_acquire, try_pause_database};

#[test]
fn database_pause_excludes_new_owners_without_revoking_existing_owners() {
    let store = Store::open(":memory:").expect("isolated database");
    let other = Store::open(":memory:").expect("other isolated database");
    let identity = store.database_identity();
    let owner = try_acquire(identity, "active")
        .expect("acquire")
        .expect("free");
    let clone = owner.clone();
    let pause = try_pause_database(identity)
        .expect("pause")
        .expect("unpaused");
    assert!(
        try_pause_database(identity)
            .expect("duplicate pause")
            .is_none()
    );
    assert!(!pause.owners_released().expect("owners remain"));
    assert!(owner.verifies(identity, "active"));
    assert!(
        try_acquire(identity, "new")
            .expect("new thread blocked")
            .is_none()
    );
    assert!(
        try_acquire(identity, "active")
            .expect("active blocked")
            .is_none()
    );
    assert!(
        try_acquire(other.database_identity(), "new")
            .expect("other database")
            .is_some()
    );
    drop(owner);
    assert!(!pause.owners_released().expect("clone remains"));
    drop(clone);
    assert!(pause.owners_released().expect("all owners released"));
    assert!(
        try_acquire(identity, "new")
            .expect("still paused")
            .is_none()
    );
    drop(pause);
    assert!(
        try_acquire(identity, "new")
            .expect("released pause")
            .is_some()
    );
}

#[test]
fn future_schema_is_rejected_without_rewriting_authority_or_schema() {
    let store = Store::open(":memory:").expect("isolated database");
    store.migrate().expect("current schema");
    let state = vega_store::permission_contracts::load_state(store.conn()).expect("authority");
    let current: u32 = store
        .conn()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("version");
    store
        .conn()
        .pragma_update(None, "user_version", current + 1)
        .expect("future version fixture");
    assert!(store.migrate().is_err());
    let actual: u32 = store
        .conn()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("unchanged version");
    assert_eq!(actual, current + 1);
    assert_eq!(
        vega_store::permission_contracts::load_state(store.conn()).expect("unchanged authority"),
        state
    );
}
