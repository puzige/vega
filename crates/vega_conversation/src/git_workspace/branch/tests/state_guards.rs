use super::branch_stub::BranchFixture;
use super::*;

// The dirty/detached/operation and marker policies run the real
// `BranchWorkspaceService` over the finite in-process command boundary; the raw
// status bytes are captured real Git output. The operation markers themselves
// stay real filesystem facts under the stub's plain `metadata` directory. The
// symlink and linked-worktree `--git-path` nofollow contract is retained as a
// real Git/filesystem test below.

#[tokio::test]
async fn dirty_detached_and_operation_state_fail_closed() {
    let fixture = BranchFixture::new("dirty-tracked");
    let service = fixture.service();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    fixture.set_case("detached");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("detached")
            .code(),
        GitWorkspaceErrorCode::BranchDetached
    );
    fixture.set_case("main-clean");
    let marker = fixture.marker("MERGE_HEAD");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("operation")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
    fs::remove_file(&marker).expect("remove marker");
    fixture.assert_clean();
}

#[tokio::test]
async fn staged_and_untracked_states_are_dirty_and_every_marker_is_rejected() {
    let fixture = BranchFixture::new("dirty-staged");
    let service = fixture.service();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("staged dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    fixture.set_case("dirty-untracked");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("untracked dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    fixture.set_case("main-clean");
    for marker in OPERATION_MARKERS {
        let path = fixture.marker(marker);
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("operation")
                .code(),
            GitWorkspaceErrorCode::BranchOperationInProgress,
            "marker {marker}"
        );
        if path.is_dir() {
            fs::remove_dir(&path).expect("remove marker dir");
        } else {
            fs::remove_file(&path).expect("remove marker file");
        }
    }
    fixture.assert_clean();
}

#[tokio::test]
async fn unmerged_index_is_dirty_and_never_enumerated_as_switchable() {
    let fixture = BranchFixture::new("unmerged");
    let service = fixture.service();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("unmerged")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    fixture.assert_clean();
}

#[tokio::test]
async fn marker_symlink_and_linked_worktree_gitdir_are_rejected_nofollow() {
    use std::os::unix::fs::symlink;

    let repo = Repo::new();
    let outside = tempfile::NamedTempFile::new().expect("outside marker");
    symlink(outside.path(), repo.path().join(".git/MERGE_HEAD")).expect("marker symlink");
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("symlink marker")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
    fs::remove_file(repo.path().join(".git/MERGE_HEAD")).expect("remove symlink");

    let linked_parent = tempfile::Builder::new()
        .prefix("vega-linked-worktree-")
        .tempdir()
        .expect("linked parent");
    let linked = linked_parent.path().join("checkout");
    let linked_text = linked.to_str().expect("fixture utf8 path");
    git(
        repo.path(),
        &["worktree", "add", "-q", "-b", "linked", linked_text],
    );
    let linked_service = BranchWorkspaceService::new(&linked).expect("linked service");
    linked_service
        .refresh(CancellationToken::new())
        .await
        .expect("linked refresh");
    let marker_output = git_output(&linked, &["rev-parse", "--git-path", "MERGE_HEAD"]);
    let marker = PathBuf::from(OsString::from_vec(
        exact_single_line(&marker_output)
            .expect("marker line")
            .to_vec(),
    ));
    let marker = if marker.is_absolute() {
        marker
    } else {
        linked.join(marker)
    };
    fs::write(marker, "linked marker\n").expect("linked marker");
    assert_eq!(
        linked_service
            .refresh(CancellationToken::new())
            .await
            .expect_err("linked operation")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
}
