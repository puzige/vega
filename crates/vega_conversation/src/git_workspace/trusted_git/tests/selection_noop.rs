use super::*;

#[tokio::test]
async fn trusted_git_empty_selection_spawns_zero_add() {
    let repo = Repo::new();
    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
    run_git(repo.path(), &["add", "staged.txt"]);
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let (_recorder, script, argv, _input) = mutation_recorder();
    let _ = fs::remove_file(&argv);
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
        .expect("trusted fake");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert!(prepared.prepared.is_some());
    assert!(!argv.exists(), "empty S must not spawn add");
}

#[tokio::test]
async fn empty_selection_never_spawns_add_for_each_real_staged_delta() {
    for kind in ["add", "modify", "mode", "delete", "rename"] {
        let repo = Repo::new();
        match kind {
            "add" => {
                fs::write(repo.path().join("added.txt"), "added\n").expect("add fixture");
                run_git(repo.path(), &["add", "added.txt"]);
            }
            "modify" => {
                fs::write(repo.path().join("tracked.txt"), "modified\n").expect("modify fixture");
                run_git(repo.path(), &["add", "tracked.txt"]);
            }
            "mode" => {
                let path = repo.path().join("tracked.txt");
                let mut permissions = fs::metadata(&path).expect("mode metadata").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(path, permissions).expect("mode fixture");
                run_git(repo.path(), &["add", "tracked.txt"]);
            }
            "delete" => run_git(repo.path(), &["rm", "-q", "tracked.txt"]),
            "rename" => run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]),
            _ => unreachable!(),
        }
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("staged refresh");
        let (_recorder, script, argv, _input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted recorder");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("{kind} staged checklist: {error:?}"));
        let completion = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert!(completion.prepared.is_some(), "{kind} staged delta");
        assert_eq!(completion.error, None, "{kind} staged delta");
        assert!(!argv.exists(), "{kind} empty selection spawned add");
    }
}

#[tokio::test]
async fn clean_and_normalized_noop_are_no_staged_changes_without_commit() {
    let repo = Repo::new();
    let (workspace, trusted) = repo.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("clean checklist");
    let clean = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert_eq!(clean.error, Some(CommitErrorCode::NoStagedChanges));
    assert!(clean.prepared.is_none());

    run_git(repo.path(), &["config", "core.filemode", "false"]);
    let path = repo.path().join("tracked.txt");
    let mut permissions = fs::metadata(&path).expect("mode metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("ignored mode change");
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("ignored mode refresh");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("ignored mode checklist");
    assert!(checklist.staged.is_empty() && checklist.optional.is_empty());
    let ignored = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert_eq!(ignored.error, Some(CommitErrorCode::NoStagedChanges));
    assert!(ignored.prepared.is_none());

    let repo = Repo::new();
    fs::write(repo.path().join(".gitattributes"), "* text eol=lf\n").expect("eol attributes");
    run_git(repo.path(), &["add", ".gitattributes"]);
    run_git(repo.path(), &["commit", "-qm", "eol policy"]);
    fs::write(repo.path().join("tracked.txt"), b"base\r\n").expect("crlf worktree");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("eol refresh");
    let (_recorder, script, argv, _input) = mutation_recorder();
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
        .expect("trusted eol");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("eol checklist");
    assert_eq!(checklist.optional.len(), 1);
    let normalized = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(normalized.error, Some(CommitErrorCode::NoStagedChanges));
    assert!(normalized.prepared.is_none());
    assert_eq!(
        fs::read(argv).expect("one normalization add"),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );

    let repo = Repo::new();
    fs::write(repo.path().join(".gitattributes"), "* text eol=lf\n").expect("drift eol attributes");
    fs::write(repo.path().join("other.txt"), "other\n").expect("other fixture");
    run_git(repo.path(), &["add", ".gitattributes", "other.txt"]);
    run_git(repo.path(), &["commit", "-qm", "eol drift policy"]);
    fs::write(repo.path().join("tracked.txt"), b"base\r\n").expect("selected crlf");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("drift A");
    let (fixture, script, ready, release) = blocking_before_mutation();
    let trusted = Arc::new(
        TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted drift"),
    );
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("drift checklist");
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
    fs::write(repo.path().join("other.txt"), b"other\r\n").expect("outside-S drift");
    fs::write(release, b"release").expect("release normalized add");
    let completion = worker.await.expect("normalized drift worker");
    assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    assert!(completion.prepared.is_none());
    let terminal = completion.workspace.as_ref().expect("terminal workspace");
    assert_terminal_workspace(&trusted, terminal);
    assert_eq!(
        fs::read(fixture.path().join("mutation-attempts")).expect("one add attempt"),
        b"x"
    );
    assert_eq!(
        fs::read(fixture.path().join("mutation-argv.bin")).expect("only add argv"),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    let state = trusted
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(!state.mutation_active);
    assert!(state.prepared.is_none());
}

#[tokio::test]
async fn selected_awkward_raw_paths_use_one_sorted_nul_stdin_and_no_path_argv() {
    let repo = Repo::new();
    let mut paths = vec![
        b"space name.txt".to_vec(),
        b"tab\tname.txt".to_vec(),
        b"line\nname.txt".to_vec(),
        b"-leading.txt".to_vec(),
    ];
    for raw in &paths {
        fs::write(
            repo.path().join(OsString::from_vec(raw.clone())),
            b"awkward\n",
        )
        .expect("awkward fixture");
    }
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("awkward refresh");
    let (_recorder, script, argv, input) = mutation_recorder();
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
        .expect("trusted recorder");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("awkward checklist");
    assert_eq!(checklist.optional.len(), paths.len());
    let selected = checklist.optional.iter().map(|row| row.file_id).collect();
    let completion = trusted
        .prepare(checklist.id, selected, CancellationToken::new())
        .await;
    assert_eq!(completion.error, None);
    paths.sort();
    let mut expected_input = Vec::new();
    for path in paths {
        expected_input.extend_from_slice(&path);
        expected_input.push(0);
    }
    assert_eq!(fs::read(input).expect("awkward add stdin"), expected_input);
    assert_eq!(
        fs::read(argv).expect("awkward add argv"),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );

    // Darwin/APFS may reject non-UTF-8 leaf creation with EILSEQ. Exercise
    // the same production mutation pipe directly so raw bytes still have
    // byte-exact evidence without claiming an unavailable filesystem E2E.
    let raw = b"nonutf8-\xff.txt\0".to_vec();
    let (_recorder, script, argv, input) = mutation_recorder();
    let runner = test_runner(repo.path());
    let result = runner.run_trusted_mutation_with_executable_and_timeout(
        "add",
        &[
            OsString::from("-A"),
            OsString::from("--pathspec-from-file=-"),
            OsString::from("--pathspec-file-nul"),
        ],
        Arc::from(raw.clone()),
        &CancellationToken::new(),
        &script,
        Duration::from_secs(3),
    );
    assert!(result.is_err(), "missing raw fixture unexpectedly staged");
    assert_eq!(fs::read(input).expect("raw byte stdin"), raw);
    let recorded = fs::read(argv).expect("raw byte argv");
    assert_eq!(
        recorded,
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    assert!(!recorded.windows(2).any(|window| window == [0xff, 0]));
}
