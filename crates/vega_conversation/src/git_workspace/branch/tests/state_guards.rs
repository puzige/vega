use super::*;

#[tokio::test]
async fn dirty_detached_and_operation_state_fail_closed() {
    let repo = Repo::new();
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    fs::write(repo.path().join("README.md"), "dirty\n").expect("dirty");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    git(repo.path(), &["restore", "README.md"]);
    git(repo.path(), &["checkout", "--detach", "-q"]);
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("detached")
            .code(),
        GitWorkspaceErrorCode::BranchDetached
    );
    git(repo.path(), &["switch", "main", "-q"]);
    fs::write(repo.path().join(".git/MERGE_HEAD"), "fixture").expect("marker");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("operation")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
}

#[tokio::test]
async fn staged_and_untracked_states_are_dirty_and_every_marker_is_rejected() {
    let repo = Repo::new();
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
    git(repo.path(), &["add", "staged.txt"]);
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("staged dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    git(repo.path(), &["reset", "-q", "--", "staged.txt"]);
    fs::remove_file(repo.path().join("staged.txt")).expect("remove staged");
    fs::write(repo.path().join("untracked.txt"), "untracked\n").expect("untracked");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("untracked dirty")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    fs::remove_file(repo.path().join("untracked.txt")).expect("remove untracked");
    for marker in OPERATION_MARKERS {
        let path = repo.path().join(".git").join(marker);
        if marker.contains('-') || *marker == "sequencer" {
            fs::create_dir(&path).expect("marker directory");
        } else {
            fs::write(&path, "marker\n").expect("marker file");
        }
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
}

#[tokio::test]
async fn unmerged_index_is_dirty_and_never_enumerated_as_switchable() {
    let repo = Repo::new();
    git(repo.path(), &["switch", "-q", "-c", "side"]);
    fs::write(repo.path().join("README.md"), "side\n").expect("side");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-q", "-m", "side"]);
    git(repo.path(), &["switch", "-q", "main"]);
    fs::write(repo.path().join("README.md"), "main changed\n").expect("main");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-q", "-m", "main"]);
    let mut merge = Command::new(GIT);
    merge.current_dir(repo.path()).args(["merge", "side"]);
    scrub_git_environment(&mut merge);
    merge
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    assert!(!merge.status().expect("conflicting merge").success());
    fs::remove_file(repo.path().join(".git/MERGE_HEAD")).expect("remove operation marker");
    let service = BranchWorkspaceService::new(repo.path()).expect("service");
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("unmerged")
            .code(),
        GitWorkspaceErrorCode::BranchDirty
    );
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
