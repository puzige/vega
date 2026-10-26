mod credentials;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vega_store::{Store, config, messages, permission_contracts as contracts, tool_calls};

use super::prepare_run_with_images_and_reasoning;
use crate::agent::{PersistenceActorConfig, persistence::PreparedRun};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, InterruptedAssistant,
    PermissionFence, PermissionMode,
};

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    store: Store,
    thread: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-resume-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated directory");
        let root = root.canonicalize().expect("canonical fixture root");
        let database = root.join("vega.db");
        let store = Store::open(&database).expect("isolated database");
        store.migrate().expect("migrate fixture");
        let thread = ulid::Ulid::generate().to_string();
        store.conn().execute(
            "INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'synthetic','execute','confirm',1,1)",
            [&thread],
        ).expect("native fixture thread");
        Self {
            root,
            database,
            store,
            thread,
        }
    }

    fn prepare(
        &self,
        tail: Option<InterruptedAssistant>,
    ) -> Result<PreparedRun, ConversationError> {
        self.prepare_with_config(tail, PersistenceActorConfig::default())
    }

    fn prepare_with_config(
        &self,
        tail: Option<InterruptedAssistant>,
        config: PersistenceActorConfig,
    ) -> Result<PreparedRun, ConversationError> {
        let content = if tail.is_some() {
            ""
        } else {
            "retained request"
        };
        prepare_run_with_images_and_reasoning(
            self.database.clone(),
            self.thread.clone(),
            content.into(),
            "synthetic system".into(),
            ulid::Ulid::generate().to_string(),
            ulid::Ulid::generate().to_string(),
            PersistenceActorConfig {
                interrupted_assistant: tail,
                ..config
            },
            false,
            None,
            None,
            Vec::new(),
            None,
        )
    }

    fn interrupt(&self) -> (InterruptedAssistant, PermissionFence, String) {
        let run = self.prepare(None).expect("prepare original run");
        assert!(run.permission_fence.is_valid());
        self.store.conn().execute(
            "UPDATE messages SET status='interrupted',content='retained partial answer' WHERE id=?1",
            [&run.assistant_message_id],
        ).expect("persist interruption");
        let tail = InterruptedAssistant {
            message_id: run.assistant_message_id.clone(),
            seq: run.assistant_seq,
        };
        let fence = run.permission_fence.clone();
        let user = run.user_message_id.clone();
        drop(run);
        (tail, fence, user)
    }

    fn running_bash(&self, assistant: &str) -> tool_calls::ToolCallState {
        tool_calls::insert_pending(
            self.store.conn(),
            tool_calls::NewToolCall {
                id: "unsettled",
                thread_id: &self.thread,
                message_id: assistant,
                seq: 1,
                tool: "bash",
                input_json: r#"{"command":"printf synthetic","timeout":null}"#,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .expect("pending synthetic Bash call");
        let approval = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: ApprovalSource::User,
            danger: None,
        }
        .to_json()
        .expect("strict user approval");
        tool_calls::approve(self.store.conn(), "unsettled", &approval, None, 2)
            .expect("approve synthetic call");
        tool_calls::mark_running(self.store.conn(), "unsettled")
            .expect("record synthetic running state without executing Bash");
        let state = tool_calls::find_state(self.store.conn(), "unsettled")
            .expect("tool state")
            .expect("retained tool");
        assert_eq!(state.status, "running");
        assert_eq!(state.approval.as_deref(), Some(approval.as_str()));
        assert!(state.output_text.is_none());
        state
    }

    fn rows(&self) -> Vec<(String, i64, String, String, String)> {
        self.store
            .conn()
            .prepare(
                "SELECT id,seq,role,content,status FROM messages WHERE thread_id=?1 ORDER BY seq",
            )
            .expect("message query")
            .query_map([&self.thread], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .expect("read messages")
            .collect::<Result<_, _>>()
            .expect("message rows")
    }

    fn refused(&self, tail: &InterruptedAssistant) {
        let before = self.rows();
        assert!(
            crate::history::interrupted_tail(&self.store, &self.thread)
                .expect("history eligibility")
                .is_none()
        );
        assert!(matches!(
            self.prepare(Some(tail.clone())),
            Err(ConversationError::InvalidTaskIdentity)
        ));
        assert_eq!(self.rows(), before);
    }

    fn begin_reset(&self) -> (PathBuf, vega_store::permission_reset::ResetRecord) {
        let path = self.root.join("config.toml");
        config::AppConfig::default()
            .save_to(&path)
            .expect("isolated config");
        let value = config::read_from(&path).expect("config source");
        let source = vega_store::permission_reset::ResetConfigSource {
            configuration_id: path.to_str().expect("utf8 fixture path").into(),
            mode: value.defaults.permission_mode,
            revision: value.defaults.permission_revision,
            grant: value.defaults.full_access_grant,
        };
        let epoch = contracts::load_state(self.store.conn())
            .expect("epoch")
            .authority_epoch;
        let tx = self
            .store
            .immediate_transaction()
            .expect("reset transaction");
        let record =
            vega_store::permission_reset::begin(&tx, &epoch, &source, 20).expect("pending reset");
        tx.commit().expect("commit pending reset");
        (path, record)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn interrupted_resume_reuses_original_user_and_unchanged_authority() {
    for full_access in [false, true] {
        let fixture = Fixture::new();
        if full_access {
            crate::permission_contracts::change_thread_permission(
                &fixture.store,
                &fixture.thread,
                0,
                PermissionMode::FullAccess,
                true,
                2,
            )
            .expect("confirmed thread authority");
        }
        let (tail, fence, user) = fixture.interrupt();
        assert_eq!(
            crate::history::interrupted_tail(&fixture.store, &fixture.thread)
                .expect("eligible tail"),
            Some(tail.clone())
        );
        assert!(
            contracts::load_run_fence(fixture.store.conn(), &tail.message_id, &fixture.thread)
                .expect("active parent lookup")
                .is_none()
        );
        let before = fixture.rows();
        let run = fixture
            .prepare(Some(tail.clone()))
            .expect("resume unchanged authority");
        assert_eq!(run.user_message_id, user);
        assert_eq!(run.permission_fence, fence);
        assert_eq!(run.assistant_seq, tail.seq + 1);
        let saved: PermissionFence = serde_json::from_str(
            &contracts::load_run_fence(
                fixture.store.conn(),
                &run.assistant_message_id,
                &fixture.thread,
            )
            .expect("resumed fence")
            .expect("persisted fence"),
        )
        .expect("decode fence");
        assert_eq!(saved, fence);
        let after = fixture.rows();
        assert_eq!(&after[..before.len()], before.as_slice());
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(after.iter().filter(|row| row.2 == "user").count(), 1);
        assert_eq!(after.last().expect("new assistant").4, "streaming");
    }
}

#[test]
fn interrupted_resume_refuses_missing_malformed_and_changed_saved_authority() {
    for mutation in ["missing", "malformed", "changed"] {
        let fixture = Fixture::new();
        let (tail, mut fence, _) = fixture.interrupt();
        if mutation == "missing" {
            fixture
                .store
                .conn()
                .execute(
                    "DELETE FROM run_permission_fences WHERE message_id=?1",
                    [&tail.message_id],
                )
                .expect("missing original authority");
        } else {
            let encoded = if mutation == "malformed" {
                "{}".into()
            } else {
                fence.permission_revision += 1;
                serde_json::to_string(&fence).expect("changed authority")
            };
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE run_permission_fences SET permission_fence=?2 WHERE message_id=?1",
                    (&tail.message_id, &encoded),
                )
                .expect("replace original authority fixture");
        }
        fixture.refused(&tail);
    }
}

#[test]
fn interrupted_resume_refuses_permission_changes_including_a_return_to_the_original_mode() {
    let fixture = Fixture::new();
    let (tail, _, _) = fixture.interrupt();
    crate::permission_contracts::change_thread_permission(
        &fixture.store,
        &fixture.thread,
        0,
        PermissionMode::FullAccess,
        true,
        2,
    )
    .expect("new full access grant");
    fixture.refused(&tail);
    crate::permission_contracts::change_thread_permission(
        &fixture.store,
        &fixture.thread,
        1,
        PermissionMode::Confirm,
        false,
        3,
    )
    .expect("return to confirm with a new revision");
    fixture.refused(&tail);
}

#[test]
fn completed_reset_rejects_old_tails_but_allows_fresh_post_reset_work() {
    let fixture = Fixture::new();
    let (tail, _, _) = fixture.interrupt();
    let (config, record) = fixture.begin_reset();
    fixture.refused(&tail);
    let receipt = crate::permission_reset::recover_pending(&fixture.store, &config)
        .expect("complete reset")
        .expect("receipt");
    assert_eq!(receipt.reset_id, record.id);
    fixture.refused(&tail);
    let current_thread = vega_store::threads::find(fixture.store.conn(), &fixture.thread)
        .expect("thread")
        .expect("existing thread");
    let current = crate::permission_contracts::capture_fence(&fixture.store, &current_thread)
        .expect("current authority");
    fixture
        .store
        .conn()
        .execute(
            "UPDATE run_permission_fences SET permission_fence=?2 WHERE message_id=?1",
            (
                &tail.message_id,
                serde_json::to_string(&current).expect("current fence"),
            ),
        )
        .expect("isolate cutoff independently of epoch mismatch");
    fixture.refused(&tail);
    let (fresh, fence, user) = fixture.interrupt();
    assert!(fresh.seq > tail.seq);
    assert_eq!(fence.authority_epoch, record.authority_epoch);
    assert_eq!(
        crate::history::interrupted_tail(&fixture.store, &fixture.thread)
            .expect("fresh eligible tail"),
        Some(fresh.clone())
    );
    let resumed = fixture
        .prepare(Some(fresh))
        .expect("resume fresh post-reset work");
    assert_eq!(resumed.user_message_id, user);
    assert_eq!(resumed.permission_fence, fence);
}

#[test]
fn reset_pause_blocks_preexisting_admission_before_history_recovery_or_credential_reads() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let reader = reads.clone();
    let config =
        PersistenceActorConfig::default().with_owner_credential_reader(Arc::new(move || {
            reader.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }));
    let run = fixture
        .prepare_with_config(None, config.clone())
        .expect("normal preparation control");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let assistant = run.assistant_message_id.clone();
    drop(run);
    let tool_before = fixture.running_bash(&assistant);
    let owner = vega_store::thread_admission::try_acquire(
        fixture.store.database_identity(),
        &fixture.thread,
    )
    .expect("admission registry")
    .expect("preexisting owner");
    let pause =
        crate::permission_reset::pause_admission(&fixture.store).expect("pause before journal");
    let before = fixture.rows();
    assert!(matches!(
        fixture.prepare_with_config(None, config.clone().with_thread_admission(owner.clone())),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.rows(), before);
    assert_eq!(
        tool_calls::find_state(fixture.store.conn(), "unsettled").expect("unchanged tool"),
        Some(tool_before.clone())
    );
    let status: String = fixture
        .store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id='unsettled'",
            [],
            |row| row.get(0),
        )
        .expect("retained tool status");
    assert_eq!(status, "running");
    assert_eq!(
        messages::find(fixture.store.conn(), &assistant)
            .expect("assistant row")
            .expect("retained assistant")
            .status,
        "streaming"
    );
    assert!(
        vega_store::permission_reset::load(fixture.store.conn())
            .expect("no journal yet")
            .is_none()
    );
    assert!(!pause.owners_released().expect("old owner still held"));
    drop(owner);
    assert!(pause.owners_released().expect("owner may drain"));
    drop(pause);
    let fresh = fixture
        .prepare_with_config(None, config)
        .expect("preparation resumes after pause release with valid retained tool history");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_ne!(fresh.assistant_message_id, assistant);
    assert_eq!(
        messages::find(fixture.store.conn(), &assistant)
            .expect("original assistant")
            .expect("retained interrupted assistant")
            .status,
        "interrupted"
    );
    let retained = tool_calls::find_state(fixture.store.conn(), "unsettled")
        .expect("recovered tool state")
        .expect("retained cancelled call");
    assert_eq!(retained.status, "cancelled");
    assert_eq!(retained.approval, tool_before.approval);
    assert_eq!(retained.input_json, tool_before.input_json);
    assert_eq!(
        retained.output_text.as_deref(),
        Some(vega_store::recovery::RECOVERY_CANCELLED_OUTPUT)
    );
    let identity = tool_calls::find_identity(fixture.store.conn(), "unsettled")
        .expect("retained identity")
        .expect("same original tool call");
    assert_eq!(identity.thread_id, fixture.thread);
    assert_eq!(identity.message_id, assistant);
}

#[test]
fn pending_reset_blocks_generic_preparation_before_history_normalization() {
    let fixture = Fixture::new();
    let run = fixture.prepare(None).expect("original streaming run");
    let assistant = run.assistant_message_id.clone();
    drop(run);
    let tool_before = fixture.running_bash(&assistant);
    let (_, record) = fixture.begin_reset();
    let before = fixture.rows();
    assert!(matches!(
        fixture.prepare(None),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert_eq!(fixture.rows(), before);
    assert_eq!(
        messages::find(fixture.store.conn(), &assistant)
            .expect("assistant")
            .expect("retained assistant")
            .status,
        "streaming"
    );
    let status: String = fixture
        .store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id='unsettled'",
            [],
            |row| row.get(0),
        )
        .expect("tool state");
    assert_eq!(status, "running");
    assert_eq!(
        tool_calls::find_state(fixture.store.conn(), "unsettled").expect("unchanged tool"),
        Some(tool_before)
    );
    assert_eq!(
        vega_store::permission_reset::load(fixture.store.conn()).expect("journal"),
        Some(record)
    );
}
