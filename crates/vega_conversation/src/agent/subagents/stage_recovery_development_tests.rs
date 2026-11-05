use super::*;
use crate::types::{DelegatedPermissionSource, PermissionFence};

struct Database(PathBuf);

impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

struct Recovery {
    database: Database,
    store: Store,
    snapshot: SubagentSnapshot,
    previous: ContinuationBoundary,
    authority: DelegatedAuthority,
    checkpoint: Value,
    input: Value,
}

fn message(conn: &vega_store::Connection, thread: &str, role: &str, status: &str) -> String {
    let id = ulid::Ulid::generate().to_string();
    vega_store::messages::insert(
        conn,
        &vega_store::messages::MessageRow {
            id: id.clone(),
            thread_id: thread.into(),
            seq: 1,
            role: role.into(),
            kind: "text".into(),
            content: "Synthetic execution".into(),
            status: status.into(),
            created_at: 100,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    id
}

fn recovery() -> Recovery {
    let root = std::env::temp_dir().join(format!("vega-resume-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let database = Database(root.join("state.sqlite"));
    let store = Store::open(&database.0).unwrap();
    store.migrate().unwrap();
    let parent = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let child = crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
    let tx = store.immediate_transaction().unwrap();
    tx.execute(
        "UPDATE threads SET parent_thread_id=?2 WHERE id=?1",
        (&child.id, &parent.id),
    )
    .unwrap();
    let parent_message = message(&tx, &parent.id, "assistant", "streaming");
    let child_message = message(&tx, &child.id, "assistant", "failed");
    let state = vega_store::permission_contracts::load_state(&tx).unwrap();
    let parent_fence = PermissionFence {
        schema_version: 1,
        execution_policy_version: 2,
        authority_epoch: state.authority_epoch,
        database_identity: store.database_identity().permission_fence_key(),
        thread_id: parent.id.clone(),
        project_id: String::new(),
        workspace: root.to_string_lossy().into_owned(),
        run_mode: "execute".into(),
        permission_mode: "confirm".into(),
        permission_revision: parent.permission_revision,
        exact_rules: Vec::new(),
        full_access_grant: None,
        delegation: None,
    };
    let child_fence = PermissionFence {
        thread_id: child.id.clone(),
        permission_revision: child.permission_revision,
        delegation: Some(DelegatedPermissionSource {
            parent_message_id: parent_message.clone(),
            child_message_id: child_message.clone(),
            parent_fence: Box::new(parent_fence.clone()),
        }),
        ..parent_fence.clone()
    };
    assert!(parent_fence.is_valid());
    assert!(child_fence.is_valid());
    vega_store::permission_contracts::record_run_fence(
        &tx,
        &parent_message,
        &parent.id,
        &serde_json::to_string(&parent_fence).unwrap(),
    )
    .unwrap();
    tx.execute(
        "INSERT INTO delegated_executions(child_thread_id, child_message_id, parent_thread_id, parent_message_id, request_call_id, ordinal, kind, name, task, state, created_at) VALUES(?1,?2,?3,?4,?5,1,'spawn','Synthetic','Inspect synthetic state','running',100)",
        (&child.id, &child_message, &parent.id, &parent_message, ulid::Ulid::generate().to_string()),
    ).unwrap();
    vega_store::permission_contracts::record_delegated_fence(
        &tx,
        &child.id,
        &child_message,
        &serde_json::to_string(&child_fence).unwrap(),
    )
    .unwrap();
    tx.execute(
        "UPDATE delegated_executions SET state='interrupted' WHERE child_message_id=?1",
        [&child_message],
    )
    .unwrap();
    stages::register(&tx, &child.id, &child_message, None, &json!([])).unwrap();
    stages::set_release(&tx, &child.id, &child_message, "released").unwrap();
    let checkpoint = stages::append(
        &tx,
        &child.id,
        &child_message,
        "interrupted",
        json!({"artifacts":[]}),
        json!([]),
        100,
    )
    .unwrap();
    tx.commit().unwrap();
    let previous = ContinuationBoundary {
        message_id: child_message.clone(),
        source_version: checkpoint["source_version"].as_u64().unwrap(),
        fingerprint: checkpoint["source_fingerprint"].as_str().unwrap().into(),
        thread: vega_store::threads::find(store.conn(), &child.id)
            .unwrap()
            .unwrap(),
    };
    let snapshot = SubagentSnapshot {
        agent_id: child.id,
        parent_thread_id: parent.id,
        parent_message_id: parent_message,
        message_id: child_message,
        name: "Synthetic".into(),
        state: SubagentState::Interrupted,
        result: String::new(),
        truncated: false,
        checkpoint: Some(checkpoint.clone()),
        release_state: "released".into(),
        ready_for_review: false,
    };
    let input = json!({
        "checkpoint_id": checkpoint["id"],
        "source_version": checkpoint["source_version"],
        "source_fingerprint": checkpoint["source_fingerprint"],
        "reconciliations": [],
    });
    Recovery {
        database,
        store,
        snapshot,
        previous,
        checkpoint,
        input,
        authority: DelegatedAuthority {
            permission_fence: parent_fence,
            run_mode: vega_runtime::RuntimeRunMode::Execute,
            permission_mode: vega_runtime::RuntimePermissionMode::Confirm,
            exact_rules: Vec::new(),
        },
    }
}

fn verify(fixture: &Recovery) -> Result<Value, VegaError> {
    verify_resume(
        &fixture.store,
        &fixture.snapshot,
        &fixture.previous,
        &fixture.input,
        &fixture.authority,
    )
}

fn assert_rejected(result: Result<Value, VegaError>, expected: &str) {
    match result {
        Err(VegaError::Tool { message, .. }) => assert_eq!(message, expected),
        other => panic!("expected {expected}, received {other:?}"),
    }
}

#[test]
fn resume_preflight_reads_committed_checkpoint_while_another_connection_holds_writer() {
    let fixture = recovery();
    let writer = Store::open_existing(&fixture.database.0).unwrap();
    let tx = writer.immediate_transaction().unwrap();
    tx.execute(
        "UPDATE permission_contract_state SET reset_state='pending'",
        [],
    )
    .unwrap();
    assert_eq!(verify(&fixture).unwrap(), fixture.checkpoint);
    assert!(!writer.conn().is_autocommit());
}

#[test]
fn resume_preflight_rejects_stale_checkpoint_without_competing_for_writer() {
    let mut fixture = recovery();
    fixture.input["source_fingerprint"] = json!("stale");
    let writer = Store::open_existing(&fixture.database.0).unwrap();
    let _tx = writer.immediate_transaction().unwrap();
    assert_rejected(verify(&fixture), "stale recovery boundary");
}

#[test]
fn resume_preflight_rejects_changed_authority_without_competing_for_writer() {
    let mut fixture = recovery();
    fixture.authority.permission_fence.permission_revision += 1;
    let writer = Store::open_existing(&fixture.database.0).unwrap();
    let _tx = writer.immediate_transaction().unwrap();
    assert_rejected(verify(&fixture), "recovery permission source changed");
}

#[test]
fn resume_preflight_rejects_changed_child_permissions_and_reset() {
    let fixture = recovery();
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1",
            [&fixture.snapshot.agent_id],
        )
        .unwrap();
    assert_rejected(verify(&fixture), "stale recovery boundary");
    fixture
        .store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state='pending'",
            [],
        )
        .unwrap();
    assert_rejected(verify(&fixture), "recovery permission source changed");
}

#[test]
fn actual_admission_recheck_rejects_checkpoint_after_child_state_changes() {
    let fixture = recovery();
    assert_eq!(verify(&fixture).unwrap(), fixture.checkpoint);
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1",
            [&fixture.snapshot.agent_id],
        )
        .unwrap();
    let tx = fixture.store.immediate_transaction().unwrap();
    assert_rejected(
        verify_resume_in_transaction(&tx, &fixture.snapshot, &fixture.previous, &fixture.input),
        "stale recovery boundary",
    );
}
