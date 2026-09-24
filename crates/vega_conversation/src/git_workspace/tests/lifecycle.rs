use super::lifecycle_stub::{GateTarget, LifecycleFixture};
use super::*;

#[tokio::test]
async fn git_workspace_latest_refresh_wins_without_stale_overwrite() {
    // The real `GitWorkspaceService::refresh` still runs its whole read
    // protocol; only the external Git process is replaced by the exact captured
    // bytes for an unborn branch with one untracked file. The first refresh is
    // held at its own declared read while the latest refresh completes, so the
    // completion order is explicit and no shell `mkdir`/`sleep` gate or
    // repository is involved.
    let fixture = LifecycleFixture::new("latest-untracked");
    let service = Arc::new(fixture.service());
    let mut gate = fixture.arm_gate(GateTarget::TopLevel, false);
    let first = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    gate.wait_entered().await;
    let latest = service.refresh(CancellationToken::new()).await.unwrap();
    gate.release();
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
    fixture.assert_clean();
}

#[tokio::test]
async fn git_workspace_owner_finalize_fences_pre_registered_poll_completion() {
    // The owner handoff is real service policy. Only the external reads are
    // served from two captured states: A is the clean tracked baseline and the
    // terminal state is the same path modified. The ordinary poll is held at
    // its status read until the owner refresh publishes the terminal
    // generation, so a poll registered before the owner cannot win.
    let fixture = LifecycleFixture::new("owner-base");
    let service = Arc::new(fixture.service());
    let a = service.refresh(CancellationToken::new()).await.unwrap();
    let owner = service.begin_owned_refresh(a.generation).unwrap();
    fixture.set_case("owner-terminal");
    let mut gate = fixture.arm_gate(GateTarget::Status, false);
    let poll = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    gate.wait_entered().await;
    let b = service
        .refresh_owned_after_mutation(owner, CancellationToken::new())
        .await
        .unwrap();
    assert!(b.generation > a.generation);
    gate.release();
    assert_eq!(
        poll.await.unwrap().unwrap_err().code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    assert_eq!(service.state.lock().unwrap().generation, b.generation);
    fixture.assert_clean();
}

#[tokio::test]
async fn git_workspace_obsolete_failure_does_not_invalidate_newer_snapshot() {
    // An obsolete refresh whose modelled external read fails must not disturb
    // the newer successful snapshot. The failure is reported only after the
    // latest-request fence, so the obsolete caller observes StaleGeneration and
    // the newer snapshot and its identifiers stay authoritative.
    let fixture = LifecycleFixture::new("newer-untracked");
    let service = Arc::new(fixture.service());
    let mut gate = fixture.arm_gate(GateTarget::TopLevel, true);
    let obsolete = tokio::spawn({
        let service = service.clone();
        async move { service.refresh(CancellationToken::new()).await }
    });
    gate.wait_entered().await;
    let latest = service.refresh(CancellationToken::new()).await.unwrap();
    gate.release();
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
    fixture.assert_clean();
}

#[tokio::test]
async fn git_workspace_ctime_detects_equal_size_edit_with_restored_mtime() {
    let fixture = LifecycleFixture::new("owner-terminal");
    let tracked = fixture.path().join("tracked.txt");
    fs::write(&tracked, b"left\n").unwrap();
    let modified = fs::metadata(&tracked).unwrap().modified().unwrap();
    let service = fixture.service();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    let file = snapshot
        .files
        .iter()
        .find(|file| file.label == "tracked.txt")
        .unwrap();
    let before = file_identity(&fs::metadata(&tracked).unwrap());
    fs::write(&tracked, b"rght\n").unwrap();
    File::options()
        .write(true)
        .open(&tracked)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
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
    struct BytesBackend(usize);
    impl GitCommandBackend for BytesBackend {
        fn execute(
            &self,
            _: &Command,
            _: Option<&[u8]>,
            limit: usize,
        ) -> Result<Output, GitWorkspaceError> {
            if self.0 > limit {
                return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
            }
            Ok(Output {
                stdout: vec![b'x'; self.0],
                overflow: false,
            })
        }
    }
    let root = tempdir().unwrap();
    let service = GitWorkspaceService::new(root.path()).unwrap();
    let runner = Runner::new(
        service.root.clone(),
        service.identity,
        RunnerExecutable::InProcess(Arc::new(BytesBackend(1024))),
    );
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
    let runner = Runner::new(
        service.root.clone(),
        service.identity,
        RunnerExecutable::InProcess(Arc::new(BytesBackend(1025))),
    );
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
