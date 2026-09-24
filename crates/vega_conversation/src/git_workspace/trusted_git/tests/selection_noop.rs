use super::*;

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

#[tokio::test]
async fn selected_awkward_raw_paths_use_one_sorted_nul_stdin_and_no_path_argv() {
    let fixture = command_stub::PolicyFixture::policy("awkward-before");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "awkward-after",
    );
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .unwrap();
    let mut paths = vec![
        b"space name.txt".to_vec(),
        b"tab\tname.txt".to_vec(),
        b"line\nname.txt".to_vec(),
        b"-leading.txt".to_vec(),
    ];
    assert_eq!(checklist.optional.len(), paths.len());
    let selected = checklist.optional.iter().map(|row| row.file_id).collect();
    let completion = trusted
        .prepare(checklist.id, selected, CancellationToken::new())
        .await;
    assert_eq!(completion.error, None);
    paths.sort();
    let expected_input: Vec<u8> = paths
        .into_iter()
        .flat_map(|mut path| {
            path.push(0);
            path
        })
        .collect();
    assert_eq!(fixture.mutation_inputs(), vec![expected_input]);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    fixture.assert_mutations_drained();

    let root = tempfile::tempdir().unwrap();
    let raw = b"nonutf8-\xff.txt\0".to_vec();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let records = recorded.clone();
    let _guard = crate::register_git_test_executor(
        root.path(),
        Arc::new(move |command, input, _| {
            let argv = command
                .get_args()
                .flat_map(|arg| arg.as_bytes().iter().copied().chain([0]))
                .collect::<Vec<_>>();
            records
                .lock()
                .unwrap()
                .push((argv, input.unwrap().to_vec()));
            Err(error(GitWorkspaceErrorCode::GitFailed))
        }),
    )
    .unwrap();
    let workspace = GitWorkspaceService::new(root.path()).unwrap();
    let runner = workspace.runner(&CancellationToken::new()).unwrap();
    let result = runner.run_trusted_mutation_with_executable_and_timeout(
        "add",
        &[
            OsString::from("-A"),
            OsString::from("--pathspec-from-file=-"),
            OsString::from("--pathspec-file-nul"),
        ],
        Arc::from(raw.clone()),
        &CancellationToken::new(),
        Path::new("/dev/null"),
        Duration::from_secs(3),
    );
    assert!(result.is_err());
    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].1, raw);
    assert_eq!(
        recorded[0].0,
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
        )
    );
    assert!(!recorded[0].0.windows(2).any(|window| window == [0xff, 0]));
}
