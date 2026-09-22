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
async fn empty_selection_never_spawns_add_for_each_staged_delta() {
    for kind in ["add", "modify", "mode", "delete", "rename"] {
        let fixture = command_stub::PolicyFixture::new(kind);
        let (_workspace, trusted) = fixture.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .unwrap_or_else(|error| panic!("{kind} staged checklist: {error:?}"));
        let completion = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert!(completion.prepared.is_some(), "{kind} staged delta");
        assert_eq!(completion.error, None, "{kind} staged delta");
        fixture.assert_no_mutation();
    }
}

#[tokio::test]
async fn clean_and_normalized_noop_are_no_staged_changes_without_commit() {
    use super::filter_gitlink::{ADD_ARGS, expected_top_mutation, top_matrix_fixture};
    for case in ["clean", "ignored-mode"] {
        let fixture = top_matrix_fixture(case);
        let (_workspace, trusted) = fixture.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("no-op checklist");
        assert!(checklist.staged.is_empty() && checklist.optional.is_empty());
        let completion = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert_eq!(
            completion.error,
            Some(CommitErrorCode::NoStagedChanges),
            "{case}"
        );
        assert!(completion.prepared.is_none());
        fixture.assert_no_mutation();
    }
    for drift in [false, true] {
        let fixture = top_matrix_fixture("normalization-before");
        fixture.expect_mutation(
            "add",
            ADD_ARGS,
            if drift {
                "normalization-drift-after"
            } else {
                "normalization-after"
            },
        );
        let (_workspace, trusted) = fixture.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("normalization checklist");
        assert_eq!(checklist.optional.len(), 1);
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        let _ = fixture.mutation_argv();
        assert_eq!(
            completion.error,
            Some(if drift {
                CommitErrorCode::ChangedDuringRead
            } else {
                CommitErrorCode::NoStagedChanges
            })
        );
        assert!(completion.prepared.is_none());
        assert_eq!(
            fixture.mutation_argv(),
            expected_top_mutation(b"add", ADD_ARGS)
        );
        assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
        if drift {
            assert_terminal_workspace(
                &trusted,
                completion.workspace.as_ref().expect("terminal workspace"),
            );
            let state = trusted
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert!(!state.mutation_active);
            assert!(state.prepared.is_none());
        }
    }
}

#[test]
fn ignored_mode_and_normalization_git_adapter_matches_captured_states() {
    use super::filter_gitlink::assert_top_adapter_state;
    let repo = Repo::new();
    assert_top_adapter_state(repo.path(), "clean");
    run_git(repo.path(), &["config", "core.filemode", "false"]);
    fs::set_permissions(
        repo.path().join("tracked.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("ignored mode");
    assert_top_adapter_state(repo.path(), "ignored-mode");
    fs::set_permissions(
        repo.path().join("tracked.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("restore mode");
    fs::write(repo.path().join(".gitattributes"), "* text eol=lf\n").expect("attributes");
    fs::write(repo.path().join("other.txt"), "other\n").expect("other");
    run_git(repo.path(), &["add", ".gitattributes", "other.txt"]);
    run_git(repo.path(), &["commit", "-qm", "normalization policy"]);
    fs::write(repo.path().join("tracked.txt"), b"base\r\n").expect("CRLF");
    assert_top_adapter_state(repo.path(), "normalization-before");
    run_git(repo.path(), &["add", "-A", "--", "tracked.txt"]);
    assert_top_adapter_state(repo.path(), "normalization-after");
    assert!(run_git_output(repo.path(), &["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(
        fs::read(repo.path().join("tracked.txt")).expect("worktree"),
        b"base\r\n"
    );
    fs::write(repo.path().join("other.txt"), b"other\r\n").expect("outside-S drift");
    assert_top_adapter_state(repo.path(), "normalization-drift-after");
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
