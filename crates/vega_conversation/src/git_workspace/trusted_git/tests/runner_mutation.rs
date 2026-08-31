use super::*;

#[tokio::test]
async fn trusted_git_mutations_use_exact_argv_and_in_memory_stdin() {
    let repo = Repo::new();
    fs::write(repo.path().join("tracked.txt"), "changed\n").expect("modify");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("refresh");
    let (_recorder, script, argv, input) = mutation_recorder();
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
        .expect("trusted fake");
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
    assert_eq!(fs::read(&input).expect("add stdin"), b"tracked.txt\0");
    assert_eq!(
        fs::read(&argv).expect("add argv"),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul",],
        )
    );
    let message = "feat: exact stdin";
    let completion = trusted
        .commit(prepared.id, message.into(), CancellationToken::new())
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    assert_eq!(fs::read(&input).expect("commit stdin"), message.as_bytes());
    assert_eq!(
        fs::read(&argv).expect("commit argv"),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
        )
    );
}

#[test]
fn trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit() {
    let repo = Repo::new();
    let runner = test_runner(repo.path());
    for verb in ["add", "commit"] {
        let missing = repo.path().join(format!("missing-{verb}"));
        assert_eq!(
            mutation_error_code(run_fake_mutation(
                &runner,
                verb,
                &missing,
                Arc::from([]),
                &CancellationToken::new(),
                Duration::from_secs(1),
            )),
            GitWorkspaceErrorCode::SpawnFailed
        );

        let (_fixture, script, attempts) = scripted_mutation("exit 7");
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            mutation_error_code(run_fake_mutation(
                &runner,
                verb,
                &script,
                Arc::from([]),
                &cancelled,
                Duration::from_secs(1),
            )),
            GitWorkspaceErrorCode::Cancelled
        );
        assert!(!attempts.exists(), "pre-cancel spawned {verb}");
        assert_eq!(
            mutation_error_code(run_fake_mutation(
                &runner,
                verb,
                &script,
                Arc::from([]),
                &CancellationToken::new(),
                Duration::from_secs(1),
            )),
            GitWorkspaceErrorCode::GitFailed
        );
        assert_eq!(fs::read(&attempts).expect("one attempt"), b"x");

        for (stream, limit) in [("stdout", MUTATION_STDOUT_LIMIT), ("stderr", STDERR_LIMIT)] {
            let body = format!(
                "/usr/bin/python3 -c 'import sys; sys.{}.buffer.write(b\"x\" * {})'",
                stream, limit
            );
            let (_fixture, script, attempts) = scripted_mutation(&body);
            run_fake_mutation(
                &runner,
                verb,
                &script,
                Arc::from([]),
                &CancellationToken::new(),
                Duration::from_secs(3),
            )
            .expect("inclusive output cap");
            assert_eq!(fs::read(&attempts).expect("inclusive attempt"), b"x");

            let body = format!(
                "/usr/bin/python3 -c 'import sys; sys.{}.buffer.write(b\"x\" * {})'",
                stream,
                limit + 1
            );
            let (_fixture, script, attempts) = scripted_mutation(&body);
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from([]),
                    &CancellationToken::new(),
                    Duration::from_secs(3),
                )),
                GitWorkspaceErrorCode::OutputTooLarge
            );
            assert_eq!(fs::read(&attempts).expect("overflow attempt"), b"x");
        }
    }
}

#[test]
fn trusted_mutation_runner_times_out_cancels_and_reaps_process_groups() {
    let repo = Repo::new();
    let runner = test_runner(repo.path());
    for verb in ["add", "commit"] {
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        // The 500ms timeout starts at spawn, but under load the script
        // can need longer than 500ms just to reach the `sleep 30` spawn
        // and pid write (flaky registry F3). Such an attempt is
        // inconclusive rather than failed: retry with fresh fixtures and
        // only run the conclusive assertions once the descendant
        // actually existed. The last attempt panics on a missing pid, so
        // the loop never falls through without a probe.
        for attempt in 0..5 {
            let timeout_dir = tempfile::tempdir().expect("timeout fixture");
            let pid_file = timeout_dir.path().join("pid");
            let body = format!(
                "trap '' TERM\n/bin/sleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait",
                quote(&pid_file)
            );
            let (_fixture, script, attempts) = scripted_mutation(&body);
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from(vec![b'i'; 2 * 1024 * 1024]),
                    &CancellationToken::new(),
                    Duration::from_millis(500),
                )),
                GitWorkspaceErrorCode::TimedOut
            );
            let pid = match fs::read_to_string(&pid_file) {
                Ok(pid) => pid,
                Err(_) => {
                    assert!(
                        attempt < 4,
                        "spawn race persisted across retries (attempt {attempt})"
                    );
                    continue;
                }
            };
            assert_eq!(fs::read(&attempts).expect("timeout attempt"), b"x");
            assert!(
                !Command::new(KILL)
                    .args(["-0", &pid])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .expect("kill probe")
                    .success(),
                "timeout descendant survived"
            );
            break;
        }

        let cancel_dir = tempfile::tempdir().expect("cancel fixture");
        let ready = cancel_dir.path().join("ready");
        let body = format!(": > '{}'\n/bin/sleep 30", quote(&ready));
        let (_fixture, script, attempts) = scripted_mutation(&body);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let ready_clone = ready.clone();
        let canceller = thread::spawn(move || {
            let started = Instant::now();
            // Generous readiness window: only bounds how long the script
            // may take to start, not the phase itself (flaky registry F3).
            while !ready_clone.exists() && started.elapsed() < Duration::from_secs(15) {
                thread::sleep(Duration::from_millis(5));
            }
            trigger.cancel();
        });
        assert_eq!(
            mutation_error_code(run_fake_mutation(
                &runner,
                verb,
                &script,
                Arc::from(vec![b'i'; 4 * 1024 * 1024]),
                &cancel,
                Duration::from_secs(20),
            )),
            GitWorkspaceErrorCode::Cancelled
        );
        canceller.join().expect("canceller");
        assert_eq!(fs::read(&attempts).expect("cancel attempt"), b"x");
    }
}

#[test]
fn trusted_mutation_runner_drains_floods_while_writing_large_stdin() {
    let repo = Repo::new();
    let runner = test_runner(repo.path());
    let body = "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"o\" * 524288); sys.stdout.flush(); sys.stderr.buffer.write(b\"e\" * 32768); sys.stderr.flush(); data=sys.stdin.buffer.read(); raise SystemExit(0 if len(data)==4194304 else 9)'";
    for verb in ["add", "commit"] {
        let (_fixture, script, attempts) = scripted_mutation(body);
        let output = run_fake_mutation(
            &runner,
            verb,
            &script,
            Arc::from(vec![b'i'; 4 * 1024 * 1024]),
            &CancellationToken::new(),
            Duration::from_secs(5),
        )
        .expect("concurrent stdin/stdout/stderr");
        assert_eq!(output.stdout.len(), 512 * 1024);
        assert_eq!(fs::read(&attempts).expect("flood attempt"), b"x");
    }
}

#[tokio::test]
async fn service_reports_authoritative_state_after_add_and_commit_process_failures() {
    for (plan, expected) in [
        ("nonzero", CommitErrorCode::GitFailed),
        ("stdout-overflow", CommitErrorCode::OutputTooLarge),
        ("wait", CommitErrorCode::TimedOut),
        ("inherited-pipe", CommitErrorCode::ProcessControlFailed),
    ] {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("add A");
        let (_fixture, script, attempts, _argv, _input) = after_git_mutation(plan);
        let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
            repo.path(),
            workspace,
            script,
            Duration::from_secs(3),
        )
        .expect("trusted add fault");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("add checklist");
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, Some(expected), "add plan {plan}");
        assert!(completion.prepared.is_none(), "add plan {plan}");
        assert!(completion.workspace.is_some(), "add plan {plan}");
        assert_eq!(
            fs::read(attempts).unwrap_or_else(|error| panic!("add plan {plan} marker: {error}")),
            b"x"
        );
        assert!(
            !run_git_output(repo.path(), &["diff", "--cached", "--name-only"]).is_empty(),
            "add plan {plan} lost real index mutation"
        );

        let repo = Repo::new();
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
        run_git(repo.path(), &["add", "staged.txt"]);
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("commit B refresh");
        // Recreate an exact prepared capability under the faulting service.
        let (_fixture, script, attempts, _argv, _input) = after_git_mutation(plan);
        let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
            repo.path(),
            workspace,
            script,
            Duration::from_secs(3),
        )
        .expect("trusted commit fault");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("commit checklist");
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await
            .prepared
            .expect("commit prepared");
        let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        let completion = trusted
            .commit(
                prepared.id,
                "test: process fault".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            completion.outcome,
            CommitOutcome::Failed(expected),
            "{plan}"
        );
        assert!(completion.workspace.is_some(), "commit plan {plan}");
        assert_eq!(fs::read(attempts).expect("one commit"), b"x");
        let after = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        assert_ne!(before, after, "commit plan {plan} lost real commit");
        let duplicate = trusted
            .commit(
                prepared.id,
                "test: no retry".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            duplicate.outcome,
            CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
        );
    }

    for phase in ["add", "commit"] {
        let repo = Repo::new();
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        if phase == "add" {
            fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        } else {
            fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
            run_git(repo.path(), &["add", "staged.txt"]);
        }
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("exact output A");
        let (_fixture, script, attempts, _argv, _input) = after_git_mutation("stdout-exact");
        let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
            repo.path(),
            workspace,
            script,
            Duration::from_secs(3),
        )
        .expect("trusted exact output");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("exact output checklist");
        let selected = if phase == "add" {
            vec![checklist.optional[0].file_id]
        } else {
            Vec::new()
        };
        let prepared = trusted
            .prepare(checklist.id, selected, CancellationToken::new())
            .await
            .prepared
            .expect("inclusive stdout prepared");
        if phase == "commit" {
            let completion = trusted
                .commit(
                    prepared.id,
                    "test: inclusive output".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(completion.outcome, CommitOutcome::Committed);
        }
        assert_eq!(fs::read(attempts).expect("inclusive attempt"), b"x");
    }
}

#[tokio::test]
async fn service_entry_pre_mutation_failures_are_authoritative_and_single_use() {
    for phase in ["add", "commit"] {
        for (case, expected, expected_attempts) in [
            ("missing", CommitErrorCode::SpawnFailed, 0_usize),
            ("pre-cancel", CommitErrorCode::Cancelled, 0),
            ("nonzero-before", CommitErrorCode::GitFailed, 1),
        ] {
            let repo = Repo::new();
            if phase == "add" {
                fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
            } else {
                fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                run_git(repo.path(), &["add", "staged.txt"]);
            }
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("entry A");
            let (fixture, executable, attempts, argv) = if case == "missing" {
                let fixture = tempfile::tempdir().expect("missing fixture");
                let executable = fixture.path().join("missing-executable");
                let attempts = fixture.path().join("attempts");
                let argv = fixture.path().join("argv");
                (fixture, executable, attempts, argv)
            } else {
                let (fixture, executable, attempts, argv, _input) = before_git_mutation("exit 17");
                (fixture, executable, attempts, argv)
            };
            let trusted =
                TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, executable)
                    .expect("trusted pre-mutation fault");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("entry checklist");
            let cancel = CancellationToken::new();
            if case == "pre-cancel" {
                cancel.cancel();
            }
            if phase == "add" {
                let completion = trusted
                    .prepare(checklist.id, vec![checklist.optional[0].file_id], cancel)
                    .await;
                assert_eq!(completion.error, Some(expected), "add {case}");
                assert!(completion.prepared.is_none(), "add {case}");
                let terminal = completion.workspace.as_ref().expect("add terminal");
                assert_terminal_workspace(&trusted, terminal);
                assert!(
                    run_git_output(repo.path(), &["diff", "--cached", "--name-only"]).is_empty(),
                    "add {case} changed index"
                );
                let duplicate = trusted
                    .prepare(checklist.id, Vec::new(), CancellationToken::new())
                    .await;
                assert_eq!(duplicate.error, Some(CommitErrorCode::StaleAuthority));
            } else {
                let prepared = trusted
                    .prepare(checklist.id, Vec::new(), CancellationToken::new())
                    .await
                    .prepared
                    .expect("entry prepared");
                let invalid = trusted
                    .commit(prepared.id, String::new(), cancel.clone())
                    .await;
                assert_eq!(
                    invalid.outcome,
                    CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
                );
                assert!(invalid.workspace.is_none());
                assert!(!attempts.exists(), "invalid message spawned {case}");
                let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                let completion = trusted
                    .commit(prepared.id, "test: entry failure".into(), cancel)
                    .await;
                assert_eq!(
                    completion.outcome,
                    CommitOutcome::Failed(expected),
                    "{case}"
                );
                let terminal = completion.workspace.as_ref().expect("commit terminal");
                assert_terminal_workspace(&trusted, terminal);
                assert_eq!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
                let duplicate = trusted
                    .commit(
                        prepared.id,
                        "test: no retry".into(),
                        CancellationToken::new(),
                    )
                    .await;
                assert_eq!(
                    duplicate.outcome,
                    CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
                );
            }
            let actual_attempts = fs::read(&attempts).map_or(0, |bytes| bytes.len());
            assert_eq!(actual_attempts, expected_attempts, "{phase} {case}");
            if expected_attempts == 1 {
                let expected_argv = if phase == "add" {
                    expected_mutation_argv(
                        b"add",
                        &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
                    )
                } else {
                    expected_mutation_argv(
                        b"commit",
                        &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
                    )
                };
                assert_eq!(fs::read(&argv).expect("exact safe argv"), expected_argv);
            } else {
                assert!(!argv.exists(), "zero-spawn {phase} {case} wrote argv");
            }
            drop(fixture);
        }
    }
}

#[tokio::test]
async fn service_entry_stderr_caps_are_exact_for_add_and_commit() {
    for phase in ["add", "commit"] {
        for (plan, expected) in [
            ("stderr-exact", None),
            ("stderr-overflow", Some(CommitErrorCode::OutputTooLarge)),
        ] {
            let repo = Repo::new();
            if phase == "add" {
                fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
            } else {
                fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                run_git(repo.path(), &["add", "staged.txt"]);
            }
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("stderr A");
            let (_fixture, script, attempts, argv, _input) = after_git_mutation(plan);
            let trusted =
                TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                    .expect("trusted stderr cap");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("stderr checklist");
            if phase == "add" {
                let completion = trusted
                    .prepare(
                        checklist.id,
                        vec![checklist.optional[0].file_id],
                        CancellationToken::new(),
                    )
                    .await;
                assert_eq!(completion.error, expected, "add {plan}");
                assert_eq!(completion.prepared.is_some(), expected.is_none());
                let terminal = completion.workspace.as_ref().expect("add stderr terminal");
                assert_terminal_workspace(&trusted, terminal);
                assert!(
                    !run_git_output(repo.path(), &["diff", "--cached", "--name-only"]).is_empty()
                );
            } else {
                let prepared = trusted
                    .prepare(checklist.id, Vec::new(), CancellationToken::new())
                    .await
                    .prepared
                    .expect("stderr prepared");
                let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                let completion = trusted
                    .commit(
                        prepared.id,
                        "test: stderr cap".into(),
                        CancellationToken::new(),
                    )
                    .await;
                let expected_outcome =
                    expected.map_or(CommitOutcome::Committed, CommitOutcome::Failed);
                assert_eq!(completion.outcome, expected_outcome, "commit {plan}");
                let terminal = completion
                    .workspace
                    .as_ref()
                    .expect("commit stderr terminal");
                assert_terminal_workspace(&trusted, terminal);
                assert_ne!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
                let duplicate = trusted
                    .commit(
                        prepared.id,
                        "test: no retry".into(),
                        CancellationToken::new(),
                    )
                    .await;
                assert_eq!(
                    duplicate.outcome,
                    CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
                );
            }
            assert_eq!(fs::read(attempts).expect("one stderr attempt"), b"x");
            let expected_argv = if phase == "add" {
                expected_mutation_argv(
                    b"add",
                    &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
                )
            } else {
                expected_mutation_argv(
                    b"commit",
                    &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
                )
            };
            assert_eq!(fs::read(argv).expect("stderr safe argv"), expected_argv);
        }
    }
}

#[tokio::test]
async fn service_cancel_after_real_add_or_commit_returns_authoritative_state_once() {
    for phase in ["add", "commit"] {
        let repo = Repo::new();
        if phase == "add" {
            fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        } else {
            fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
            run_git(repo.path(), &["add", "staged.txt"]);
        }
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("cancel A");
        let (_fixture, script, ready, _release) = blocking_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                .expect("trusted cancel"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("cancel checklist");
        let selected = if phase == "add" {
            vec![checklist.optional[0].file_id]
        } else {
            Vec::new()
        };
        let cancel = CancellationToken::new();
        if phase == "add" {
            let worker = tokio::spawn({
                let trusted = trusted.clone();
                let cancel = cancel.clone();
                async move { trusted.prepare(checklist.id, selected, cancel).await }
            });
            wait_for_path(&ready).await;
            cancel.cancel();
            let completion = worker.await.expect("cancel add worker");
            assert_eq!(completion.error, Some(CommitErrorCode::Cancelled));
            assert!(completion.workspace.is_some());
            assert!(completion.prepared.is_none());
        } else {
            let prepared = trusted
                .prepare(checklist.id, selected, CancellationToken::new())
                .await
                .prepared
                .expect("cancel commit prepared");
            let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
            let worker = tokio::spawn({
                let trusted = trusted.clone();
                let cancel = cancel.clone();
                async move {
                    trusted
                        .commit(prepared.id, "test: cancel".into(), cancel)
                        .await
                }
            });
            wait_for_path(&ready).await;
            cancel.cancel();
            let completion = worker.await.expect("cancel commit worker");
            assert_eq!(
                completion.outcome,
                CommitOutcome::Failed(CommitErrorCode::Cancelled)
            );
            assert!(completion.workspace.is_some());
            assert_ne!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
        }
    }
}
