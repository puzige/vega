use super::branch_stub::BranchFixture;
use super::*;

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
async fn marker_symlink_and_external_metadata_are_checked_nofollow() {
    use std::os::unix::fs::symlink;

    let fixture = BranchFixture::new("main-clean");
    let outside = tempfile::NamedTempFile::new().expect("outside marker");
    let marker = fixture.path().join("metadata/MERGE_HEAD");
    symlink(outside.path(), &marker).expect("marker symlink");
    let service = fixture.service();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .expect_err("symlink marker")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
    fs::remove_file(&marker).expect("remove symlink");
    let linked_metadata = tempfile::tempdir().unwrap();
    fixture.set_raw("metadata", linked_metadata.path().to_str().unwrap());
    let linked_service = fixture.service();
    linked_service
        .refresh(CancellationToken::new())
        .await
        .expect("external metadata refresh");
    fs::write(linked_metadata.path().join("MERGE_HEAD"), "linked marker\n").unwrap();
    assert_eq!(
        linked_service
            .refresh(CancellationToken::new())
            .await
            .expect_err("linked operation")
            .code(),
        GitWorkspaceErrorCode::BranchOperationInProgress
    );
}
