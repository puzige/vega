use super::*;

struct Fixture {
    store: Store,
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("vega-resume-{}", ulid::Ulid::generate()));
        fs::create_dir(&directory).expect("create fixture directory");
        let store = Store::open(directory.join("history.sqlite")).expect("open fixture store");
        store.migrate().expect("migrate fixture store");
        vega_store::threads::create_standalone(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread",
                project_id: "",
                title: "Resume fixture",
                mode: "ask",
                permission_mode: "readonly",
                model: "fixture-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("create thread");
        let fixture = Self { store, directory };
        fixture.message("user", 1, "user", "done");
        fixture.message("stopped", 2, "assistant", "interrupted");
        fixture
    }

    fn message(&self, id: &str, seq: i64, role: &str, status: &str) {
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: format!("original {id}"),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("insert message");
    }

    fn tail(&self) -> Option<crate::types::InterruptedAssistant> {
        crate::history::interrupted_tail(&self.store, "thread").expect("read interrupted tail")
    }

    fn config(&self) -> PersistenceActorConfig {
        PersistenceActorConfig {
            interrupted_assistant: Some(crate::types::InterruptedAssistant {
                message_id: "stopped".into(),
                seq: 2,
            }),
            ..Default::default()
        }
    }

    fn prepare(&self, config: PersistenceActorConfig) -> Result<PreparedRun, ConversationError> {
        prepare_run_with_images_and_reasoning(
            self.directory.join("history.sqlite"),
            "thread".into(),
            String::new(),
            "fixture system".into(),
            "must-not-be-inserted".into(),
            "resumed".into(),
            config,
            false,
            None,
            None,
            Vec::new(),
            None,
        )
    }

    fn rows(&self) -> Vec<(String, i64, String, String, String)> {
        let mut query = self
            .store
            .conn()
            .prepare("SELECT id, seq, role, status, content FROM messages WHERE thread_id = 'thread' ORDER BY seq")
            .expect("prepare history query");
        query
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .expect("read history")
            .collect::<Result<_, _>>()
            .expect("decode history")
    }

    fn credential(&self, assistant: &str, user: &str, state: &str) {
        self.store
            .conn()
            .execute(
                "INSERT INTO credential_continuations (id, thread_id, user_message_id, source_version, source_fingerprint, raw_user_fingerprint, state, reason, created_at) VALUES (?1, 'thread', ?2, 1, 'source', 'raw', ?3, 'fixture', 1)",
                (assistant, user, state),
            )
            .expect("insert credential receipt");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn resume_reuses_user_preserves_history_and_rejects_duplicate_request() {
    let fixture = Fixture::new();
    let before = fixture.rows();
    let prepared = fixture.prepare(fixture.config()).expect("prepare resume");
    assert_eq!(prepared.user_message_id, "user");
    assert_eq!(prepared.assistant_seq, 3);
    assert!(
        prepared
            .request
            .history
            .iter()
            .any(|row| row.content == "original user")
    );
    assert!(
        prepared
            .request
            .history
            .iter()
            .any(|row| row.content == "original stopped")
    );
    let after = fixture.rows();
    assert_eq!(&after[..2], before.as_slice());
    assert_eq!(after.len(), 3);
    assert_eq!(
        after[2],
        (
            "resumed".into(),
            3,
            "assistant".into(),
            "streaming".into(),
            String::new()
        )
    );
    assert!(fixture.tail().is_none());
    drop(prepared);
    assert!(fixture.prepare(fixture.config()).is_err());
    assert_eq!(fixture.rows(), after);
}

#[test]
fn resume_candidate_requires_native_active_actual_interrupted_tail() {
    let fixture = Fixture::new();
    assert!(fixture.tail().is_some());
    let reopened = Store::open(fixture.directory.join("history.sqlite")).expect("reopen store");
    assert_eq!(
        crate::history::interrupted_tail(&reopened, "thread").expect("reload tail"),
        fixture.tail()
    );
    for status in ["done", "failed", "streaming"] {
        fixture
            .store
            .conn()
            .execute(
                "UPDATE messages SET status = ?1 WHERE id = 'stopped'",
                [status],
            )
            .expect("change terminal status");
        assert!(fixture.tail().is_none(), "status {status}");
    }
    fixture
        .store
        .conn()
        .execute(
            "UPDATE messages SET status = 'interrupted' WHERE id = 'stopped'",
            [],
        )
        .expect("restore interrupted");
    fixture
        .store
        .conn()
        .execute("UPDATE threads SET backend = 'codex'", [])
        .expect("switch backend");
    assert!(fixture.tail().is_none());
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET backend = 'native', status = 'archived'",
            [],
        )
        .expect("archive thread");
    assert!(fixture.tail().is_none());
    fixture
        .store
        .conn()
        .execute("UPDATE threads SET status = 'active'", [])
        .expect("restore active thread");
    fixture.message("new-user", 3, "user", "done");
    assert!(fixture.tail().is_none());
    let before = fixture.rows();
    assert!(fixture.prepare(fixture.config()).is_err());
    assert_eq!(fixture.rows(), before);
}

#[test]
fn resume_waits_for_pending_messages_and_current_turn_credential_receipts() {
    let fixture = Fixture::new();
    vega_store::pending_messages::insert(
        fixture.store.conn(),
        &vega_store::pending_messages::PendingMessageRow {
            id: "queued".into(),
            thread_id: "thread".into(),
            ordinal: 0,
            behavior: "queue".into(),
            generation: "fixture".into(),
            payload: "{}".into(),
            created_at: 3,
        },
    )
    .expect("queue message");
    assert!(fixture.tail().is_none());
    let before = fixture.rows();
    assert!(fixture.prepare(fixture.config()).is_err());
    assert_eq!(fixture.rows(), before);
    fixture
        .store
        .conn()
        .execute("UPDATE pending_messages SET state = 'delivered'", [])
        .expect("deliver queue");
    assert!(fixture.tail().is_some());
    fixture.credential("stopped", "user", "unknown");
    assert!(fixture.tail().is_none());
    assert!(fixture.prepare(fixture.config()).is_err());
    assert_eq!(fixture.rows(), before);
    fixture.message("next-user", 3, "user", "done");
    fixture.message("next-stopped", 4, "assistant", "interrupted");
    assert_eq!(
        fixture
            .tail()
            .expect("unrelated receipt does not block")
            .message_id,
        "next-stopped"
    );
    fixture
        .store
        .conn()
        .execute(
            "UPDATE credential_continuations SET user_message_id = 'next-user'",
            [],
        )
        .expect("associate current user");
    assert!(fixture.tail().is_none());
    fixture.store.conn().execute("UPDATE credential_continuations SET user_message_id = 'user', continuation_message_id = 'next-stopped'", []).expect("associate continuation");
    assert!(fixture.tail().is_none());
    fixture
        .store
        .conn()
        .execute(
            "UPDATE credential_continuations SET state = 'completed'",
            [],
        )
        .expect("complete receipt");
    assert!(fixture.tail().is_some());
}

#[test]
fn resume_rejects_stale_identity_and_rolls_back_cancelled_preparation() {
    let fixture = Fixture::new();
    let before = fixture.rows();
    for expected in [
        crate::types::InterruptedAssistant {
            message_id: "other".into(),
            seq: 2,
        },
        crate::types::InterruptedAssistant {
            message_id: "stopped".into(),
            seq: 1,
        },
    ] {
        let config = PersistenceActorConfig {
            interrupted_assistant: Some(expected),
            ..Default::default()
        };
        assert!(fixture.prepare(config).is_err());
        assert_eq!(fixture.rows(), before);
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut config = fixture.config();
    config.steering_cancel = Some(cancel);
    assert!(
        matches!(fixture.prepare(config), Err(ConversationError::Runtime(error)) if matches!(error.as_ref(), VegaError::Cancelled))
    );
    assert_eq!(fixture.rows(), before);
    assert!(fixture.tail().is_some());
}

#[test]
fn resume_rehydrates_terminal_tool_results_without_mutating_them() {
    let fixture = Fixture::new();
    tool_calls::insert_pending(
        fixture.store.conn(),
        tool_calls::NewToolCall {
            id: "completed-read",
            thread_id: "thread",
            message_id: "stopped",
            seq: 1,
            tool: "read",
            input_json: r#"{"path":"notes.txt","offset":null,"limit":null}"#,
            status: "pending_approval",
            created_at: 2,
        },
    )
    .expect("insert tool");
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .expect("encode approval");
    tool_calls::approve(fixture.store.conn(), "completed-read", &approval, None, 2)
        .expect("approve tool");
    tool_calls::mark_running(fixture.store.conn(), "completed-read").expect("start tool");
    tool_calls::finish(
        fixture.store.conn(),
        tool_calls::FinishToolCall {
            id: "completed-read",
            status: "success",
            output_text: "retained result",
            exit_code: None,
            duration_ms: None,
            finished_at: 2,
        },
    )
    .expect("finish tool");
    let before =
        tool_calls::terminal_results(fixture.store.conn(), "thread").expect("load original tools");
    let prepared = fixture
        .prepare(fixture.config())
        .expect("prepare resume with tools");
    let completed = &prepared.request.completed_tool_results["completed-read"];
    assert_eq!(completed.tool, "read");
    assert_eq!(completed.result.status, RuntimeToolStatus::Success);
    assert_eq!(completed.result.output, "retained result");
    assert!(completed.result.reused);
    assert_eq!(
        prepared
            .request
            .history
            .iter()
            .filter(|row| row.tool_call_id.as_deref() == Some("completed-read"))
            .count(),
        1
    );
    assert_eq!(
        tool_calls::terminal_results(fixture.store.conn(), "thread")
            .expect("reload original tools"),
        before
    );
}
