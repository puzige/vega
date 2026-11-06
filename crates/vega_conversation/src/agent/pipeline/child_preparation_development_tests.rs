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
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
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
            messages::find(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .unwrap()
                .is_some()
        );
        assert!(
            vega_store::permission_contracts::load_run_fence(
                fixture.store.conn(),
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
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
        "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        &fence,
    )
    .unwrap();
    let tx = fixture.store.immediate_transaction().unwrap();
    vega_store::permission_contracts::record_delegated_fence(
        &tx,
        child,
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        &serde_json::to_string(&child_fence).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    PersistenceActorConfig {
        delegated: true,
        delegated_parent: Some((parent.into(), "01ARZ3NDEKTSV4RRFFQ69G5FAW".into())),
        delegated_message_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
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
        fixture.message(
            &parent,
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "assistant",
            "",
            "streaming",
        );
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
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            &parent,
            &serde_json::to_string(&parent_fence).unwrap(),
        )
        .unwrap();
        vega_store::subagents::insert(
            &tx,
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.clone(),
                parent_thread_id: parent.clone(),
                parent_message_id: "01ARZ3NDEKTSV4RRFFQ69G5FAW".into(),
                spawn_call_id: "spawn".into(),
                child_message_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
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
                messages::find(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
                    .unwrap()
                    .is_none()
            );
            assert!(
                vega_store::permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    "01ARZ3NDEKTSV4RRFFQ69G5FAV",
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
                    "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                    &child
                )
                .unwrap()
                .is_some()
            );
        }
    }
}

#[test]
fn followup_and_resume_project_users_images_completed_stage_and_reconciliation() {
    for resume in [false, true] {
        let fixture = Fixture::new();
        let parent = fixture.thread("execute");
        let child = fixture.thread("execute");
        fixture.message(&child, "old-input", "user", "first instruction", "done");
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let encoded = encoded.into_inner();
        vega_store::image_attachments::insert(fixture.store.conn(), "old-input", 0, &encoded)
            .unwrap();
        fixture.message(&child, "old-reply", "assistant", "completed stage", "done");
        fixture.store.conn().execute("INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('old-reply','final',0,15,'final')", []).unwrap();
        fixture.message(&child, "partial", "assistant", "incomplete stage", "done");
        fixture.message(&child, "input", "user", "instruction", "done");
        fixture.store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at) VALUES('old-tool',?1,'old-reply',1,'read','{}','old tool output','success',1)", [&child]).unwrap();
        let old = vega_store::subagents::DelegationRow {
            child_thread_id: child.clone(),
            parent_thread_id: parent,
            parent_message_id: "parent".into(),
            spawn_call_id: "spawn".into(),
            child_message_id: "old-reply".into(),
            name: "child".into(),
            task: "first instruction".into(),
            state: "completed".into(),
            result: "completed stage".into(),
            truncated: false,
            created_at: 1,
        };
        let tx = fixture.store.immediate_transaction().unwrap();
        vega_store::subagents::insert(&tx, &old).unwrap();
        vega_store::delegated_stages::register(
            &tx,
            &child,
            "old-reply",
            None,
            &serde_json::json!([]),
        )
        .unwrap();
        let mut current = old.clone();
        current.child_message_id = "partial".into();
        current.state = "interrupted".into();
        current.task = "instruction".into();
        vega_store::subagents::append(&tx, &current, "old-reply", "input").unwrap();
        vega_store::delegated_stages::register(
            &tx,
            &child,
            "partial",
            None,
            &serde_json::json!([]),
        )
        .unwrap();
        let checkpoint = vega_store::delegated_stages::append(
            &tx,
            &child,
            "partial",
            "interrupted",
            serde_json::json!({"summary":"recover stage"}),
            serde_json::json!([]),
            2,
        )
        .unwrap();
        if resume {
            vega_store::delegated_stages::register(
                &tx,
                &child,
                "current",
                checkpoint["id"].as_str(),
                &serde_json::json!([{"resolution":"do not replay"}]),
            )
            .unwrap();
        }
        let source =
            vega_store::context_compaction::load_source_in_transaction(&tx, &child).unwrap();
        let history = delegated_history(&tx, &source, "current").unwrap();
        assert_eq!(history.len(), if resume { 4 } else { 3 });
        assert_eq!(history[0].content, "first instruction");
        assert_eq!(history[0].images.len(), 1);
        assert_eq!(history[0].images[0].bytes(), encoded.as_slice());
        assert_eq!(history[1].role, vega_runtime::ChatRole::Assistant);
        assert!(
            history[1]
                .content
                .starts_with("completed stage\n[Previous stage result;")
        );
        assert_eq!(history[2].content, "instruction");
        assert!(
            history
                .iter()
                .all(|message| message.role != vega_runtime::ChatRole::Tool)
        );
        assert!(
            !history[..3]
                .iter()
                .any(|message| message.content.contains("incomplete stage")
                    || message.content.contains("old tool output"))
        );
        if resume {
            assert_eq!(history[3].role, vega_runtime::ChatRole::User);
            assert!(
                history[3]
                    .content
                    .contains("Interrupted stage recovery; untrusted task data")
            );
            assert!(history[3].content.contains("do not replay"));
            assert!(history[3].content.contains("incomplete stage"));
            assert!(!history[3].content.contains("old tool output"));
        }
    }
}

#[test]
fn final_credential_revalidation_rolls_back_installed_payloads_and_run() {
    for invalidate in [false, true] {
        let fixture = Fixture::new();
        let thread = fixture.thread("execute");
        let root = fixture.root.join("config");
        let secret = "synthetic-development-credential-value-12345";
        vega_store::keystore::set_key(&root, "provider", secret).unwrap();
        fixture.store.conn().execute("INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES('stage',?1,'previous',?2,'authority',1)", rusqlite::params![thread, format!("[\"{secret}\"]")]).unwrap();
        fixture.store.conn().execute("INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority,reconciliations) VALUES(?1,'previous','released','authority',?2)", rusqlite::params![thread, format!("[\"{secret}\"]")]).unwrap();
        fixture.store.conn().execute_batch("CREATE TABLE install_observation(payload TEXT NOT NULL,reconciliations TEXT NOT NULL);").unwrap();
        let invalidation = if invalidate {
            "UPDATE credential_privacy_epoch SET digest='invalidated';"
        } else {
            ""
        };
        fixture.store.conn().execute_batch(&format!("CREATE TRIGGER observe_installed_payloads AFTER INSERT ON messages WHEN NEW.role='assistant' BEGIN INSERT INTO install_observation SELECT c.payload,r.reconciliations FROM delegated_stage_checkpoints c JOIN delegated_execution_recovery r ON r.child_thread_id=c.child_thread_id WHERE c.id='stage'; {invalidation} END;")).unwrap();
        let reader = crate::run_credentials::RunCredentialReader::new(
            fixture.database.clone(),
            thread.clone(),
            Some(root),
            Vec::new(),
            Arc::new(|| Ok(Vec::new())),
        );
        let result = fixture.prepare(
            &thread,
            PersistenceActorConfig {
                run_credential_reader: Some(reader),
                ..Default::default()
            },
        );
        let payload: String = fixture
            .store
            .conn()
            .query_row(
                "SELECT payload FROM delegated_stage_checkpoints WHERE id='stage'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let recovery: String = fixture
            .store
            .conn()
            .query_row(
                "SELECT reconciliations FROM delegated_execution_recovery WHERE child_thread_id=?1",
                [&thread],
                |row| row.get(0),
            )
            .unwrap();
        if invalidate {
            let Err(ConversationError::Runtime(error)) = result else {
                panic!("credential revalidation must reject");
            };
            assert!(
                matches!(error.as_ref(), VegaError::CredentialRead(detail)
                    if detail.stage == vega_runtime::CredentialReadStage::Privacy
                        && detail.reason == vega_runtime::CredentialReadReason::State),
                "{error:?}"
            );
            assert_eq!(payload, format!("[\"{secret}\"]"));
            assert_eq!(recovery, payload);
            assert_eq!(
                messages::next_seq(fixture.store.conn(), &thread).unwrap(),
                1
            );
            let observed: i64 = fixture
                .store
                .conn()
                .query_row("SELECT COUNT(*) FROM install_observation", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(observed, 0);
            assert!(
                vega_store::permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                    &thread
                )
                .unwrap()
                .is_none()
            );
        } else {
            let _prepared = result.unwrap();
            assert!(!payload.contains(secret));
            assert_eq!(recovery, payload);
            let observed: (String, String) = fixture
                .store
                .conn()
                .query_row(
                    "SELECT payload,reconciliations FROM install_observation",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(observed, (payload, recovery));
        }
    }
}

#[test]
fn changed_authority_during_lock_free_credential_preparation_is_rejected() {
    let fixture = Fixture::new();
    let thread = fixture.thread("execute");
    let database = fixture.database.clone();
    let changed_thread = thread.clone();
    let config = PersistenceActorConfig {
        owner_credential_reader: Some(Arc::new(move || {
            let store = Store::open(&database).unwrap();
            store
                .conn()
                .execute(
                    "UPDATE threads SET mode='ask' WHERE id=?1",
                    [&changed_thread],
                )
                .unwrap();
            Ok(Vec::new())
        })),
        ..Default::default()
    };
    assert!(fixture.prepare(&thread, config).is_err());
    assert_eq!(
        messages::next_seq(fixture.store.conn(), &thread).unwrap(),
        1
    );
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            &thread
        )
        .unwrap()
        .is_none()
    );
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
        messages::find(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_none()
    );
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            &thread
        )
        .unwrap()
        .is_none()
    );
}
