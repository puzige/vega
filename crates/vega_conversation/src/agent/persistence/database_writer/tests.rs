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
        PersistenceActor::start(PersistenceActorStart {
            database_path: self.root.join("fixture.db"),
            project_id: String::new(),
            thread_id: thread,
            message_id: message,
            model: "model".into(),
            is_plan: false,
            next_tool_seq: 1,
            config: config.with_thread_admission(admission),
        })
        .await
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
