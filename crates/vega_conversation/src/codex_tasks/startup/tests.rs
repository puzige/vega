use super::*;
use crate::codex_persistence::*;
use crate::database_writer::WriteRequest;
use crate::types::*;
use std::sync::mpsc;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-codex-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let root = std::fs::canonicalize(root)?;
        let store = Store::open(root.join("database.db"))?;
        store.migrate()?;
        Ok(Self { root, store })
    }
    fn path(&self) -> PathBuf {
        self.root.join("database.db")
    }
    fn draft(&self) -> Thread {
        let mut draft = crate::threads::draft_thread(None, "model", "confirm");
        draft.backend = TaskBackend::Codex;
        draft
    }
    fn snapshot(&self) -> CodexExecutionSnapshot {
        CodexExecutionSnapshot {
            profile: CodexProfileReference {
                id: "profile".into(),
                display_name: "Profile".into(),
            },
            adapter: CodexAdapterKind::CodexAcp,
            executable: "/mock-codex".into(),
            arguments: Vec::new(),
            adapter_version: "1.0".into(),
            codex_version: "1.0".into(),
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
                canonical_working_directory: self.root.to_string_lossy().into_owned(),
                additional_directories: Vec::new(),
            },
        }
    }
    fn startup(&self) -> Result<CodexStartup, String> {
        prepare_codex_startup(&self.path(), self.draft(), self.snapshot())
    }
    fn count(&self, table: &str) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self
            .store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn startup_installs_draft_snapshot_and_intent_atomically_with_one_owner() -> TestResult {
    let fixture = Fixture::new()?;
    let startup = fixture.startup()?;
    assert_eq!(startup.thread.backend, TaskBackend::Codex);
    let identity = codex_task_identity(&fixture.store, &startup.thread.id)?.unwrap();
    assert_eq!(identity.snapshot, fixture.snapshot());
    assert_eq!(identity.session_creation, startup.intent);
    assert!(matches!(
        startup.intent,
        CodexSessionCreationState::Intent { .. }
    ));
    assert!(crate::thread_access::acquire(&fixture.store, &startup.thread.id).is_err());
    assert_eq!(fixture.count("threads")?, 1);
    assert_eq!(fixture.count("codex_task_snapshots")?, 1);
    assert_eq!(fixture.count("codex_session_creations")?, 1);
    startup.persistence.validate_external_start()?;
    Ok(())
}

#[test]
fn failed_session_install_rolls_back_thread_snapshot_and_committed_draft() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_codex_intent BEFORE UPDATE ON codex_session_creations BEGIN SELECT RAISE(ABORT,'injected session SQL failure'); END;")?;
    assert!(fixture.startup().is_err());
    for table in ["threads", "codex_task_snapshots", "codex_session_creations"] {
        assert_eq!(fixture.count(table)?, 0);
    }
    let committed: i64 = fixture.store.conn().query_row(
        "SELECT COUNT(*) FROM draft_creation_intents WHERE state='committed'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(committed, 0);
    Ok(())
}

struct QueuedChange {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    change: &'static str,
}
impl Sealed for QueuedChange {}
impl DatabaseCommand for QueuedChange {
    type Output = Result<(), String>;
    fn apply(self, store: &Store) -> Self::Output {
        self.entered.send(()).map_err(|error| error.to_string())?;
        self.release
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        store
            .conn()
            .execute_batch(self.change)
            .map_err(|error| error.to_string())
    }
}

#[test]
fn queued_authority_default_and_owner_changes_reject_without_partial_draft() -> TestResult {
    for sql in [
        "UPDATE permission_contract_state SET authority_epoch='changed'",
        "UPDATE default_permission_intent SET generation='changed'",
        "UPDATE default_permission_intent SET pending_id='change-in-flight'",
    ] {
        let fixture = Fixture::new()?;
        let prepared = prepare(&fixture.store, fixture.draft(), fixture.snapshot())?;
        let (entered, wait) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let path = fixture.path();
        let database = fixture.store.database_identity();
        let blocking = std::thread::spawn(move || {
            crate::database_writer::commit(
                &path,
                database,
                QueuedChange {
                    entered,
                    release: released,
                    change: sql,
                },
            )
        });
        wait.recv_timeout(Duration::from_secs(5))?;
        let path = fixture.path();
        let pending =
            std::thread::spawn(move || crate::database_writer::commit(&path, database, prepared));
        release.send(())?;
        blocking.join().unwrap()??;
        assert!(pending.join().unwrap()?.is_err());
        for table in ["threads", "codex_task_snapshots", "codex_session_creations"] {
            assert_eq!(fixture.count(table)?, 0);
        }
    }
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    let mut prepared = prepare(&fixture.store, fixture.draft(), fixture.snapshot())?;
    prepared.owner = crate::thread_access::acquire(&other.store, &prepared.thread_id)?;
    assert!(
        crate::database_writer::commit(
            &fixture.path(),
            fixture.store.database_identity(),
            prepared
        )?
        .is_err()
    );
    assert_eq!(fixture.count("threads")?, 0);
    Ok(())
}

#[test]
fn default_file_changed_before_preparation_is_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let config = fixture.root.join("config.toml");
    vega_store::config::update_from(&config, |_| {})?;
    let defaults = vega_store::config::read_from(&config)?.defaults;
    let mut draft = crate::threads::draft_thread_from_defaults(
        None,
        "model",
        crate::permission_contracts::default_permission_snapshot(Some(&config), &defaults),
    );
    draft.backend = TaskBackend::Codex;
    vega_store::config::update_from(&config, |config| config.defaults.permission_revision += 1)?;
    assert!(prepare_codex_startup(&fixture.path(), draft, fixture.snapshot()).is_err());
    assert_eq!(fixture.count("threads")?, 0);
    Ok(())
}

#[test]
fn abandoned_startup_is_definitive_before_external_start_and_unknown_after() -> TestResult {
    for external_started in [false, true] {
        let fixture = Fixture::new()?;
        let startup = fixture.startup()?;
        let thread = startup.thread.id.clone();
        if external_started {
            startup.persistence.mark_external_started();
        }
        drop(startup);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let identity = codex_task_identity(&fixture.store, &thread)?.unwrap();
            if !matches!(
                identity.session_creation,
                CodexSessionCreationState::Intent { .. }
            ) {
                assert!(matches!(
                    (external_started, identity.session_creation),
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
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "abandoned intent not settled"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}

#[test]
fn changed_permission_or_session_rejects_external_launch_and_sensitive_writes() -> TestResult {
    for change in [
        "UPDATE threads SET permission_revision=permission_revision+1",
        "UPDATE codex_session_creations SET state='uncertain',uncertainty_code='outcome_unknown'",
    ] {
        let fixture = Fixture::new()?;
        let startup = fixture.startup()?;
        fixture.store.conn().execute_batch(change)?;
        assert!(startup.persistence.validate_external_start().is_err());
        assert!(
            startup
                .persistence
                .session_outcome(
                    CodexSessionOutcome::Confirmed {
                        session_id: "session".into()
                    },
                    now_ms()
                )
                .is_err()
        );
        assert_eq!(fixture.count("messages")?, 0);
    }
    Ok(())
}

fn ready(fixture: &Fixture) -> Result<CodexStartup, String> {
    let startup = fixture.startup()?;
    startup.persistence.session_outcome(
        CodexSessionOutcome::Confirmed {
            session_id: "session".into(),
        },
        now_ms(),
    )?;
    Ok(startup)
}
fn tool_source(call: &str, message: &str) -> CodexToolSource {
    CodexToolSource {
        call_id: call.into(),
        message_id: message.into(),
        identity: CodexAcpActivityIdentity {
            kind: "read".into(),
            arguments_bytes: 2,
            arguments_sha256: "a".repeat(64),
            argument_preview: CodexArgumentPreview::Object { count: 0 }.canonical(),
        },
        preview: CodexArgumentPreview::Object { count: 0 },
    }
}

#[test]
fn messages_and_tools_preserve_atomic_sequence_and_true_terminal_facts() -> TestResult {
    let fixture = Fixture::new()?;
    let startup = ready(&fixture)?;
    let service = &startup.persistence;
    let pair = service.create_messages(
        "question".into(),
        "user".into(),
        "assistant".into(),
        now_ms(),
    )?;
    assert_eq!(pair.assistant_seq, 2);
    assert!(
        service
            .create_messages("duplicate".into(), "u2".into(), "a2".into(), now_ms())
            .is_err()
    );
    for (call, user_selected) in [("auto", false), ("user", true), ("reject", false)] {
        let source = tool_source(call, "assistant");
        service.propose_tool(CodexToolProposal {
            call_id: call.into(),
            message_id: "assistant".into(),
            identity: source.identity.clone(),
            preview: source.preview,
            created_at: now_ms(),
        })?;
        if call == "reject" {
            service.change_tool(
                source,
                CodexToolChange::Reject {
                    source: ApprovalSource::User,
                    reason: CodexTerminalReason::PermissionRejected,
                },
                now_ms(),
            )?;
        } else {
            assert!(
                service
                    .change_tool(
                        tool_source(call, "assistant"),
                        CodexToolChange::Start {
                            selected_by_user: user_selected,
                            observed_in_progress: false
                        },
                        now_ms()
                    )
                    .is_err()
            );
            service.change_tool(
                source,
                CodexToolChange::Start {
                    selected_by_user: user_selected,
                    observed_in_progress: true,
                },
                now_ms(),
            )?;
            service.change_tool(
                tool_source(call, "assistant"),
                CodexToolChange::Finish {
                    status: ToolCallStatus::Success,
                    reason: CodexTerminalReason::ToolCompleted,
                    selected_by_user: user_selected,
                    rejected_by_user: false,
                },
                now_ms(),
            )?;
        }
        let state = vega_store::tool_calls::find_state(fixture.store.conn(), call)?.unwrap();
        assert_eq!(
            state.status,
            if call == "reject" {
                "rejected"
            } else {
                "success"
            }
        );
        let audit: serde_json::Value = serde_json::from_str(state.approval.as_deref().unwrap())?;
        assert_eq!(
            audit["source"],
            if user_selected || call == "reject" {
                "user"
            } else {
                "auto"
            }
        );
    }
    let seq: Vec<i64> = fixture
        .store
        .conn()
        .prepare("SELECT seq FROM tool_calls ORDER BY seq")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(seq, vec![1, 2, 3]);
    service.stream("assistant".into(), "answer".into())?;
    service.finish_message(
        "assistant".into(),
        "answer".into(),
        CodexMessageStatus::Done,
        1,
    )?;
    assert!(service.stream("assistant".into(), "late".into()).is_err());
    Ok(())
}

#[test]
fn message_pair_sql_failure_rolls_back_user_and_does_not_auto_retry() -> TestResult {
    let fixture = Fixture::new()?;
    let startup = ready(&fixture)?;
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_assistant BEFORE INSERT ON messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT,'injected assistant failure'); END;")?;
    assert!(
        startup
            .persistence
            .create_messages(
                "question".into(),
                "user".into(),
                "assistant".into(),
                now_ms()
            )
            .is_err()
    );
    assert_eq!(fixture.count("messages")?, 0);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(fixture.count("messages")?, 0);
    Ok(())
}

struct LostStartupReceipt {
    prepared: PreparedStartup,
    applied: mpsc::SyncSender<Result<(Thread, CodexSource), String>>,
}
impl WriteRequest for LostStartupReceipt {
    fn execute(self: Box<Self>, store: &Store) {
        let _ = self.applied.send(self.prepared.apply(store));
    }
}

#[test]
fn lost_startup_receipt_does_not_replay_and_reconciles_the_same_intent() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = prepare(&fixture.store, fixture.draft(), fixture.snapshot())?;
    let thread = prepared.thread_id.clone();
    let fallback = AbandonedStartup {
        thread_id: thread.clone(),
        snapshot: prepared.snapshot.clone(),
        intent_id: prepared.intent_id.clone(),
        owner: prepared.owner.clone(),
        external_started: false,
    };
    let (applied, lost_receipt) = mpsc::sync_channel(1);
    drop(lost_receipt);
    let writer =
        crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(fixture.path())?;
    writer.submit(Box::new(LostStartupReceipt { prepared, applied }))?;
    crate::database_writer::commit(&fixture.path(), fixture.store.database_identity(), fallback)??;
    assert_eq!(fixture.count("threads")?, 1);
    let identity = codex_task_identity(&fixture.store, &thread)?.unwrap();
    assert!(matches!(
        identity.session_creation,
        CodexSessionCreationState::DefinitivelyFailed { .. }
    ));
    assert_eq!(fixture.count("codex_task_snapshots")?, 1);
    Ok(())
}
