use super::*;
use vega_runtime::{ChatMessage, ChatRole, RuntimeSteeringInput};
use vega_store::{messages, pending_messages, permission_contracts, projects, threads};

struct Fixture {
    _directory: tempfile::TempDir,
    store: Store,
    database: PathBuf,
    parent: String,
    root: String,
    current: String,
    project: String,
    settings: ChildSettings,
}

fn id() -> String {
    ulid::Ulid::generate().to_string()
}

fn message(store: &Store, thread: &str, message_id: &str, role: &str, status: &str) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: message_id.into(),
            thread_id: thread.into(),
            seq: messages::next_seq(store.conn(), thread).unwrap(),
            role: role.into(),
            kind: "text".into(),
            content: "content".into(),
            status: status.into(),
            created_at: now_ms(),
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("vega.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = projects::create(
            store.conn(),
            directory.path().canonicalize().unwrap().to_str().unwrap(),
            "project",
            None,
        )
        .unwrap()
        .id;
        let parent = id();
        threads::create(
            store.conn(),
            threads::NewThread {
                id: &parent,
                project_id: &project,
                title: "parent",
                mode: "execute",
                permission_mode: "confirm",
                permission_revision: 0,
                full_access_grant: None,
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: now_ms(),
                updated_at: now_ms(),
                reasoning_preferences: None,
            },
        )
        .unwrap();
        message(&store, &parent, &id(), "user", "done");
        let root = id();
        message(&store, &parent, &root, "assistant", "streaming");
        let row = threads::find(store.conn(), &parent).unwrap().unwrap();
        let fence = crate::permission_contracts::capture_fence(&store, &row).unwrap();
        let tx = store.immediate_transaction().unwrap();
        permission_contracts::record_run_fence(
            &tx,
            &root,
            &parent,
            &serde_json::to_string(&fence).unwrap(),
        )
        .unwrap();
        tx.commit().unwrap();
        let settings = ChildSettings {
            model: "model".into(),
            system_prompt: String::new(),
            authority: DelegatedAuthority {
                permission_fence: fence,
                run_mode: RuntimeRunMode::Execute,
                permission_mode: RuntimePermissionMode::Confirm,
                exact_rules: Vec::new(),
            },
            pricing_catalog: None,
            reasoning: None,
            context_budget: None,
            automation_claim: None,
            automation_hook: None,
            owner_credential_reader: None,
            run_credential_reader: None,
            turn_limit: 10,
            steering_source: None,
        };
        Self {
            _directory: directory,
            store,
            database,
            parent,
            current: root.clone(),
            root,
            project,
            settings,
        }
    }

    fn steer(&mut self) {
        let input_id = id();
        pending_messages::insert(
            self.store.conn(),
            &pending_messages::PendingMessageRow {
                id: input_id.clone(),
                thread_id: self.parent.clone(),
                ordinal: 0,
                behavior: "steer".into(),
                generation: self.root.clone(),
                payload: "{}".into(),
                created_at: now_ms(),
            },
        )
        .unwrap();
        assert!(
            pending_messages::claim_steer(self.store.conn(), &self.parent, &input_id, &self.root)
                .unwrap()
        );
        let next = id();
        let input = RuntimeSteeringInput {
            id: input_id,
            assistant_message_id: next.clone(),
            original_content: "steer".into(),
            user_message_seq: messages::next_seq(self.store.conn(), &self.parent).unwrap(),
            message: ChatMessage::new(ChatRole::User, "steer"),
        };
        super::super::events::persist_runtime_event(
            &self.store,
            &self.project,
            &self.parent,
            &self.current,
            "model",
            false,
            "previous",
            &mut 0,
            &RuntimeEvent::SteeringAccepted(input),
            None,
        )
        .unwrap();
        assert_eq!(
            messages::find(self.store.conn(), &self.current)
                .unwrap()
                .unwrap()
                .status,
            "done"
        );
        self.current = next;
        assert_eq!(
            messages::find(self.store.conn(), &self.current)
                .unwrap()
                .unwrap()
                .status,
            "streaming"
        );
    }

    fn snapshot(&self, child: Option<&str>) -> SubagentSnapshot {
        SubagentSnapshot {
            agent_id: child.map(str::to_owned).unwrap_or_else(id),
            parent_thread_id: self.parent.clone(),
            parent_message_id: self.root.clone(),
            message_id: id(),
            name: id(),
            state: SubagentState::Running,
            result: String::new(),
            truncated: false,
            checkpoint: None,
            release_state: "held".into(),
            ready_for_review: false,
        }
    }

    async fn admit(
        &self,
        snapshot: &SubagentSnapshot,
        previous: Option<ContinuationBoundary>,
        cancel: CancellationToken,
    ) -> Result<(), VegaError> {
        let owner = DelegatedOwnership::acquire(&self.database, &snapshot.agent_id).unwrap();
        let admission = crate::thread_access::acquire(&self.store, &snapshot.agent_id).unwrap();
        create_child(
            self.database.clone(),
            snapshot.clone(),
            "task".into(),
            id(),
            self.settings.clone(),
            cancel,
            previous,
            None,
            owner,
            admission,
        )
        .await
        .map(|_| ())
    }

    fn complete(&self, snapshot: &SubagentSnapshot) -> ContinuationBoundary {
        assert!(
            vega_store::subagents::finish(
                self.store.conn(),
                &snapshot.agent_id,
                &snapshot.message_id,
                "completed",
                "result",
                false
            )
            .unwrap()
        );
        vega_store::subagents::mark_usage_complete(
            self.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id,
            true,
        )
        .unwrap();
        vega_store::delegated_stages::set_release(
            self.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id,
            "released",
        )
        .unwrap();
        let tx = self.store.immediate_transaction().unwrap();
        let source = vega_store::context_compaction::load_source_version_in_transaction(
            &tx,
            &snapshot.agent_id,
        )
        .unwrap();
        let thread = threads::find(&tx, &snapshot.agent_id).unwrap().unwrap();
        tx.commit().unwrap();
        ContinuationBoundary {
            message_id: snapshot.message_id.clone(),
            source_version: source.source_version,
            fingerprint: source.fingerprint,
            thread,
        }
    }

    fn assert_admitted(&self, snapshot: &SubagentSnapshot) {
        let record = vega_store::subagents::execution(
            self.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id,
        )
        .unwrap()
        .unwrap()
        .record;
        assert_eq!(record.parent_message_id, self.root);
        assert_eq!(record.parent_thread_id, self.parent);
        let fence: crate::types::PermissionFence = serde_json::from_str(
            &permission_contracts::load_delegated_fence(
                self.store.conn(),
                &snapshot.agent_id,
                &snapshot.message_id,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            fence.delegation.as_ref().unwrap().parent_message_id,
            self.root
        );
        assert_eq!(
            *fence.delegation.unwrap().parent_fence,
            self.settings.authority.permission_fence
        );
    }

    fn counts(&self) -> Vec<i64> {
        [
            "threads",
            "messages",
            "delegated_runs",
            "delegated_executions",
            "delegated_permission_fences",
            "delegated_execution_recovery",
        ]
        .iter()
        .map(|table| {
            self.store
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        })
        .collect()
    }

    async fn rejected(
        &self,
        snapshot: &SubagentSnapshot,
        previous: Option<ContinuationBoundary>,
        cancel: CancellationToken,
    ) {
        let counts = self.counts();
        assert!(self.admit(snapshot, previous, cancel).await.is_err());
        assert_eq!(self.counts(), counts);
        assert!(
            vega_store::subagents::execution(
                self.store.conn(),
                &snapshot.agent_id,
                &snapshot.message_id
            )
            .unwrap()
            .is_none()
        );
        let _owner = DelegatedOwnership::acquire(&self.database, &snapshot.agent_id).unwrap();
        let _admission = crate::thread_access::acquire(&self.store, &snapshot.agent_id).unwrap();
    }
}

#[tokio::test]
async fn spawn_and_followup_survive_repeated_persisted_steering() {
    for hops in 0..=3 {
        let mut fixture = Fixture::new();
        for _ in 0..hops {
            fixture.steer();
        }
        let first = fixture.snapshot(None);
        fixture
            .admit(&first, None, CancellationToken::new())
            .await
            .unwrap();
        fixture.assert_admitted(&first);
        let mut boundary = fixture.complete(&first);
        let mut previous = first;
        for _ in 0..3 {
            fixture.steer();
            let next = fixture.snapshot(Some(&previous.agent_id));
            fixture
                .admit(&next, Some(boundary), CancellationToken::new())
                .await
                .unwrap();
            fixture.assert_admitted(&next);
            boundary = fixture.complete(&next);
            previous = next;
        }
        let other = fixture.snapshot(None);
        fixture
            .admit(&other, None, CancellationToken::new())
            .await
            .unwrap();
        fixture.assert_admitted(&other);
        assert_ne!(other.agent_id, previous.agent_id);
        if hops > 0 {
            assert!(
                permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    &fixture.root,
                    &fixture.parent
                )
                .unwrap()
                .is_none()
            );
        }
    }
}

#[tokio::test]
async fn permissions_cannot_be_refreshed_by_steering_for_spawn_or_followup() {
    for change in [
        "revision",
        "readonly",
        "full_access",
        "epoch",
        "reset",
        "rules",
        "workspace",
        "project",
        "archived",
        "backend",
    ] {
        for followup in [false, true] {
            for before_last_steer in [false, true] {
                let mut fixture = Fixture::new();
                let (snapshot, boundary) = if followup {
                    let first = fixture.snapshot(None);
                    fixture
                        .admit(&first, None, CancellationToken::new())
                        .await
                        .unwrap();
                    let boundary = fixture.complete(&first);
                    (fixture.snapshot(Some(&first.agent_id)), Some(boundary))
                } else {
                    (fixture.snapshot(None), None)
                };
                fixture.steer();
                if !before_last_steer {
                    fixture.steer();
                    fixture.steer();
                }
                match change {
                    "revision" => {
                        fixture.store.conn().execute("UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1", [&fixture.parent]).unwrap();
                    }
                    "readonly" | "full_access" => {
                        fixture.store.conn().execute("UPDATE threads SET permission_mode=?1,permission_revision=permission_revision+1 WHERE id=?2", [change, &fixture.parent]).unwrap();
                    }
                    "epoch" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE permission_contract_state SET authority_epoch=?1",
                                [id()],
                            )
                            .unwrap();
                    }
                    "reset" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE permission_contract_state SET reset_state='pending'",
                                [],
                            )
                            .unwrap();
                    }
                    "rules" => {
                        vega_store::permissions::insert_exact(
                            fixture.store.conn(),
                            vega_store::permissions::InsertExactRule {
                                project_id: &fixture.project,
                                tool: "bash",
                                pattern: "printf test",
                                created_at: now_ms(),
                            },
                        )
                        .unwrap();
                    }
                    "workspace" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE projects SET path=?1 WHERE id=?2",
                                [
                                    fixture
                                        ._directory
                                        .path()
                                        .join("elsewhere")
                                        .to_str()
                                        .unwrap(),
                                    &fixture.project,
                                ],
                            )
                            .unwrap();
                    }
                    "project" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE threads SET project_id=NULL WHERE id=?1",
                                [&fixture.parent],
                            )
                            .unwrap();
                    }
                    "archived" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE threads SET status='archived' WHERE id=?1",
                                [&fixture.parent],
                            )
                            .unwrap();
                    }
                    "backend" => {
                        fixture
                            .store
                            .conn()
                            .execute(
                                "UPDATE threads SET backend='codex' WHERE id=?1",
                                [&fixture.parent],
                            )
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
                if before_last_steer && change != "reset" {
                    fixture.steer();
                    fixture.steer();
                }
                fixture
                    .rejected(&snapshot, boundary, CancellationToken::new())
                    .await;
            }
        }
    }
}

#[tokio::test]
async fn stopped_or_ended_execution_rejects_spawn_and_followup_after_steering() {
    for state in ["cancel", "done", "interrupted", "failed"] {
        for hops in [0, 1, 3] {
            for followup in [false, true] {
                let mut fixture = Fixture::new();
                let (snapshot, boundary) = if followup {
                    let first = fixture.snapshot(None);
                    fixture
                        .admit(&first, None, CancellationToken::new())
                        .await
                        .unwrap();
                    let boundary = fixture.complete(&first);
                    (fixture.snapshot(Some(&first.agent_id)), Some(boundary))
                } else {
                    (fixture.snapshot(None), None)
                };
                for _ in 0..hops {
                    fixture.steer();
                }
                let cancel = CancellationToken::new();
                if state == "cancel" {
                    cancel.cancel();
                } else {
                    fixture
                        .store
                        .conn()
                        .execute(
                            "UPDATE messages SET status=?1 WHERE id=?2",
                            [state, &fixture.current],
                        )
                        .unwrap();
                }
                fixture.rejected(&snapshot, boundary, cancel).await;
            }
        }
    }
}

#[tokio::test]
async fn broken_steering_proof_and_independent_execution_cannot_reactivate_old_parent() {
    for change in [
        "link",
        "delivered",
        "invalidated",
        "behavior",
        "new_run",
        "root_fence",
        "database_identity",
    ] {
        let mut fixture = Fixture::new();
        fixture.steer();
        fixture.steer();
        fixture.steer();
        match change {
            "link" => {
                fixture.store.conn().execute("UPDATE pending_messages SET accepted_after_assistant_id=NULL WHERE accepted_after_assistant_id=?1", [&fixture.root]).unwrap();
            }
            "delivered" => {
                fixture.store.conn().execute("UPDATE pending_messages SET state='pending' WHERE accepted_after_assistant_id=?1", [&fixture.root]).unwrap();
            }
            "invalidated" => {
                fixture.store.conn().execute("UPDATE pending_messages SET permission_invalidated=1 WHERE accepted_after_assistant_id=?1", [&fixture.root]).unwrap();
            }
            "behavior" => {
                fixture.store.conn().execute("UPDATE pending_messages SET behavior='queue' WHERE accepted_after_assistant_id=?1", [&fixture.root]).unwrap();
            }
            "new_run" => {
                let tx = fixture.store.immediate_transaction().unwrap();
                permission_contracts::record_run_fence(
                    &tx,
                    &fixture.current,
                    &fixture.parent,
                    &serde_json::to_string(&fixture.settings.authority.permission_fence).unwrap(),
                )
                .unwrap();
                tx.commit().unwrap();
            }
            "root_fence" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "DELETE FROM run_permission_fences WHERE message_id=?1",
                        [&fixture.root],
                    )
                    .unwrap();
            }
            "database_identity" => {
                fixture
                    .settings
                    .authority
                    .permission_fence
                    .database_identity = "other".into();
            }
            _ => unreachable!(),
        }
        fixture
            .rejected(&fixture.snapshot(None), None, CancellationToken::new())
            .await;
    }
}

#[tokio::test]
async fn invalid_followup_source_still_rolls_back_after_multiple_steering() {
    for change in ["stale", "foreign_parent", "running", "external_message"] {
        let mut fixture = Fixture::new();
        let first = fixture.snapshot(None);
        fixture
            .admit(&first, None, CancellationToken::new())
            .await
            .unwrap();
        let mut boundary = fixture.complete(&first);
        fixture.steer();
        fixture.steer();
        fixture.steer();
        let mut next = fixture.snapshot(Some(&first.agent_id));
        match change {
            "stale" => boundary.message_id = id(),
            "foreign_parent" => next.parent_message_id = fixture.current.clone(),
            "running" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE delegated_executions SET state='running' WHERE child_message_id=?1",
                        [&first.message_id],
                    )
                    .unwrap();
            }
            "external_message" => message(&fixture.store, &first.agent_id, &id(), "user", "done"),
            _ => unreachable!(),
        }
        fixture
            .rejected(&next, Some(boundary), CancellationToken::new())
            .await;
    }
}

#[tokio::test]
async fn new_execution_can_delegate_after_old_permission_authority_is_invalidated() {
    let mut fixture = Fixture::new();
    fixture.steer();
    fixture.steer();
    fixture.steer();
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode='full_access',permission_revision=1 WHERE id=?1",
            [&fixture.parent],
        )
        .unwrap();
    fixture
        .rejected(&fixture.snapshot(None), None, CancellationToken::new())
        .await;
    fixture
        .store
        .conn()
        .execute(
            "UPDATE messages SET status='done' WHERE id=?1",
            [&fixture.current],
        )
        .unwrap();
    message(&fixture.store, &fixture.parent, &id(), "user", "done");
    fixture.root = id();
    fixture.current = fixture.root.clone();
    message(
        &fixture.store,
        &fixture.parent,
        &fixture.root,
        "assistant",
        "streaming",
    );
    let row = threads::find(fixture.store.conn(), &fixture.parent)
        .unwrap()
        .unwrap();
    fixture.settings.authority.permission_fence =
        crate::permission_contracts::capture_fence(&fixture.store, &row).unwrap();
    fixture.settings.authority.permission_mode = RuntimePermissionMode::FullAccess;
    let tx = fixture.store.immediate_transaction().unwrap();
    permission_contracts::record_run_fence(
        &tx,
        &fixture.root,
        &fixture.parent,
        &serde_json::to_string(&fixture.settings.authority.permission_fence).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    fixture.steer();
    fixture.steer();
    fixture.steer();
    let next = fixture.snapshot(None);
    fixture
        .admit(&next, None, CancellationToken::new())
        .await
        .unwrap();
    fixture.assert_admitted(&next);
}

#[path = "steering_scope_tests.rs"]
mod steering_scope_tests;

#[path = "steering_lifecycle_tests.rs"]
mod steering_lifecycle_tests;
