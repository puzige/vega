use super::branch_stub::{BranchFixture, GateTarget};
use super::*;

// The two lease/cleanup race policies below run the real `BranchWorkspaceService`
// over the finite in-process command boundary. The production capture, permit,
// owner-exclusive refresh and cleanup protocol are unchanged; only the external
// `git` process is replaced by captured bytes and an explicit completion channel.
// Every original assertion is preserved verbatim.

#[tokio::test]
async fn rejected_execute_cannot_compete_with_owner_cleanup_refresh() {
    let fixture = BranchFixture::new("main-clean");
    let service = Arc::new(fixture.service());
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let target = branch_id(&snapshot, "topic");
    let owner_permit = service
        .prepare_switch(target, CancellationToken::new())
        .await
        .expect("owner permit");
    fixture.expect_switch(Some("topic-current"));
    let mut switch_gate = fixture.arm_gate(GateTarget::Switch, false);
    let owner_service = service.clone();
    let owner = tokio::spawn(async move {
        owner_service
            .execute_switch(owner_permit, CancellationToken::new())
            .await
    });
    switch_gate.wait_entered().await;

    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("owner-exclusive refresh")
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    assert_eq!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .generation,
        snapshot.generation
    );

    let rejected_permit = service
        .prepare_switch(target, CancellationToken::new())
        .await
        .expect("concurrent permit");
    let rejected = service
        .execute_switch(rejected_permit, CancellationToken::new())
        .await;
    assert_eq!(
        rejected.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
    );
    assert!(rejected.snapshot.is_none());
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_some()
    );

    let third_permit = service
        .prepare_switch(target, CancellationToken::new())
        .await
        .expect("third permit");
    let third = service
        .execute_switch(third_permit, CancellationToken::new())
        .await;
    assert!(third.snapshot.is_none());
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_some()
    );

    switch_gate.release();
    let completion = owner.await.expect("owner join");
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    assert!(
        completion
            .snapshot
            .expect("owner snapshot")
            .branches
            .iter()
            .any(|branch| branch.label == "topic" && branch.current)
    );
    assert_eq!(fixture.switch_attempts().len(), 1);
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_none()
    );
    fixture.assert_clean();
}

#[tokio::test]
async fn refresh_registered_before_owner_cannot_commit_after_lease_acquisition() {
    let fixture = BranchFixture::new("main-clean");
    let service = Arc::new(fixture.service());
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("initial refresh");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit");

    let mut read_gate = fixture.arm_gate(GateTarget::TopLevel, false);
    fixture.expect_switch(Some("topic-current"));
    let mut switch_gate = fixture.arm_gate(GateTarget::Switch, false);

    let refresh_service = service.clone();
    let refresh =
        tokio::spawn(async move { refresh_service.refresh(CancellationToken::new()).await });
    read_gate.wait_entered().await;

    let owner_service = service.clone();
    let owner = tokio::spawn(async move {
        owner_service
            .execute_switch(permit, CancellationToken::new())
            .await
    });
    switch_gate.wait_entered().await;
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_some()
    );

    read_gate.release();
    assert_eq!(
        refresh
            .await
            .expect("refresh join")
            .expect_err("late refresh stale")
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    {
        let state = service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(state.generation, snapshot.generation);
        assert!(state.snapshot.as_ref().is_some_and(|current| {
            current.generation == snapshot.generation
                && current
                    .branches
                    .iter()
                    .any(|branch| branch.label == "main" && branch.current)
        }));
        assert!(state.active_mutation.is_some());
    }

    switch_gate.release();
    let completion = owner.await.expect("owner join");
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    assert!(
        completion
            .snapshot
            .expect("authoritative snapshot")
            .branches
            .iter()
            .any(|branch| branch.label == "topic" && branch.current)
    );
    assert_eq!(fixture.switch_attempts().len(), 1);
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_none()
    );
    fixture.assert_clean();
}

#[test]
fn rejected_concurrent_call_cannot_release_another_mutation_lease() {
    let mut state = BranchState::default();
    state.issued_permits.extend([1, 2]);
    assert!(acquire_mutation(&mut state, 1));
    assert!(!acquire_mutation(&mut state, 2));
    release_mutation(&mut state, 2);
    assert_eq!(state.active_mutation, Some(1));

    state.issued_permits.insert(3);
    assert!(!acquire_mutation(&mut state, 3));
    assert_eq!(state.active_mutation, Some(1));
    release_mutation(&mut state, 1);
    state.issued_permits.insert(4);
    assert!(acquire_mutation(&mut state, 4));
}
