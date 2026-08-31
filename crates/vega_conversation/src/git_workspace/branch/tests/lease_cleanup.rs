use super::*;

#[tokio::test]
async fn rejected_execute_cannot_compete_with_owner_cleanup_refresh() {
    let repo = Repo::new();
    git(repo.path(), &["switch", "-q", "-c", "topic"]);
    fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
    git(repo.path(), &["add", "topic.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "topic"]);
    git(repo.path(), &["switch", "-q", "main"]);

    let controls = tempfile::Builder::new()
        .prefix("vega-branch-barrier-")
        .tempdir()
        .expect("controls");
    let started = controls.path().join("started");
    let release = controls.path().join("release");
    let attempts = controls.path().join("attempts");
    let script = controls.path().join("blocking-switch.sh");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf started > '{}'\nwhile [ ! -f '{}' ]; do /bin/sleep 0.01; done\nprintf 'attempt\\n' >> '{}'\nexec /usr/bin/git \"$@\"\n",
                started.display(),
                release.display(),
                attempts.display()
            ),
        )
        .expect("blocking script");
    let mut permissions = fs::metadata(&script).expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).expect("chmod");

    let service = Arc::new(
        BranchWorkspaceService::new_with_mutation_for_test(repo.path(), script).expect("service"),
    );
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let target = branch_id(&snapshot, "topic");
    let owner_permit = service
        .prepare_switch(target, CancellationToken::new())
        .await
        .expect("owner permit");
    let owner_service = service.clone();
    let owner = tokio::spawn(async move {
        owner_service
            .execute_switch(owner_permit, CancellationToken::new())
            .await
    });
    for _ in 0..500 {
        if started.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(started.exists());
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

    fs::write(&release, "release\n").expect("release");
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
    assert_eq!(
        fs::read_to_string(&attempts).expect("attempts"),
        "attempt\n"
    );
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_none()
    );
}

#[tokio::test]
async fn refresh_registered_before_owner_cannot_commit_after_lease_acquisition() {
    let repo = Repo::new();
    git(repo.path(), &["switch", "-q", "-c", "topic"]);
    fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
    git(repo.path(), &["add", "topic.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "topic"]);
    git(repo.path(), &["switch", "-q", "main"]);

    let controls = tempfile::Builder::new()
        .prefix("vega-refresh-owner-race-")
        .tempdir()
        .expect("controls");
    let read_arm = controls.path().join("read-arm");
    let read_claim = controls.path().join("read-claim");
    let read_entered = controls.path().join("read-entered");
    let read_release = controls.path().join("read-release");
    let mutation_entered = controls.path().join("mutation-entered");
    let mutation_release = controls.path().join("mutation-release");
    let attempts = controls.path().join("attempts");
    let read_wrapper = controls.path().join("read-wrapper.sh");
    fs::write(
            &read_wrapper,
            format!(
                "#!/bin/sh\nif [ -f '{}' ] && /bin/mkdir '{}' 2>/dev/null; then\n  printf entered > '{}'\n  while [ ! -f '{}' ]; do /bin/sleep 0.01; done\nfi\nexec /usr/bin/git \"$@\"\n",
                read_arm.display(),
                read_claim.display(),
                read_entered.display(),
                read_release.display()
            ),
        )
        .expect("read wrapper");
    let mutation_wrapper = controls.path().join("mutation-wrapper.sh");
    fs::write(
            &mutation_wrapper,
            format!(
                "#!/bin/sh\nprintf entered > '{}'\nwhile [ ! -f '{}' ]; do /bin/sleep 0.01; done\nprintf 'attempt\\n' >> '{}'\nexec /usr/bin/git \"$@\"\n",
                mutation_entered.display(),
                mutation_release.display(),
                attempts.display()
            ),
        )
        .expect("mutation wrapper");
    for script in [&read_wrapper, &mutation_wrapper] {
        let mut permissions = fs::metadata(script).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(script, permissions).expect("chmod");
    }

    let service = Arc::new(
        BranchWorkspaceService::new_with_executables_for_test(
            repo.path(),
            read_wrapper,
            mutation_wrapper,
        )
        .expect("service"),
    );
    let snapshot = service
        .refresh(CancellationToken::new())
        .await
        .expect("initial refresh");
    let permit = service
        .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
        .await
        .expect("permit");

    fs::write(&read_arm, "arm\n").expect("arm read");
    let refresh_service = service.clone();
    let refresh =
        tokio::spawn(async move { refresh_service.refresh(CancellationToken::new()).await });
    for _ in 0..500 {
        if read_entered.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(
        read_entered.exists(),
        "refresh did not enter capture barrier"
    );

    let owner_service = service.clone();
    let owner = tokio::spawn(async move {
        owner_service
            .execute_switch(permit, CancellationToken::new())
            .await
    });
    for _ in 0..500 {
        if mutation_entered.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(
        mutation_entered.exists(),
        "owner did not enter mutation barrier"
    );
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_some()
    );

    fs::write(&read_release, "release\n").expect("release read");
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

    fs::write(&mutation_release, "release\n").expect("release mutation");
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
    assert_eq!(
        fs::read_to_string(&attempts).expect("attempts"),
        "attempt\n"
    );
    assert!(
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation
            .is_none()
    );
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
