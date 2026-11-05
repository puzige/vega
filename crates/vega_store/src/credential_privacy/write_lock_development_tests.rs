use super::*;
use crate::Store;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> (Self, Store) {
        let root = std::env::temp_dir().join(format!("vega-epoch-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("state.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        store.conn().busy_timeout(Duration::ZERO).unwrap();
        (Self(path), store)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn unchanged_epoch_reads_without_joining_writer_queue() {
    let (fixture, store) = Fixture::new();
    refresh_epoch(store.conn(), "current").unwrap();
    let writer = Store::open_existing(&fixture.0).unwrap();
    let held = writer.immediate_transaction().unwrap();
    let before = crate::write_coordination::recent_observations(store.conn());
    refresh_epoch(store.conn(), "current").unwrap();
    assert_eq!(
        crate::write_coordination::recent_observations(store.conn()),
        before
    );
    assert_eq!(
        store
            .conn()
            .query_row("SELECT digest FROM credential_privacy_epoch", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "current"
    );
    held.rollback().unwrap();
}

#[test]
fn changed_epoch_clears_old_exceptions_atomically_and_failed_write_preserves_them() {
    let (fixture, store) = Fixture::new();
    store.conn().execute("INSERT INTO threads(id,title,model,created_at,updated_at) VALUES('synthetic','synthetic','synthetic',0,0)", []).unwrap();
    refresh_epoch(store.conn(), "before").unwrap();
    allow(
        store.conn(),
        "synthetic",
        "synthetic-rule",
        "synthetic-fingerprint",
    )
    .unwrap();
    let writer = Connection::open(&fixture.0).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(refresh_epoch(store.conn(), "after").is_err());
    assert_eq!(exceptions(store.conn(), "synthetic").unwrap().len(), 1);
    assert_eq!(
        store
            .conn()
            .query_row("SELECT digest FROM credential_privacy_epoch", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "before"
    );
    writer.execute_batch("ROLLBACK").unwrap();
    refresh_epoch(store.conn(), "after").unwrap();
    assert!(exceptions(store.conn(), "synthetic").unwrap().is_empty());
    assert_eq!(
        store
            .conn()
            .query_row("SELECT digest FROM credential_privacy_epoch", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "after"
    );
}
