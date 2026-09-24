use super::*;

fn run_git_input(root: &Path, args: &[&str], input: &[u8]) {
    let mut command = Command::new(GIT);
    command
        .current_dir(root)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    scrub_git_environment(&mut command);
    let mut child = command.spawn().expect("git fixture");
    std::io::Write::write_all(&mut child.stdin.take().expect("git stdin"), input)
        .expect("git fixture input");
    let output = child.wait_with_output().expect("git fixture output");
    assert!(
        output.status.success(),
        "git fixture failed: {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn reset_selection_repo(root: &Path, base: &str) {
    run_git(root, &["reset", "--hard", base]);
    run_git(root, &["clean", "-fdx"]);
}

fn assert_selection_adapter_state(root: &Path, fixtures: &serde_json::Value, case: &str) {
    let raw = &fixtures[case]["raw"];
    let head = String::from_utf8(run_git_output(root, &["rev-parse", "HEAD"])).expect("head");
    let fixture_head = raw["head"].as_str().expect("fixture head");
    for (key, args) in [
        (
            "status",
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--renames",
                "--untracked-files=all",
            ][..],
        ),
        (
            "paths",
            &["ls-files", "-z", "--cached", "--deduplicate"][..],
        ),
        ("stage", &["ls-files", "--stage", "-z"][..]),
        ("tree", &["ls-tree", "-r", "-z", "--full-tree", "HEAD"][..]),
        (
            "refs",
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(objectname)%00%(refname)%00",
                "refs/heads/",
            ][..],
        ),
        (
            "staged_raw",
            &[
                "diff",
                "--cached",
                "--raw",
                "-z",
                "--abbrev=64",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ][..],
        ),
        (
            "unstaged_raw",
            &[
                "diff",
                "--raw",
                "-z",
                "--abbrev=64",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ][..],
        ),
        (
            "staged_numstat",
            &[
                "diff",
                "--cached",
                "--numstat",
                "-z",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ][..],
        ),
        (
            "unstaged_numstat",
            &[
                "diff",
                "--numstat",
                "-z",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ][..],
        ),
        (
            "summary",
            &[
                "-c",
                "core.quotePath=true",
                "diff",
                "--cached",
                "--patch",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--full-index",
                "--",
            ][..],
        ),
    ] {
        let actual = String::from_utf8(run_git_output(root, args))
            .expect("captured UTF-8 fixture")
            .replace(head.trim(), fixture_head);
        assert_eq!(
            actual.as_bytes(),
            raw[key].as_str().expect("raw fixture key").as_bytes(),
            "{case}: {key}"
        );
    }
    if let Some(attrs) = fixtures[case]["attrs_by_input"].as_object() {
        for (input, expected) in attrs {
            let mut command = Command::new(GIT);
            command
                .current_dir(root)
                .args(["check-attr", "-z", "--stdin", "--all"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            scrub_git_environment(&mut command);
            let mut child = command.spawn().expect("real attribute adapter");
            std::io::Write::write_all(
                &mut child.stdin.take().expect("attribute stdin"),
                input.as_bytes(),
            )
            .expect("selected path input");
            let output = child.wait_with_output().expect("attribute result");
            assert!(output.status.success());
            assert_eq!(
                output.stdout,
                expected.as_str().expect("selected attributes").as_bytes(),
                "{case}: selected attributes {input:?}"
            );
        }
    }
}

#[test]
fn selection_topology_captured_states_match_real_git() {
    let repo = Repo::new();
    let root = repo.path();
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("command-fixtures.json")).expect("raw fixtures");
    let service_fixtures: serde_json::Value =
        serde_json::from_str(include_str!("service-fixtures.json")).expect("service fixtures");
    let base = String::from_utf8(run_git_output(root, &["rev-parse", "HEAD"])).expect("base head");

    fs::write(root.join("staged.txt"), "staged\n").expect("staged fixture");
    run_git(root, &["add", "staged.txt"]);
    assert_selection_adapter_state(root, &service_fixtures, "commit-staged");
    run_git_input(
        root,
        &["commit", "--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        b"test: staged only",
    );
    assert_selection_adapter_state(root, &service_fixtures, "commit-after");

    reset_selection_repo(root, base.trim());
    fs::write(root.join("added.txt"), "first\n").expect("first add");
    run_git(root, &["add", "added.txt"]);
    fs::write(root.join("added.txt"), "second\n").expect("forced add");
    assert_selection_adapter_state(root, &fixtures, "am-forced");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"added.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "am-forced-after");

    reset_selection_repo(root, base.trim());
    fs::write(root.join("untracked.txt"), "new\n").expect("untracked fixture");
    assert_selection_adapter_state(root, &fixtures, "untracked");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"untracked.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "untracked-after");

    reset_selection_repo(root, base.trim());
    fs::remove_file(root.join("tracked.txt")).expect("delete tracked fixture");
    fs::write(root.join("renamed.txt"), "base\n").expect("rename destination");
    assert_selection_adapter_state(root, &fixtures, "delete-untracked");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"renamed.txt\0tracked.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "delete-untracked-after");

    reset_selection_repo(root, base.trim());
    run_git(root, &["mv", "tracked.txt", "renamed.txt"]);
    fs::write(root.join("renamed.txt"), "renamed and edited\n").expect("rename edit");
    assert_selection_adapter_state(root, &fixtures, "rename-rm");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"renamed.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "rename-rm-after");

    reset_selection_repo(root, base.trim());
    run_git(root, &["mv", "tracked.txt", "renamed.txt"]);
    fs::write(root.join("renamed.txt"), "renamed and edited\n").expect("mode rename edit");
    assert_selection_adapter_state(root, &fixtures, "rename-rm");
    fs::set_permissions(root.join("renamed.txt"), fs::Permissions::from_mode(0o755))
        .expect("mode flip");
    assert_selection_adapter_state(root, &fixtures, "rename-rm-mode-flipped");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"renamed.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "rename-rm-mode-after");

    reset_selection_repo(root, base.trim());
    run_git(root, &["mv", "tracked.txt", "renamed.txt"]);
    fs::remove_file(root.join("renamed.txt")).expect("delete rename destination");
    assert_selection_adapter_state(root, &fixtures, "rename-rd");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"renamed.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "rename-rd-after");
    run_git_input(
        root,
        &["commit", "--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        b"test: delete renamed file",
    );
    assert_selection_adapter_state(root, &fixtures, "rename-rd-committed");

    reset_selection_repo(root, base.trim());
    fs::remove_file(root.join("tracked.txt")).expect("remove regular file");
    std::os::unix::fs::symlink("missing-target", root.join("tracked.txt"))
        .expect("replace with symlink");
    assert_selection_adapter_state(root, &fixtures, "symlink-type");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"tracked.txt\0",
    );
    assert_selection_adapter_state(root, &fixtures, "symlink-after");

    reset_selection_repo(root, base.trim());
    fs::write(root.join("run.sh"), "#!/bin/sh\nexit 0\n").expect("executable fixture");
    fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o755))
        .expect("executable mode");
    assert_selection_adapter_state(root, &fixtures, "exec-add");
    run_git_input(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        b"run.sh\0",
    );
    assert_selection_adapter_state(root, &fixtures, "exec-add-after");
}

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
}
