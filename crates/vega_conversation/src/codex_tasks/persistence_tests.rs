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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    let startup = crate::codex_tasks::prepare_codex_startup(&path, draft, snapshot)?;
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
            now: now,
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
