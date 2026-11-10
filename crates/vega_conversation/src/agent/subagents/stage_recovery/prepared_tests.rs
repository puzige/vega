use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};
use std::sync::mpsc;
use std::time::Duration;
use vega_store::context_compaction::{
    ContextCheckpointInstall, NewContextCheckpoint, PreparedContextCheckpoint,
    source_version::PreparedSourceVersion,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    store: Store,
    root: PathBuf,
    parent: String,
    parent_message: String,
    child: String,
    message: String,
    input: Value,
    authority: DelegatedAuthority,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "vega-prepared-delegation-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let parent = ulid::Ulid::generate().to_string();
        let parent_message = ulid::Ulid::generate().to_string();
        let child = ulid::Ulid::generate().to_string();
        let message = ulid::Ulid::generate().to_string();
        let project = ulid::Ulid::generate().to_string();
        store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES(?1,?2,'fixture',1,1)", [project.as_str(),root.to_string_lossy().as_ref()])?;
        for id in [&parent, &child] {
            store.conn().execute("INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES(?1,?2,'model',1,1)", [id.as_str(),project.as_str()])?;
        }
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [parent_message.as_str(),parent.as_str()])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('instruction',?1,1,'user','Task','done',1),(?2,?1,2,'assistant','Recovered','interrupted',2)", [child.as_str(),message.as_str()])?;
        vega_store::subagents::insert(
            store.conn(),
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.clone(),
                parent_thread_id: parent.clone(),
                parent_message_id: parent_message.clone(),
                spawn_call_id: ulid::Ulid::generate().to_string(),
                child_message_id: message.clone(),
                name: "worker".into(),
                task: "Task".into(),
                state: "running".into(),
                result: String::new(),
                truncated: false,
                created_at: 2,
            },
        )?;
        let parent_row =
            vega_store::threads::find(store.conn(), &parent)?.ok_or("missing parent")?;
        let parent_fence = crate::permission_contracts::capture_fence(&store, &parent_row)?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_run_fence(
            &tx,
            &parent_message,
            &parent,
            &serde_json::to_string(&parent_fence)?,
        )?;
        tx.commit()?;
        let child_row = vega_store::threads::find(store.conn(), &child)?.ok_or("missing child")?;
        let child_fence = crate::permission_contracts::capture_delegated_fence_for_commit(
            &store,
            &child_row,
            &parent,
            &parent_message,
            &message,
            &parent_fence,
        )?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_delegated_fence(
            &tx,
            &child,
            &message,
            &serde_json::to_string(&child_fence)?,
        )?;
        vega_store::subagents::finish(&tx, &child, &message, "interrupted", "Interrupted", false)?;
        stages::register(&tx, &child, &message, None, &json!([]))?;
        tx.execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,output_text,created_at) VALUES('effect',?1,?2,3,'bash','{}','cancelled','Interrupted',3),('evidence',?1,?2,4,'read','{}','success','Verified',4)", [child.as_str(),message.as_str()])?;
        tx.commit()?;
        let prepared = stages::PreparedStageCheckpoint::prepare(store.conn(), &child, &message)?;
        let tx = store.immediate_transaction()?;
        let checkpoint = prepared.append(
            &tx,
            &child,
            &message,
            json!({"artifacts":[],"ready_for_review":false}),
            5,
        )?;
        stages::set_release(&tx, &child, &message, "released")?;
        tx.commit()?;
        let input = json!({
            "checkpoint_id":checkpoint["id"],
            "source_version":checkpoint["source_version"],
            "source_fingerprint":checkpoint["source_fingerprint"],
            "reconciliations":[{"call_id":"effect","evidence_call_ids":["evidence"]}]
        });
        Ok(Self {
            store,
            root,
            parent,
            parent_message,
            child,
            message,
            input,
            authority: DelegatedAuthority {
                permission_fence: parent_fence,
                run_mode: vega_runtime::RuntimeRunMode::Execute,
                permission_mode: vega_runtime::RuntimePermissionMode::Confirm,
                exact_rules: vec![],
            },
        })
    }

    fn snapshot(&self) -> SubagentSnapshot {
        SubagentSnapshot {
            agent_id: self.child.clone(),
            parent_thread_id: self.parent.clone(),
            parent_message_id: self.parent_message.clone(),
            message_id: self.message.clone(),
            name: "worker".into(),
            state: SubagentState::Interrupted,
            result: String::new(),
            truncated: false,
            checkpoint: None,
            release_state: "released".into(),
            ready_for_review: false,
        }
    }

    fn boundary(&self) -> Result<ContinuationBoundary, Box<dyn std::error::Error>> {
        let source =
            vega_store::context_compaction::load_source_version(self.store.conn(), &self.child)?;
        Ok(ContinuationBoundary {
            message_id: self.message.clone(),
            source_version: source.source_version,
            fingerprint: source.fingerprint,
            thread: vega_store::threads::find(self.store.conn(), &self.child)?
                .ok_or("missing child")?,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn prepared_resume_preserves_permission_source_release_and_explicit_effect_reconciliation()
-> TestResult {
    let fixture = Fixture::new()?;
    let snapshot = fixture.snapshot();
    let boundary = fixture.boundary()?;
    let checkpoint = verify_resume(
        &fixture.store,
        &snapshot,
        &boundary,
        &fixture.input,
        &fixture.authority,
    )?;
    assert_eq!(checkpoint["unknown_effects"][0]["call_id"], "effect");
    let prepared = PreparedSourceVersion::capture(fixture.store.conn(), &fixture.child)?;
    let tx = fixture.store.immediate_transaction()?;
    assert!(prepared.validate(&tx, &fixture.child)?);
    assert!(
        verify_resume_in_transaction(&tx, &snapshot, &boundary, &fixture.input, prepared.source())
            .is_ok()
    );
    for reconciliations in [
        json!([]),
        json!([{"call_id":"unmatched","evidence_call_ids":["evidence"]}]),
        json!([{"call_id":"effect","evidence_call_ids":["missing"]}]),
        json!([{"call_id":"effect","evidence_call_ids":["evidence"]},{"call_id":"effect","evidence_call_ids":["evidence"]}]),
    ] {
        let mut input = fixture.input.clone();
        input["reconciliations"] = reconciliations;
        assert!(
            verify_resume_in_transaction(&tx, &snapshot, &boundary, &input, prepared.source())
                .is_err()
        );
    }
    tx.execute("UPDATE delegated_execution_recovery SET release_state='quarantined' WHERE child_thread_id=?1", [&fixture.child])?;
    assert!(
        verify_resume_in_transaction(&tx, &snapshot, &boundary, &fixture.input, prepared.source())
            .is_err()
    );
    tx.rollback()?;
    Ok(())
}

#[test]
fn prepared_resume_rejects_external_source_and_permission_snapshot_changes() -> TestResult {
    for change_permission in [false, true] {
        let fixture = Fixture::new()?;
        let snapshot = fixture.snapshot();
        let boundary = fixture.boundary()?;
        let prepared = PreparedSourceVersion::capture(fixture.store.conn(), &fixture.child)?;
        let external = Store::open_existing(fixture.root.join("fixture.db"))?;
        if change_permission {
            external.conn().execute("UPDATE delegated_permission_fences SET permission_fence='{}' WHERE child_thread_id=?1", [&fixture.child])?;
        } else {
            external.conn().execute(
                "UPDATE messages SET content='Changed at the same sequence' WHERE id=?1",
                [&fixture.message],
            )?;
        }
        let tx = fixture.store.immediate_transaction()?;
        assert!(!prepared.validate(&tx, &fixture.child)?);
        let current = vega_store::context_compaction::load_source_version_in_transaction(
            &tx,
            &fixture.child,
        )?;
        assert!(
            verify_resume_in_transaction(&tx, &snapshot, &boundary, &fixture.input, &current)
                .is_err()
        );
        tx.rollback()?;
        assert_eq!(
            stages::release_state(fixture.store.conn(), &fixture.child, &fixture.message)?,
            "released"
        );
    }
    Ok(())
}

struct TouchThread {
    thread: String,
}

impl Sealed for TouchThread {}
impl DatabaseCommand for TouchThread {
    type Output = Result<usize, String>;

    fn apply(self, store: &Store) -> Self::Output {
        store
            .conn()
            .execute(
                "UPDATE threads SET updated_at=updated_at+1 WHERE id=?1",
                [self.thread],
            )
            .map_err(|error| error.to_string())
    }
}

#[test]
fn outside_source_preparation_keeps_shared_actor_available_and_prepared_commit_stale() -> TestResult
{
    let fixture = Fixture::new()?;
    let database = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    assert_eq!(
        crate::database_writer::commit(
            &database,
            identity,
            TouchThread {
                thread: fixture.parent.clone()
            }
        )
        .map_err(std::io::Error::other)??,
        1
    );
    let child = fixture.child.clone();
    let path = database.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let preparing = std::thread::spawn(move || -> Result<PreparedContextCheckpoint, String> {
        let store = Store::open_existing(&path).map_err(|error| error.to_string())?;
        let tx = store
            .conn()
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let source = vega_store::context_compaction::load_source_in_transaction(&tx, &child)
            .map_err(|error| error.to_string())?;
        let checkpoint = NewContextCheckpoint {
            thread_id: child,
            model: "model".into(),
            source_version: source.source_version,
            covered_through_seq: 2,
            covered_assistant_id: None,
            covered_assistant_text_bytes: 0,
            covered_assistant_tool_seq: 0,
            source_fingerprint: source.fingerprint,
            summary: "Prepared outside actor".into(),
            estimator_version: "development-estimator".into(),
            expected_previous_id: None,
            projection_plan: None,
            created_at: 10,
        };
        entered_tx.send(()).map_err(|error| error.to_string())?;
        release_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| error.to_string())?;
        let prepared = PreparedContextCheckpoint::prepare_in_transaction(&tx, checkpoint)
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(prepared)
    });
    entered_rx.recv_timeout(Duration::from_secs(5))?;
    let (receipt_tx, receipt_rx) = mpsc::channel();
    let thread = fixture.child.clone();
    let writer = std::thread::spawn(move || {
        let result = crate::database_writer::commit(&database, identity, TouchThread { thread });
        let _ = receipt_tx.send(result);
    });
    let receipt = receipt_rx.recv_timeout(Duration::from_secs(3));
    let released = release_tx.send(());
    let prepared = preparing
        .join()
        .map_err(|_| "prepare thread panicked")?
        .map_err(std::io::Error::other)?;
    writer.join().map_err(|_| "writer thread panicked")?;
    released?;
    assert_eq!(receipt?.map_err(std::io::Error::other)??, 1);
    let (installed, retired) = prepared.install_and_retire(fixture.store.conn(), || false)?;
    assert!(matches!(installed, ContextCheckpointInstall::Stale));
    assert_eq!(retired, None);
    Ok(())
}
