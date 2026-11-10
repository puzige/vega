use super::*;
use crate::database_writer::WriteRequest;

struct LostMessageReceipt {
    command: CodexCommand,
    reply: std::sync::mpsc::SyncSender<Result<CodexReceipt, String>>,
}
impl WriteRequest for LostMessageReceipt {
    fn execute(self: Box<Self>, store: &Store) {
        let _ = self.reply.send(self.command.apply(store));
    }
}
struct Barrier;
impl Sealed for Barrier {}
impl DatabaseCommand for Barrier {
    type Output = ();
    fn apply(self, _store: &Store) {}
}

#[test]
fn lost_message_receipt_commits_once_and_stale_source_does_not_replay()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("vega-codex-receipt-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root)?;
    let root = std::fs::canonicalize(root)?;
    let path = root.join("database.db");
    let store = Store::open(&path)?;
    store.migrate()?;
    let mut draft = crate::threads::draft_thread(None, "model", "confirm");
    draft.backend = TaskBackend::Codex;
    let snapshot = CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "profile".into(),
            display_name: "Profile".into(),
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: "/mock-codex".into(),
        arguments: Vec::new(),
        adapter_version: "1".into(),
        codex_version: "1".into(),
        settings: CodexRunSettings {
            model: None,
            model_provider: None,
            reasoning_effort: None,
            sandbox_mode: CodexSandboxMode::WorkspaceWrite,
            approval_policy: CodexApprovalPolicy::OnRequest,
        },
        workspace: CodexWorkspaceSnapshot {
            project_id: None,
            worktree_id: Some("worktree".into()),
            canonical_working_directory: root.to_string_lossy().into_owned(),
            additional_directories: Vec::new(),
        },
    };
    let startup = crate::codex_tasks::prepare_codex_startup(&path, draft, snapshot)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    startup.persistence.session_outcome(
        CodexSessionOutcome::Confirmed {
            session_id: "session".into(),
        },
        now,
    )?;
    let source = startup.persistence.source.lock().unwrap().clone();
    let command = CodexCommand {
        source,
        operation: CodexWrite::Messages {
            prompt: "question".into(),
            user_id: "user".into(),
            assistant_id: "assistant".into(),
            now,
        },
    };
    let (reply, lost_receipt) = std::sync::mpsc::sync_channel(1);
    drop(lost_receipt);
    let writer =
        crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(path.clone())?;
    writer.submit(Box::new(LostMessageReceipt { command, reply }))?;
    crate::database_writer::commit(&path, store.database_identity(), Barrier)?;
    let count = || {
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| {
                row.get::<_, i64>(0)
            })
    };
    assert_eq!(count()?, 2);
    assert!(
        startup
            .persistence
            .create_messages(
                "question".into(),
                "new-user".into(),
                "new-assistant".into(),
                now
            )
            .is_err()
    );
    assert_eq!(count()?, 2);
    assert_eq!(
        vega_store::messages::find(store.conn(), "assistant")?
            .unwrap()
            .seq,
        2
    );
    drop(startup);
    drop(writer);
    drop(store);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct StartupFixture {
    root: PathBuf,
    store: Store,
}

impl StartupFixture {
    fn new() -> Result<(Self, crate::codex_tasks::startup::CodexStartup), Box<dyn std::error::Error>>
    {
        let root =
            std::env::temp_dir().join(format!("vega-codex-settlement-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let root = std::fs::canonicalize(root)?;
        let path = root.join("database.db");
        let store = Store::open(&path)?;
        store.migrate()?;
        let mut draft = crate::threads::draft_thread(None, "model", "confirm");
        draft.backend = TaskBackend::Codex;
        let snapshot = CodexExecutionSnapshot {
            profile: CodexProfileReference {
                id: "profile".into(),
                display_name: "Profile".into(),
            },
            adapter: CodexAdapterKind::CodexAcp,
            executable: "/mock-codex".into(),
            arguments: Vec::new(),
            adapter_version: "1".into(),
            codex_version: "1".into(),
            settings: CodexRunSettings {
                model: None,
                model_provider: None,
                reasoning_effort: None,
                sandbox_mode: CodexSandboxMode::WorkspaceWrite,
                approval_policy: CodexApprovalPolicy::OnRequest,
            },
            workspace: CodexWorkspaceSnapshot {
                project_id: None,
                worktree_id: Some("worktree".into()),
                canonical_working_directory: root.to_string_lossy().into_owned(),
                additional_directories: Vec::new(),
            },
        };
        let startup = crate::codex_tasks::prepare_codex_startup(&path, draft, snapshot)?;
        Ok((Self { root, store }, startup))
    }

    fn state(&self, thread: &str) -> CodexSessionCreationState {
        crate::codex_tasks::codex_task_identity(&self.store, thread)
            .unwrap()
            .unwrap()
            .session_creation
    }

    fn pause(&self) -> Result<PausedWriter, Box<dyn std::error::Error>> {
        let (entered, wait) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let path = self.root.join("database.db");
        let database = self.store.database_identity();
        let worker = std::thread::spawn(move || {
            crate::database_writer::commit(&path, database, SettlementGate { entered, released })
        });
        wait.recv_timeout(std::time::Duration::from_secs(5))?;
        Ok(PausedWriter { release, worker })
    }

    fn wait_terminal(&self, thread: &str) -> CodexSessionCreationState {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let state = self.state(thread);
            if !matches!(state, CodexSessionCreationState::Intent { .. }) {
                return state;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "necessary settlement missing"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for StartupFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct SettlementGate {
    entered: std::sync::mpsc::SyncSender<()>,
    released: std::sync::mpsc::Receiver<()>,
}
impl Sealed for SettlementGate {}
impl DatabaseCommand for SettlementGate {
    type Output = Result<(), String>;
    fn apply(self, _store: &Store) -> Self::Output {
        self.entered.send(()).map_err(error)?;
        self.released
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(error)
    }
}

struct PausedWriter {
    release: std::sync::mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<Result<Result<(), String>, String>>,
}
impl PausedWriter {
    fn resume(self) -> TestResult {
        self.release.send(())?;
        self.worker.join().map_err(|_| "gate panicked")???;
        Ok(())
    }
}

#[test]
fn abandoned_drop_returns_while_actor_paused_and_necessary_write_survives_lost_ack() -> TestResult {
    for external_started in [false, true] {
        let (fixture, startup) = StartupFixture::new()?;
        let thread = startup.thread.id.clone();
        if external_started {
            startup.persistence.mark_external_started();
        }
        let paused = fixture.pause()?;
        let (finished, returned) = std::sync::mpsc::sync_channel(1);
        let dropping = std::thread::spawn(move || {
            drop(startup);
            finished.send(()).unwrap();
        });
        let before_resume = returned.recv_timeout(std::time::Duration::from_secs(1));
        assert!(matches!(
            fixture.state(&thread),
            CodexSessionCreationState::Intent { .. }
        ));
        paused.resume()?;
        dropping.join().unwrap();
        before_resume?;
        assert!(matches!(
            (external_started, fixture.wait_terminal(&thread)),
            (
                false,
                CodexSessionCreationState::DefinitivelyFailed {
                    code: CodexSessionFailureCode::ProcessStartFailed,
                    ..
                }
            ) | (
                true,
                CodexSessionCreationState::Uncertain {
                    code: CodexSessionUncertaintyCode::OutcomeUnknown,
                    ..
                }
            )
        ));
    }
    Ok(())
}

#[test]
fn explicit_settlement_waits_for_real_necessary_ack_and_consumes_lease_once() -> TestResult {
    for external_started in [false, true] {
        let (fixture, startup) = StartupFixture::new()?;
        let thread = startup.thread.id.clone();
        if external_started {
            startup.persistence.mark_external_started();
        }
        let paused = fixture.pause()?;
        let (begun, began) = std::sync::mpsc::sync_channel(1);
        let (finished, returned) = std::sync::mpsc::sync_channel(1);
        let settling = std::thread::spawn(move || {
            begun.send(()).unwrap();
            let result = startup.persistence.settle_abandoned_startup();
            finished.send(result).unwrap();
            assert!(startup.persistence.settle_abandoned_startup().is_err());
            drop(startup);
        });
        began.recv_timeout(std::time::Duration::from_secs(5))?;
        let before_resume = returned.recv_timeout(std::time::Duration::from_millis(100));
        assert!(matches!(
            fixture.state(&thread),
            CodexSessionCreationState::Intent { .. }
        ));
        paused.resume()?;
        assert!(matches!(
            before_resume,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        returned.recv_timeout(std::time::Duration::from_secs(5))??;
        settling.join().unwrap();
        assert!(matches!(
            (external_started, fixture.state(&thread)),
            (false, CodexSessionCreationState::DefinitivelyFailed { .. })
                | (true, CodexSessionCreationState::Uncertain { .. })
        ));
    }
    Ok(())
}

#[test]
fn abandoned_settlement_preserves_durable_confirmed_and_terminal_when_local_ack_is_stale()
-> TestResult {
    for outcome in [
        CodexSessionOutcome::Confirmed {
            session_id: "session".into(),
        },
        CodexSessionOutcome::Uncertain {
            code: CodexSessionUncertaintyCode::OutcomeUnknown,
        },
        CodexSessionOutcome::DefinitivelyFailed {
            code: CodexSessionFailureCode::AdapterRejected,
        },
    ] {
        let (fixture, startup) = StartupFixture::new()?;
        let thread = startup.thread.id.clone();
        let source = startup.persistence.source.lock().unwrap().clone();
        crate::database_writer::commit(
            &fixture.root.join("database.db"),
            fixture.store.database_identity(),
            CodexCommand {
                source,
                operation: CodexWrite::Session {
                    outcome,
                    now: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis() as i64,
                },
            },
        )??;
        let before = fixture.state(&thread);
        startup.persistence.mark_external_started();
        startup.persistence.settle_abandoned_startup()?;
        assert_eq!(fixture.state(&thread), before);
        drop(startup);
        assert_eq!(fixture.state(&thread), before);
    }
    Ok(())
}

#[test]
fn failed_necessary_settlement_reports_error_and_is_not_retried_on_drop() -> TestResult {
    let (fixture, startup) = StartupFixture::new()?;
    let thread = startup.thread.id.clone();
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_settlement BEFORE UPDATE ON codex_session_creations BEGIN SELECT RAISE(ABORT,'injected settlement failure'); END;")?;
    assert!(startup.persistence.settle_abandoned_startup().is_err());
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER fail_settlement;")?;
    assert!(startup.persistence.settle_abandoned_startup().is_err());
    drop(startup);
    crate::database_writer::commit(
        &fixture.root.join("database.db"),
        fixture.store.database_identity(),
        Barrier,
    )?;
    assert!(matches!(
        fixture.state(&thread),
        CodexSessionCreationState::Intent { .. }
    ));
    Ok(())
}
