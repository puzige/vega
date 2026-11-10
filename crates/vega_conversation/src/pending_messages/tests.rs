use super::*;
use crate::database_writer::{DatabaseCommand, WriteRequest};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    store: Store,
    message: PendingMessage,
}

struct Prepared {
    authority: crate::types::PendingInputAuthority,
    input: RuntimeSteeringInput,
    source: QueueSteeringSource,
    claim: PendingWrite,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-pending-{}", ulid::Ulid::generate()));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let thread = ulid::Ulid::generate().to_string();
        store.conn().execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project',?1,'fixture',1,1)",
            [workspace.to_str().ok_or("workspace encoding")?],
        )?;
        store.conn().execute(
            "INSERT INTO threads(id,project_id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'project','model','execute','confirm',1,1)",
            [&thread],
        )?;
        let row = vega_store::threads::find(store.conn(), &thread)?.ok_or("thread missing")?;
        let fence = crate::permission_contracts::capture_fence(&store, &row)?;
        let message = PendingMessage {
            id: ulid::Ulid::generate().to_string(),
            thread_id: thread,
            generation: "generation".into(),
            behavior: PendingMessageBehavior::Steer,
            content: "queued instruction".into(),
            images: Vec::new(),
            references: Vec::new(),
            reference_mentions: Vec::new(),
            skill_mentions: Vec::new(),
            permission_fence: Some(fence),
        };
        enqueue(&store, &message)?;
        Ok(Self {
            root,
            workspace,
            store,
            message,
        })
    }

    fn source(&self) -> Result<QueueSteeringSource, Box<dyn std::error::Error>> {
        let source = QueueSteeringSource::new(
            self.root.join("fixture.db"),
            self.workspace.clone(),
            self.message.thread_id.clone(),
            self.message.generation.clone(),
            Arc::new(|| Ok(Vec::new())),
            CancellationToken::new(),
        );
        source
            .authority
            .set(
                self.message
                    .permission_fence
                    .clone()
                    .ok_or("fence missing")?,
            )
            .map_err(|_| "authority already set")?;
        Ok(source)
    }

    fn prepare(&self) -> Result<Prepared, Box<dyn std::error::Error>> {
        let authority = prepare_dispatch(
            &self.store,
            &self.message.thread_id,
            &self.message.id,
            &self.message.content,
            &self.message.images,
            &self.message.references,
        )?;
        let source = self.source()?;
        let runtime = tokio::runtime::Runtime::new()?;
        let input = runtime
            .block_on(source.take(Vec::new()))?
            .ok_or("input missing")?;
        let staged = source.prepared.lock().map_err(|_| "prepared poisoned")?;
        let staged = staged.as_ref().ok_or("prepared missing")?;
        let claim = PendingWrite::Claim {
            row: staged.row.clone(),
            seq: input.user_message_seq,
            admission: staged.admission.clone(),
            cancel: source.cancel.clone(),
            fence: self
                .message
                .permission_fence
                .clone()
                .ok_or("fence missing")?,
            skill_generation: vega_store::skills::read_settings(self.store.conn())?
                .consent_generation,
        };
        Ok(Prepared {
            authority,
            input,
            source: source.clone(),
            claim,
        })
    }

    fn state(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.store.conn().query_row(
            "SELECT state FROM pending_messages WHERE id=?1",
            [&self.message.id],
            |row| row.get(0),
        )?)
    }

    fn assert_no_claim_or_messages(&self) -> TestResult {
        assert_eq!(self.state()?, "pending");
        let count: i64 =
            self.store
                .conn()
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        assert_eq!(count, 0);
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct VerifySources {
    authority: crate::types::PendingInputAuthority,
    input: RuntimeSteeringInput,
}

impl crate::database_writer::Sealed for VerifySources {}

impl DatabaseCommand for VerifySources {
    type Output = Result<(bool, bool), ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _permission = crate::permission_contracts::coordinate()?;
        let _tx = store.immediate_transaction().map_err(|_| failure())?;
        let thread = &self.authority.message.thread_id;
        Ok((
            verify_dispatch(store, &self.authority, thread, &self.input.id).is_ok(),
            verify_steering_acceptance(store, &self.input, thread).is_ok(),
        ))
    }
}

struct Queued<C: DatabaseCommand> {
    command: C,
    reply: SyncSender<C::Output>,
}

impl<C: DatabaseCommand> WriteRequest for Queued<C> {
    fn execute(self: Box<Self>, store: &Store) {
        let _ = self.reply.send(self.command.apply(store));
    }
}

struct Gate {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

impl WriteRequest for Gate {
    fn execute(self: Box<Self>, _store: &Store) {
        self.entered.send(()).expect("gate entered");
        self.release
            .recv_timeout(Duration::from_secs(10))
            .expect("gate release");
    }
}

fn queue<C: DatabaseCommand>(
    writer: &crate::agent::persistence::database_writer::DatabaseWriter,
    command: C,
) -> Result<Receiver<C::Output>, VegaError> {
    let (reply, receipt) = mpsc::sync_channel(1);
    writer.submit(Box::new(Queued { command, reply }))?;
    Ok(receipt)
}

#[test]
fn missing_workspace_rejects_dispatch_and_steering_preparation() -> TestResult {
    let fixture = Fixture::new()?;
    std::fs::remove_dir(&fixture.workspace)?;
    assert!(validate_authority(&fixture.store, &fixture.message).is_err());
    assert!(
        prepare_dispatch(
            &fixture.store,
            &fixture.message.thread_id,
            &fixture.message.id,
            &fixture.message.content,
            &[],
            &[],
        )
        .is_err()
    );
    let source = fixture.source()?;
    let runtime = tokio::runtime::Runtime::new()?;
    assert!(runtime.block_on(source.take(Vec::new())).is_err());
    assert!(
        source
            .prepared
            .lock()
            .map_err(|_| "prepared poisoned")?
            .is_none()
    );
    fixture.assert_no_claim_or_messages()
}

#[test]
fn prepared_commits_use_registered_workspace_without_directory_probe() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = fixture.prepare()?;
    let expected = rows::list(fixture.store.conn(), &fixture.message.thread_id)?;
    let mut next = expected.first().ok_or("pending missing")?.clone();
    next.id = ulid::Ulid::generate().to_string();
    let enqueue = PendingWrite::Enqueue {
        expected,
        row: next,
        fence: fixture
            .message
            .permission_fence
            .clone()
            .ok_or("fence missing")?,
    };
    std::fs::remove_dir(&fixture.workspace)?;
    assert!(validate_authority(&fixture.store, &fixture.message).is_err());
    let runtime = tokio::runtime::Runtime::new()?;
    assert!(
        runtime
            .block_on(prepared.source.acknowledge(&prepared.input))
            .is_err()
    );
    fixture.assert_no_claim_or_messages()?;
    let verified = crate::database_writer::commit(
        fixture.store.database_path().ok_or("database missing")?,
        fixture.store.database_identity(),
        VerifySources {
            authority: prepared.authority,
            input: prepared.input,
        },
    )??;
    assert_eq!(verified, (true, true));
    commit_pending(&fixture.store, prepared.claim)?;
    assert_eq!(fixture.state()?, "claimed");
    commit_pending(&fixture.store, enqueue)?;
    assert_eq!(
        rows::count(fixture.store.conn(), &fixture.message.thread_id)?,
        2
    );
    assert!(!fixture.workspace.exists());
    Ok(())
}

#[test]
fn queued_permission_changes_reject_sources_and_claim_without_partial_write() -> TestResult {
    for change in ["revision", "epoch", "exact_rule", "workspace_identity"] {
        let fixture = Fixture::new()?;
        let prepared = fixture.prepare()?;
        let writer = crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(
            fixture.root.join("fixture.db"),
        )?;
        let (entered, started) = mpsc::sync_channel(1);
        let (release, waiting) = mpsc::sync_channel(1);
        writer.submit(Box::new(Gate {
            entered,
            release: waiting,
        }))?;
        started.recv_timeout(Duration::from_secs(10))?;
        let verified = queue(
            &writer,
            VerifySources {
                authority: prepared.authority,
                input: prepared.input,
            },
        )?;
        let claimed = queue(&writer, prepared.claim)?;
        {
            let _permission = crate::permission_contracts::coordinate()?;
            match change {
                "revision" => {
                    fixture.store.conn().execute(
                        "UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1",
                        [&fixture.message.thread_id],
                    )?;
                }
                "epoch" => {
                    fixture.store.conn().execute(
                        "UPDATE permission_contract_state SET authority_epoch=?1 WHERE singleton=1",
                        [ulid::Ulid::generate().to_string()],
                    )?;
                }
                "exact_rule" => {
                    fixture.store.conn().execute("INSERT INTO permissions(project_id,tool,pattern,created_at) VALUES('project','bash','pwd',1)", [])?;
                }
                "workspace_identity" => {
                    fixture.store.conn().execute(
                        "UPDATE projects SET path=?1 WHERE id='project'",
                        [fixture
                            .root
                            .join("other-workspace")
                            .to_str()
                            .ok_or("workspace encoding")?],
                    )?;
                }
                _ => unreachable!(),
            }
        }
        release.send(())?;
        assert_eq!(
            verified.recv_timeout(Duration::from_secs(10))??,
            (false, false),
            "{change}"
        );
        assert!(
            claimed.recv_timeout(Duration::from_secs(10))?.is_err(),
            "{change}"
        );
        fixture.assert_no_claim_or_messages()?;
    }
    Ok(())
}

#[test]
fn queued_source_changes_reject_stale_dispatch_and_claim_without_partial_write() -> TestResult {
    for change in ["payload", "generation"] {
        let fixture = Fixture::new()?;
        let prepared = fixture.prepare()?;
        let writer = crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(
            fixture.root.join("fixture.db"),
        )?;
        let (entered, started) = mpsc::sync_channel(1);
        let (release, waiting) = mpsc::sync_channel(1);
        writer.submit(Box::new(Gate {
            entered,
            release: waiting,
        }))?;
        started.recv_timeout(Duration::from_secs(10))?;
        let verified = queue(
            &writer,
            VerifySources {
                authority: prepared.authority,
                input: prepared.input,
            },
        )?;
        let claimed = queue(&writer, prepared.claim)?;
        if change == "payload" {
            let mut payload: serde_json::Value = serde_json::from_str(
                &rows::find(
                    fixture.store.conn(),
                    &fixture.message.thread_id,
                    &fixture.message.id,
                )?
                .ok_or("pending missing")?
                .payload,
            )?;
            payload["content"] = serde_json::Value::String("changed instruction".into());
            fixture.store.conn().execute(
                "UPDATE pending_messages SET payload=?1 WHERE id=?2",
                [serde_json::to_string(&payload)?, fixture.message.id.clone()],
            )?;
        } else {
            fixture.store.conn().execute(
                "UPDATE pending_messages SET generation='new-generation' WHERE id=?1",
                [&fixture.message.id],
            )?;
        }
        let changed = rows::find(
            fixture.store.conn(),
            &fixture.message.thread_id,
            &fixture.message.id,
        )?;
        release.send(())?;
        let (dispatch, steering) = verified.recv_timeout(Duration::from_secs(10))??;
        assert!(!dispatch, "{change}");
        if change == "payload" {
            assert!(!steering);
        }
        assert!(
            claimed.recv_timeout(Duration::from_secs(10))?.is_err(),
            "{change}"
        );
        assert!(
            rows::find(
                fixture.store.conn(),
                &fixture.message.thread_id,
                &fixture.message.id
            )? == changed
        );
        fixture.assert_no_claim_or_messages()?;
    }
    Ok(())
}
