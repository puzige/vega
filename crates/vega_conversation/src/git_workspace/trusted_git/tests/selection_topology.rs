use super::*;

#[tokio::test]
async fn trusted_git_empty_selection_commits_existing_staged_delta() {
    let fixture = command_stub::PolicyFixture::service("commit-staged");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "commit-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert!(completion.workspace.is_some());
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        )
    );
    assert_eq!(
        fixture.mutation_inputs(),
        vec![b"test: staged only".to_vec()]
    );
    fixture.assert_mutations_drained();
}

#[tokio::test]
async fn mocked_git_checklist_prepare_draft_commit() {
    let fixture = command_stub::PolicyFixture::policy("sha256-before");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "sha256-staged",
    );
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "sha256-after",
    );
    let base = fixture.raw("head");
    let (workspace, trusted) = fixture.services().await;
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
    assert!(completion.workspace.unwrap().files.is_empty());
    assert_eq!(fixture.raw("parents").trim(), base);
    assert!(fixture.raw("tree").ends_with("\ttracked.txt\0"));
    fixture.assert_mutations_drained();
}

#[tokio::test]
async fn owner_refresh_prepare_first_capture_failure_retries_exact_owner() {
    // Real `prepare` over the finite command boundary: the boundary reports one
    // transient failure on the owner's first post-mutation status read, and the
    // real owner-refresh retry must recover the same owner/generation.
    let fixture = command_stub::PolicyFixture::service("add-selected");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "add-staged-after",
    );
    fixture.fail_status_after_next_mutation();
    let (_workspace, trusted) = fixture.services().await;
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
    assert!(completion.error.is_none());
    let terminal = completion.workspace.expect("authoritative B");
    assert!(terminal.generation > checklist.workspace_generation);
    assert!(completion.prepared.is_some());
    assert_terminal_workspace(&trusted, &terminal);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
        )
    );
    assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
    assert_eq!(
        fixture.faults_served(),
        1,
        "the owner's first post-mutation capture must have been faulted once"
    );
}

#[tokio::test]
async fn owner_refresh_commit_first_capture_failure_recovers_new_head_once() {
    // Real `commit` over the finite command boundary: one transient failure on
    // the owner's first post-commit status read must not lose the commit; the
    // real retry recovers the new head once.
    let fixture = command_stub::PolicyFixture::service("commit-staged");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "commit-after",
    );
    fixture.fail_status_after_next_mutation();
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("prepared");
    let before = fixture.raw("head").to_owned();
    let completion = trusted
        .commit(
            prepared.id,
            "test: owner retry".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    let terminal = completion.workspace.expect("authoritative head");
    assert_terminal_workspace(&trusted, &terminal);
    let after = fixture.raw("head").to_owned();
    assert_ne!(before, after);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
        )
    );
    assert_eq!(
        fixture.mutation_inputs(),
        vec![b"test: owner retry".to_vec()]
    );
    assert_eq!(
        fixture.faults_served(),
        1,
        "the owner's first post-commit capture must have been faulted once"
    );
}

#[tokio::test]
async fn disconnected_recovery_consumes_zombie_owner_before_future_checklist() {
    // Real refresh over captured raw bytes then a real owner/generation
    // recovery; no Git process or repository is needed for this business rule.
    let fixture = command_stub::PolicyFixture::new("staged");
    let (workspace, trusted) = fixture.services().await;
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
    // The terminal mutation changed the worktree; the real owner refresh must
    // observe a new authoritative snapshot (a real content generation), not
    // merely re-read the pre-mutation raw bytes.
    fixture.set_case("modify");
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
    let fixture = command_stub::PolicyFixture::new("am-forced");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "am-forced-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert_eq!(fixture.mutation_inputs(), vec![b"added.txt\0".to_vec()]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();
}

#[tokio::test]
async fn untracked_entry_is_optional_only_and_prepares_as_added() {
    let fixture = command_stub::PolicyFixture::new("untracked");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "untracked-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert_eq!(fixture.mutation_inputs(), vec![b"untracked.txt\0".to_vec()]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();
}
#[tokio::test]
async fn selected_delete_and_untracked_destination_may_canonicalize_to_staged_rename() {
    let fixture = command_stub::PolicyFixture::new("delete-untracked");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "delete-untracked-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
        fixture.mutation_inputs(),
        vec![b"renamed.txt\0tracked.txt\0".to_vec()]
    );
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();
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
    let fixture = command_stub::PolicyFixture::new("rename-rm");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "rename-rm-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert_eq!(fixture.mutation_inputs(), vec![b"renamed.txt\0".to_vec()]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();
}

#[tokio::test]
async fn staged_rename_destination_mode_flip_is_rejected_after_one_add() {
    let fixture = command_stub::PolicyFixture::new("rename-rm");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "rename-rm-mode-after",
    );
    let (workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("rename checklist");
    let selected = checklist.optional[0].file_id;
    let mut gate = fixture.arm_gate(command_stub::GateTarget::BeforeMutation);
    let worker = tokio::spawn({
        let trusted = Arc::new(trusted);
        async move {
            trusted
                .prepare(checklist.id, vec![selected], CancellationToken::new())
                .await
        }
    });
    gate.wait_entered().await;
    fixture.set_case("rename-rm-mode-flipped");
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("external mode refresh");
    gate.release();
    let completion = worker.await.expect("prepare worker");
    assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    assert!(completion.prepared.is_none());
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    assert_eq!(fixture.mutation_inputs(), vec![b"renamed.txt\0".to_vec()]);
    fixture.assert_mutations_drained();
}

#[tokio::test]
async fn staged_rename_source_recreation_is_not_owned_by_destination_edit() {
    let fixture = command_stub::PolicyFixture::policy("rename-before");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "rename-after",
    );
    let (_workspace, trusted) = fixture.services().await;
    let trusted = Arc::new(trusted);
    let mut gate = fixture.arm_gate(command_stub::GateTarget::BeforeMutation);
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
    gate.wait_entered().await;
    fs::write(fixture.path().join("tracked.txt"), "outside S\n").expect("recreate source");
    gate.release();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !worker.is_finished(),
        "unsafe source recreation must not publish a terminal owner snapshot"
    );
    fs::remove_file(fixture.path().join("tracked.txt")).expect("restore safe source absence");
    let completion = worker.await.expect("prepare worker");
    assert!(completion.workspace.is_some());
}

#[tokio::test]
async fn staged_rename_destination_delete_claims_only_canonical_old_deletion() {
    let fixture = command_stub::PolicyFixture::new("rename-rd");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "rename-rd-after",
    );
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "rename-rd-committed",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert!(!fixture.path().join("tracked.txt").exists());
    assert!(!fixture.path().join("renamed.txt").exists());
    assert_eq!(
        fixture.mutation_argv(),
        [
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
            ),
            expected_mutation_argv(
                b"commit",
                &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
            )
        ]
        .concat()
    );
    assert_eq!(
        fixture.mutation_inputs(),
        vec![
            b"renamed.txt\0".to_vec(),
            b"test: delete renamed file".to_vec()
        ]
    );
    fixture.assert_mutations_drained();
}
#[tokio::test]
async fn trusted_git_selected_regular_to_symlink_binds_type_change() {
    let fixture = command_stub::PolicyFixture::new("symlink-type");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "symlink-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert!(completion.prepared.is_some());
    assert_eq!(
        fs::read_link(fixture.path().join("tracked.txt")).expect("symlink target"),
        Path::new("missing-target")
    );
    assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();
}
#[tokio::test]
async fn trusted_git_selected_executable_add_binds_exact_worktree_mode() {
    let fixture = command_stub::PolicyFixture::new("exec-add");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "exec-add-after",
    );
    let (_workspace, trusted) = fixture.services().await;
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
    assert!(completion.prepared.is_some());
    assert_eq!(fixture.mutation_inputs(), vec![b"run.sh\0".to_vec()]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    assert!(fixture.raw("stage").starts_with("100755 "));
    assert!(fixture.raw("stage").contains("\trun.sh\0"));
    assert_eq!(
        fs::metadata(fixture.path().join("run.sh"))
            .expect("executable metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    fixture.assert_mutations_drained();
}
