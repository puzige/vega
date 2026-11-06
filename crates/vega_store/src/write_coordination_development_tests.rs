use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn queue_timeout_attributes_the_live_preparation_owner_not_the_engine() {
    let root = std::env::temp_dir().join(format!("queue-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).expect("root");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let root = root.canonicalize().expect("canonical root");
    let path = root.join("vega.db");
    let conn = Connection::open(&path).expect("connection");
    conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE sample(value INTEGER)")
        .expect("schema");
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let owner_path = path.clone();
    let worker = std::thread::spawn(move || {
        let conn = Connection::open(owner_path).expect("owner connection");
        let _execution = ExecutionScope::begin(
            "01M6PER2BCE1EVGCK522P31RN8",
            "01M6PFXFYMVAAEQCQXVKFJJV27",
            WriteActivity::Preparation,
        );
        let permit = acquire(&conn, WriteCategory::RuntimeEvent).expect("owner permit");
        ready.send(()).expect("ready");
        released.recv().expect("release");
        drop(permit);
    });
    waiting.recv().expect("owner acquired");
    let scope = ObservationScope::begin(&conn);
    let error = match acquire_with_deadline(
        &conn,
        WriteCategory::RuntimeEvent,
        || false,
        Duration::from_millis(30),
    ) {
        Ok(_) => panic!("must time out"),
        Err(error) => error,
    };
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    let observations = scope.observations();
    let blocked = observations
        .iter()
        .find(|event| event.operation == StorageOperation::QueueTimeout)
        .expect("queue timeout");
    let owner = blocked.blocking_owner.as_ref().expect("owner");
    assert_eq!(owner.category, WriteCategory::RuntimeEvent);
    assert_eq!(
        owner.execution.as_ref().expect("execution").activity,
        WriteActivity::Preparation
    );
    assert_eq!(
        owner.execution.as_ref().expect("execution").run_id,
        "01M6PFXFYMVAAEQCQXVKFJJV27"
    );
    assert!(owner.held_ms >= 30);
    assert!(
        !observations
            .iter()
            .any(|event| event.operation == StorageOperation::Begin)
    );
    let failure = crate::persistence_failures::PersistenceFailure {
        thread_id: "01M6PDYTPK83CNB2WENXDAYYE4".into(),
        run_id: "01M6PDYTPK83CNB2WENXDAYYE5".into(),
        message_id: "01M6PDYTPK83CNB2WENXDAYYE5".into(),
        occurred_at: 10,
        category: PersistenceCategory::ResponseStarted,
        elapsed_ms: 30,
        sqlite_extended_code: Some(5),
        observations,
    };
    crate::persistence_failures::record(&conn, failure.clone()).expect("save while owner held");
    for _ in 0..300 {
        record_persistence(
            &conn,
            PersistenceCategory::TextDelta,
            Instant::now(),
            true,
            None,
        );
    }
    let saved =
        crate::persistence_failures::read_by_run(&conn, &failure.thread_id, &failure.run_id)
            .expect("saved");
    assert_eq!(saved, vec![failure.clone()]);
    release.send(()).expect("release owner");
    worker.join().expect("owner join");
    drop(scope);
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn successful_transaction_has_command_scoped_begin_and_commit_without_failure_file() {
    let conn = Connection::open_in_memory().expect("memory");
    let scope = ObservationScope::begin(&conn);
    let transaction =
        WriteTransaction::new(&conn, WriteCategory::RuntimeEvent).expect("transaction");
    transaction
        .execute_batch("CREATE TABLE sample(value INTEGER)")
        .expect("write");
    transaction.commit().expect("commit");
    let events = scope.observations();
    assert!(
        events
            .iter()
            .any(|event| event.operation == StorageOperation::Begin && event.succeeded)
    );
    assert!(
        events
            .iter()
            .any(|event| event.operation == StorageOperation::Commit && event.succeeded)
    );
    assert!(
        !events
            .iter()
            .any(|event| event.operation == StorageOperation::QueueTimeout)
    );
}
