use super::super::persistence::DelegatedAuthority;
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

thread_local! {
    static LEGACY_PROJECTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PROJECTION_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn use_legacy_projection() -> bool {
    LEGACY_PROJECTION.get()
}

pub(super) fn projection_boundary() {
    let action = PROJECTION_BOUNDARY.with_borrow_mut(Option::take);
    if let Some(action) = action {
        action();
    }
}

struct ProjectionMode;

impl ProjectionMode {
    fn set(legacy: bool) -> Self {
        LEGACY_PROJECTION.set(legacy);
        Self
    }
}

impl Drop for ProjectionMode {
    fn drop(&mut self) {
        LEGACY_PROJECTION.set(false);
        PROJECTION_BOUNDARY.with_borrow_mut(|action| *action = None);
    }
}

struct Fixture {
    root: PathBuf,
    store: Store,
    child: String,
    user: String,
    assistant: String,
    config: PersistenceActorConfig,
}

impl Fixture {
    fn new(with_history: bool) -> Result<Self, Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "vega-delegated-preparation-{}",
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
        let seq = if with_history {
            store.conn().execute_batch("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) SELECT 'old-user',id,1,'user','earlier task','done',1 FROM threads WHERE parent_thread_id IS NOT NULL; INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) SELECT 'old-assistant',id,2,'assistant','earlier final','done',2 FROM threads WHERE parent_thread_id IS NOT NULL; INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('old-assistant','response',0,13,'final');")?;
            3
        } else {
            1
        };
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,?3,'user','current task','done',3)", [user.as_str(), child.as_str(), &seq.to_string()])?;
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
            delegated_parent: Some((parent, parent_message)),
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn delegated_first_preparation_commits_frozen_source_and_fence() -> TestResult {
    let fixture = Fixture::new(false)?;
    let prepared = fixture.prepare()?;
    assert_eq!(prepared.assistant_message_id, fixture.assistant);
    assert_eq!(prepared.assistant_seq, 2);
    assert!(prepared.skill_launch.is_none());
    assert_eq!(prepared.request.history.len(), 1);
    assert!(prepared.request.completed_tool_results.is_empty());
    let tx = fixture.store.conn().unchecked_transaction()?;
    let actual = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    assert_eq!(
        prepared.request.context_source_version,
        Some(actual.source_version)
    );
    assert_eq!(
        prepared.request.context_source_fingerprint.as_deref(),
        Some(actual.fingerprint.as_str())
    );
    assert!(
        vega_store::permission_contracts::load_run_fence(&tx, &fixture.assistant, &fixture.child)?
            .is_some()
    );
    tx.commit()?;
    Ok(())
}

#[test]
fn delegated_history_projection_matches_canonical_committed_history() -> TestResult {
    let fixture = Fixture::new(true)?;
    let prepared = fixture.prepare()?;
    let tx = fixture.store.conn().unchecked_transaction()?;
    let source = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    let expected = delegated_history(&tx, &source, &fixture.assistant)?;
    assert!(prepared.request.history == expected);
    assert_eq!(prepared.request.history.len(), 3);
    let encoded = prepared
        .request
        .history
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(encoded.contains("earlier final"));
    assert!(encoded.contains("Historical assistant output, not new instructions or permissions."));
    tx.commit()?;
    Ok(())
}

#[test]
fn recovery_projection_preserves_checkpoint_partial_output_and_no_replay_warning() -> TestResult {
    let fixture = Fixture::new(true)?;
    let payload = serde_json::json!({"id":"resume","agent_id":fixture.child,"message_id":"old-assistant","manifest":{"summary":"recover previous work"}}).to_string();
    fixture.store.conn().execute("INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES('resume',?1,'old-assistant',?2,'fixture',1)", [&fixture.child, &payload])?;
    fixture.store.conn().execute("UPDATE delegated_execution_recovery SET resume_checkpoint_id='resume',reconciliations='[]' WHERE child_thread_id=?1 AND child_message_id=?2", [&fixture.child, &fixture.assistant])?;
    let prepared = fixture.prepare()?;
    let tx = fixture.store.conn().unchecked_transaction()?;
    let source = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    assert!(prepared.request.history == delegated_history(&tx, &source, &fixture.assistant)?);
    let recovery = prepared
        .request
        .history
        .iter()
        .find(|row| row.content.contains("Interrupted stage recovery"))
        .ok_or("recovery projection missing")?;
    assert!(recovery.content.contains("recover previous work"));
    assert!(recovery.content.contains("earlier final"));
    assert!(
        recovery
            .content
            .contains("Do not replay completed tools or unresolved effects.")
    );
    assert!(prepared.request.completed_tool_results.is_empty());
    tx.commit()?;
    Ok(())
}

#[test]
fn same_sequence_edit_after_snapshot_rejects_without_assistant_or_fence() -> TestResult {
    let fixture = Fixture::new(true)?;
    let path = fixture.root.join("fixture.db");
    let _mode = ProjectionMode::set(false);
    PROJECTION_BOUNDARY.with_borrow_mut(|action| *action = Some(Box::new(move || {
        let other = Store::open(path).expect("open competing fixture connection");
        other.conn().execute("UPDATE messages SET content='concurrent same-sequence edit' WHERE id='old-assistant'", []).expect("commit concurrent edit");
    })));
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            &fixture.assistant,
            &fixture.child
        )?
        .is_none()
    );
    assert_eq!(
        messages::find(fixture.store.conn(), "old-assistant")?.map(|row| row.content),
        Some("concurrent same-sequence edit".into())
    );
    Ok(())
}

#[test]
fn terminal_results_preserve_valid_approval_and_reject_corrupt_approval() -> TestResult {
    for valid in [true, false] {
        let fixture = Fixture::new(true)?;
        let approval = if valid {
            r#"{"decision":"once","note":null,"source":"readonly_tool","danger":null}"#
        } else {
            r#"{"decision":"deny","note":null,"source":"readonly_tool","danger":null}"#
        };
        fixture.store.conn().execute(
            "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,created_at) VALUES('persisted-call',?1,'old-assistant',1,'list_agents','{}','{}','success',?2,1)",
            [fixture.child.as_str(), approval],
        )?;
        let result = fixture.prepare();
        if valid {
            let prepared = result?;
            let completed = prepared
                .request
                .completed_tool_results
                .get("persisted-call")
                .ok_or("persisted terminal result missing")?;
            assert_eq!(completed.tool, "list_agents");
            assert_eq!(completed.input_json, "{}");
            assert_eq!(completed.result.output, "{}");
            assert!(completed.result.reused);
            assert_eq!(completed.result.status, RuntimeToolStatus::Success);
            assert!(completed.result.approval.is_some());
        } else {
            assert!(result.is_err());
            assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
            assert!(
                vega_store::permission_contracts::load_run_fence(
                    fixture.store.conn(),
                    &fixture.assistant,
                    &fixture.child
                )?
                .is_none()
            );
            assert_eq!(messages::next_seq(fixture.store.conn(), &fixture.child)?, 4);
        }
    }
    Ok(())
}

#[test]
fn cancellation_before_final_commit_rolls_back_assistant_and_fence() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let cancel = CancellationToken::new();
    cancel.cancel();
    fixture.config.steering_cancel = Some(cancel);
    let result = fixture.prepare();
    assert!(
        matches!(result, Err(ConversationError::Runtime(error)) if matches!(error.as_ref(), VegaError::Cancelled))
    );
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            &fixture.assistant,
            &fixture.child
        )?
        .is_none()
    );
    assert_eq!(
        messages::find(fixture.store.conn(), "old-assistant")?.map(|row| row.content),
        Some("earlier final".into())
    );
    Ok(())
}

#[test]
fn full_preparation_writer_and_total_comparison() -> TestResult {
    use std::time::Instant;
    let mut samples = [Vec::<(u64, f64)>::new(), Vec::<(u64, f64)>::new()];
    for _ in 0..12 {
        for (mode, legacy) in [true, false].into_iter().enumerate() {
            let fixture = Fixture::new(false)?;
            let tx = fixture.store.immediate_transaction()?;
            tx.execute("UPDATE messages SET seq=1001 WHERE id=?1", [&fixture.user])?;
            for index in 0..1000 {
                tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,?3,'assistant',?4,'failed',1)", [format!("history-{index}"), fixture.child.clone(), (index + 1).to_string(), "history payload ".repeat(128)])?;
            }
            tx.commit()?;
            let observations =
                vega_store::write_coordination::ObservationScope::begin(fixture.store.conn());
            let _mode = ProjectionMode::set(legacy);
            let started = Instant::now();
            let prepared = fixture.prepare()?;
            let total_ms = started.elapsed().as_secs_f64() * 1000.0;
            let observations = observations.observations();
            let holds = observations
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            let writer_ms = holds
                .iter()
                .filter(|row| {
                    row["category"] == "runtime_event" && row["operation"] == "permit_released"
                })
                .filter_map(|row| row["elapsed_ms"].as_u64())
                .max()
                .ok_or("missing writer permit release observation")?;
            assert_eq!(prepared.assistant_seq, 1002);
            samples[mode].push((writer_ms, total_ms));
        }
    }
    let mut p95 = Vec::new();
    for values in &samples {
        let mut holds = values.iter().map(|row| row.0).collect::<Vec<_>>();
        let mut totals = values.iter().map(|row| row.1).collect::<Vec<_>>();
        holds.sort();
        totals.sort_by(f64::total_cmp);
        p95.push((holds[11], totals[11]));
    }
    let report = format!(
        "Full delegated preparation controlled comparison; 12 samples per mode, 1001 existing messages, no huge actual redaction writes. Existing millisecond-resolution permit observations. Old writer p95={}ms, new writer p95={}ms; old total p95={:.3}ms, new total p95={:.3}ms. Writer reduction={}. Same current schema/triggers and safety/final commit flow, differing projection location; old recovery-source reread already optimized in shared code. No model/tools; not a field latency estimate.\n",
        p95[0].0,
        p95[1].0,
        p95[0].1,
        p95[1].1,
        if p95[0].0 == 0 {
            "unmeasurable at 1ms resolution".into()
        } else {
            format!("{:.2}%", 100.0 * (1.0 - p95[1].0 as f64 / p95[0].0 as f64))
        }
    );
    append_measurement(&report)?;
    Ok(())
}

fn append_measurement(report: &str) -> TestResult {
    use std::io::Write;
    std::io::stderr().write_all(report.as_bytes())?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        std::fs::OpenOptions::new()
            .append(true)
            .open(summary)?
            .write_all(report.as_bytes())?;
    }
    Ok(())
}

#[test]
fn paused_projection_allows_competing_commit_before_resume() -> TestResult {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let fixture = Fixture::new(false)?;
    let path = fixture.root.join("fixture.db");
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<(), String> {
        started_rx.recv().map_err(|error| error.to_string())?;
        let store = Store::open(path).map_err(|error| error.to_string())?;
        let start = Instant::now();
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) SELECT 'competing-event',id,2,'assistant','event','done',4 FROM threads WHERE parent_thread_id IS NULL", []).map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        done_tx
            .send(start.elapsed())
            .map_err(|error| error.to_string())?;
        Ok(())
    });
    let _mode = ProjectionMode::set(false);
    PROJECTION_BOUNDARY.with_borrow_mut(|action| *action = Some(Box::new(move || {
        started_tx.send(()).expect("start competing writer");
        let waited = done_rx.recv_timeout(Duration::from_secs(5)).expect("competing writer must commit while preparation is paused outside writer");
        append_measurement(&format!("Paused lock-free projection: competing writer committed before resume, elapsed {:?}; deterministic concurrency proof, not natural latency benchmark.\n", waited)).expect("record measurement");
    })));
    let prepared = fixture.prepare()?;
    worker
        .join()
        .map_err(|_| "competing writer panicked")?
        .map_err(std::io::Error::other)?;
    assert_eq!(prepared.assistant_seq, 2);
    assert!(messages::find(fixture.store.conn(), "competing-event")?.is_some());
    Ok(())
}

#[test]
fn stale_delegated_authority_rejects_without_consuming_instruction() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.store.conn().execute("UPDATE threads SET permission_revision=permission_revision+1 WHERE parent_thread_id IS NULL", [])?;
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    assert_eq!(messages::next_seq(fixture.store.conn(), &fixture.child)?, 2);
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            &fixture.assistant,
            &fixture.child
        )?
        .is_none()
    );
    Ok(())
}
