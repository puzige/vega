use super::*;

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-child-preparation-{}", ulid::Ulid::generate()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let database = root.join("data.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        Self {
            root,
            database,
            store,
        }
    }

    fn thread(&self, mode: &str) -> String {
        let thread =
            crate::threads::create_standalone_thread(&self.store, "model", "confirm").unwrap();
        self.store
            .conn()
            .execute(
                "UPDATE threads SET mode=?1 WHERE id=?2",
                [mode, thread.id.as_str()],
            )
            .unwrap();
        thread.id
    }

    fn message(&self, thread: &str, id: &str, role: &str, content: &str, status: &str) {
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq: messages::next_seq(self.store.conn(), thread).unwrap(),
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }

    fn prepare(
        &self,
        thread: &str,
        config: PersistenceActorConfig,
    ) -> Result<PreparedRun, ConversationError> {
        prepare_run_with_images_and_reasoning(
            self.database.clone(),
            thread.into(),
            "instruction".into(),
            "system".into(),
            "input".into(),
            "reply".into(),
            config.clone(),
            config.delegated,
            None,
            None,
            Vec::new(),
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ordinary_modes_keep_history_fences_and_checkpoint_directory_behavior() {
    for mode in ["ask", "plan", "execute"] {
        let fixture = Fixture::new();
        let thread = fixture.thread(mode);
        let prepared = fixture
            .prepare(&thread, PersistenceActorConfig::default())
            .unwrap();
        assert_eq!(
            prepared.request.history.last().unwrap().content,
            "instruction"
        );
        assert_eq!(
            prepared.request.history.last().unwrap().role,
            vega_runtime::ChatRole::User
        );
        assert_eq!(
            prepared
                .request
                .replay_frame
                .as_ref()
                .unwrap()
                .cold_prefix_len(),
            1
        );
        assert_eq!(prepared.permission_fence.run_mode, mode);
        assert!(
            messages::find(fixture.store.conn(), "reply")
                .unwrap()
                .is_some()
        );
        assert!(
            vega_store::permission_contracts::load_run_fence(
                fixture.store.conn(),
                "reply",
                &thread
            )
            .unwrap()
            .is_some()
        );
        assert_eq!(fixture.root.join("checkpoints").is_dir(), mode == "execute");
        let source =
            vega_store::context_compaction::load_source(fixture.store.conn(), &thread).unwrap();
        assert_eq!(
            prepared.request.context_source_version,
            Some(source.source_version)
        );
        assert_eq!(
            prepared.request.context_source_fingerprint.as_deref(),
            Some(source.fingerprint.as_str())
        );
    }
}

fn delegated_config(fixture: &Fixture, child: &str, parent: &str) -> PersistenceActorConfig {
    let parent_row = vega_store::threads::find(fixture.store.conn(), parent)
        .unwrap()
        .unwrap();
    let fence = crate::permission_contracts::capture_fence(&fixture.store, &parent_row).unwrap();
    let child_row = vega_store::threads::find(fixture.store.conn(), child)
        .unwrap()
        .unwrap();
    let child_fence = crate::permission_contracts::capture_delegated_fence(
        &fixture.store,
        &child_row,
        parent,
        "parent-reply",
        "reply",
        &fence,
    )
    .unwrap();
    let tx = fixture.store.immediate_transaction().unwrap();
    vega_store::permission_contracts::record_delegated_fence(
        &tx,
        child,
        "reply",
        &serde_json::to_string(&child_fence).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    PersistenceActorConfig {
        delegated: true,
        delegated_parent: Some((parent.into(), "parent-reply".into())),
        delegated_message_id: Some("reply".into()),
        delegated_authority: Some(DelegatedAuthority {
            permission_fence: fence,
            run_mode: RuntimeRunMode::Execute,
            permission_mode: RuntimePermissionMode::Confirm,
            exact_rules: Vec::new(),
        }),
        ..Default::default()
    }
}

#[test]
fn delegated_preparation_preserves_instruction_projection_and_atomic_admission() {
    for stale_instruction in [false, true] {
        let fixture = Fixture::new();
        let parent = fixture.thread("execute");
        let child = fixture.thread("execute");
        fixture.message(&parent, "parent-input", "user", "parent task", "done");
        fixture.message(&parent, "parent-reply", "assistant", "", "streaming");
        fixture.message(&child, "input", "user", "instruction", "done");
        crate::threads::task_workspace_root(&fixture.store, &parent).unwrap();
        let parent_row = vega_store::threads::find(fixture.store.conn(), &parent)
            .unwrap()
            .unwrap();
        let parent_fence =
            crate::permission_contracts::capture_fence(&fixture.store, &parent_row).unwrap();
        let tx = fixture.store.immediate_transaction().unwrap();
        vega_store::permission_contracts::record_run_fence(
            &tx,
            "parent-reply",
            &parent,
            &serde_json::to_string(&parent_fence).unwrap(),
        )
        .unwrap();
        vega_store::subagents::insert(
            &tx,
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.clone(),
                parent_thread_id: parent.clone(),
                parent_message_id: "parent-reply".into(),
                spawn_call_id: "spawn".into(),
                child_message_id: "reply".into(),
                name: "child".into(),
                task: "instruction".into(),
                state: "running".into(),
                result: String::new(),
                truncated: false,
                created_at: 1,
            },
        )
        .unwrap();
        tx.commit().unwrap();
        let config = delegated_config(&fixture, &child, &parent);
        if stale_instruction {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE messages SET content='replaced' WHERE id='input'",
                    [],
                )
                .unwrap();
        }
        let result = fixture.prepare(&child, config);
        if stale_instruction {
            assert!(result.is_err());
            assert!(
                messages::find(fixture.store.conn(), "reply")
                    .unwrap()
                    .is_none()
            );
            assert!(
                vega_store::permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    "reply",
                    &child
                )
                .unwrap()
                .is_none()
            );
        } else {
            let prepared = result.unwrap();
            assert_eq!(prepared.request.history.len(), 1);
            assert_eq!(prepared.request.history[0].content, "instruction");
            assert_eq!(
                prepared.request.history[0].role,
                vega_runtime::ChatRole::User
            );
            assert_eq!(
                prepared
                    .request
                    .replay_frame
                    .as_ref()
                    .unwrap()
                    .cold_prefix_len(),
                1
            );
            assert_eq!(prepared.assistant_seq, 2);
            assert!(prepared.replay_lease.is_none());
            assert!(fixture.root.join("checkpoints").is_dir());
            assert_eq!(messages::next_seq(fixture.store.conn(), &child).unwrap(), 3);
            assert!(
                vega_store::permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    "reply",
                    &child
                )
                .unwrap()
                .is_some()
            );
        }
    }
}

#[test]
fn checkpoint_creation_failure_does_not_commit_messages_or_fences() {
    let fixture = Fixture::new();
    let thread = fixture.thread("execute");
    fs::write(fixture.root.join("checkpoints"), b"blocked").unwrap();
    assert!(
        fixture
            .prepare(&thread, PersistenceActorConfig::default())
            .is_err()
    );
    assert!(
        messages::find(fixture.store.conn(), "input")
            .unwrap()
            .is_none()
    );
    assert!(
        messages::find(fixture.store.conn(), "reply")
            .unwrap()
            .is_none()
    );
    assert!(
        vega_store::permission_contracts::load_run_fence(fixture.store.conn(), "reply", &thread)
            .unwrap()
            .is_none()
    );
}
