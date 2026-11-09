use super::super::persistence::DelegatedAuthority;
use super::*;
use std::sync::mpsc as sync_channel;
use std::time::Instant;

type TestResult = Result<(), Box<dyn std::error::Error>>;

thread_local! {
    static FOREIGN_IDS_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn foreign_ids_boundary() {
    if let Some(action) = FOREIGN_IDS_BOUNDARY.with_borrow_mut(Option::take) {
        action();
    }
}

struct Fixture {
    root: PathBuf,
    store: Store,
    parent: String,
    parent_message: String,
    child: String,
    user: String,
    assistant: String,
    config: PersistenceActorConfig,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "vega-concurrent-preparation-{}",
            ulid::Ulid::generate()
        ));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let parent = ulid::Ulid::generate().to_string();
        let child = ulid::Ulid::generate().to_string();
        let parent_message = ulid::Ulid::generate().to_string();
        let user = ulid::Ulid::generate().to_string();
        let assistant = ulid::Ulid::generate().to_string();
        store.conn().execute("INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'model','execute','confirm',1,1)", [&parent])?;
        store.conn().execute("INSERT INTO threads(id,parent_thread_id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,?2,'model','execute','confirm',1,1)", [&child, &parent])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [&parent_message, &parent])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'user','current task','done',3)", [&user, &child])?;
        crate::threads::task_workspace_root(&store, &parent)?;
        let parent_row =
            vega_store::threads::find(store.conn(), &parent)?.ok_or("parent missing")?;
        let parent_fence = crate::permission_contracts::capture_fence(&store, &parent_row)?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_run_fence(
            &tx,
            &parent_message,
            &parent,
            &serde_json::to_string(&parent_fence)?,
        )?;
        tx.execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,instruction_message_id,name,task,state,created_at) VALUES(?1,?2,?3,?4,'spawn-call',1,'spawn',?5,'fixture','current task','running',3)", [&assistant, &child, &parent, &parent_message, &user])?;
        tx.commit()?;
        let child_row = vega_store::threads::find(store.conn(), &child)?.ok_or("child missing")?;
        let child_fence = crate::permission_contracts::capture_delegated_fence(
            &store,
            &child_row,
            &parent,
            &parent_message,
            &assistant,
            &parent_fence,
        )?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_delegated_fence(
            &tx,
            &child,
            &assistant,
            &serde_json::to_string(&child_fence)?,
        )?;
        vega_store::delegated_stages::register(
            &tx,
            &child,
            &assistant,
            None,
            &serde_json::json!([]),
        )?;
        tx.commit()?;
        let config = PersistenceActorConfig {
            delegated: true,
            delegated_parent: Some((parent.clone(), parent_message.clone())),
            delegated_message_id: Some(assistant.clone()),
            delegated_authority: Some(DelegatedAuthority {
                permission_fence: parent_fence,
                run_mode: RuntimeRunMode::Execute,
                permission_mode: RuntimePermissionMode::Confirm,
                exact_rules: Vec::new(),
            }),
            turn_limit: 10,
            ..Default::default()
        };
        Ok(Self {
            root,
            store,
            parent,
            parent_message,
            child,
            user,
            assistant,
            config,
        })
    }

    fn prepare(&self) -> Result<PreparedRun, ConversationError> {
        prepare_run_with_images_and_reasoning(
            self.root.join("fixture.db"),
            self.child.clone(),
            "current task".into(),
            "system".into(),
            self.user.clone(),
            self.assistant.clone(),
            self.config.clone(),
            true,
            None,
            None,
            Vec::new(),
            None,
        )
    }

    fn add_foreign_history(&self, count: usize) -> TestResult {
        let tx = self.store.immediate_transaction()?;
        let mut insert = tx.prepare("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at) VALUES(?1,?2,?3,?4,'read','{}',?5,'success',1)")?;
        let payload = "synthetic history ".repeat(128);
        for index in 0..count {
            insert.execute([
                format!("foreign-{index:08}"),
                self.parent.clone(),
                self.parent_message.clone(),
                index.to_string(),
                payload.clone(),
            ])?;
        }
        drop(insert);
        tx.commit()?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| *action = None);
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn slow_foreign_history_preparation_allows_other_persistence_before_resume() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let parent_message = fixture.parent_message.clone();
    let parent = fixture.parent.clone();
    let mut targets = vec![(parent.clone(), parent_message.clone())];
    for _ in 0..2 {
        let child = ulid::Ulid::generate().to_string();
        let message = ulid::Ulid::generate().to_string();
        fixture.store.conn().execute("INSERT INTO threads(id,parent_thread_id,model,created_at,updated_at) VALUES(?1,?2,'model',1,1)", [&child, &parent])?;
        fixture.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [&message, &child])?;
        targets.push((child, message));
    }
    let expected = targets.clone();
    let (start_tx, start_rx) = sync_channel::channel();
    let (done_tx, done_rx) = sync_channel::channel();
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            start_rx.recv().map_err(|e| e.to_string())?;
            drop(crate::permission_contracts::coordinate().map_err(|e| e.to_string())?);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            runtime.block_on(async {
                for (index, (thread, message)) in targets.into_iter().enumerate() {
                    let store = Store::open(&path).map_err(|e| e.to_string())?;
                    let admission =
                        super::super::ordinary::verified_admission(&store, &thread, None)
                            .map_err(|e| e.to_string())?;
                    let actor = super::super::persistence::PersistenceActor::start(
                        super::super::persistence::PersistenceActorStart {
                            database_path: path.clone(),
                            project_id: String::new(),
                            thread_id: thread,
                            message_id: message,
                            model: "model".into(),
                            is_plan: false,
                            next_tool_seq: 1,
                            config: PersistenceActorConfig::default()
                                .with_thread_admission(admission),
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    actor
                        .snapshot("parent streaming".into())
                        .await
                        .map_err(|e| e.to_string())?;
                    actor
                        .event(
                            RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall {
                                id: format!("concurrent-result-{index}"),
                                name: "read".into(),
                                input_json: "{\"path\":\"fixture.txt\"}".into(),
                                description: None,
                            }),
                            "parent streaming".into(),
                            None,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    let audit = vega_runtime::RuntimeApprovalAudit {
                        decision: vega_runtime::RuntimeApprovalDecision::Once,
                        note: None,
                        source: vega_runtime::RuntimeApprovalSource::User,
                        danger: None,
                    };
                    actor
                        .event(
                            RuntimeEvent::ToolCallApproved {
                                call_id: format!("concurrent-result-{index}"),
                                audit: audit.clone(),
                                remember_rule: None,
                            },
                            "parent streaming".into(),
                            None,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    actor
                        .event(
                            RuntimeEvent::ToolCallRunning {
                                call_id: format!("concurrent-result-{index}"),
                                bash_started_at: None,
                            },
                            "parent streaming".into(),
                            None,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    actor
                        .event(
                            RuntimeEvent::ToolCallFinished(vega_runtime::RuntimeToolResult {
                                call_id: format!("concurrent-result-{index}"),
                                output: "saved result".into(),
                                status: RuntimeToolStatus::Success,
                                reused: false,
                                exit_code: None,
                                duration_ms: None,
                                truncated: Some(false),
                                approval: Some(audit),
                                remember_rule: None,
                                credential_failure: None,
                            }),
                            "parent streaming".into(),
                            None,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    actor.close().await.map_err(|e| e.to_string())?;
                }
                Ok::<_, String>(())
            })?;
            Ok(())
        })();
        let _ = done_tx.send(result);
    });
    FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| {
        *action = Some(Box::new(move || {
            start_tx.send(()).unwrap();
            done_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("other task must commit before slow preparation resumes")
                .unwrap();
        }))
    });
    let prepared = fixture.prepare();
    worker.join().unwrap();
    prepared?;
    assert_eq!(
        messages::find(fixture.store.conn(), &fixture.parent_message)?
            .unwrap()
            .content,
        "parent streaming"
    );
    for (index, (thread, message)) in expected.iter().enumerate() {
        assert_eq!(
            messages::find(fixture.store.conn(), message)?
                .unwrap()
                .content,
            "parent streaming"
        );
        let result =
            tool_calls::find_state(fixture.store.conn(), &format!("concurrent-result-{index}"))?
                .unwrap();
        assert_eq!(result.thread_id, *thread);
        assert_eq!(result.status, "success");
        assert_eq!(result.output_text.as_deref(), Some("saved result"));
    }
    Ok(())
}

#[test]
fn full_preparation_with_large_foreign_history_reports_writer_hold() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.add_foreign_history(180_000)?;
    let long_history = std::env::var_os("VEGA_PREPARATION_LONG_HISTORY").is_some();
    if long_history {
        let tx = fixture.store.immediate_transaction()?;
        for index in 0..1000 {
            tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,?3,'user',?4,'done',1)", [format!("history-{index}"), fixture.child.clone(), (index-1000).to_string(), "synthetic long history ".repeat(64)])?;
        }
        tx.commit()?;
    }
    for sample in 0..6 {
        let scope = vega_store::write_coordination::ObservationScope::begin(fixture.store.conn());
        let started = Instant::now();
        let prepared = fixture.prepare()?;
        let elapsed = started.elapsed();
        let observations = serde_json::to_value(scope.observations())?;
        assert!(
            observations
                .as_array()
                .unwrap()
                .iter()
                .filter(|value| value["operation"] == "permit_released")
                .all(|value| value["elapsed_ms"].as_u64().unwrap() < 250)
        );
        eprintln!(
            "MEASUREMENT sample={sample} global_tools=180000 long_history={long_history} total_ms={:.3} observations={observations}",
            elapsed.as_secs_f64() * 1000.0
        );
        assert_eq!(prepared.assistant_seq, 2);
        drop(prepared);
        let tx = fixture.store.immediate_transaction()?;
        tx.execute(
            "DELETE FROM run_permission_fences WHERE message_id=?1",
            [&fixture.assistant],
        )?;
        tx.execute("DELETE FROM messages WHERE id=?1", [&fixture.assistant])?;
        tx.commit()?;
    }
    Ok(())
}

#[test]
fn cancellation_during_preparation_rolls_back_assistant() -> TestResult {
    let mut fixture = Fixture::new()?;
    let cancel = CancellationToken::new();
    fixture.config.steering_cancel = Some(cancel.clone());
    FOREIGN_IDS_BOUNDARY
        .with_borrow_mut(|action| *action = Some(Box::new(move || cancel.cancel())));
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    Ok(())
}

#[test]
fn stale_context_during_preparation_cannot_install_old_projection() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let user = fixture.user.clone();
    FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| {
        *action = Some(Box::new(move || {
            let store = Store::open(path).unwrap();
            let tx = store.immediate_transaction().unwrap();
            tx.execute(
                "UPDATE messages SET content='changed input' WHERE id=?1",
                [&user],
            )
            .unwrap();
            tx.commit().unwrap();
        }))
    });
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    Ok(())
}

#[test]
fn historical_context_conflict_recomputes_without_replaying_task() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('old-context',?1,0,'user','old history','done',1)", [&fixture.child])?;
    let path = fixture.root.join("fixture.db");
    FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| {
        *action = Some(Box::new(move || {
            let store = Store::open(path).unwrap();
            store
                .conn()
                .execute(
                    "UPDATE messages SET content='fresh history' WHERE id='old-context'",
                    [],
                )
                .unwrap();
        }))
    });
    let prepared = fixture.prepare()?;
    let tx = fixture.store.conn().unchecked_transaction()?;
    let source = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    assert_eq!(
        prepared.request.history,
        delegated_history(&tx, &source, &fixture.assistant)?
    );
    assert!(
        source
            .messages
            .iter()
            .any(|message| message.content == "fresh history")
    );
    assert_eq!(prepared.assistant_seq, 2);
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM messages WHERE id=?1",
            [&fixture.assistant],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    tx.commit()?;
    Ok(())
}

#[test]
fn transient_begin_busy_recovers_before_writing_once() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let (locked_tx, locked_rx) = sync_channel::channel();
    let holder = std::thread::spawn(move || {
        let store = Store::open(path).unwrap();
        let tx = store.conn().unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE threads SET title='external writer' WHERE id=(SELECT id FROM threads LIMIT 1)",
            [],
        )
        .unwrap();
        locked_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        tx.commit().unwrap();
    });
    locked_rx.recv()?;
    fixture.store.conn().busy_timeout(Duration::ZERO)?;
    let scope = vega_store::write_coordination::ObservationScope::begin(fixture.store.conn());
    let tx = fixture
        .store
        .immediate_transaction_with_budget(|| false, Duration::from_secs(1))?;
    tx.execute(
        "UPDATE threads SET title='one committed update' WHERE id=?1",
        [&fixture.child],
    )?;
    tx.commit()?;
    holder.join().unwrap();
    let observations = serde_json::to_value(scope.observations())?;
    let begins = observations
        .as_array()
        .unwrap()
        .iter()
        .filter(|observation| observation["operation"] == "begin")
        .collect::<Vec<_>>();
    assert!(
        begins
            .iter()
            .any(|observation| observation["succeeded"] == false
                && observation["sqlite_primary_code"] == 5)
    );
    assert_eq!(
        begins
            .iter()
            .filter(|observation| observation["succeeded"] == true)
            .count(),
        1
    );
    Ok(())
}

#[test]
fn sqlite_busy_wait_and_fifo_wait_are_cancellable_within_budget() -> TestResult {
    for coordinated in [false, true] {
        let fixture = Fixture::new()?;
        let path = fixture.root.join("fixture.db");
        let (locked_tx, locked_rx) = sync_channel::channel();
        let (release_tx, release_rx) = sync_channel::channel();
        let holder = std::thread::spawn(move || {
            let store = Store::open(path).unwrap();
            if coordinated {
                let tx = store.immediate_transaction().unwrap();
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                tx.rollback().unwrap();
            } else {
                let tx = store.conn().unchecked_transaction().unwrap();
                tx.execute(
                    "UPDATE threads SET title='held' WHERE id=(SELECT id FROM threads LIMIT 1)",
                    [],
                )
                .unwrap();
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                tx.rollback().unwrap();
            }
        });
        locked_rx.recv()?;
        fixture.store.conn().busy_timeout(Duration::ZERO)?;
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            token.cancel();
        });
        let started = Instant::now();
        let result = fixture
            .store
            .immediate_transaction_with_budget(|| cancel.is_cancelled(), Duration::from_secs(1));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
        release_tx.send(())?;
        holder.join().unwrap();
        canceller.join().unwrap();
    }
    Ok(())
}

#[test]
fn admission_budget_expires_without_partial_write() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let (locked_tx, locked_rx) = sync_channel::channel();
    let (release_tx, release_rx) = sync_channel::channel();
    let holder = std::thread::spawn(move || {
        let store = Store::open(path).unwrap();
        let tx = store.conn().unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE threads SET title='held' WHERE id=(SELECT id FROM threads LIMIT 1)",
            [],
        )
        .unwrap();
        locked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        tx.rollback().unwrap();
    });
    locked_rx.recv()?;
    fixture.store.conn().busy_timeout(Duration::ZERO)?;
    let started = Instant::now();
    let result = fixture
        .store
        .immediate_transaction_with_budget(|| false, Duration::from_millis(120));
    let elapsed = started.elapsed();
    assert!(result.is_err());
    assert!(elapsed >= Duration::from_millis(120) && elapsed < Duration::from_millis(500));
    release_tx.send(())?;
    holder.join().unwrap();
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    Ok(())
}

#[test]
fn nested_transaction_error_is_not_retried_as_busy() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().busy_timeout(Duration::ZERO)?;
    let tx = fixture.store.conn().unchecked_transaction()?;
    tx.execute(
        "UPDATE threads SET title='uncommitted' WHERE id=?1",
        [&fixture.child],
    )?;
    let scope = vega_store::write_coordination::ObservationScope::begin(fixture.store.conn());
    assert!(
        fixture
            .store
            .immediate_transaction_with_budget(|| false, Duration::from_secs(1))
            .is_err()
    );
    let observations = serde_json::to_value(scope.observations())?;
    assert_eq!(
        observations
            .as_array()
            .unwrap()
            .iter()
            .filter(|observation| observation["operation"] == "begin")
            .count(),
        1
    );
    tx.rollback()?;
    Ok(())
}

#[test]
fn completed_preparation_rejects_late_foreign_tool_identity_without_overwrite() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = fixture.prepare()?;
    tool_calls::insert(
        fixture.store.conn(),
        tool_calls::NewToolCall {
            id: "late-conflict",
            thread_id: &fixture.parent,
            message_id: &fixture.parent_message,
            seq: 1,
            tool: "read",
            input_json: "{}",
            description: None,
            status: "success",
            created_at: 2,
        },
    )?;
    let mut next_seq = prepared.next_tool_seq;
    let result = super::super::events::persist_runtime_event(
        &fixture.store,
        "",
        &fixture.child,
        &fixture.assistant,
        "model",
        false,
        "",
        &mut next_seq,
        &RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall {
            id: "late-conflict".into(),
            name: "read".into(),
            input_json: "{}".into(),
            description: None,
        }),
        None,
        None,
    );
    assert!(result.is_err());
    assert_eq!(next_seq, prepared.next_tool_seq);
    let existing = tool_calls::find_state(fixture.store.conn(), "late-conflict")?.unwrap();
    assert_eq!(existing.thread_id, fixture.parent);
    assert_eq!(existing.status, "success");
    Ok(())
}

#[test]
fn followup_and_recovery_use_same_nonblocking_preparation_boundary() -> TestResult {
    let mut fixture = Fixture::new()?;
    let prepared = fixture.prepare()?;
    drop(prepared);
    let previous = fixture.assistant.clone();
    let user = ulid::Ulid::generate().to_string();
    let assistant = ulid::Ulid::generate().to_string();
    messages::update_streaming_content(fixture.store.conn(), &previous, "previous result")?;
    assert!(messages::record_assistant_response(
        fixture.store.conn(),
        &previous,
        "previous result",
        &messages::AssistantResponseRow {
            message_id: previous.clone(),
            response_id: "final-response".into(),
            text_start_bytes: 0,
            text_end_bytes: "previous result".len(),
            kind: "final".into()
        }
    )?);
    let tx = fixture.store.immediate_transaction()?;
    messages::finish(&tx, &previous, "previous result", "done")?;
    tx.execute("UPDATE delegated_executions SET state='completed',result='previous result' WHERE child_message_id=?1", [&previous])?;
    tx.execute("UPDATE delegated_execution_recovery SET release_state='released' WHERE child_message_id=?1", [&previous])?;
    tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,3,'user','current task','done',4)", [&user, &fixture.child])?;
    tx.execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,created_at) VALUES(?1,?2,?3,?4,'followup-call',2,?5,'followup',?6,'fixture','current task','running',4)", [&assistant, &fixture.child, &fixture.parent, &fixture.parent_message, &previous, &user])?;
    tx.commit()?;
    let child_row = vega_store::threads::find(fixture.store.conn(), &fixture.child)?.unwrap();
    let parent_fence = &fixture
        .config
        .delegated_authority
        .as_ref()
        .unwrap()
        .permission_fence;
    let fence = crate::permission_contracts::capture_delegated_fence(
        &fixture.store,
        &child_row,
        &fixture.parent,
        &fixture.parent_message,
        &assistant,
        parent_fence,
    )?;
    let tx = fixture.store.immediate_transaction()?;
    vega_store::permission_contracts::record_delegated_fence(
        &tx,
        &fixture.child,
        &assistant,
        &serde_json::to_string(&fence)?,
    )?;
    vega_store::delegated_stages::register(
        &tx,
        &fixture.child,
        &assistant,
        None,
        &serde_json::json!([]),
    )?;
    let payload = serde_json::json!({"id":"resume","agent_id":fixture.child,"message_id":previous,"manifest":{"summary":"preserved work"}}).to_string();
    tx.execute("INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES('resume',?1,?2,?3,'fixture',4)", [&fixture.child, &previous, &payload])?;
    tx.execute("UPDATE delegated_execution_recovery SET resume_checkpoint_id='resume' WHERE child_thread_id=?1 AND child_message_id=?2", [&fixture.child, &assistant])?;
    tx.commit()?;
    fixture.user = user;
    fixture.assistant = assistant.clone();
    fixture.config.delegated_message_id = Some(assistant);
    let path = fixture.root.join("fixture.db");
    let parent_message = fixture.parent_message.clone();
    FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| {
        *action = Some(Box::new(move || {
            let store = Store::open(path).unwrap();
            assert_eq!(
                messages::update_streaming_content(
                    store.conn(),
                    &parent_message,
                    "followup still streaming"
                )
                .unwrap(),
                1
            );
        }))
    });
    let prepared = fixture.prepare()?;
    assert_eq!(prepared.assistant_seq, 4);
    assert_eq!(
        messages::find(fixture.store.conn(), &fixture.parent_message)?
            .unwrap()
            .content,
        "followup still streaming"
    );
    let tx = fixture.store.conn().unchecked_transaction()?;
    let source = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    assert!(prepared.request.history == delegated_history(&tx, &source, &fixture.assistant)?);
    tx.commit()?;
    Ok(())
}

#[test]
fn permission_change_during_preparation_rejects_without_partial_assistant() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let parent = fixture.parent.clone();
    FOREIGN_IDS_BOUNDARY.with_borrow_mut(|action| {
        *action = Some(Box::new(move || {
            let store = Store::open(path).unwrap();
            let _coordination = crate::permission_contracts::coordinate().unwrap();
            let tx = store.immediate_transaction().unwrap();
            tx.execute(
                "UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1",
                [&parent],
            )
            .unwrap();
            tx.commit().unwrap();
        }))
    });
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    Ok(())
}
