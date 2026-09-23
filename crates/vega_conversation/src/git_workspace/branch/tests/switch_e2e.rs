use super::branch_stub::BranchFixture;
use super::*;

#[tokio::test]
async fn safe_temp_repo_switch_is_exact_and_authoritatively_refreshed() {
    let repo = Repo::new();
    git(repo.path(), &["switch", "-q", "-c", "topic"]);
    fs::write(repo.path().join("topic.txt"), "topic\n").expect("write topic");
    git(repo.path(), &["add", "topic.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "topic"]);
    git(repo.path(), &["switch", "-q", "main"]);
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
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
async fn ignored_collision_is_not_overwritten_and_failure_refresh_is_authoritative() {
    let repo = Repo::new();
    fs::write(repo.path().join(".gitignore"), "ignored.txt\n").expect("gitignore");
    git(repo.path(), &["add", ".gitignore"]);
    git(repo.path(), &["commit", "-q", "-m", "ignore"]);
    git(repo.path(), &["switch", "-q", "-c", "tracked-ignored"]);
    fs::write(repo.path().join("ignored.txt"), "target tracked\n").expect("target file");
    git(repo.path(), &["add", "-f", "ignored.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "tracked ignored"]);
    git(repo.path(), &["switch", "-q", "main"]);
    fs::write(repo.path().join("ignored.txt"), "local ignored\n").expect("local ignored");

    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("ignored remains clean");
    let permit = service
        .prepare_switch(
            branch_id(&snapshot, "tracked-ignored"),
            CancellationToken::new(),
        )
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
        fs::read(repo.path().join("ignored.txt")).expect("preserved ignored"),
        b"local ignored\n"
    );
}

#[tokio::test]
async fn target_gitattributes_and_explicit_filter_are_rejected() {
    let repo = Repo::new();
    git(repo.path(), &["switch", "-q", "-c", "unsafe-attrs"]);
    fs::write(repo.path().join(".gitattributes"), "*.txt text\n").expect("attrs");
    git(repo.path(), &["add", ".gitattributes"]);
    git(repo.path(), &["commit", "-q", "-m", "attrs"]);
    git(repo.path(), &["switch", "-q", "main"]);
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "unsafe-attrs")
        .expect("branch");
    assert_eq!(
        service
            .prepare_switch(target.id, CancellationToken::new())
            .await
            .expect_err("reject attrs")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );

    let filter_repo = Repo::new();
    fs::write(
        filter_repo.path().join(".gitattributes"),
        "*.txt filter=demo\n",
    )
    .expect("attrs");
    fs::write(filter_repo.path().join("file.txt"), "base\n").expect("base file");
    git(filter_repo.path(), &["add", "."]);
    git(filter_repo.path(), &["commit", "-q", "-m", "shared attrs"]);
    git(filter_repo.path(), &["switch", "-q", "-c", "unsafe-filter"]);
    fs::write(filter_repo.path().join("file.txt"), "filtered\n").expect("file");
    git(filter_repo.path(), &["add", "file.txt"]);
    git(filter_repo.path(), &["commit", "-q", "-m", "filter"]);
    git(filter_repo.path(), &["switch", "-q", "main"]);
    let recorder_dir = tempfile::Builder::new()
        .prefix("vega-filter-recorder-")
        .tempdir()
        .expect("recorder tempdir");
    let sentinel = recorder_dir.path().join("filter-side-effect");
    let recorder = recorder_dir.path().join("filter-recorder.sh");
    fs::write(
        &recorder,
        format!(
            "#!/bin/sh\nprintf side-effect >> '{}'\n/bin/cat\n",
            sentinel.display()
        ),
    )
    .expect("recorder");
    let mut permissions = fs::metadata(&recorder).expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&recorder, permissions).expect("chmod");
    let recorder = recorder.to_str().expect("fixture utf8 path");
    git(
        filter_repo.path(),
        &["config", "filter.demo.clean", recorder],
    );
    git(
        filter_repo.path(),
        &["config", "filter.demo.smudge", recorder],
    );
    git(
        filter_repo.path(),
        &["config", "filter.demo.process", recorder],
    );
    let filter_service = BranchWorkspaceService::new(filter_repo.path()).expect("service");
    assert_eq!(
        filter_service
            .refresh(CancellationToken::new())
            .await
            .expect_err("reject filter")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );
    assert!(
        !sentinel.exists(),
        "filter driver executed during preflight"
    );
    assert_eq!(
        git_output(filter_repo.path(), &["branch", "--show-current"]),
        b"main\n"
    );
}

#[tokio::test]
async fn deleted_and_renamed_away_gitattributes_are_rejected() {
    let repo = Repo::new();
    git(repo.path(), &["branch", "without-attrs"]);
    fs::write(repo.path().join(".gitattributes"), "*.txt text\n").expect("attrs");
    git(repo.path(), &["add", ".gitattributes"]);
    git(repo.path(), &["commit", "-q", "-m", "attrs on main"]);
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(
        service
            .prepare_switch(
                branch_id(&snapshot, "without-attrs"),
                CancellationToken::new()
            )
            .await
            .expect_err("deleted attrs")
            .code(),
        GitWorkspaceErrorCode::BranchUnsafeFilter
    );

    git(repo.path(), &["switch", "-q", "-c", "rename-away"]);
    git(repo.path(), &["mv", ".gitattributes", "attributes.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "rename attrs"]);
    git(repo.path(), &["switch", "-q", "main"]);
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    assert_eq!(
        service
            .prepare_switch(
                branch_id(&snapshot, "rename-away"),
                CancellationToken::new()
            )
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
