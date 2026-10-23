use super::*;
use crate::{Store, context_compaction, credential_privacy, messages, threads, token_usage};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

struct Database {
    root: PathBuf,
    path: PathBuf,
}

impl Database {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-storage-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("store.sqlite");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: "thread",
                project_id: "",
                title: "original",
                mode: "execute",
                permission_mode: "confirm",
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        for (id, seq, role) in [("user", 1, "user"), ("assistant", 2, "assistant")] {
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: id.into(),
                    thread_id: "thread".into(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: "original".into(),
                    status: "done".into(),
                    created_at: seq,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        Self { root, path }
    }

    fn open(&self) -> Store {
        Store::open(&self.path).unwrap()
    }

    fn checkpoint(&self) -> context_compaction::NewContextCheckpoint {
        let store = self.open();
        let source = context_compaction::load_source(store.conn(), "thread").unwrap();
        context_compaction::NewContextCheckpoint {
            thread_id: "thread".into(),
            model: "model".into(),
            source_version: source.source_version,
            covered_through_seq: 2,
            covered_assistant_id: None,
            covered_assistant_text_bytes: 0,
            covered_assistant_tool_seq: 0,
            source_fingerprint: source.fingerprint,
            summary: "summary".into(),
            estimator_version: "estimator".into(),
            expected_previous_id: None,
            created_at: 5,
        }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn wait_for_waiters(queue: &WriterQueue, count: usize) {
    let started = Instant::now();
    loop {
        if queue.state.lock().unwrap().waiters.len() == count {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn sqlite_code<T>(result: rusqlite::Result<T>) -> i32 {
    match result {
        Err(rusqlite::Error::SqliteFailure(error, _)) => error.extended_code,
        _ => panic!("expected SQLite failure"),
    }
}

#[test]
fn same_database_writers_are_fifo_and_other_databases_remain_available() {
    let database = Database::new();
    let unrelated = Database::new();
    let owner = database.open();
    let first = database.open();
    let second = database.open();
    let permit = acquire(owner.conn(), WriteCategory::Messages).unwrap();
    let writer_queue = queue(owner.conn()).unwrap();
    let (sent, received) = mpsc::channel();
    std::thread::scope(|scope| {
        let first_sent = sent.clone();
        scope.spawn(move || {
            let _permit = acquire(first.conn(), WriteCategory::Threads).unwrap();
            threads::rename(first.conn(), "thread", "first", 2).unwrap();
            first_sent.send(1).unwrap();
        });
        wait_for_waiters(&writer_queue, 1);
        scope.spawn(move || {
            let _permit = acquire(second.conn(), WriteCategory::Threads).unwrap();
            threads::rename(second.conn(), "thread", "second", 3).unwrap();
            sent.send(2).unwrap();
        });
        wait_for_waiters(&writer_queue, 2);
        let other = unrelated.open();
        threads::rename(other.conn(), "thread", "unrelated", 4).unwrap();
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(permit);
        assert_eq!(received.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        assert_eq!(received.recv_timeout(Duration::from_secs(3)).unwrap(), 2);
    });
    assert_eq!(
        threads::find(owner.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "second"
    );
}

#[test]
fn same_connection_nesting_is_allowed_but_cross_connection_reentry_is_locked() {
    let database = Database::new();
    let owner = database.open();
    let another = database.open();
    let permit = acquire(owner.conn(), WriteCategory::Messages).unwrap();
    let nested = acquire(owner.conn(), WriteCategory::Threads).unwrap();
    threads::rename(owner.conn(), "thread", "nested", 2).unwrap();
    assert_eq!(
        sqlite_code(acquire(another.conn(), WriteCategory::Messages)),
        rusqlite::ffi::SQLITE_LOCKED
    );
    drop(permit);
    assert_eq!(
        sqlite_code(acquire(another.conn(), WriteCategory::Messages)),
        rusqlite::ffi::SQLITE_LOCKED
    );
    drop(nested);
    threads::rename(another.conn(), "thread", "released", 3).unwrap();
}

#[test]
fn aliases_of_the_same_file_share_a_writer_queue() {
    let database = Database::new();
    let alias = database.root.join("alias.sqlite");
    std::os::unix::fs::symlink(&database.path, &alias).unwrap();
    let owner = database.open();
    let aliased = Store::open(&alias).unwrap();
    let permit = acquire(owner.conn(), WriteCategory::Messages).unwrap();
    assert_eq!(
        sqlite_code(acquire(aliased.conn(), WriteCategory::Messages)),
        rusqlite::ffi::SQLITE_LOCKED
    );
    drop(permit);
    threads::rename(aliased.conn(), "thread", "alias", 2).unwrap();
}

#[test]
fn committed_read_snapshots_do_not_wait_for_a_coordinated_writer() {
    let database = Database::new();
    let writer = database.open();
    let reader = Store::open_read_only(&database.path).unwrap();
    let transaction = writer.immediate_transaction().unwrap();
    threads::rename(&transaction, "thread", "uncommitted", 2).unwrap();
    assert_eq!(
        context_compaction::load_source(reader.conn(), "thread")
            .unwrap()
            .messages
            .len(),
        2
    );
    let snapshot = reader.conn().unchecked_transaction().unwrap();
    assert_eq!(
        threads::find(&snapshot, "thread").unwrap().unwrap().title,
        "original"
    );
    transaction.commit().unwrap();
    assert_eq!(
        threads::find(&snapshot, "thread").unwrap().unwrap().title,
        "original"
    );
    snapshot.commit().unwrap();
    assert_eq!(
        threads::find(reader.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "uncommitted"
    );
    assert_eq!(
        context_compaction::load_source(reader.conn(), "thread")
            .unwrap()
            .messages
            .len(),
        2
    );
}

#[test]
fn dropped_and_failed_transactions_release_after_rollback() {
    let database = Database::new();
    let owner = database.open();
    let next = database.open();
    {
        let transaction = owner.immediate_transaction().unwrap();
        threads::rename(&transaction, "thread", "rolled back", 2).unwrap();
        assert_eq!(
            sqlite_code(transaction.execute(
                "INSERT INTO threads SELECT * FROM threads WHERE id='thread'",
                []
            )),
            rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
        );
    }
    threads::rename(next.conn(), "thread", "after rollback", 3).unwrap();
    assert_eq!(
        threads::find(owner.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "after rollback"
    );
    let transaction = owner.immediate_transaction().unwrap();
    threads::rename(&transaction, "thread", "explicit rollback", 4).unwrap();
    transaction.rollback().unwrap();
    assert_eq!(
        threads::find(next.conn(), "thread").unwrap().unwrap().title,
        "after rollback"
    );
}

#[test]
fn cancelled_and_timed_out_waiters_do_not_block_following_writes() {
    let database = Database::new();
    let owner = database.open();
    let cancelled_store = database.open();
    let timeout_store = database.open();
    let permit = acquire(owner.conn(), WriteCategory::Messages).unwrap();
    let writer_queue = queue(owner.conn()).unwrap();
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let cancel = &cancelled;
        let cancelled = scope.spawn(move || {
            sqlite_code(acquire_with_cancel(
                cancelled_store.conn(),
                WriteCategory::Messages,
                || cancel.load(Ordering::SeqCst),
            ))
        });
        wait_for_waiters(&writer_queue, 1);
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(cancelled.join().unwrap(), rusqlite::ffi::SQLITE_INTERRUPT);
        let timed_out = scope.spawn(move || {
            sqlite_code(acquire_with_deadline(
                timeout_store.conn(),
                WriteCategory::Messages,
                || false,
                Duration::from_millis(100),
            ))
        });
        assert_eq!(timed_out.join().unwrap(), rusqlite::ffi::SQLITE_BUSY);
        wait_for_waiters(&writer_queue, 0);
    });
    drop(permit);
    threads::rename(owner.conn(), "thread", "after timeout", 2).unwrap();
}

#[test]
fn a_full_writer_queue_rejects_additional_work_without_unbounded_waiting() {
    let database = Database::new();
    let store = database.open();
    let writer_queue = queue(store.conn()).unwrap();
    let another = database.open();
    let (ready, received) = mpsc::channel();
    let (release, released) = mpsc::channel();
    std::thread::scope(|scope| {
        let held_queue = writer_queue.clone();
        scope.spawn(move || {
            let owner = acquire(store.conn(), WriteCategory::Messages).unwrap();
            held_queue
                .state
                .lock()
                .unwrap()
                .waiters
                .extend(0..MAX_WAITERS as u64);
            ready.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(3)).unwrap();
            held_queue.state.lock().unwrap().waiters.clear();
            drop(owner);
        });
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            sqlite_code(acquire(another.conn(), WriteCategory::Messages)),
            rusqlite::ffi::SQLITE_BUSY
        );
        release.send(()).unwrap();
    });
    let store = database.open();
    threads::rename(store.conn(), "thread", "after backpressure", 2).unwrap();
}

fn raw_writer(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.busy_timeout(Duration::from_millis(100)).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    connection
}

#[test]
fn short_external_writer_allows_one_save_without_command_replay() {
    let database = Database::new();
    let store = database.open();
    let external = raw_writer(&database.path);
    let (started, received) = mpsc::channel();
    std::thread::scope(|scope| {
        let saving = scope.spawn(move || {
            started.send(()).unwrap();
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id: "thread",
                    message_id: Some("assistant"),
                    model: "model",
                    input_tokens: 7,
                    output_tokens: 3,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    cost_microcents: 0,
                    created_at: 3,
                    pricing_version: None,
                    pricing_profile: None,
                    call_started_at: None,
                },
            )
            .unwrap();
        });
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        let writer_queue = queue(&external).unwrap();
        let started = Instant::now();
        while writer_queue.state.lock().unwrap().owner.is_none() {
            assert!(started.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(1));
        }
        external.execute_batch("COMMIT").unwrap();
        saving.join().unwrap();
    });
    let store = database.open();
    let total = token_usage::aggregate_by_thread(store.conn(), "thread").unwrap();
    assert_eq!(
        (total.row_count, total.input_tokens, total.output_tokens),
        (1, 7, 3)
    );
}

#[test]
fn persistent_external_lock_is_bounded_and_a_later_explicit_write_can_recover() {
    let database = Database::new();
    let store = database.open();
    store
        .conn()
        .busy_timeout(Duration::from_millis(100))
        .unwrap();
    let external = raw_writer(&database.path);
    let started = Instant::now();
    assert_eq!(
        sqlite_code(threads::rename(store.conn(), "thread", "blocked", 2)),
        rusqlite::ffi::SQLITE_BUSY
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        threads::find(store.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "original"
    );
    external.execute_batch("ROLLBACK").unwrap();
    threads::rename(store.conn(), "thread", "recovered", 3).unwrap();
    assert_eq!(
        threads::find(store.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "recovered"
    );
}

#[test]
fn stale_read_upgrade_and_constraint_errors_keep_their_original_classification() {
    let database = Database::new();
    let reader = database.open();
    let writer = database.open();
    let snapshot = reader.conn().unchecked_transaction().unwrap();
    threads::find(&snapshot, "thread").unwrap();
    threads::rename(writer.conn(), "thread", "newer", 2).unwrap();
    assert_eq!(
        sqlite_code(threads::rename(&snapshot, "thread", "stale", 3)),
        rusqlite::ffi::SQLITE_BUSY_SNAPSHOT
    );
    snapshot.rollback().unwrap();
    let duplicate = threads::NewThread {
        id: "thread",
        project_id: "",
        title: "duplicate",
        mode: "execute",
        permission_mode: "confirm",
        model: "model",
        status: "active",
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    };
    assert_eq!(
        sqlite_code(threads::create_standalone(writer.conn(), duplicate)),
        rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
    );
    assert_eq!(
        threads::find(reader.conn(), "thread")
            .unwrap()
            .unwrap()
            .title,
        "newer"
    );
}

#[test]
fn checkpoint_cancellation_exits_the_queue_without_installing_a_row() {
    let database = Database::new();
    let checkpoint = database.checkpoint();
    let owner = database.open();
    let installing = database.open();
    let permit = acquire(owner.conn(), WriteCategory::Messages).unwrap();
    let writer_queue = queue(owner.conn()).unwrap();
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let cancel = &cancelled;
        let installing = scope.spawn(move || {
            context_compaction::install_checkpoint_with_guard(
                installing.conn(),
                &checkpoint,
                || cancel.load(Ordering::SeqCst),
            )
        });
        wait_for_waiters(&writer_queue, 1);
        cancel.store(true, Ordering::SeqCst);
        assert!(matches!(
            installing.join().unwrap(),
            Err(context_compaction::ContextCheckpointError::Cancelled)
        ));
    });
    drop(permit);
    assert!(
        context_compaction::latest_checkpoint(owner.conn(), "thread", "model")
            .unwrap()
            .is_none()
    );
}

#[test]
fn checkpoint_source_model_and_predecessor_fences_remain_fail_closed() {
    let database = Database::new();
    let original = database.checkpoint();
    let store = database.open();
    let mut wrong_model = original.clone();
    wrong_model.model = "other".into();
    assert!(matches!(
        context_compaction::install_checkpoint(store.conn(), &wrong_model).unwrap(),
        context_compaction::ContextCheckpointInstall::Stale
    ));
    let mut wrong_predecessor = original.clone();
    wrong_predecessor.expected_previous_id = Some(99);
    assert!(matches!(
        context_compaction::install_checkpoint(store.conn(), &wrong_predecessor).unwrap(),
        context_compaction::ContextCheckpointInstall::Stale
    ));
    messages::finish(store.conn(), "assistant", "changed", "done").unwrap();
    assert!(matches!(
        context_compaction::install_checkpoint(store.conn(), &original).unwrap(),
        context_compaction::ContextCheckpointInstall::Stale
    ));
    assert!(
        context_compaction::latest_checkpoint(store.conn(), "thread", "model")
            .unwrap()
            .is_none()
    );
    let fresh = database.checkpoint();
    assert!(matches!(
        context_compaction::install_checkpoint(store.conn(), &fresh).unwrap(),
        context_compaction::ContextCheckpointInstall::Applied(_)
    ));
    assert!(matches!(
        context_compaction::install_checkpoint(store.conn(), &fresh).unwrap(),
        context_compaction::ContextCheckpointInstall::AlreadyCommitted(_)
    ));
}

#[test]
fn diagnostics_are_bounded_content_free_and_available_in_existing_export() {
    let database = Database::new();
    let store = database.open();
    let unrelated = Database::new();
    let other = unrelated.open();
    threads::rename(other.conn(), "thread", "not exported", 2).unwrap();
    for _ in 0..300 {
        let permit = acquire(store.conn(), WriteCategory::Messages).unwrap();
        drop(permit);
    }
    let observations = recent_observations(store.conn());
    assert!(!observations.is_empty());
    assert!(observations.len() <= 256);
    let encoded = serde_json::to_string(&observations).unwrap();
    assert!(!encoded.contains(&database.path.to_string_lossy().to_string()));
    assert!(!encoded.contains("not exported"));
    assert!(!encoded.contains("SELECT"));
    assert!(!encoded.contains("thread_id"));
    crate::run_diagnostics::insert(
        store.conn(),
        &crate::run_diagnostics::NewDiagnosticEvent {
            thread_id: "thread".into(),
            run_id: "assistant".into(),
            attempt_id: "attempt".into(),
            parent_attempt_id: None,
            tool_call_id: None,
            phase: crate::run_diagnostics::DiagnosticPhase::Run,
            state: crate::run_diagnostics::DiagnosticState::Started,
            failure_code: None,
            occurred_at: 1,
            duration_ms: None,
            metrics: crate::run_diagnostics::DiagnosticMetrics::default(),
        },
    )
    .unwrap();
    let exported: serde_json::Value = serde_json::from_str(
        &crate::run_diagnostics::export_run(store.conn(), "thread", "assistant").unwrap(),
    )
    .unwrap();
    assert_eq!(exported["schema_version"], 2);
    assert!(
        !exported["recent_process_storage"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn credential_cleanup_rolls_back_on_changed_source_and_recovers_on_fresh_source() {
    let database = Database::new();
    let store = database.open();
    let original = credential_privacy::source_fingerprint(store.conn(), "thread").unwrap();
    messages::finish(store.conn(), "assistant", "secret", "done").unwrap();
    assert!(
        credential_privacy::clean(store.conn(), "thread", &original, |_, offsets| (
            "clean".into(),
            offsets.to_vec()
        ))
        .is_err()
    );
    assert_eq!(
        messages::find(store.conn(), "assistant")
            .unwrap()
            .unwrap()
            .content,
        "secret"
    );
    let fresh = credential_privacy::source_fingerprint(store.conn(), "thread").unwrap();
    credential_privacy::clean(store.conn(), "thread", &fresh, |text, offsets| {
        (text.replace("secret", "clean"), offsets.to_vec())
    })
    .unwrap();
    assert_eq!(
        messages::find(store.conn(), "assistant")
            .unwrap()
            .unwrap()
            .content,
        "clean"
    );
}
