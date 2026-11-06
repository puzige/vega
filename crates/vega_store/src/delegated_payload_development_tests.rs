use crate::{Store, delegated_stages::prepare_payload_redaction};
use rusqlite::params;
use std::{path::PathBuf, sync::mpsc, time::Duration};

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-payload-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data.db")).unwrap();
        store.migrate().unwrap();
        for id in ["child", "other"] {
            store.conn().execute("INSERT INTO threads(id,title,mode,permission_mode,model,status,pinned,unread,created_at,updated_at) VALUES(?1,'child','execute','confirm','model','active',0,0,1,1)", [id]).unwrap();
        }
        store.conn().execute("INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES('stage','child','execution',?1,'authority',1)", [r#"{"nested":["secret",{"value":"secret"}],"number":42,"flag":true}"#]).unwrap();
        store.conn().execute("INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority,reconciliations) VALUES('child','execution','released','authority',?1)", [r#"[{"resolution":"secret"}]"#]).unwrap();
        Self { root, store }
    }

    fn contents(&self) -> (String, String) {
        (
            self.store.conn().query_row("SELECT payload FROM delegated_stage_checkpoints WHERE id='stage'", [], |row| row.get(0)).unwrap(),
            self.store.conn().query_row("SELECT reconciliations FROM delegated_execution_recovery WHERE child_thread_id='child' AND child_message_id='execution'", [], |row| row.get(0)).unwrap(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn preparation_reads_committed_payloads_without_waiting_for_writer() {
    let fixture = Fixture::new();
    let before = fixture.contents();
    let path = fixture.root.join("data.db");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let store = Store::open(path).unwrap();
        let tx = store.immediate_transaction().unwrap();
        tx.execute(
            "UPDATE delegated_stage_checkpoints SET payload='[]' WHERE id='stage'",
            [],
        )
        .unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let plan = prepare_payload_redaction(fixture.store.conn(), "child", |text| {
        assert!(fixture.store.conn().is_autocommit());
        text.replace("secret", "redacted")
    })
    .unwrap();
    assert_eq!(fixture.contents(), before);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).unwrap());
    tx.commit().unwrap();
    let (payload, recovery) = fixture.contents();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        serde_json::json!({"nested":["redacted",{"value":"redacted"}],"number":42,"flag":true})
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&recovery).unwrap(),
        serde_json::json!([{"resolution":"redacted"}])
    );
}

#[test]
fn changed_added_deleted_and_reassigned_rows_reject_prepared_payloads() {
    for sql in [
        "UPDATE delegated_stage_checkpoints SET payload='{}' WHERE id='stage'",
        "INSERT INTO delegated_stage_checkpoints SELECT 'new',child_thread_id,child_message_id,payload,authority,created_at FROM delegated_stage_checkpoints WHERE id='stage'",
        "DELETE FROM delegated_stage_checkpoints WHERE id='stage'",
        "UPDATE delegated_stage_checkpoints SET child_thread_id='other' WHERE id='stage'",
        "UPDATE delegated_execution_recovery SET reconciliations='[]' WHERE child_thread_id='child'",
        "INSERT INTO delegated_execution_recovery SELECT child_thread_id,'new',release_state,authority,handoff_checkpoint_id,resume_checkpoint_id,reconciliations FROM delegated_execution_recovery WHERE child_thread_id='child'",
        "DELETE FROM delegated_execution_recovery WHERE child_thread_id='child'",
        "UPDATE delegated_execution_recovery SET child_thread_id='other' WHERE child_thread_id='child'",
    ] {
        let fixture = Fixture::new();
        let plan = prepare_payload_redaction(fixture.store.conn(), "child", |text| {
            text.replace("secret", "redacted")
        })
        .unwrap();
        fixture.store.conn().execute(sql, []).unwrap();
        let mut tx = fixture.store.immediate_transaction().unwrap();
        assert!(plan.install(&mut tx).is_err(), "{sql}");
        tx.commit().unwrap();
        let remaining: i64 = fixture
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM delegated_stage_checkpoints WHERE payload LIKE '%redacted%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0, "{sql}");
    }
}

#[test]
fn failed_second_update_rolls_back_first_even_if_outer_transaction_commits() {
    let fixture = Fixture::new();
    let before = fixture.contents();
    let plan = prepare_payload_redaction(fixture.store.conn(), "child", |text| {
        text.replace("secret", "redacted")
    })
    .unwrap();
    fixture.store.conn().execute_batch("CREATE TEMP TRIGGER reject_recovery BEFORE UPDATE ON delegated_execution_recovery BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).is_err());
    tx.commit().unwrap();
    assert_eq!(fixture.contents(), before);
}

#[test]
fn prepared_installation_obeys_outer_rollback_and_child_identity() {
    let fixture = Fixture::new();
    let before = fixture.contents();
    fixture.store.conn().execute("INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority,reconciliations) VALUES('other','execution','released','authority',?1)", params![r#"["secret"]"#]).unwrap();
    let plan = prepare_payload_redaction(fixture.store.conn(), "child", |text| {
        text.replace("secret", "redacted")
    })
    .unwrap();
    {
        let mut tx = fixture.store.immediate_transaction().unwrap();
        assert!(plan.install(&mut tx).unwrap());
    }
    assert_eq!(fixture.contents(), before);
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).unwrap());
    tx.commit().unwrap();
    let other: String = fixture.store.conn().query_row("SELECT reconciliations FROM delegated_execution_recovery WHERE child_thread_id='other'", [], |row| row.get(0)).unwrap();
    assert_eq!(other, r#"["secret"]"#);
}

#[test]
fn unchanged_payloads_still_reject_new_rows_and_malformed_input_never_writes() {
    let fixture = Fixture::new();
    let before = fixture.contents();
    let plan = prepare_payload_redaction(fixture.store.conn(), "child", str::to_owned).unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(!plan.install(&mut tx).unwrap());
    tx.commit().unwrap();
    fixture.store.conn().execute("UPDATE delegated_execution_recovery SET reconciliations='{}' WHERE child_thread_id='child'", []).unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).is_err());
    tx.commit().unwrap();
    fixture.store.conn().execute("UPDATE delegated_execution_recovery SET reconciliations='invalid' WHERE child_thread_id='child'", []).unwrap();
    assert!(
        prepare_payload_redaction(fixture.store.conn(), "child", |text| text
            .replace("secret", "redacted"))
        .is_err()
    );
    assert_eq!(fixture.contents().0, before.0);
}
