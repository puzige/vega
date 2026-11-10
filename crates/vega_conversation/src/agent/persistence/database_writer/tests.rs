use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-writer-{}", ulid::Ulid::generate()));
        fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        Ok(Self { root, store })
    }

    async fn actor(&self) -> Result<PersistenceActor, VegaError> {
        self.actor_with_config(PersistenceActorConfig::default())
            .await
    }

    async fn actor_with_config(
        &self,
        config: PersistenceActorConfig,
    ) -> Result<PersistenceActor, VegaError> {
        PersistenceActor::start(self.start_with_config(config)?).await
    }

    fn start_with_config(
        &self,
        config: PersistenceActorConfig,
    ) -> Result<PersistenceActorStart, VegaError> {
        let thread = ulid::Ulid::generate().to_string();
        let message = ulid::Ulid::generate().to_string();
        self.store.conn().execute("INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'model','execute','confirm',1,1)", [&thread])?;
        self.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [&message, &thread])?;
        crate::threads::task_workspace_root(&self.store, &thread)
            .map_err(|_| persistence_actor_error("fixture workspace"))?;
        let row = vega_store::threads::find(self.store.conn(), &thread)?
            .ok_or_else(|| persistence_actor_error("fixture thread"))?;
        let fence = crate::permission_contracts::capture_fence(&self.store, &row)
            .map_err(|_| persistence_actor_error("fixture fence"))?;
        let tx = self.store.immediate_transaction()?;
        let encoded = serde_json::to_string(&fence)
            .map_err(|_| persistence_actor_error("fixture encoding"))?;
        vega_store::permission_contracts::record_run_fence(&tx, &message, &thread, &encoded)?;
        tx.commit()?;
        let admission =
            super::super::super::ordinary::verified_admission(&self.store, &thread, None)
                .map_err(|_| persistence_actor_error("fixture admission"))?;
        Ok(PersistenceActorStart {
            database_path: self.root.join("fixture.db"),
            project_id: String::new(),
            thread_id: thread,
            message_id: message,
            model: "model".into(),
            is_plan: false,
            next_tool_seq: 1,
            config: config.with_thread_admission(admission),
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn same_database_and_path_alias_share_writer_but_other_database_does_not() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    let first = fixture.actor().await?;
    let second = fixture.actor().await?;
    let third = other.actor().await?;
    let alias = fixture.root.join("alias.db");
    std::os::unix::fs::symlink(fixture.root.join("fixture.db"), &alias)?;
    let aliased = DatabaseWriter::open(alias).await?;
    assert!(Arc::ptr_eq(&first.sender.writer, &second.sender.writer));
    assert!(Arc::ptr_eq(&first.sender.writer, &aliased));
    assert!(!Arc::ptr_eq(&first.sender.writer, &third.sender.writer));
    first.snapshot("first".into()).await?;
    second.snapshot("second".into()).await?;
    first.close().await?;
    second.close().await?;
    third.close().await?;
    Ok(())
}

#[tokio::test]
async fn dropped_receipt_still_commits_and_last_sender_close_drains() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let (ack, received) = oneshot::channel();
    drop(received);
    actor
        .sender
        .send(PersistenceCommand::Snapshot {
            operation_id: ulid::Ulid::generate().to_string(),
            content: "saved despite lost receipt".into(),
            ack,
        })
        .await?;
    tokio::time::timeout(Duration::from_secs(2), actor.close()).await??;
    let content: String =
        fixture
            .store
            .conn()
            .query_row("SELECT content FROM messages", [], |row| row.get(0))?;
    assert_eq!(content, "saved despite lost receipt");
    Ok(())
}

#[tokio::test]
async fn full_queue_cancel_does_not_enqueue_and_other_database_can_commit() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    let cancel = CancellationToken::new();
    let config = PersistenceActorConfig {
        steering_cancel: Some(cancel.clone()),
        ..Default::default()
    };
    let actor = fixture.actor_with_config(config).await?;
    let other_actor = other.actor().await?;
    let blocker = Store::open(fixture.root.join("fixture.db"))?;
    let tx = blocker.immediate_transaction()?;
    let mut receipts = Vec::new();
    for index in 0..PERSISTENCE_CHANNEL_CAPACITY {
        let (ack, received) = oneshot::channel();
        actor
            .sender
            .send(PersistenceCommand::Snapshot {
                operation_id: ulid::Ulid::generate().to_string(),
                content: format!("accepted {index}"),
                ack,
            })
            .await?;
        receipts.push(received);
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    loop {
        let (ack, received) = oneshot::channel();
        match actor
            .sender
            .sender
            .as_ref()
            .ok_or("sender closed")?
            .try_send(PersistenceCommand::Snapshot {
                operation_id: ulid::Ulid::generate().to_string(),
                content: "accepted overflow".into(),
                ack,
            }) {
            Ok(()) => receipts.push(received),
            Err(mpsc::error::TrySendError::Full(_)) => break,
            Err(_) => return Err("sender disconnected".into()),
        }
    }
    let mut queued = Box::pin(actor.snapshot("cancelled write must not appear".into()));
    tokio::select! { result = &mut queued => return Err(format!("queue unexpectedly writable: {result:?}").into()), _ = tokio::time::sleep(Duration::from_millis(30)) => {} }
    cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut queued)
            .await
            .map_err(|_| "cancelled reserve did not exit")?
            .is_err()
    );
    drop(queued);
    tokio::time::timeout(
        Duration::from_millis(200),
        other_actor.snapshot("independent".into()),
    )
    .await
    .map_err(|_| "other database did not commit")??;
    tx.rollback()?;
    for received in receipts {
        received.await??;
    }
    actor.close().await?;
    other_actor.close().await?;
    let content: String =
        fixture
            .store
            .conn()
            .query_row("SELECT content FROM messages", [], |row| row.get(0))?;
    assert!(content.starts_with("accepted "));
    Ok(())
}

#[tokio::test]
async fn permission_revision_change_rejects_queued_tool_proposal() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    fixture.store.conn().execute(
        "UPDATE threads SET permission_revision=permission_revision+1",
        [],
    )?;
    let result = actor
        .event(
            RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall {
                id: "revoked-proposal".into(),
                name: "read".into(),
                input_json: "{}".into(),
                description: None,
            }),
            String::new(),
            None,
        )
        .await;
    assert!(result.is_err());
    actor.close().await?;
    let count: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT count(*) FROM tool_calls", [], |row| row.get(0))?;
    assert_eq!(count, 0);
    Ok(())
}

#[tokio::test]
async fn duplicate_usage_operation_commits_only_one_accounting_row() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let operation_id = ulid::Ulid::generate().to_string();
    for _ in 0..2 {
        let (ack, received) = oneshot::channel();
        actor
            .sender
            .send(PersistenceCommand::Event {
                operation_id: operation_id.clone(),
                event: Box::new(RuntimeEvent::UsageUpdated {
                    usage: vega_runtime::RuntimeTokenUsage {
                        input: 10,
                        output: 4,
                        cache_read: None,
                        cache_write: None,
                    },
                    cost_microcents: 17,
                    pricing: None,
                }),
                content: String::new(),
                execution_duration_ms: None,
                ack,
            })
            .await?;
        received.await??;
    }
    actor.close().await?;
    let count: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT count(*) FROM token_usage", [], |row| row.get(0))?;
    let receipts: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM persistence_command_receipts WHERE state='committed'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    assert_eq!(receipts, 1);
    Ok(())
}

#[tokio::test]
async fn pending_operation_never_replays_unconfirmed_business_write() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let thread: String = fixture
        .store
        .conn()
        .query_row("SELECT id FROM threads", [], |row| row.get(0))?;
    let message: String = fixture
        .store
        .conn()
        .query_row("SELECT id FROM messages", [], |row| row.get(0))?;
    let operation_id = ulid::Ulid::generate().to_string();
    save_receipt(
        fixture.store.conn(),
        &operation_id,
        &thread,
        &message,
        "snapshot",
        "pending",
    )?;
    let (ack, received) = oneshot::channel();
    actor
        .sender
        .send(PersistenceCommand::Snapshot {
            operation_id,
            content: "must not appear".into(),
            ack,
        })
        .await?;
    assert!(received.await?.is_err());
    actor.close().await?;
    let content: String =
        fixture
            .store
            .conn()
            .query_row("SELECT content FROM messages", [], |row| row.get(0))?;
    assert!(content.is_empty());
    Ok(())
}

#[tokio::test]
async fn closing_one_run_keeps_other_run_live_and_reopening_drains() -> TestResult {
    let fixture = Fixture::new()?;
    let first = fixture.actor().await?;
    let second = fixture.actor().await?;
    first.close().await?;
    second.snapshot("still live".into()).await?;
    second.close().await?;
    for _ in 0..12 {
        let actor = fixture.actor().await?;
        actor.snapshot("reopened".into()).await?;
        tokio::time::timeout(Duration::from_secs(2), actor.close()).await??;
    }
    let count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM messages WHERE content='reopened'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 12);
    Ok(())
}

async fn send_proposal(
    actor: &PersistenceActor,
    operation: &str,
    call: &str,
) -> Result<(), VegaError> {
    let (ack, received) = oneshot::channel();
    actor
        .sender
        .send(PersistenceCommand::Event {
            operation_id: operation.to_owned(),
            event: Box::new(RuntimeEvent::ToolCallProposed(
                vega_runtime::RuntimeToolCall {
                    id: call.to_owned(),
                    name: "read".into(),
                    input_json: "{\"path\":\"fixture.txt\"}".into(),
                    description: None,
                },
            )),
            content: String::new(),
            execution_duration_ms: None,
            ack,
        })
        .await?;
    received
        .await
        .map_err(|_| persistence_actor_error("test receipt lost"))??;
    Ok(())
}

#[tokio::test]
async fn replaying_older_receipt_does_not_regress_sequence_and_restart_uses_latest_watermark()
-> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let operation = ulid::Ulid::generate().to_string();
    send_proposal(&actor, &operation, "first-proposal").await?;
    send_proposal(
        &actor,
        &ulid::Ulid::generate().to_string(),
        "second-proposal",
    )
    .await?;
    send_proposal(&actor, &operation, "first-proposal").await?;
    send_proposal(
        &actor,
        &ulid::Ulid::generate().to_string(),
        "third-proposal",
    )
    .await?;
    actor.close().await?;
    let thread: String = fixture
        .store
        .conn()
        .query_row("SELECT id FROM threads", [], |row| row.get(0))?;
    let message: String = fixture
        .store
        .conn()
        .query_row("SELECT id FROM messages", [], |row| row.get(0))?;
    let admission = crate::agent::ordinary::verified_admission(&fixture.store, &thread, None)
        .map_err(|_| persistence_actor_error("restart admission"))?;
    let reopened = PersistenceActor::start(PersistenceActorStart {
        database_path: fixture.root.join("fixture.db"),
        project_id: String::new(),
        thread_id: thread,
        message_id: message,
        model: "model".into(),
        is_plan: false,
        next_tool_seq: 1,
        config: PersistenceActorConfig::default().with_thread_admission(admission),
    })
    .await?;
    send_proposal(
        &reopened,
        &ulid::Ulid::generate().to_string(),
        "fourth-proposal",
    )
    .await?;
    reopened.close().await?;
    let sequences = fixture
        .store
        .conn()
        .prepare("SELECT seq FROM tool_calls ORDER BY seq")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(sequences, vec![1, 2, 3, 4]);
    Ok(())
}

#[tokio::test]
async fn response_receipt_replay_preserves_original_timestamp() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let operation = ulid::Ulid::generate().to_string();
    let mut clocks = Vec::new();
    for _ in 0..2 {
        let (ack, received) = oneshot::channel();
        actor
            .sender
            .send(PersistenceCommand::Event {
                operation_id: operation.clone(),
                event: Box::new(RuntimeEvent::AssistantResponseStarted {
                    response_id: "response-one".into(),
                    text_start_bytes: 0,
                }),
                content: String::new(),
                execution_duration_ms: None,
                ack,
            })
            .await?;
        let metadata = received.await??;
        let clock = metadata
            .into_iter()
            .find_map(|event| match event {
                ConversationEvent::MessageTimestamp { sent_at_ms, .. } => Some(sent_at_ms),
                _ => None,
            })
            .ok_or("timestamp metadata missing")?;
        assert!(clock.is_some());
        clocks.push(clock);
    }
    assert_eq!(clocks[0], clocks[1]);
    actor.close().await?;
    Ok(())
}

#[tokio::test]
async fn receipt_failure_rolls_back_usage_and_does_not_leave_partial_business_write() -> TestResult
{
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON persistence_command_receipts BEGIN SELECT RAISE(ABORT,'receipt fault'); END;")?;
    let result = actor
        .event(
            RuntimeEvent::UsageUpdated {
                usage: vega_runtime::RuntimeTokenUsage {
                    input: 10,
                    output: 4,
                    cache_read: None,
                    cache_write: None,
                },
                cost_microcents: 17,
                pricing: None,
            },
            String::new(),
            None,
        )
        .await;
    assert!(result.is_err());
    actor.close().await?;
    let usage: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT count(*) FROM token_usage", [], |row| row.get(0))?;
    let receipts: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM persistence_command_receipts",
        [],
        |row| row.get(0),
    )?;
    assert_eq!((usage, receipts), (0, 0));
    Ok(())
}

struct AdmissionGate {
    started: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl crate::database_writer::Sealed for AdmissionGate {}
impl crate::database_writer::DatabaseCommand for AdmissionGate {
    type Output = Result<(), String>;
    fn apply(self, _: &Store) -> Self::Output {
        self.started.send(()).map_err(|error| error.to_string())?;
        self.release.recv().map_err(|error| error.to_string())
    }
}

#[test]
fn queued_service_cancellation_prevents_execution_after_capacity_acceptance() -> TestResult {
    use crate::database_writer::{CommitCancellation, CommitError, commit, commit_with_admission};
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held_path = path.clone();
    let held = std::thread::spawn(move || {
        commit(
            &held_path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let cancel = CancellationToken::new();
    let cancellation = cancel.clone();
    let queued_path = path.clone();
    let pending = std::thread::spawn(move || {
        commit_with_admission(
            &queued_path,
            identity,
            NecessaryInsert,
            CommitCancellation {
                token: Some(cancellation),
                submission: None,
            },
            Duration::from_secs(2),
        )
    });
    let writer = DatabaseWriter::open_blocking(path)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while writer
        .commands
        .state
        .lock()
        .map_err(|_| "queue poisoned")?
        .commands
        .is_empty()
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    cancel.cancel();
    assert!(matches!(
        pending.join().map_err(|_| "submit panicked")?,
        Err(CommitError::CancelledBeforeStart)
    ));
    release.send(())?;
    held.join().map_err(|_| "gate panicked")???;
    writer.begin_close()?;
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    let count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='necessary_drain'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn started_service_waits_for_real_result_despite_cancelled_caller() -> TestResult {
    use crate::database_writer::{CommitCancellation, commit_with_admission};
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let cancel = CancellationToken::new();
    let cancellation = cancel.clone();
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let (completed, receipt) = std::sync::mpsc::channel();
    let pending = std::thread::spawn(move || {
        let result = commit_with_admission(
            &path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
            CommitCancellation {
                token: Some(cancellation),
                submission: None,
            },
            Duration::from_secs(2),
        );
        completed.send(result).unwrap();
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    cancel.cancel();
    assert!(matches!(
        receipt.recv_timeout(Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    release.send(())?;
    receipt.recv_timeout(Duration::from_secs(2))???;
    pending.join().map_err(|_| "submit panicked")?;
    Ok(())
}

#[test]
fn queued_service_budget_expiration_does_not_execute_business_command() -> TestResult {
    use crate::database_writer::{CommitCancellation, CommitError, commit, commit_with_admission};
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held_path = path.clone();
    let held = std::thread::spawn(move || {
        commit(
            &held_path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let result = commit_with_admission(
        &path,
        identity,
        NecessaryInsert,
        CommitCancellation::default(),
        Duration::from_millis(30),
    );
    assert!(matches!(result, Err(CommitError::BudgetBeforeStart)));
    release.send(())?;
    held.join().map_err(|_| "gate panicked")???;
    let writer = DatabaseWriter::open_blocking(path)?;
    writer.begin_close()?;
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    let count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='necessary_drain'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

struct BudgetedBegin;
impl crate::database_writer::Sealed for BudgetedBegin {}
impl crate::database_writer::DatabaseCommand for BudgetedBegin {
    type Output = (Duration, Result<(), String>);
    fn apply(self, _: &Store) -> Self::Output {
        panic!("admission budget was not passed")
    }
    fn apply_with_admission(self, store: &Store, remaining: Duration) -> Self::Output {
        let result = (|| {
            store
                .conn()
                .busy_timeout(Duration::ZERO)
                .map_err(|error| error.to_string())?;
            let tx = vega_store::write_coordination::WriteTransaction::new_with_budget(
                store.conn(),
                vega_store::write_coordination::WriteCategory::RuntimeEvent,
                || false,
                remaining,
            )
            .map_err(|error| error.to_string())?;
            tx.execute_batch("CREATE TABLE budget_probe(value INTEGER);")
                .map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())
        })();
        (remaining, result)
    }
}

#[test]
fn service_queue_wait_and_engine_busy_share_one_remaining_budget() -> TestResult {
    use crate::database_writer::{CommitCancellation, commit, commit_with_admission};
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held_path = path.clone();
    let held = std::thread::spawn(move || {
        commit(
            &held_path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let writer = DatabaseWriter::open_blocking(path.clone())?;
    let pending = std::thread::spawn(move || {
        commit_with_admission(
            &path,
            identity,
            BudgetedBegin,
            CommitCancellation::default(),
            Duration::from_millis(500),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while writer
        .commands
        .state
        .lock()
        .map_err(|_| "queue poisoned")?
        .commands
        .is_empty()
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.store.conn().execute_batch("BEGIN IMMEDIATE")?;
    std::thread::sleep(Duration::from_millis(150));
    release.send(())?;
    held.join().map_err(|_| "gate panicked")???;
    let (remaining, result) = pending.join().map_err(|_| "submit panicked")??;
    fixture.store.conn().execute_batch("ROLLBACK")?;
    assert!(remaining <= Duration::from_millis(350));
    assert!(!remaining.is_zero());
    assert!(result.is_err());
    let count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='budget_probe'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

struct NecessaryInsert;
impl crate::database_writer::Sealed for NecessaryInsert {}
impl crate::database_writer::DatabaseCommand for NecessaryInsert {
    type Output = Result<(), String>;
    fn apply(self, store: &Store) -> Self::Output {
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        tx.execute_batch(
            "CREATE TABLE necessary_drain(value INTEGER); INSERT INTO necessary_drain VALUES(1);",
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }
}

#[test]
fn reopening_after_last_producer_release_reuses_live_database_actor() -> TestResult {
    let fixture = Fixture::new()?;
    let writer = DatabaseWriter::open_blocking(fixture.root.join("fixture.db"))?;
    let original = Arc::downgrade(&writer);
    drop(writer);
    let reopened = DatabaseWriter::open_blocking(fixture.root.join("fixture.db"))?;
    assert!(Arc::ptr_eq(
        &original
            .upgrade()
            .ok_or("writer disappeared before shutdown")?,
        &reopened
    ));
    reopened.begin_close()?;
    reopened.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    Ok(())
}

#[tokio::test]
async fn shutdown_waits_for_active_run_and_reserved_terminal_even_without_receipt() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let writer = actor.sender.writer.clone();
    let terminal = writer.reserve_necessary()?;
    writer.begin_close()?;
    assert!(writer.reserve_necessary().is_err());
    assert!(
        writer
            .wait_stopped(std::time::Instant::now() + Duration::from_millis(10))
            .is_err()
    );
    actor
        .snapshot("accepted run settles during close".into())
        .await?;
    actor.close().await?;
    assert!(
        writer
            .wait_stopped(std::time::Instant::now() + Duration::from_millis(10))
            .is_err()
    );
    let receipt = terminal.submit(fixture.store.database_identity(), NecessaryInsert)?;
    drop(receipt);
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    let count: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT count(*) FROM necessary_drain", [], |row| row.get(0))?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn necessary_terminal_failure_is_not_reported_as_successful_shutdown() -> TestResult {
    let fixture = Fixture::new()?;
    fixture
        .store
        .conn()
        .execute_batch("CREATE TABLE necessary_drain(value INTEGER);")?;
    let writer = DatabaseWriter::open_blocking(fixture.root.join("fixture.db"))?;
    let terminal = writer.reserve_necessary()?;
    writer.begin_close()?;
    let receipt = terminal.submit(fixture.store.database_identity(), NecessaryInsert)?;
    assert!(receipt.recv_timeout(Duration::from_secs(2))?.is_err());
    assert!(
        writer
            .wait_stopped(std::time::Instant::now() + Duration::from_secs(2))
            .is_err()
    );
    let count: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT count(*) FROM necessary_drain", [], |row| row.get(0))?;
    assert_eq!(count, 0);
    Ok(())
}

#[tokio::test]
async fn moved_database_rejects_command_instead_of_writing_replacement() -> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let path = fixture.root.join("fixture.db");
    assert!(
        fixture
            .store
            .verify_database_identity(fixture.store.database_identity())
            .is_ok()
    );
    std::fs::rename(&path, fixture.root.join("original.db"))?;
    let replacement = Store::open(&path)?;
    replacement.migrate()?;
    assert!(
        fixture
            .store
            .verify_database_identity(fixture.store.database_identity())
            .is_err()
    );
    assert!(actor.snapshot("must-not-persist".into()).await.is_err());
    actor.close().await?;
    let old_count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM messages WHERE content='must-not-persist'",
        [],
        |row| row.get(0),
    )?;
    let new_count: i64 =
        replacement
            .conn()
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))?;
    assert_eq!((old_count, new_count), (0, 0));
    drop(replacement);
    Ok(())
}

struct PendingRun {
    sender: mpsc::Sender<PersistenceCommand>,
    ready: oneshot::Receiver<Result<(), VegaError>>,
    drained: oneshot::Receiver<Result<(), VegaError>>,
}

fn pending_run(start: &PersistenceActorStart) -> (RunRegistration, PendingRun) {
    let (sender, receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
    let (ready, opened) = oneshot::channel();
    let (drained, completed) = oneshot::channel();
    (
        RunRegistration {
            state: RunState {
                start: PersistenceActorStart {
                    database_path: start.database_path.clone(),
                    project_id: start.project_id.clone(),
                    thread_id: start.thread_id.clone(),
                    message_id: start.message_id.clone(),
                    model: start.model.clone(),
                    is_plan: start.is_plan,
                    next_tool_seq: start.next_tool_seq,
                    config: start.config.clone(),
                },
                ordinary_task: None,
                run_id: start.message_id.clone(),
                active_response_id: None,
            },
            receiver,
            ready,
            drained,
            capacity: None,
        },
        PendingRun {
            sender,
            ready: opened,
            drained: completed,
        },
    )
}

struct ReservedTerminalInsert;
impl crate::database_writer::Sealed for ReservedTerminalInsert {}
impl crate::database_writer::DatabaseCommand for ReservedTerminalInsert {
    type Output = Result<(), String>;
    fn apply(self, store: &Store) -> Self::Output {
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        tx.execute_batch(
            "CREATE TABLE reserved_terminal_drain(value INTEGER); INSERT INTO reserved_terminal_drain VALUES(1);",
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }
}

#[tokio::test]
async fn full_run_capacity_preserves_service_terminal_and_reopens_only_after_drain() -> TestResult {
    assert_eq!(RUN_CAPACITY, 128);
    let fixture = Fixture::new()?;
    let first = fixture.actor().await?;
    let writer = first.sender.writer.clone();
    let terminal = writer.reserve_necessary()?;
    let mut actors = vec![first];
    for _ in 1..RUN_CAPACITY {
        actors.push(tokio::time::timeout(Duration::from_secs(5), fixture.actor()).await??);
    }
    assert_eq!(writer.run_capacity.available_permits(), 0);
    for _ in 0..2 {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), fixture.actor())
                .await?
                .is_err()
        );
        assert_eq!(writer.run_capacity.available_permits(), 0);
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        actors[0].snapshot("snapshot at run capacity".into()),
    )
    .await??;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let service_path = path.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            crate::database_writer::commit(&service_path, identity, NecessaryInsert)
        }),
    )
    .await????;
    terminal
        .submit(identity, ReservedTerminalInsert)?
        .recv_timeout(Duration::from_secs(2))??;
    let counts: (i64, i64, i64) = fixture.store.conn().query_row(
        "SELECT (SELECT count(*) FROM messages WHERE content='snapshot at run capacity'), (SELECT count(*) FROM necessary_drain), (SELECT count(*) FROM reserved_terminal_drain)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(counts, (1, 1, 1));
    assert_eq!(writer.run_capacity.available_permits(), 0);
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held = tokio::task::spawn_blocking(move || {
        crate::database_writer::commit(
            &path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let mut closing = Box::pin(actors.pop().ok_or("missing full-capacity run")?.close());
    tokio::select! {
        result = &mut closing => panic!("run drained while writer was gated: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
    }
    assert_eq!(writer.run_capacity.available_permits(), 0);
    release.send(())?;
    held.await???;
    tokio::time::timeout(Duration::from_secs(2), &mut closing).await??;
    assert_eq!(writer.run_capacity.available_permits(), 1);
    actors.push(tokio::time::timeout(Duration::from_secs(2), fixture.actor()).await??);
    assert_eq!(writer.run_capacity.available_permits(), 0);
    tokio::time::timeout(Duration::from_secs(5), async {
        for actor in actors {
            actor.close().await?;
        }
        Ok::<(), VegaError>(())
    })
    .await??;
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY);
    writer.begin_close()?;
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    let state = writer
        .lifecycle
        .state
        .lock()
        .map_err(|_| "lifecycle poisoned")?;
    assert!(state.closing && state.stopped && state.settled);
    assert!(
        writer
            .commands
            .state
            .lock()
            .map_err(|_| "queue poisoned")?
            .commands
            .is_empty()
    );
    assert!(
        writer
            .necessary
            .lock()
            .map_err(|_| "necessary queue poisoned")?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn registration_churn_keeps_existing_snapshot_service_and_necessary_progressing() -> TestResult
{
    use std::sync::atomic::{AtomicUsize, Ordering};

    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let template = fixture.start_with_config(PersistenceActorConfig::default())?;
    let writer = actor.sender.writer.clone();
    let identity = fixture.store.database_identity();
    let path = fixture.root.join("fixture.db");
    let terminal = writer.reserve_necessary()?;
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held_path = path.clone();
    let held = tokio::task::spawn_blocking(move || {
        crate::database_writer::commit(
            &held_path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let mut receipts = VecDeque::new();
    for _ in 1..RUN_CAPACITY {
        let (registration, pending) = pending_run(&template);
        writer.register_run(registration)?;
        drop(pending.sender);
        receipts.push_back((pending.ready, pending.drained));
    }
    assert_eq!(writer.run_capacity.available_permits(), 0);
    let stop = CancellationToken::new();
    let stop_guard = stop.clone().drop_guard();
    let attempts = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::new(AtomicUsize::new(0));
    let drained = Arc::new(AtomicUsize::new(0));
    let producer_writer = writer.clone();
    let producer_stop = stop.clone();
    let producer_attempts = attempts.clone();
    let producer_accepted = accepted.clone();
    let producer_drained = drained.clone();
    let producer = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut receipts = receipts;
        while !producer_stop.is_cancelled() {
            producer_attempts.fetch_add(1, Ordering::SeqCst);
            let (registration, pending) = pending_run(&template);
            if producer_writer.register_run(registration).is_ok() {
                drop(pending.sender);
                receipts.push_back((pending.ready, pending.drained));
                producer_accepted.fetch_add(1, Ordering::SeqCst);
            }
            let count = receipts.len();
            for _ in 0..count {
                let (mut ready, mut drained) = receipts.pop_front().ok_or("missing receipt")?;
                match drained.try_recv() {
                    Ok(Ok(())) => {
                        ready
                            .try_recv()
                            .map_err(|error| error.to_string())?
                            .map_err(|error| error.to_string())?;
                        producer_drained.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(Err(error)) => return Err(error.to_string()),
                    Err(oneshot::error::TryRecvError::Empty) => {
                        receipts.push_back((ready, drained))
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            std::thread::yield_now();
        }
        for (ready, drained) in receipts {
            ready
                .blocking_recv()
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            drained
                .blocking_recv()
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while attempts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let service = tokio::task::spawn_blocking(move || {
        crate::database_writer::commit(&path, identity, NecessaryInsert)
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !writer
                .commands
                .state
                .lock()
                .map_err(|_| "queue poisoned")?
                .commands
                .is_empty()
            {
                return Ok::<(), String>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let terminal_receipt = terminal.submit(identity, ReservedTerminalInsert)?;
    let necessary =
        tokio::task::spawn_blocking(move || terminal_receipt.recv_timeout(Duration::from_secs(5)));
    release.send(())?;
    held.await???;
    tokio::time::timeout(Duration::from_secs(5), async {
        let (snapshot, service, necessary) = tokio::join!(
            actor.snapshot("snapshot during registration churn".into()),
            service,
            necessary,
        );
        snapshot?;
        service???;
        necessary???;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await??;
    assert!(!stop.is_cancelled());
    let accepted_at_completion = accepted.load(Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), async {
        while accepted.load(Ordering::SeqCst) <= accepted_at_completion
            || drained.load(Ordering::SeqCst) < RUN_CAPACITY - 1
        {
            assert!(
                !producer.is_finished(),
                "registration producer stopped before completion"
            );
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let counts: (i64, i64, i64) = fixture.store.conn().query_row(
        "SELECT (SELECT count(*) FROM messages WHERE content='snapshot during registration churn'), (SELECT count(*) FROM necessary_drain), (SELECT count(*) FROM reserved_terminal_drain)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(counts, (1, 1, 1));
    assert!(!stop.is_cancelled());
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), producer).await???;
    drop(stop_guard);
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY - 1);
    tokio::time::timeout(Duration::from_secs(2), actor.close()).await??;
    writer.begin_close()?;
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY);
    assert!(
        writer
            .commands
            .state
            .lock()
            .map_err(|_| "queue poisoned")?
            .commands
            .is_empty()
    );
    assert!(
        writer
            .necessary
            .lock()
            .map_err(|_| "necessary queue poisoned")?
            .is_empty()
    );
    let state = writer
        .lifecycle
        .state
        .lock()
        .map_err(|_| "lifecycle poisoned")?;
    assert!(state.closing && state.stopped && state.settled);
    Ok(())
}

#[tokio::test]
async fn cancelled_or_abandoned_registration_releases_capacity_and_close_settles_accepted_run()
-> TestResult {
    let fixture = Fixture::new()?;
    let actor = fixture.actor().await?;
    let writer = actor.sender.writer.clone();
    let before = CancellationToken::new();
    before.cancel();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.actor_with_config(PersistenceActorConfig {
                steering_cancel: Some(before),
                ..Default::default()
            })
        )
        .await?
        .is_err()
    );
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY - 1);
    let queued_cancel = CancellationToken::new();
    let queued_start = fixture.start_with_config(PersistenceActorConfig {
        steering_cancel: Some(queued_cancel.clone()),
        ..Default::default()
    })?;
    let abandoned_start = fixture.start_with_config(PersistenceActorConfig::default())?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let (started, observed) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let held = tokio::task::spawn_blocking(move || {
        crate::database_writer::commit(
            &path,
            identity,
            AdmissionGate {
                started,
                release: gate,
            },
        )
    });
    observed.recv_timeout(Duration::from_secs(2))?;
    let (registration, mut queued) = pending_run(&queued_start);
    writer.register_run(registration)?;
    assert!(matches!(
        queued.ready.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    queued_cancel.cancel();
    let (registration, abandoned) = pending_run(&abandoned_start);
    writer.register_run(registration)?;
    let PendingRun {
        sender: abandoned_sender,
        ready: abandoned_ready,
        drained: abandoned_drained,
    } = abandoned;
    drop(abandoned_ready);
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY - 3);
    writer.begin_close()?;
    assert!(writer.run_capacity.is_closed());
    let (registration, rejected) = pending_run(&abandoned_start);
    assert!(writer.register_run(registration).is_err());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), rejected.ready)
            .await?
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), fixture.actor())
            .await?
            .is_err()
    );
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY - 3);
    assert!(
        writer
            .wait_stopped(std::time::Instant::now() + Duration::from_millis(10))
            .is_err()
    );
    release.send(())?;
    held.await???;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), queued.ready).await??,
        Err(VegaError::Cancelled)
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), queued.drained).await??,
        Err(VegaError::Cancelled)
    ));
    assert!(
        tokio::time::timeout(Duration::from_secs(2), abandoned_drained)
            .await??
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while writer.run_capacity.available_permits() != RUN_CAPACITY - 1 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(queued.sender.is_closed());
    assert!(abandoned_sender.is_closed());
    tokio::time::timeout(
        Duration::from_secs(2),
        actor.snapshot("accepted run settles after rejected registrations".into()),
    )
    .await??;
    tokio::time::timeout(Duration::from_secs(2), actor.close()).await??;
    writer.wait_stopped(std::time::Instant::now() + Duration::from_secs(2))?;
    assert_eq!(writer.run_capacity.available_permits(), RUN_CAPACITY);
    let count: i64 = fixture.store.conn().query_row(
        "SELECT count(*) FROM messages WHERE content='accepted run settles after rejected registrations'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    assert!(
        writer
            .commands
            .state
            .lock()
            .map_err(|_| "queue poisoned")?
            .commands
            .is_empty()
    );
    assert!(
        writer
            .necessary
            .lock()
            .map_err(|_| "necessary queue poisoned")?
            .is_empty()
    );
    let state = writer
        .lifecycle
        .state
        .lock()
        .map_err(|_| "lifecycle poisoned")?;
    assert!(state.closing && state.stopped && state.settled);
    Ok(())
}
