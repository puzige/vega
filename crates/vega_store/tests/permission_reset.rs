use vega_store::Store;
use vega_store::pending_messages::{self as pending, PendingMessageRow};
use vega_store::permission_contracts;
use vega_store::permission_reset::{self as reset, ResetConfigSource, ResetRecord};

fn fixture() -> Store {
    let store = Store::open(":memory:").expect("isolated database");
    store.migrate().expect("migrate isolated database");
    store.conn().execute_batch(
        "INSERT INTO threads(id,model,created_at,updated_at,permission_mode,permission_revision,full_access_grant) VALUES('full','synthetic',1,1,'full_access',7,'{}'),('restricted','synthetic',1,1,'confirm',5,NULL);
         INSERT INTO messages(id,thread_id,seq,role,content,created_at) VALUES('user','full',0,'user','preserve instruction',1),('assistant','full',1,'assistant','preserve result',2);
         INSERT INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,raw_user_fingerprint,state,reason,created_at) VALUES('assistant','full','user',1,'synthetic','synthetic','unknown','unsettled',2);
         INSERT INTO command_scope_audits(request_id,thread_id,run_id,call_id,request,state,created_at,updated_at) VALUES('pending','full','run','pending','{}','pending',1,1),('approved','full','run','approved','{}','approved',1,1),('consumed','full','run','consumed','{}','consumed',1,1),('unknown','full','run','unknown','{}','unknown',1,1);",
    ).expect("seed authority and retained evidence");
    pending::insert(store.conn(), &queued("old")).expect("enqueue before reset");
    store
}

fn queued(id: &str) -> PendingMessageRow {
    PendingMessageRow {
        id: id.into(),
        thread_id: "full".into(),
        ordinal: 0,
        behavior: "steer".into(),
        generation: "generation".into(),
        payload: "recoverable user input".into(),
        created_at: 1,
    }
}

fn source() -> ResetConfigSource {
    ResetConfigSource {
        configuration_id: "/synthetic/config.toml".into(),
        mode: "full_access".into(),
        revision: 9,
        grant: Some("synthetic authority metadata".into()),
    }
}

fn target(record: &ResetRecord) -> ResetConfigSource {
    ResetConfigSource {
        configuration_id: record.source.configuration_id.clone(),
        mode: "confirm".into(),
        revision: record.target_revision,
        grant: None,
    }
}

fn begin(store: &Store) -> ResetRecord {
    let state = permission_contracts::load_state(store.conn()).expect("source authority");
    let tx = store.immediate_transaction().expect("begin write");
    let record = reset::begin(&tx, &state.authority_epoch, &source(), 10).expect("begin reset");
    tx.commit().expect("publish pending reset");
    record
}

#[test]
fn pending_reset_revokes_dispatch_without_erasing_input_or_unknown_effects() {
    let store = fixture();
    let before = permission_contracts::load_state(store.conn()).expect("original authority");
    assert!(reset::replay_allowed(store.conn(), "full", 0).expect("before reset"));
    let record = begin(&store);
    let state = permission_contracts::load_state(store.conn()).expect("pending authority");
    assert!(state.reset_pending);
    assert_ne!(state.authority_epoch, before.authority_epoch);
    assert_eq!(state.authority_epoch, record.authority_epoch);
    assert_eq!(
        reset::load(store.conn()).expect("journal"),
        Some(record.clone())
    );
    let full = vega_store::threads::find(store.conn(), "full")
        .expect("read thread")
        .expect("thread");
    assert_eq!(full.permission_mode, "confirm");
    assert_eq!(full.permission_revision, 8);
    assert!(full.full_access_grant.is_none());
    let restricted = vega_store::threads::find(store.conn(), "restricted")
        .expect("read restricted")
        .expect("thread");
    assert_eq!(restricted.permission_revision, 5);
    assert_eq!(
        pending::find(store.conn(), "full", "old")
            .expect("retained input")
            .expect("row")
            .payload,
        "recoverable user input"
    );
    assert_eq!(
        pending::count(store.conn(), "full").expect("retained count"),
        1
    );
    assert!(
        pending::find_dispatchable(store.conn(), "full", "old")
            .expect("dispatch gate")
            .is_none()
    );
    assert!(
        pending::next_steer(store.conn(), "full", "generation")
            .expect("steer gate")
            .is_none()
    );
    assert!(
        pending::next_steer_metadata(store.conn(), "full", "generation")
            .expect("metadata gate")
            .is_none()
    );
    assert!(!pending::promote(store.conn(), "full", "old", "generation").expect("promote gate"));
    assert!(!pending::claim_steer(store.conn(), "full", "old", "generation").expect("claim gate"));
    assert!(!pending::mark_delivered(store.conn(), "full", "old").expect("delivery gate"));
    assert!(pending::insert(store.conn(), &queued("during")).is_err());
    assert!(!reset::replay_allowed(store.conn(), "full", 2).expect("pending blocks any replay"));
    let continuation = vega_store::credential_continuation::find(store.conn(), "assistant")
        .expect("retained continuation")
        .expect("record");
    assert_eq!(continuation.state, "unknown");
    let states: Vec<(String, String)> = store
        .conn()
        .prepare("SELECT request_id,state FROM command_scope_audits ORDER BY request_id")
        .expect("audit query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("read audit")
        .collect::<Result<_, _>>()
        .expect("audit rows");
    assert_eq!(
        states,
        vec![
            ("approved".into(), "cancelled".into()),
            ("consumed".into(), "consumed".into()),
            ("pending".into(), "cancelled".into()),
            ("unknown".into(), "unknown".into())
        ]
    );
    let retained: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("retained history");
    assert_eq!(retained, 2);
}

#[test]
fn reset_retry_keeps_one_epoch_and_revision_and_rejects_config_conflicts() {
    let store = fixture();
    let record = begin(&store);
    for config in [source(), target(&record)] {
        let tx = store.immediate_transaction().expect("retry write");
        assert_eq!(
            reset::begin(&tx, &record.authority_epoch, &config, 20).expect("retry same reset"),
            record
        );
        tx.commit().expect("commit retry");
    }
    let mut conflict = source();
    conflict.revision += 1;
    let tx = store.immediate_transaction().expect("conflict write");
    assert!(reset::begin(&tx, &record.authority_epoch, &conflict, 20).is_err());
    assert!(reset::finish(&tx, &record, &conflict).is_err());
    drop(tx);
    assert_eq!(
        reset::load(store.conn()).expect("same journal"),
        Some(record.clone())
    );
    assert_eq!(
        vega_store::threads::find(store.conn(), "full")
            .expect("thread")
            .expect("present")
            .permission_revision,
        8
    );
    let tx = store.immediate_transaction().expect("finish write");
    reset::finish(&tx, &record, &target(&record)).expect("finish with readback");
    tx.commit().expect("publish ready");
    let tx = store.immediate_transaction().expect("duplicate finish");
    reset::finish(&tx, &record, &target(&record)).expect("lost completion receipt is idempotent");
    tx.commit().expect("duplicate commit");
    let completed = reset::load(store.conn())
        .expect("completion journal")
        .expect("retained receipt");
    assert!(completed.completed);
    assert_eq!(completed.id, record.id);
    assert!(
        !permission_contracts::load_state(store.conn())
            .expect("ready authority")
            .reset_pending
    );
    assert!(
        !reset::replay_allowed(store.conn(), "full", 1).expect("old history remains invalidated")
    );
    assert!(reset::replay_allowed(store.conn(), "full", 2).expect("new history is eligible"));
    assert!(
        pending::find_dispatchable(store.conn(), "full", "old")
            .expect("old queue stays invalidated")
            .is_none()
    );
    pending::insert(store.conn(), &queued("new")).expect("new accepted input");
    assert!(
        pending::find_dispatchable(store.conn(), "full", "new")
            .expect("new input eligible")
            .is_some()
    );
    let tx = store.immediate_transaction().expect("stale completion");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
}

#[test]
fn reset_finish_refuses_unsettled_automation_and_leaves_pending() {
    let store = fixture();
    let record = begin(&store);
    store.conn().execute("INSERT INTO automation_tasks(thread_id,kind,identity,revision,data) VALUES('full','goal','task',1,?1)", [r#"{"status":"paused","revision":1,"permission_authorization":{"identity":"old"}}"#]).expect("retained automation authorization");
    let tx = store.immediate_transaction().expect("finish write");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
    drop(tx);
    store
        .conn()
        .execute(
            "UPDATE automation_tasks SET data=?1",
            [r#"{"status":"active","revision":1}"#],
        )
        .expect("active automation");
    let tx = store.immediate_transaction().expect("active finish write");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
    drop(tx);
    store
        .conn()
        .execute(
            "UPDATE automation_tasks SET data=?1",
            [r#"{"status":"paused","revision":2}"#],
        )
        .expect("inconsistent revision");
    let tx = store
        .immediate_transaction()
        .expect("revision finish write");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
    drop(tx);
    store
        .conn()
        .execute("UPDATE automation_tasks SET revision=2", [])
        .expect("consistent paused task");
    store
        .conn()
        .execute(
            "INSERT INTO automation_runs(id,thread_id,data) VALUES('claim','full','{}')",
            [],
        )
        .expect("unsettled run");
    let tx = store.immediate_transaction().expect("claim finish write");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
    drop(tx);
    let finished: bool = store
        .conn()
        .query_row(
            "SELECT finished FROM automation_runs WHERE id='claim'",
            [],
            |row| row.get(0),
        )
        .expect("retained claim");
    assert!(!finished);
    assert_eq!(
        reset::load(store.conn()).expect("still pending"),
        Some(record)
    );
}

#[test]
fn reset_invalidates_an_otherwise_claimable_credential_continuation() {
    let store = fixture();
    let tx = store.immediate_transaction().expect("prepare continuation");
    tx.execute("DELETE FROM credential_continuations", [])
        .expect("replace unknown fixture");
    tx.execute(
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        [],
    )
    .expect("active fixture turn");
    permission_contracts::record_run_fence(&tx, "assistant", "full", "{}")
        .expect("record original fence");
    tx.execute("UPDATE messages SET status='done' WHERE id='assistant'", [])
        .expect("settled fixture turn");
    vega_store::credential_continuation::suspend(&tx, "full", "assistant", "synthetic", 3)
        .expect("suspend continuation");
    tx.commit().expect("persist continuation");
    let receipt = vega_store::credential_continuation::find(store.conn(), "assistant")
        .expect("load continuation")
        .expect("receipt");
    assert_eq!(receipt.state, "ready");
    let tx = store.immediate_transaction().expect("claim before reset");
    assert!(
        vega_store::credential_continuation::claim(&tx, &receipt, "operation", "{}")
            .expect("otherwise eligible")
    );
    drop(tx);
    begin(&store);
    let tx = store.immediate_transaction().expect("claim after reset");
    let source = vega_store::context_compaction::load_source_version_in_transaction(&tx, "full")
        .expect("source unchanged");
    assert_eq!(source.source_version, receipt.source_version);
    assert_eq!(source.fingerprint, receipt.source_fingerprint);
    assert!(
        !vega_store::credential_continuation::claim(&tx, &receipt, "operation", "{}")
            .expect("reset rejects old authority")
    );
    drop(tx);
    assert_eq!(
        vega_store::credential_continuation::find(store.conn(), "assistant")
            .expect("retained evidence")
            .expect("receipt")
            .state,
        "ready"
    );
}

#[test]
fn reset_rollback_overflow_and_corrupt_journal_never_publish_ready() {
    let store = fixture();
    let state = permission_contracts::load_state(store.conn()).expect("source authority");
    let tx = store.immediate_transaction().expect("rollback write");
    reset::begin(&tx, &state.authority_epoch, &source(), 10).expect("uncommitted reset");
    drop(tx);
    assert_eq!(
        permission_contracts::load_state(store.conn()).expect("rolled back state"),
        state
    );
    assert!(
        reset::load(store.conn())
            .expect("rolled back journal")
            .is_none()
    );
    assert!(
        pending::find_dispatchable(store.conn(), "full", "old")
            .expect("rollback preserves eligibility")
            .is_some()
    );
    let mut overflow = source();
    overflow.revision = i64::MAX;
    let tx = store.immediate_transaction().expect("overflow write");
    assert!(reset::begin(&tx, &state.authority_epoch, &overflow, 10).is_err());
    drop(tx);
    store
        .conn()
        .execute(
            "UPDATE threads SET permission_revision=?1 WHERE id='full'",
            [i64::MAX],
        )
        .expect("thread overflow fixture");
    let tx = store
        .immediate_transaction()
        .expect("thread overflow write");
    assert!(reset::begin(&tx, &state.authority_epoch, &source(), 10).is_err());
    drop(tx);
    assert_eq!(
        permission_contracts::load_state(store.conn()).expect("unchanged authority"),
        state
    );
    store
        .conn()
        .execute(
            "UPDATE threads SET permission_revision=7 WHERE id='full'",
            [],
        )
        .expect("restore fixture revision");
    let record = begin(&store);
    store
        .conn()
        .execute(
            "UPDATE permission_reset_journal SET authority_epoch=?1",
            [ulid::Ulid::generate().to_string()],
        )
        .expect("mismatched journal fixture");
    assert!(reset::load(store.conn()).is_err());
    let tx = store.immediate_transaction().expect("corrupt finish write");
    assert!(reset::finish(&tx, &record, &target(&record)).is_err());
    drop(tx);
    assert!(
        permission_contracts::load_state(store.conn())
            .expect("pending retained")
            .reset_pending
    );
}
