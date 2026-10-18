use super::branch_stub::BranchFixture;
use super::*;

#[tokio::test]
async fn mocked_switch_is_exact_and_authoritatively_refreshed() {
    let fixture = BranchFixture::new("main-clean");
    fixture.expect_switch(Some("topic-current"));
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "topic")
        .expect("topic");
    let permit = service
        .prepare_switch(target.id, CancellationToken::new())
        .await
        .expect("preflight");
    let completion = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    assert!(
        completion
            .snapshot
            .expect("authoritative snapshot")
            .branches
            .iter()
            .any(|branch| branch.label == "topic" && branch.current)
    );
}

#[tokio::test]
async fn failed_switch_preserves_local_file_and_failure_refresh_is_authoritative() {
    let fixture = BranchFixture::new("main-clean");
    fixture.expect_switch_failure(GitWorkspaceErrorCode::GitFailed);
    fs::write(fixture.path().join("ignored.txt"), b"local ignored\n").unwrap();
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("ignored remains clean");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit");
    let completion = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(
        completion.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::GitFailed)
    );
    assert!(
        completion
            .snapshot
            .expect("failure refresh")
            .branches
            .iter()
            .any(|branch| branch.label == "main" && branch.current)
    );
    assert_eq!(
        fs::read(fixture.path().join("ignored.txt")).expect("preserved ignored"),
        b"local ignored\n"
    );
}

#[tokio::test]
async fn target_gitattributes_and_explicit_filter_are_rejected() {
    let fixture = BranchFixture::new("main-clean");
    fixture.set_raw("acmrt", "A\0.gitattributes\0");
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "topic")
        .expect("branch");
    assert_eq!(
        service
            .prepare_switch(target.id, CancellationToken::new())
            .await
            .expect_err("reject attrs")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );

    let filter_fixture = BranchFixture::new("main-clean");
    filter_fixture.set_raw("attrs", "README.md\0filter\0demo\0");
    let filter_service = filter_fixture.service();
    assert_eq!(
        filter_service
            .refresh(CancellationToken::new())
            .await
            .expect_err("reject filter")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );
    fixture.assert_clean();
    filter_fixture.assert_clean();
    assert!(fixture.switch_attempts().is_empty());
    assert!(filter_fixture.switch_attempts().is_empty());
}

#[tokio::test]
async fn deleted_and_renamed_away_gitattributes_are_rejected() {
    let fixture = BranchFixture::new("main-clean");
    fixture.set_raw("deletes", "D\0.gitattributes\0");
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(
        service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect_err("deleted attrs")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );

    fixture.set_raw("deletes", "");
    fixture.set_raw("acmrt", "R100\0.gitattributes\0attributes.txt\0");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(
        service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect_err("renamed attrs")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );
}

#[tokio::test]
async fn newer_permit_invalidates_older_and_target_move_fails_before_switch() {
    let fixture = BranchFixture::new("main-clean");
    let service = fixture.service();
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let id = branch_id(&snapshot, "topic");
    let older = service
        .prepare_switch(id, CancellationToken::new())
        .await
        .expect("older permit");
    let newer = service
        .prepare_switch(id, CancellationToken::new())
        .await
        .expect("newer permit");
    let rejected = service
        .execute_switch(older, CancellationToken::new())
        .await;
    assert_eq!(
        rejected.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
    );
    assert!(rejected.snapshot.is_none());
    fixture.expect_switch(Some("topic-current"));
    let switched = service
        .execute_switch(newer, CancellationToken::new())
        .await;
    assert_eq!(switched.outcome, BranchSwitchOutcome::Switched);
    assert_eq!(fixture.switch_attempts().len(), 1);

    fixture.set_case("main-clean");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit");
    // The target ref moves onto main's commit after the permit: the mutation
    // preflight authority differs, so the switch must not run.
    fixture.set_case("main-topic-forced");
    let raced = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(
        raced.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::ChangedDuringRead)
    );
    assert!(raced.snapshot.is_some());
    assert_eq!(fixture.switch_attempts().len(), 1);
    fixture.assert_clean();
}

#[tokio::test]
async fn dirty_and_operation_races_are_zero_switch_with_owner_cleanup() {
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
    // The worktree turns dirty after the permit; the pre-mutation capture must
    // fail closed and no switch may run.
    fixture.set_case("dirty-untracked");
    let dirty = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(
        dirty.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::BranchDirty)
    );
    assert!(dirty.snapshot.is_none());
    assert!(fixture.switch_attempts().is_empty());

    fixture.set_case("main-clean");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh after dirty");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit after dirty");
    // A real operation marker under the captured git dir must also fail closed.
    fixture.marker("MERGE_HEAD");
    let operation = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(
        operation.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::BranchOperationInProgress)
    );
    assert!(operation.snapshot.is_none());
    assert!(fixture.switch_attempts().is_empty());
    fixture.assert_clean();
}
