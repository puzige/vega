use super::*;

fn runner(
    root: &Path,
    callback: crate::GitTestCommandExecutor,
) -> (Runner, crate::GitTestCommandGuard) {
    let guard = crate::register_git_test_executor(root, callback).unwrap();
    let root = root.canonicalize().unwrap();
    let metadata = fs::metadata(&root).unwrap();
    let backend = crate::git_workspace::test_support::backend_for_root(&root).unwrap();
    (
        Runner::new(
            root,
            RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            RunnerExecutable::InProcess(backend),
        ),
        guard,
    )
}

#[test]
fn trusted_switch_argv_is_exact_and_read_limits_remain_frozen() {
    let root = tempfile::tempdir().unwrap();
    let (runner, _guard) = runner(
        root.path(),
        Arc::new(|command, input, limit| {
            let actual = command
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect::<Vec<_>>();
            let mut expected = PREFIX.to_vec();
            expected.extend([
                "-c",
                "core.hooksPath=/dev/null",
                "switch",
                "--no-guess",
                "--no-overwrite-ignore",
                "--no-recurse-submodules",
                "topic",
            ]);
            assert_eq!(actual, expected);
            assert!(input.is_none());
            assert_eq!(limit, MUTATION_STDOUT_LIMIT);
            Ok((Vec::new(), false))
        }),
    );
    runner
        .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
        .unwrap();
    assert_eq!(READ_TIMEOUT, Duration::from_secs(10));
    assert_eq!(MUTATION_TIMEOUT, Duration::from_secs(120));
    assert_eq!(MUTATION_STDOUT_LIMIT, 1024 * 1024);
    assert_eq!(STDERR_LIMIT, 64 * 1024);
}

#[test]
fn target_check_attr_uses_exact_source_argv_and_literal_nul_stdin() {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let (runner, _guard) = runner(
        root.path(),
        Arc::new(move |command, input, _| {
            let args = command
                .get_args()
                .map(|arg| arg.to_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            recorded.lock().unwrap().push(args.clone());
            if args.iter().any(|arg| arg == "check-attr") {
                let at = args.iter().position(|arg| arg == "check-attr").unwrap();
                assert_eq!(
                    &args[at + 1..],
                    [
                        "--source=1111111111111111111111111111111111111111",
                        "-z",
                        "--stdin",
                        "--all"
                    ]
                );
                assert_eq!(input, Some(b"literal path\0".as_slice()));
            } else {
                assert!(input.is_none());
            }
            Ok((
                if args.iter().any(|arg| arg == "--diff-filter=ACMRT") {
                    b"M\0literal path\0".to_vec()
                } else {
                    Vec::new()
                },
                false,
            ))
        }),
    );
    let current = b"0000000000000000000000000000000000000000";
    let target = b"1111111111111111111111111111111111111111";
    validate_target_changes(
        &runner,
        current,
        target,
        1024,
        target.len() + b"topic".len(),
        &CancellationToken::new(),
    )
    .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|args| args.iter().any(|arg| arg == "check-attr"))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|args| args.iter().any(|arg| arg == "diff"))
            .count(),
        2
    );
}
