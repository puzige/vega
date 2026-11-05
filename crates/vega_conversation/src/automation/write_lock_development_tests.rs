use super::*;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

struct Database(PathBuf);

impl Database {
    fn new() -> (Self, Store) {
        let root = std::env::temp_dir().join(format!("vega-automation-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let database = Self(root.join("state.sqlite"));
        let store = Store::open(&database.0).unwrap();
        store.migrate().unwrap();
        (database, store)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn loop_task(store: &Store, thread: &str) -> AutomationTask {
    command(
        store,
        thread,
        &AutomationCommand::Create {
            kind: AutomationKind::Loop,
            instruction: "Inspect synthetic state".into(),
            token_budget: None,
            interval_seconds: Some(60),
        },
        100,
    )
    .unwrap()
    .remove(0)
}

#[test]
fn timing_reads_committed_tasks_and_run_while_another_connection_holds_writer() {
    let (database, store) = Database::new();
    let thread = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let task = loop_task(&store, &thread.id);
    let run = claim(&store, &thread.id, None, 100).unwrap().unwrap();
    let writer = Store::open_existing(&database.0).unwrap();
    let tx = writer.immediate_transaction().unwrap();
    let mut uncommitted = task.clone();
    uncommitted.instruction = "Uncommitted task".into();
    save(&tx, &uncommitted).unwrap();
    let mut uncommitted_run = run.clone();
    uncommitted_run.tokens = 999;
    db::save_run(
        &tx,
        &thread.id,
        &run.id,
        true,
        &encode(&uncommitted_run).unwrap(),
    )
    .unwrap();

    assert_eq!(
        timing_snapshot(&store, &thread.id).unwrap(),
        (vec![task], Some(run))
    );
    assert!(!writer.conn().is_autocommit());
}

#[test]
fn refresh_reads_absent_empty_and_unexpired_threads_without_writer() {
    let (database, store) = Database::new();
    let empty = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let thread = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let task = loop_task(&store, &thread.id);
    let writer = Store::open_existing(&database.0).unwrap();
    let tx = writer.immediate_transaction().unwrap();
    let mut uncommitted = task.clone();
    uncommitted.expires_at_ms = Some(100);
    save(&tx, &uncommitted).unwrap();

    assert!(
        refresh(&store, &ulid::Ulid::generate().to_string(), 100)
            .unwrap()
            .is_empty()
    );
    assert!(refresh(&store, &empty.id, 100).unwrap().is_empty());
    assert_eq!(refresh(&store, &thread.id, 100).unwrap(), vec![task]);
    assert!(!writer.conn().is_autocommit());
}

#[test]
fn refresh_keeps_inactive_expired_and_active_nonexpiring_tasks_read_only() {
    let (database, store) = Database::new();
    let thread = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let mut task = loop_task(&store, &thread.id);
    task.status = AutomationStatus::Paused;
    task.expires_at_ms = Some(100);
    save(store.conn(), &task).unwrap();
    let writer = Store::open_existing(&database.0).unwrap();
    let tx = writer.immediate_transaction().unwrap();
    assert_eq!(
        refresh(&store, &thread.id, 100).unwrap(),
        vec![task.clone()]
    );
    drop(tx);

    task.status = AutomationStatus::Active;
    task.expires_at_ms = None;
    save(store.conn(), &task).unwrap();
    let _tx = writer.immediate_transaction().unwrap();
    assert_eq!(refresh(&store, &thread.id, 100).unwrap(), vec![task]);
}

#[test]
fn concurrent_refresh_expires_a_task_once_at_the_boundary() {
    let (database, store) = Database::new();
    let thread = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let mut task = loop_task(&store, &thread.id);
    task.expires_at_ms = Some(200);
    save(store.conn(), &task).unwrap();
    assert_eq!(
        refresh(&store, &thread.id, 199).unwrap(),
        vec![task.clone()]
    );
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                let database = &database.0;
                let thread = &thread.id;
                scope.spawn(move || {
                    let store = Store::open_existing(database).unwrap();
                    barrier.wait();
                    refresh(&store, thread, 200).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    task.status = AutomationStatus::Expired;
    task.set_system_reason(AutomationReason::LoopExpired);
    task.revision += 1;
    for result in results {
        assert_eq!(result, vec![task.clone()]);
    }
    assert_eq!(load(&store, &thread.id).unwrap(), vec![task.clone()]);
    assert_eq!(refresh(&store, &thread.id, 201).unwrap(), vec![task]);
}
