use super::*;

#[test]
fn git_workspace_read_timeout_is_typed_and_bounded() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    let pid_file = repo.path().join("timeout-descendant.pid");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script));
    let started = Instant::now();
    let timeout_error = match runner.run(
        "rev-parse",
        &[OsString::from("--show-toplevel")],
        1,
        &CancellationToken::new(),
    ) {
        Ok(_) => panic!("read timeout was not enforced"),
        Err(error) => error,
    };
    assert_eq!(timeout_error.code(), GitWorkspaceErrorCode::TimedOut);
    assert!(started.elapsed() >= READ_TIMEOUT);
    assert!(started.elapsed() < READ_TIMEOUT + Duration::from_secs(3));
    let pid = fs::read_to_string(pid_file).unwrap();
    assert!(
        !Command::new(KILL)
            .args(["-0", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "timeout descendant survived cleanup"
    );
}

#[tokio::test]
async fn git_workspace_latest_refresh_wins_without_stale_overwrite() {
    let repo = Repo::new();
    repo.write("latest.txt", b"latest\n");
    let script = repo.path().join("fixture-git");
    let gate = tempdir().unwrap();
    let lock = gate.path().join("first.lock");
    let ready = gate.path().join("first.ready");
    let release = gate.path().join("first.release");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nif mkdir '{}' 2>/dev/null; then : > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; fi\nexec /usr/bin/git \"$@\"\n",
                lock.display(),
                ready.display(),
                release.display()
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
    let first = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ready.exists(), "first refresh did not enter fixture delay");
    let latest = service.refresh(CancellationToken::new()).await.unwrap();
    fs::write(&release, b"release\n").unwrap();
    assert_eq!(
        first.await.unwrap().unwrap_err().code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    let file = latest
        .files
        .iter()
        .find(|file| file.label == "latest.txt")
        .unwrap();
    assert_eq!(
        service
            .diff(file.id, CancellationToken::new())
            .await
            .unwrap()
            .file_id(),
        file.id
    );
}

#[tokio::test]
async fn git_workspace_owner_finalize_fences_pre_registered_poll_completion() {
    let repo = Repo::new();
    repo.write("tracked.txt", b"base\n");
    repo.commit_all();
    let fixture = tempdir().unwrap();
    let script = fixture.path().join("fixture-git");
    let arm = fixture.path().join("arm");
    let lock = fixture.path().join("poll.lock");
    let ready = fixture.path().join("poll.ready");
    let release = fixture.path().join("poll.release");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nis_status=0\nfor arg in \"$@\"; do [ \"$arg\" = status ] && is_status=1 || true; done\nif [ \"$is_status\" = 1 ] && [ -e '{}' ] && mkdir '{}' 2>/dev/null; then : > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; fi\nexec /usr/bin/git \"$@\"\n",
                arm.display(),
                lock.display(),
                ready.display(),
                release.display(),
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
    let a = service.refresh(CancellationToken::new()).await.unwrap();
    let owner = service.begin_owned_refresh(a.generation).unwrap();
    repo.write("tracked.txt", b"terminal B\n");
    fs::write(&arm, b"arm").unwrap();
    let poll = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ready.exists(), "ordinary poll did not enter barrier");
    let b = service
        .refresh_owned_after_mutation(owner, CancellationToken::new())
        .await
        .unwrap();
    assert!(b.generation > a.generation);
    fs::write(&release, b"release").unwrap();
    assert_eq!(
        poll.await.unwrap().unwrap_err().code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    assert_eq!(service.state.lock().unwrap().generation, b.generation);
}

#[tokio::test]
async fn git_workspace_obsolete_failure_does_not_invalidate_newer_snapshot() {
    let repo = Repo::new();
    repo.write("newer.txt", b"newer\n");
    let script = repo.path().join("fixture-git");
    let gate = tempdir().unwrap();
    let lock = gate.path().join("first.lock");
    let ready = gate.path().join("first.ready");
    let release = gate.path().join("first.release");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nif mkdir '{}' 2>/dev/null; then : > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; exit 91; fi\nexec /usr/bin/git \"$@\"\n",
                lock.display(),
                ready.display(),
                release.display()
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
    let obsolete = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        ready.exists(),
        "obsolete refresh did not enter fixture delay"
    );
    let latest = service.refresh(CancellationToken::new()).await.unwrap();
    fs::write(&release, b"release\n").unwrap();
    assert_eq!(
        obsolete.await.unwrap().unwrap_err().code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    let file = latest
        .files
        .iter()
        .find(|file| file.label == "newer.txt")
        .unwrap();
    assert_eq!(
        service
            .diff(file.id, CancellationToken::new())
            .await
            .unwrap()
            .file_id(),
        file.id
    );
}

#[tokio::test]
async fn git_workspace_ctime_detects_equal_size_edit_with_restored_mtime() {
    let repo = Repo::new();
    repo.write("tracked.txt", b"base\n");
    repo.commit_all();
    repo.write("tracked.txt", b"left\n");
    let reference = repo.path().join("mtime-reference");
    let tracked = repo.path().join("tracked.txt");
    assert!(
        Command::new("/bin/cp")
            .args([OsStr::new("-p"), tracked.as_os_str(), reference.as_os_str()])
            .status()
            .unwrap()
            .success()
    );
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    let file = snapshot
        .files
        .iter()
        .find(|file| file.label == "tracked.txt")
        .unwrap();
    let before = file_identity(&fs::metadata(&tracked).unwrap());
    repo.write("tracked.txt", b"rght\n");
    assert!(
        Command::new("/usr/bin/touch")
            .args([OsStr::new("-r"), reference.as_os_str(), tracked.as_os_str()])
            .status()
            .unwrap()
            .success()
    );
    let after = file_identity(&fs::metadata(&tracked).unwrap());
    assert_eq!(before.size, after.size);
    assert_eq!(
        (before.mtime, before.mtime_ns),
        (after.mtime, after.mtime_ns)
    );
    assert_ne!(
        (before.ctime, before.ctime_ns),
        (after.ctime, after.ctime_ns)
    );
    assert_eq!(
        service
            .diff(file.id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::ChangedDuringRead
    );
}

#[test]
fn git_workspace_metadata_remaining_cap_is_inclusive_and_plus_one_fails() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    let write_fixture = |size: usize| {
        fs::write(
            &script,
            format!("#!/bin/sh\npython3 -c 'import sys; sys.stdout.write(\"x\" * {size})'\n"),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
    };
    write_fixture(1024);
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script.clone()));
    assert_eq!(
        verify_filter_bytes_with_retained(
            &runner,
            &[],
            &[],
            SNAPSHOT_LIMIT - 1024,
            &CancellationToken::new(),
        )
        .unwrap_err()
        .code(),
        GitWorkspaceErrorCode::MalformedOutput
    );
    write_fixture(1025);
    assert_eq!(
        verify_filter_bytes_with_retained(
            &runner,
            &[],
            &[],
            SNAPSHOT_LIMIT - 1024,
            &CancellationToken::new(),
        )
        .unwrap_err()
        .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[tokio::test]
async fn git_workspace_cancel_is_typed_and_reaps_fixture_group() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    let pid_file = repo.path().join("descendant.pid");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let service = service.clone();
        let cancel = cancel.clone();
        async move { service.refresh(cancel).await }
    });
    for _ in 0..500 {
        if pid_file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(pid_file.exists(), "fixture descendant was not started");
    cancel.cancel();
    assert_eq!(
        task.await.unwrap().unwrap_err().code(),
        GitWorkspaceErrorCode::Cancelled
    );
    let pid = fs::read_to_string(pid_file).unwrap();
    let mut gone = false;
    for _ in 0..50 {
        let status = Command::new(KILL)
            .args(["-0", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if !status.success() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gone, "descendant process survived cancellation");
}

#[tokio::test]
async fn git_workspace_early_parent_exit_with_inherited_pipes_fails_and_reaps_group() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    let pid_file = repo.path().join("early-descendant.pid");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script).unwrap();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::ProcessControlFailed
    );
    let pid = fs::read_to_string(pid_file).unwrap();
    assert!(
        !Command::new(KILL)
            .args(["-0", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "inherited-pipe descendant survived cleanup"
    );
}
