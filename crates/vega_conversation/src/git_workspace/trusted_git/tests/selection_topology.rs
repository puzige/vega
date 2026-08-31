use super::*;

#[tokio::test]
async fn trusted_git_empty_selection_commits_existing_staged_delta() {
    let repo = Repo::new();
    fs::write(repo.path().join("staged.txt"), "staged\n").expect("write staged");
    run_git(repo.path(), &["add", "staged.txt"]);
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh staged");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    assert_eq!(checklist.staged.len(), 1);
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert!(prepared.error.is_none());
    let prepared = prepared.prepared.expect("prepared");
    let completion = trusted
        .commit(
            prepared.id,
            "test: staged only".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
}

#[tokio::test]
async fn e2e_owned_repo_checklist_prepare_mock_draft_commit() {
    let repo = Repo::new();
    let base = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
    let base = base.strip_suffix(b"\n").expect("base newline").to_vec();
    fs::write(repo.path().join("tracked.txt"), "changed\n").expect("modify");
    let (workspace, trusted) = repo.services().await;
    let snapshot = workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh modified");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.optional[0].file_id, snapshot.files[0].id);
    let prepared = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(prepared.error, None);
    let prepared = prepared.prepared.expect("prepared");
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("test: production headless e2e"),
        vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]));
    let draft = trusted
        .draft(
            prepared.id,
            "mock-e2e".into(),
            provider.clone(),
            CancellationToken::new(),
        )
        .await
        .expect("mock draft");
    assert_eq!(provider.requests().len(), 1);
    assert!(provider.requests()[0].tools.is_empty());
    assert_eq!(provider.requests()[0].max_tokens, Some(256));
    let completion = trusted
        .commit(
            prepared.id,
            draft.text().to_owned(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    assert_terminal_workspace(
        &trusted,
        completion.workspace.as_ref().expect("terminal workspace"),
    );
    assert!(run_git_output(repo.path(), &["status", "--porcelain=v2", "-z"]).is_empty());
    let parents = run_git_output(repo.path(), &["rev-list", "--parents", "-n", "1", "HEAD"]);
    let parents = parents
        .strip_suffix(b"\n")
        .expect("parent newline")
        .split(|byte| *byte == b' ')
        .collect::<Vec<_>>();
    assert_eq!(parents.len(), 2);
    assert_eq!(parents[1], base);
    let tree = run_git_output(repo.path(), &["ls-tree", "-rz", "--full-tree", "HEAD"]);
    assert!(tree.ends_with(b"\ttracked.txt\0"));
}

#[tokio::test]
async fn owner_refresh_prepare_first_capture_failure_retries_exact_owner() {
    let repo = Repo::new();
    fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
    let (_mutation_dir, mutation, mutation_argv, _input) = mutation_recorder();
    let (_read_dir, read, failed) = fail_first_status_after_trigger(&mutation_argv);
    let workspace =
        Arc::new(GitWorkspaceService::new_for_test(repo.path(), read).expect("fault workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("A refresh");
    let trusted =
        TrustedGitService::new_with_mutation_for_test(repo.path(), workspace.clone(), mutation)
            .expect("trusted");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert!(failed.exists(), "first owner status was faulted");
    assert!(completion.error.is_none());
    let terminal = completion.workspace.expect("authoritative B");
    assert!(terminal.generation > checklist.workspace_generation);
    assert!(completion.prepared.is_some());
}

#[tokio::test]
async fn owner_refresh_commit_first_capture_failure_recovers_new_head_once() {
    let repo = Repo::new();
    fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
    let (_mutation_dir, mutation, mutation_argv, _input) = mutation_recorder();
    let (_read_dir, read, failed) = fail_first_status_after_trigger(&mutation_argv);
    let workspace =
        Arc::new(GitWorkspaceService::new_for_test(repo.path(), read).expect("fault workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("A refresh");
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
        .expect("trusted");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let prepared = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await
        .prepared
        .expect("prepared");
    fs::remove_file(&mutation_argv).expect("re-arm commit trigger");
    let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
    let completion = trusted
        .commit(
            prepared.id,
            "test: owner retry".into(),
            CancellationToken::new(),
        )
        .await;
    assert!(failed.exists(), "first post-commit status was faulted");
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    assert!(completion.workspace.is_some());
    let after = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
    assert_ne!(before, after);
}

#[tokio::test]
async fn disconnected_recovery_consumes_zombie_owner_before_future_checklist() {
    let repo = Repo::new();
    let (workspace, trusted) = repo.services().await;
    let parent = workspace
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .generation;
    let _owner = workspace
        .begin_owned_refresh(parent)
        .expect("mutation owner");
    trusted
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .mutation_active = true;
    fs::write(repo.path().join("tracked.txt"), "terminal state\n").expect("mutate");
    let recovered = trusted
        .recover_disconnected_mutation()
        .await
        .expect("authoritative recovery");
    assert!(recovered.generation > parent);
    assert!(workspace.active_owned_refresh().is_none());
    assert!(
        !trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .mutation_active
    );
    trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("fresh checklist after recovery");
}

#[tokio::test]
async fn trusted_git_selected_am_component_preserves_forced_add_topology() {
    let repo = Repo::new();
    fs::write(repo.path().join("added.txt"), "first\n").expect("new file");
    run_git(repo.path(), &["add", "added.txt"]);
    fs::write(repo.path().join("added.txt"), "second\n").expect("unstaged edit");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh AM");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("AM checklist");
    assert_eq!(checklist.staged.len(), 1);
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.staged[0].file_id, checklist.optional[0].file_id);
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, None);
    assert!(completion.prepared.is_some());
    let status = run_git_output(repo.path(), &["status", "--porcelain"]);
    assert_eq!(status, b"A  added.txt\n");
}

#[tokio::test]
async fn untracked_entry_is_optional_only_and_prepares_as_added() {
    let repo = Repo::new();
    fs::write(repo.path().join("untracked.txt"), "new\n").expect("new file");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh untracked");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("untracked checklist");
    assert!(checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Added);
    assert!(!checklist.optional[0].forced);
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, None);
    assert!(completion.prepared.is_some());
    assert_eq!(
        run_git_output(repo.path(), &["status", "--porcelain"]),
        b"A  untracked.txt\n"
    );
}

#[tokio::test]
async fn selected_delete_and_untracked_destination_may_canonicalize_to_staged_rename() {
    let repo = Repo::new();
    fs::rename(
        repo.path().join("tracked.txt"),
        repo.path().join("renamed.txt"),
    )
    .expect("rename fixture");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh delete and untracked");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("rename checklist");
    assert!(checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 2);
    assert!(
        checklist
            .optional
            .iter()
            .any(|row| row.kind == CommitSelectionKind::Deleted)
    );
    assert!(
        checklist
            .optional
            .iter()
            .any(|row| row.kind == CommitSelectionKind::Added)
    );
    let selected = checklist.optional.iter().map(|row| row.file_id).collect();
    let completion = trusted
        .prepare(checklist.id, selected, CancellationToken::new())
        .await;
    assert_eq!(completion.error, None);
    assert_eq!(
        completion
            .prepared
            .as_ref()
            .map(|prepared| prepared.staged_file_count),
        Some(1)
    );
    assert_eq!(
        run_git_output(repo.path(), &["status", "--porcelain"]),
        b"R  tracked.txt -> renamed.txt\n"
    );
}

#[test]
fn delete_untracked_joint_rename_rejects_any_extra_touching_b_record() {
    let oid = |byte: u8| vec![byte; 40];
    let source_record = StatusRecord {
        shape: StatusShape::Ordinary,
        x: b'.',
        y: b'D',
        sub: b"N...".to_vec(),
        head_mode: b"100644".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"000000".to_vec(),
        head_oid: oid(b'1'),
        index_oid: oid(b'1'),
        path: b"source.txt".to_vec(),
        previous: None,
    };
    let destination_record = StatusRecord {
        shape: StatusShape::Untracked,
        x: b'?',
        y: b'?',
        sub: b"N...".to_vec(),
        head_mode: b"000000".to_vec(),
        index_mode: b"000000".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: oid(b'0'),
        index_oid: oid(b'0'),
        path: b"destination.txt".to_vec(),
        previous: None,
    };
    let row = |slot: u32,
               record: StatusRecord,
               kind: CommitSelectionKind,
               mode: Option<Vec<u8>>| ChecklistRow {
        public: CommitSelection {
            file_id: WorkspaceFileId {
                generation: 1,
                slot,
                seal: u64::from(slot),
            },
            label: String::new(),
            previous_label: None,
            kind,
            forced: false,
        },
        closure: vec![record.path.clone()],
        record,
        optional_kind: kind,
        worktree_mode: mode,
    };
    let rows = [
        row(1, source_record.clone(), CommitSelectionKind::Deleted, None),
        row(
            2,
            destination_record,
            CommitSelectionKind::Added,
            Some(b"100644".to_vec()),
        ),
    ];
    let selected = vec![&rows[0], &rows[1]];
    let merged = StatusRecord {
        shape: StatusShape::Rename,
        x: b'R',
        y: b'.',
        sub: b"N...".to_vec(),
        head_mode: b"100644".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: oid(b'1'),
        index_oid: oid(b'1'),
        path: b"destination.txt".to_vec(),
        previous: Some(b"source.txt".to_vec()),
    };
    assert!(is_selected_delete_untracked_rename(
        &selected,
        std::slice::from_ref(&merged),
        b"source.txt",
        b"destination.txt"
    ));
    assert!(!is_selected_delete_untracked_rename(
        &selected,
        &[merged, source_record],
        b"source.txt",
        b"destination.txt"
    ));
}

#[tokio::test]
async fn trusted_git_selected_staged_rename_with_unstaged_edit_proves_structural_split() {
    let repo = Repo::new();
    run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
    fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh RM");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("RM checklist");
    assert_eq!(checklist.staged.len(), 1);
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.staged[0].kind, CommitSelectionKind::Renamed);
    assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Modified);
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, None);
    let status = run_git_output(repo.path(), &["status", "--porcelain"]);
    assert_eq!(status, b"A  renamed.txt\nD  tracked.txt\n");
}

#[tokio::test]
async fn staged_rename_destination_mode_flip_is_rejected_after_one_add() {
    let repo = Repo::new();
    run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
    fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("A refresh");
    let (_gate, mutation, ready, release) = blocking_before_mutation();
    let trusted = Arc::new(
        TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
            .expect("trusted"),
    );
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("rename checklist");
    let selected = checklist.optional[0].file_id;
    let worker = tokio::spawn({
        let trusted = trusted.clone();
        async move {
            trusted
                .prepare(checklist.id, vec![selected], CancellationToken::new())
                .await
        }
    });
    wait_for_path(&ready).await;
    let path = repo.path().join("renamed.txt");
    let mut mode = fs::metadata(&path).expect("rename metadata").permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&path, mode).expect("flip executable mode");
    fs::write(&release, b"release").expect("release add");
    let completion = worker.await.expect("prepare worker");
    assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    assert!(completion.prepared.is_none());
}

#[tokio::test]
async fn staged_rename_source_recreation_is_not_owned_by_destination_edit() {
    let repo = Repo::new();
    run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
    fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("A refresh");
    let (_gate, mutation, ready, release) = blocking_before_mutation();
    let trusted = Arc::new(
        TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
            .expect("trusted"),
    );
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("rename checklist");
    let selected = checklist.optional[0].file_id;
    let worker = tokio::spawn({
        let trusted = trusted.clone();
        async move {
            trusted
                .prepare(checklist.id, vec![selected], CancellationToken::new())
                .await
        }
    });
    wait_for_path(&ready).await;
    fs::write(repo.path().join("tracked.txt"), "outside S\n").expect("recreate source");
    fs::write(&release, b"release").expect("release add");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !worker.is_finished(),
        "unsafe source recreation must not publish a terminal owner snapshot"
    );
    fs::remove_file(repo.path().join("tracked.txt")).expect("restore safe source absence");
    let completion = worker.await.expect("prepare worker");
    assert!(completion.workspace.is_some());
}

#[tokio::test]
async fn staged_rename_destination_delete_claims_only_canonical_old_deletion() {
    let repo = Repo::new();
    run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
    fs::remove_file(repo.path().join("renamed.txt")).expect("delete rename destination");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("RD refresh");
    let (_recorder, mutation, argv, input) = mutation_recorder();
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
        .expect("trusted");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("RD checklist");
    let selected = checklist
        .optional
        .iter()
        .find(|row| row.kind == CommitSelectionKind::Deleted)
        .expect("RD optional delete")
        .file_id;
    let completion = trusted
        .prepare(checklist.id, vec![selected], CancellationToken::new())
        .await;
    assert_eq!(completion.error, None);
    let prepared = completion.prepared.expect("prepared RD");
    assert_eq!(fs::read(&input).expect("RD add stdin"), b"renamed.txt\0");
    assert_eq!(
        fs::read(&argv).expect("RD add argv"),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul",],
        )
    );
    {
        let state = trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let authority = &state.prepared.as_ref().expect("stored RD").authority;
        assert!(authority.records.iter().any(|record| {
            record.shape == StatusShape::Ordinary
                && record.path == b"tracked.txt"
                && record.previous.is_none()
                && record.x == b'D'
                && record.y == b'.'
        }));
        assert!(
            !authority
                .stages
                .iter()
                .any(|entry| { entry.path == b"tracked.txt" || entry.path == b"renamed.txt" })
        );
    }
    let committed = trusted
        .commit(
            prepared.id,
            "test: delete renamed file".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(committed.outcome, CommitOutcome::Committed);
    assert!(!repo.path().join("tracked.txt").exists());
    assert!(!repo.path().join("renamed.txt").exists());
}

#[tokio::test]
async fn trusted_git_selected_regular_to_symlink_binds_type_change() {
    let repo = Repo::new();
    fs::remove_file(repo.path().join("tracked.txt")).expect("remove regular");
    std::os::unix::fs::symlink("missing-target", repo.path().join("tracked.txt")).expect("symlink");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh type change");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("type checklist");
    assert_eq!(checklist.optional[0].kind, CommitSelectionKind::TypeChanged);
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, None);
    let index = run_git_output(repo.path(), &["ls-files", "--stage", "--", "tracked.txt"]);
    assert!(index.starts_with(b"120000 "));
}

#[tokio::test]
async fn trusted_git_selected_executable_add_binds_exact_worktree_mode() {
    let repo = Repo::new();
    let path = repo.path().join("run.sh");
    fs::write(&path, "#!/bin/sh\nexit 0\n").expect("script");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod");
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh executable");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("executable checklist");
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, None);
    let index = run_git_output(repo.path(), &["ls-files", "--stage", "--", "run.sh"]);
    assert!(index.starts_with(b"100755 "));
}
