use super::*;
use serde_json::json;
use vega_store::delegated_stages as stages;

#[path = "recovery_tests/fixture.rs"]
mod fixture;
use fixture::{Fixture, assert_failure, history, recovery_rows, rows, spawn};

#[tokio::test]
async fn legacy_terminals_release_capacity_without_changing_history() {
    let fixture = Fixture::new();
    let originals = (0..8)
        .map(|index| {
            fixture.child(
                "parent",
                &format!("legacy-{index}"),
                if index < 5 { "completed" } else { "failed" },
            )
        })
        .collect::<Vec<_>>();
    fixture.parent("other-parent");
    let other = fixture.child("other-parent", "other-child", "completed");
    fixture.message("parent", "next-parent-run", "assistant", "done");
    let before = history(&fixture.store);
    let supervisor = Supervisor::new();
    stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
    let snapshots = supervisor.registry.snapshots(None).unwrap();
    assert_eq!(snapshots.len(), 8);
    for original in &originals {
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.agent_id == original.child_thread_id)
            .unwrap();
        assert_eq!(snapshot.parent_thread_id, original.parent_thread_id);
        assert_eq!(snapshot.parent_message_id, original.parent_message_id);
        assert_eq!(snapshot.message_id, original.child_message_id);
        assert_eq!(snapshot.state.as_str(), original.state);
        assert_eq!(snapshot.result, original.result);
        assert_eq!(snapshot.truncated, original.truncated);
        assert_eq!(snapshot.release_state, "recovered_abandoned");
        assert!(!snapshot.ready_for_review);
        assert!(snapshot.checkpoint.is_none());
        assert_eq!(
            stages::execution_message(
                fixture.store.conn(),
                &snapshot.agent_id,
                &snapshot.message_id,
            )
            .unwrap(),
            Some(snapshot.message_id.clone())
        );
        assert!(
            stages::handoff(
                fixture.store.conn(),
                &snapshot.agent_id,
                &snapshot.message_id,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            supervisor
                .registry
                .released_terminal(&[(snapshot.agent_id.clone(), snapshot.message_id.clone())])
                .unwrap()
        );
    }
    assert_eq!(
        stages::release_state(
            fixture.store.conn(),
            &other.child_thread_id,
            &other.child_message_id,
        )
        .unwrap(),
        "unknown"
    );
    let metadata = recovery_rows(&fixture.store);
    assert_eq!(metadata[0].len(), 8);
    assert_eq!(metadata[1].len(), 8);
    assert!(metadata[2].is_empty());
    assert!(metadata[3].is_empty());
    assert_eq!(history(&fixture.store), before);

    stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
    let restarted = Supervisor::new();
    stage_recovery::load(&restarted.registry, &fixture.database, "parent").unwrap();
    assert_eq!(restarted.registry.snapshots(None).unwrap(), snapshots);
    assert_eq!(recovery_rows(&fixture.store), metadata);
    assert_eq!(history(&fixture.store), before);
    let listed = restarted
        .hook()
        .call(
            "list".into(),
            "list_agents".into(),
            json!({}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(listed, receipt(snapshots.clone(), false).unwrap());

    let launch = fixture.launch("next-parent-run");
    let reservations = (0..4)
        .map(|index| spawn(&restarted, &launch, &format!("new-{index}")).unwrap())
        .collect::<Vec<_>>();
    assert_failure(spawn(&restarted, &launch, "fifth"), "capacity_exceeded");
    assert_eq!(restarted.registry.snapshots(None).unwrap(), snapshots);
    assert_eq!(history(&fixture.store), before);
    drop(reservations);
}

#[test]
fn legacy_terminal_recovery_requires_matching_owner_proof() {
    for state in ["completed", "failed"] {
        let fixture = Fixture::new();
        let child = fixture.child("parent", "legacy", state);
        let before = history(&fixture.store);
        let wrong_owner = DelegatedOwnership::acquire(&fixture.database, "another-child").unwrap();
        for owner in [None, Some(wrong_owner.as_ref())] {
            let tx = fixture.store.immediate_transaction().unwrap();
            let error = vega_store::recovery::recover_thread_with_owner(
                &tx,
                &child.child_thread_id,
                20,
                owner,
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("delegated writer proof required")
            );
            tx.commit().unwrap();
            assert!(recovery_rows(&fixture.store).iter().all(Vec::is_empty));
            assert_eq!(history(&fixture.store), before);
        }
    }
}

#[tokio::test]
async fn locked_terminals_keep_truthful_receipts_without_freeing_occupancy() {
    for state in ["completed", "failed", "interrupted"] {
        let fixture = Fixture::new();
        let child = fixture.child("parent", "locked", state);
        let before = history(&fixture.store);
        let owner = DelegatedOwnership::acquire(&fixture.database, &child.child_thread_id).unwrap();
        assert!(vega_store::recovery::recover_thread(fixture.store.conn(), "locked", 20).is_err());
        let supervisor = Supervisor::new();
        stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
        let key = (
            child.child_thread_id.clone(),
            child.child_message_id.clone(),
        );
        let snapshot = supervisor.registry.snapshots(None).unwrap().remove(0);
        assert_eq!(snapshot.state.as_str(), state);
        assert_eq!(snapshot.result, child.result);
        assert_eq!(snapshot.truncated, child.truncated);
        assert_eq!(snapshot.release_state, "unknown");
        assert!(!snapshot.ready_for_review);
        assert!(snapshot.checkpoint.is_none());
        assert!(
            !supervisor
                .registry
                .released_terminal(std::slice::from_ref(&key))
                .unwrap()
        );
        let hook = supervisor.hook();
        let listed = hook
            .call(
                "list".into(),
                "list_agents".into(),
                json!({}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(listed["agents"][0]["state"], state);
        assert_eq!(listed["agents"][0]["release_state"], "unknown");
        assert_eq!(listed["agents"][0]["ready_for_review"], false);
        let wait_input =
            json!({"targets": [{"agent_id": key.0, "message_id": key.1}], "timeout_ms": 0});
        let waited = hook
            .call(
                "wait".into(),
                "wait_agent".into(),
                wait_input.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(waited["wake_reason"], "timeout");
        assert_eq!(waited["timed_out"], true);
        assert_eq!(waited["agents"][0]["state"], state);
        let wait_cancel = CancellationToken::new();
        let mut waiting = hook.call(
            "wait-again".into(),
            "wait_agent".into(),
            wait_input,
            wait_cancel.clone(),
        );
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        wait_cancel.cancel();
        assert!(matches!(waiting.await, Err(VegaError::Cancelled)));

        let launch = fixture.launch("parent-run");
        let cancelled = supervisor.cancel(&launch, &key).await.unwrap();
        assert_eq!(cancelled["agent"], serde_json::to_value(&snapshot).unwrap());
        assert!(
            !supervisor
                .registry
                .released_terminal(std::slice::from_ref(&key))
                .unwrap()
        );
        for tool in ["followup_agent", "resume_agent"] {
            let mut input = json!({"agent_id": key.0, "message_id": key.1, "task": "Continue"});
            if tool == "resume_agent" {
                input["checkpoint_id"] = "unavailable".into();
                input["source_version"] = 0.into();
                input["source_fingerprint"] = "unavailable".into();
                input["reconciliations"] = json!([]);
            }
            assert!(vega_runtime::valid_subagent_input(tool, &input.to_string()));
            assert_failure(
                supervisor.reserve(&launch, tool, tool, &input),
                "agent_busy",
            );
        }
        let reservations = (0..3)
            .map(|index| spawn(&supervisor, &launch, &format!("new-{index}")).unwrap())
            .collect::<Vec<_>>();
        assert_failure(spawn(&supervisor, &launch, "fourth"), "capacity_exceeded");
        supervisor.registry.cancel_all();
        assert_eq!(
            supervisor
                .registry
                .snapshots(Some(std::slice::from_ref(&key)))
                .unwrap(),
            vec![snapshot]
        );
        assert!(recovery_rows(&fixture.store).iter().all(Vec::is_empty));
        assert_eq!(history(&fixture.store), before);
        drop(reservations);
        drop(owner);
        let restarted = Supervisor::new();
        stage_recovery::load(&restarted.registry, &fixture.database, "parent").unwrap();
        assert!(restarted.registry.released_terminal(&[key]).unwrap());
        assert_eq!(history(&fixture.store), before);
    }
}

#[test]
fn existing_release_metadata_is_preserved_and_quarantine_blocks_admission() {
    for state in ["completed", "failed"] {
        for release in ["held", "quarantined", "released", "recovered_abandoned"] {
            let fixture = Fixture::new();
            let child = fixture.child("parent", "existing", state);
            stages::register(
                fixture.store.conn(),
                "existing",
                &child.child_message_id,
                None,
                &json!([]),
            )
            .unwrap();
            if release != "held" {
                stages::set_release(
                    fixture.store.conn(),
                    "existing",
                    &child.child_message_id,
                    release,
                )
                .unwrap();
            }
            let before = history(&fixture.store);
            let metadata = recovery_rows(&fixture.store);
            let recovered =
                vega_store::recovery::recover_thread(fixture.store.conn(), "existing", 20);
            assert_eq!(recovered.is_err(), release == "quarantined");
            let supervisor = Supervisor::new();
            for _ in 0..2 {
                stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
                assert_eq!(recovery_rows(&fixture.store), metadata);
                assert_eq!(history(&fixture.store), before);
            }
            let occupied = matches!(release, "held" | "quarantined");
            let snapshot = supervisor.registry.snapshots(None).unwrap().remove(0);
            assert_eq!(snapshot.release_state, release);
            assert_eq!(
                snapshot.state.as_str(),
                if occupied { "running" } else { state }
            );
            assert!(!snapshot.ready_for_review);
            assert!(snapshot.checkpoint.is_none());
            let key = (child.child_thread_id, child.child_message_id);
            assert_eq!(
                supervisor
                    .registry
                    .released_terminal(std::slice::from_ref(&key))
                    .unwrap(),
                !occupied
            );
            let launch = fixture.launch("parent-run");
            if release == "quarantined" {
                assert_failure(
                    spawn(&supervisor, &launch, "new"),
                    "Subagent ownership remains unknown",
                );
            } else {
                let available = if occupied { 3 } else { 4 };
                let reservations = (0..available)
                    .map(|index| spawn(&supervisor, &launch, &format!("new-{index}")).unwrap())
                    .collect::<Vec<_>>();
                assert_failure(spawn(&supervisor, &launch, "extra"), "capacity_exceeded");
                drop(reservations);
            }
            supervisor.registry.cancel_all();
            let cancelled = supervisor
                .registry
                .snapshots(Some(&[key]))
                .unwrap()
                .remove(0);
            assert_eq!(
                cancelled.state.as_str(),
                if occupied { "cancelling" } else { state }
            );
        }
    }
}

#[test]
fn abandoned_execution_recovery_retains_interruption_and_unknown_effects() {
    for state in ["running", "cancelling", "interrupted"] {
        let fixture = Fixture::new();
        let child = fixture.child("parent", "abandoned", state);
        for (seq, id, tool, status) in [
            (1, "write-call", "write", "running"),
            (2, "read-call", "read", "running"),
            (3, "pending-call", "bash", "pending_approval"),
        ] {
            tool_calls::insert(
                fixture.store.conn(),
                tool_calls::NewToolCall {
                    id,
                    thread_id: "abandoned",
                    message_id: &child.child_message_id,
                    seq,
                    tool,
                    input_json: "{}",
                    status,
                    created_at: 5,
                },
            )
            .unwrap();
        }
        let usage = rows(&fixture.store, "token_usage");
        let owner = DelegatedOwnership::acquire(&fixture.database, "abandoned").unwrap();
        let supervisor = Supervisor::new();
        stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
        assert!(recovery_rows(&fixture.store).iter().all(Vec::is_empty));
        assert_eq!(
            tool_calls::find_state(fixture.store.conn(), "write-call")
                .unwrap()
                .unwrap()
                .status,
            "running"
        );
        drop(owner);
        stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
        let snapshot = supervisor.registry.snapshots(None).unwrap().remove(0);
        assert_eq!(snapshot.state, SubagentState::Interrupted);
        assert_eq!(snapshot.release_state, "recovered_abandoned");
        assert!(!snapshot.ready_for_review);
        assert_eq!(
            snapshot.result,
            if state == "interrupted" {
                child.result.as_str()
            } else {
                "Interrupted before completion"
            }
        );
        let checkpoint = snapshot.checkpoint.unwrap();
        assert_eq!(checkpoint["origin"], "interrupted");
        assert_eq!(checkpoint["manifest"]["ready_for_review"], false);
        assert!(checkpoint["manifest"]["stage"].is_null());
        assert_eq!(
            checkpoint["unknown_effects"],
            json!([{"call_id": "write-call", "tool": "write", "status": "running"}])
        );
        assert!(
            stages::handoff(fixture.store.conn(), "abandoned", &child.child_message_id)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            messages::find(fixture.store.conn(), &child.child_message_id)
                .unwrap()
                .unwrap()
                .status,
            "interrupted"
        );
        for id in ["write-call", "read-call"] {
            let call = tool_calls::find_state(fixture.store.conn(), id)
                .unwrap()
                .unwrap();
            assert_eq!(call.status, "cancelled");
            assert_eq!(
                call.output_text.as_deref(),
                Some(vega_store::recovery::RECOVERY_CANCELLED_OUTPUT)
            );
        }
        let pending = tool_calls::find_state(fixture.store.conn(), "pending-call")
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, "rejected");
        assert_eq!(
            pending.approval.as_deref(),
            Some(vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON)
        );
        assert_eq!(rows(&fixture.store, "token_usage"), usage);
        let metadata = recovery_rows(&fixture.store);
        assert_eq!(metadata[2].len(), 1);
        stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
        assert_eq!(recovery_rows(&fixture.store), metadata);
    }
}

#[tokio::test]
async fn current_terminal_persistence_stays_held_until_explicit_release() {
    let fixture = Fixture::new();
    let launch = fixture.launch("parent-run");
    let supervisor = Supervisor::new();
    let reserved = spawn(&supervisor, &launch, "current").unwrap();
    create_child(
        fixture.database.clone(),
        reserved.snapshot.clone(),
        reserved.task.clone(),
        reserved.call_id.clone(),
        launch.settings.clone(),
        reserved.cancel.clone(),
        None,
        None,
        reserved.owner.clone(),
    )
    .await
    .unwrap();
    fixture.message(
        &reserved.snapshot.agent_id,
        &reserved.snapshot.message_id,
        "assistant",
        "streaming",
    );
    let observer = Supervisor::new();
    stage_recovery::load(&observer.registry, &fixture.database, "parent").unwrap();
    let key = (
        reserved.snapshot.agent_id.clone(),
        reserved.snapshot.message_id.clone(),
    );
    assert_eq!(
        observer.registry.snapshots(None).unwrap()[0].state,
        SubagentState::Running
    );
    assert!(
        !observer
            .registry
            .released_terminal(std::slice::from_ref(&key))
            .unwrap()
    );
    let mut terminal = reserved.snapshot.clone();
    terminal.state = SubagentState::Completed;
    terminal.result = "Current result".into();
    assert_eq!(
        messages::finish(
            fixture.store.conn(),
            &terminal.message_id,
            &terminal.result,
            "done"
        )
        .unwrap(),
        1
    );
    let boundary = persist_child_terminal(
        fixture.database.clone(),
        terminal.clone(),
        true,
        reserved.owner.clone(),
    )
    .await
    .unwrap();
    assert!(boundary.is_some());
    stage_recovery::load(&observer.registry, &fixture.database, "parent").unwrap();
    let draining = observer.registry.snapshots(None).unwrap().remove(0);
    assert_eq!(draining.state, SubagentState::Running);
    assert_eq!(draining.release_state, "held");
    assert!(!draining.ready_for_review);
    assert!(
        !observer
            .registry
            .released_terminal(std::slice::from_ref(&key))
            .unwrap()
    );
    assert!(
        stages::latest(fixture.store.conn(), &key.0, &key.1)
            .unwrap()
            .is_none()
    );
    observer.registry.cancel_all();
    assert_eq!(
        observer.registry.snapshots(None).unwrap()[0].state,
        SubagentState::Cancelling
    );
    stages::set_release(fixture.store.conn(), &key.0, &key.1, "released").unwrap();
    stage_recovery::refresh(&fixture.store, &mut terminal).unwrap();
    supervisor.registry.finish(terminal.clone(), boundary);
    drop(reserved);
    assert!(
        supervisor
            .registry
            .released_terminal(std::slice::from_ref(&key))
            .unwrap()
    );
    assert_eq!(
        supervisor
            .registry
            .snapshots(Some(std::slice::from_ref(&key)))
            .unwrap(),
        vec![terminal.clone()]
    );
    let restarted = Supervisor::new();
    stage_recovery::load(&restarted.registry, &fixture.database, "parent").unwrap();
    assert_eq!(restarted.registry.snapshots(None).unwrap(), vec![terminal]);
    assert!(restarted.registry.released_terminal(&[key]).unwrap());
}

#[test]
fn failed_release_write_rolls_back_metadata_and_keeps_capacity_occupied() {
    for state in ["completed", "failed"] {
        let fixture = Fixture::new();
        let child = fixture.child("parent", "legacy", state);
        let before = history(&fixture.store);
        fixture.store.conn().execute_batch(
            "CREATE TRIGGER reject_recovered_release BEFORE UPDATE OF release_state ON delegated_execution_recovery
             WHEN NEW.release_state = 'recovered_abandoned'
             BEGIN SELECT RAISE(ABORT, 'injected release write failure'); END;"
        ).unwrap();
        let error =
            vega_store::recovery::recover_thread(fixture.store.conn(), "legacy", 20).unwrap_err();
        assert!(error.to_string().contains("injected release write failure"));
        assert!(recovery_rows(&fixture.store).iter().all(Vec::is_empty));
        assert_eq!(history(&fixture.store), before);
        let supervisor = Supervisor::new();
        stage_recovery::load(&supervisor.registry, &fixture.database, "parent").unwrap();
        let snapshot = supervisor.registry.snapshots(None).unwrap().remove(0);
        assert_eq!(snapshot.state.as_str(), state);
        assert_eq!(snapshot.release_state, "unknown");
        assert!(!snapshot.ready_for_review);
        assert!(snapshot.checkpoint.is_none());
        assert!(
            !supervisor
                .registry
                .released_terminal(&[("legacy".into(), child.child_message_id.clone())])
                .unwrap()
        );
        assert!(recovery_rows(&fixture.store).iter().all(Vec::is_empty));
        assert_eq!(history(&fixture.store), before);
        let launch = fixture.launch("parent-run");
        let reservations = (0..3)
            .map(|index| spawn(&supervisor, &launch, &format!("new-{index}")).unwrap())
            .collect::<Vec<_>>();
        assert_failure(spawn(&supervisor, &launch, "fourth"), "capacity_exceeded");
        fixture
            .store
            .conn()
            .execute_batch("DROP TRIGGER reject_recovered_release")
            .unwrap();
        let restarted = Supervisor::new();
        stage_recovery::load(&restarted.registry, &fixture.database, "parent").unwrap();
        assert_eq!(
            restarted.registry.snapshots(None).unwrap()[0].release_state,
            "recovered_abandoned"
        );
        assert_eq!(history(&fixture.store), before);
        drop(reservations);
    }
}
