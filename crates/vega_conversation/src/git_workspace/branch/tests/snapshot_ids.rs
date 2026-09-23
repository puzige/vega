use super::branch_stub::BranchFixture;
use super::*;

// These generation/permit policies run the real `BranchWorkspaceService` over
// the finite in-process command boundary. Capture, generation rotation, opaque
// id sealing and permit consumption are unchanged; only the external `git`
// process is replaced by captured bytes. Every original assertion is preserved.

#[tokio::test]
async fn unchanged_refresh_keeps_ids_and_branch_change_rotates() {
    let fixture = BranchFixture::new("main-clean");
    let service = fixture.service();
    let first = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let second = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(first, second);
    fixture.set_case("main-another");
    let third = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_ne!(third.generation, first.generation);
    assert_ne!(third.branches[0].id, first.branches[0].id);
    fixture.assert_clean();
}

#[tokio::test]
async fn opaque_ids_are_service_generation_slot_and_seal_bound() {
    let fixture = BranchFixture::new("main-clean");
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let topic = branch_id(&snapshot, "topic");
    let current = snapshot
        .branches
        .iter()
        .find(|branch| branch.current)
        .expect("current")
        .id;
    assert_eq!(
        service
            .prepare_switch(current, CancellationToken::new())
            .await
            .expect_err("already current")
            .code(),
        GitWorkspaceErrorCode::BranchAlreadyCurrent
    );
    for forged in [
        BranchId {
            generation: topic.generation,
            slot: u32::MAX,
            seal: topic.seal,
        },
        BranchId {
            generation: topic.generation,
            slot: topic.slot,
            seal: topic.seal ^ 1,
        },
    ] {
        assert!(
            service
                .prepare_switch(forged, CancellationToken::new())
                .await
                .is_err()
        );
    }

    let permit = service
        .prepare_switch(topic, CancellationToken::new())
        .await
        .expect("permit");
    let other = fixture.service();
    other
        .refresh(CancellationToken::new())
        .await
        .expect("other refresh");
    let rejected = other.execute_switch(permit, CancellationToken::new()).await;
    assert_eq!(
        rejected.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
    );
    assert!(rejected.snapshot.is_none());

    fixture.set_case("main-temporary");
    let changed = service
        .refresh(CancellationToken::new())
        .await
        .expect("changed");
    fixture.set_case("main-clean");
    let aba = service
        .refresh(CancellationToken::new())
        .await
        .expect("aba");
    assert_ne!(changed.generation, snapshot.generation);
    assert_ne!(aba.generation, snapshot.generation);
    assert!(
        service
            .prepare_switch(topic, CancellationToken::new())
            .await
            .is_err()
    );
    fixture.assert_clean();
}

#[tokio::test]
async fn stale_permit_after_generation_rotation_does_not_leak_mutation_lease() {
    let fixture = BranchFixture::new("main-clean");
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit");
    // The target ref moves onto main's commit; the next refresh rotates the
    // generation, invalidating the older permit before it can switch.
    fixture.set_case("main-topic-forced");
    let rotated = service
        .refresh(CancellationToken::new())
        .await
        .expect("rotated refresh");
    assert_ne!(rotated.generation, snapshot.generation);

    let stale = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(
        stale.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
    );
    assert!(stale.snapshot.is_none());
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_none()
    );

    let current = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh remains available");
    let fresh = service
        .prepare_switch(branch_id(&current, "topic"), CancellationToken::new())
        .await
        .expect("fresh permit");
    fixture.expect_switch(Some("topic-forced-current"));
    let completion = service
        .execute_switch(fresh, CancellationToken::new())
        .await;
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    fixture.assert_clean();
}

#[tokio::test]
async fn shared_oid_refs_are_distinct_and_current_is_selected_by_raw_ref() {
    let fixture = BranchFixture::new("main-aliases-only");
    let snapshot = fixture
        .service()
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(snapshot.branches.len(), 3);
    assert_eq!(
        snapshot
            .branches
            .iter()
            .filter(|branch| branch.current)
            .map(|branch| branch.label.as_str())
            .collect::<Vec<_>>(),
        vec!["main"]
    );
    fixture.assert_clean();
}
