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
            // Generate fixed bytes before the runner's deadline starts, avoiding
            // interpreter startup in the output-boundary measurement.
            let payload_dir = tempfile::tempdir().expect("output payload fixture");
            let output = |size: usize| {
                let payload = payload_dir.path().join(size.to_string());
                fs::write(&payload, vec![b'x'; size]).expect("output payload");
                format!(
                    "/bin/cat '{}'{}",
                    payload.to_string_lossy().replace('\'', "'\\''"),
                    if stream == "stderr" { " >&2" } else { "" }
                )
            };
            let body = output(limit);
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

            let body = output(limit + 1);
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
        let pid = fs::read_to_string(&pid_file).expect("timeout descendant pid");
        assert!(!pid.trim().is_empty(), "timeout descendant pid was empty");
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

// Keep each existing matrix case independently discoverable by nextest.
//
// The service-level cases below drive the real `TrustedGitService` over finite,
// captured external states and one explicit mutation outcome: the exact argv the
// production code must build, the captured post-state a real mutation produced,
// and the typed result the process boundary reports. They create no Git
// repository and start no replaced external process. The real process
// argv/stdin, spawn failure, timeout, signal/drain and reaping contracts remain
// covered by the runner-level `trusted_mutation_runner_*` tests above and by the
// captured-state adapter contract in `filter_gitlink.rs`.

const ADD_MUTATION_ARGS: &[&str] = &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"];
const COMMIT_MUTATION_ARGS: &[&str] = &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"];
const ADD_SELECTED: &str = "add-selected";
const ADD_STAGED: &str = "add-staged-after";
const COMMIT_STAGED: &str = "commit-staged";
const COMMIT_AFTER: &str = "commit-after";
const COMMIT_MESSAGE: &str = "test: process fault";

fn expected_add_argv() -> Vec<u8> {
    expected_mutation_argv(
        b"add",
        &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
    )
}

fn expected_commit_argv() -> Vec<u8> {
    expected_mutation_argv(
        b"commit",
        &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
    )
}

/// One add-phase case: the boundary reports `result` after applying `post_case`
/// (the captured state a real `add` produced). `expected` is the public service
/// error; `None` means the real `prepare` must succeed. Exactly one external
/// attempt is recorded with the exact production argv and in-memory stdin.
async fn assert_add_case(
    post_case: Option<&str>,
    result: Option<GitWorkspaceErrorCode>,
    expected: Option<CommitErrorCode>,
) {
    let fixture = command_stub::PolicyFixture::service(ADD_SELECTED);
    fixture.expect_mutation_result("add", ADD_MUTATION_ARGS, post_case, result, true);
    let (_workspace, trusted) = fixture.services().await;
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
    assert_eq!(
        completion.error, expected,
        "add post={post_case:?} result={result:?}"
    );
    assert_eq!(completion.prepared.is_some(), expected.is_none());
    let terminal = completion
        .workspace
        .as_ref()
        .expect("add terminal workspace");
    assert_terminal_workspace(&trusted, terminal);
    if post_case.is_some() {
        assert!(
            terminal.generation > checklist.workspace_generation,
            "add post={post_case:?} did not publish the mutated snapshot"
        );
    } else {
        assert_eq!(terminal.generation, checklist.workspace_generation);
    }
    assert_eq!(fixture.mutation_argv(), expected_add_argv());
    assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
    let duplicate = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert_eq!(duplicate.error, Some(CommitErrorCode::StaleAuthority));
}

/// One commit-phase case: the boundary reports `result` after applying
/// `post_case` (the captured state a real `commit` produced). The prepared
/// capability is consumed exactly once; a retry is always stale.
async fn assert_commit_case(
    post_case: Option<&str>,
    result: Option<GitWorkspaceErrorCode>,
    expected: Option<CommitErrorCode>,
) {
    let fixture = command_stub::PolicyFixture::service(COMMIT_STAGED);
    fixture.expect_mutation_result("commit", COMMIT_MUTATION_ARGS, post_case, result, true);
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("commit checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("commit prepared");
    let completion = trusted
        .commit(prepared.id, COMMIT_MESSAGE.into(), CancellationToken::new())
        .await;
    let expected_outcome = expected.map_or(CommitOutcome::Committed, CommitOutcome::Failed);
    assert_eq!(
        completion.outcome, expected_outcome,
        "commit post={post_case:?} result={result:?}"
    );
    let terminal = completion
        .workspace
        .as_ref()
        .expect("commit terminal workspace");
    assert_terminal_workspace(&trusted, terminal);
    if post_case.is_some() {
        assert!(terminal.generation > checklist.workspace_generation);
    } else {
        assert_eq!(terminal.generation, checklist.workspace_generation);
    }
    assert_eq!(fixture.mutation_argv(), expected_commit_argv());
    assert_eq!(
        fixture.mutation_inputs(),
        vec![COMMIT_MESSAGE.as_bytes().to_vec()]
    );
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

/// One add pre-spawn denial. `pre-cancel` cancels before the process boundary is
/// reached; `missing` models the boundary reporting an absent executable without
/// spawning. Either way no external attempt is recorded and the service still
/// publishes an authoritative terminal state and consumes the checklist.
async fn assert_add_pre_spawn(case: &str, expected: CommitErrorCode) {
    let fixture = command_stub::PolicyFixture::service(ADD_SELECTED);
    if case == "missing" {
        fixture.expect_mutation_result(
            "add",
            ADD_MUTATION_ARGS,
            None,
            Some(GitWorkspaceErrorCode::SpawnFailed),
            false,
        );
    }
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("add checklist");
    let cancel = CancellationToken::new();
    if case == "pre-cancel" {
        cancel.cancel();
    }
    let completion = trusted
        .prepare(checklist.id, vec![checklist.optional[0].file_id], cancel)
        .await;
    assert_eq!(completion.error, Some(expected), "add {case}");
    assert!(completion.prepared.is_none(), "add {case}");
    let terminal = completion.workspace.as_ref().expect("add terminal");
    assert_terminal_workspace(&trusted, terminal);
    assert_eq!(terminal.generation, checklist.workspace_generation);
    assert!(
        fixture.mutation_argv().is_empty(),
        "add {case} recorded an external attempt"
    );
    let duplicate = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert_eq!(duplicate.error, Some(CommitErrorCode::StaleAuthority));
}

/// One commit pre-spawn denial. The empty message is always rejected before any
/// boundary call. `registered` declares the single expected boundary outcome for
/// the real commit: `(code, record)` where `record` marks whether the boundary
/// actually spawned (a pre-spawn denial records zero attempts).
async fn assert_commit_pre_spawn(
    case: &str,
    expected: CommitErrorCode,
    registered: Option<(GitWorkspaceErrorCode, bool)>,
) {
    let fixture = command_stub::PolicyFixture::service(COMMIT_STAGED);
    if let Some((code, record)) = registered {
        fixture.expect_mutation_result("commit", COMMIT_MUTATION_ARGS, None, Some(code), record);
    }
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("commit checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("commit prepared");
    let invalid = trusted
        .commit(prepared.id, String::new(), CancellationToken::new())
        .await;
    assert_eq!(
        invalid.outcome,
        CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
    );
    assert!(invalid.workspace.is_none());
    let cancel = CancellationToken::new();
    if case == "pre-cancel" {
        cancel.cancel();
    }
    let completion = trusted
        .commit(prepared.id, COMMIT_MESSAGE.into(), cancel)
        .await;
    assert_eq!(
        completion.outcome,
        CommitOutcome::Failed(expected),
        "commit {case}"
    );
    let terminal = completion.workspace.as_ref().expect("commit terminal");
    assert_terminal_workspace(&trusted, terminal);
    assert_eq!(terminal.generation, checklist.workspace_generation);
    match registered {
        Some((_, true)) => {
            assert_eq!(
                fixture.mutation_argv(),
                expected_commit_argv(),
                "commit {case}"
            );
            assert_eq!(
                fixture.mutation_inputs(),
                vec![COMMIT_MESSAGE.as_bytes().to_vec()],
                "commit {case}"
            );
        }
        _ => assert!(
            fixture.mutation_argv().is_empty(),
            "commit {case} recorded an external attempt"
        ),
    }
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

#[tokio::test]
async fn service_add_process_failure_nonzero() {
    assert_add_case(
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::GitFailed),
        Some(CommitErrorCode::GitFailed),
    )
    .await;
}

#[tokio::test]
async fn service_commit_process_failure_nonzero() {
    assert_commit_case(
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::GitFailed),
        Some(CommitErrorCode::GitFailed),
    )
    .await;
}

#[tokio::test]
async fn service_add_process_failure_stdout_overflow() {
    assert_add_case(
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::OutputTooLarge),
        Some(CommitErrorCode::OutputTooLarge),
    )
    .await;
}

#[tokio::test]
async fn service_commit_process_failure_stdout_overflow() {
    assert_commit_case(
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::OutputTooLarge),
        Some(CommitErrorCode::OutputTooLarge),
    )
    .await;
}

#[tokio::test]
async fn service_add_process_failure_wait() {
    assert_add_case(
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::TimedOut),
        Some(CommitErrorCode::TimedOut),
    )
    .await;
}

#[tokio::test]
async fn service_commit_process_failure_wait() {
    assert_commit_case(
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::TimedOut),
        Some(CommitErrorCode::TimedOut),
    )
    .await;
}

#[tokio::test]
async fn service_add_process_failure_inherited_pipe() {
    assert_add_case(
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::ProcessControlFailed),
        Some(CommitErrorCode::ProcessControlFailed),
    )
    .await;
}

#[tokio::test]
async fn service_commit_process_failure_inherited_pipe() {
    assert_commit_case(
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::ProcessControlFailed),
        Some(CommitErrorCode::ProcessControlFailed),
    )
    .await;
}

// The inclusive stdout/stderr caps are proven at the process boundary by the
// real `trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit`
// integration test; here the service only maps the boundary result.
#[tokio::test]
async fn service_add_stdout_exact() {
    assert_add_case(Some(ADD_STAGED), None, None).await;
}

#[tokio::test]
async fn service_commit_stdout_exact() {
    assert_commit_case(Some(COMMIT_AFTER), None, None).await;
}

#[tokio::test]
async fn service_add_pre_mutation_missing() {
    assert_add_pre_spawn("missing", CommitErrorCode::SpawnFailed).await;
}

#[tokio::test]
async fn service_add_pre_mutation_pre_cancel() {
    assert_add_pre_spawn("pre-cancel", CommitErrorCode::Cancelled).await;
}

#[tokio::test]
async fn service_add_pre_mutation_nonzero_before() {
    assert_add_case(
        None,
        Some(GitWorkspaceErrorCode::GitFailed),
        Some(CommitErrorCode::GitFailed),
    )
    .await;
}

#[tokio::test]
async fn service_commit_pre_mutation_missing() {
    assert_commit_pre_spawn(
        "missing",
        CommitErrorCode::SpawnFailed,
        Some((GitWorkspaceErrorCode::SpawnFailed, false)),
    )
    .await;
}

#[tokio::test]
async fn service_commit_pre_mutation_pre_cancel() {
    assert_commit_pre_spawn("pre-cancel", CommitErrorCode::Cancelled, None).await;
}

#[tokio::test]
async fn service_commit_pre_mutation_nonzero_before() {
    assert_commit_pre_spawn(
        "nonzero-before",
        CommitErrorCode::GitFailed,
        Some((GitWorkspaceErrorCode::GitFailed, true)),
    )
    .await;
}

#[tokio::test]
async fn service_add_stderr_exact() {
    assert_add_case(Some(ADD_STAGED), None, None).await;
}

#[tokio::test]
async fn service_add_stderr_overflow() {
    assert_add_case(
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::OutputTooLarge),
        Some(CommitErrorCode::OutputTooLarge),
    )
    .await;
}

#[tokio::test]
async fn service_commit_stderr_exact() {
    assert_commit_case(Some(COMMIT_AFTER), None, None).await;
}

#[tokio::test]
async fn service_commit_stderr_overflow() {
    assert_commit_case(
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::OutputTooLarge),
        Some(CommitErrorCode::OutputTooLarge),
    )
    .await;
}

// A cancellation reported after the mutation already applied must still publish
// the mutated authoritative snapshot exactly once and consume the capability.
// The real cancel-during-spawn signal/reap boundary is retained by
// `trusted_mutation_runner_times_out_cancels_and_reaps_process_groups`.
#[tokio::test]
async fn service_cancel_after_real_add_or_commit_returns_authoritative_state_once() {
    let fixture = command_stub::PolicyFixture::service(ADD_SELECTED);
    fixture.expect_mutation_result(
        "add",
        ADD_MUTATION_ARGS,
        Some(ADD_STAGED),
        Some(GitWorkspaceErrorCode::Cancelled),
        true,
    );
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("cancel add checklist");
    let completion = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.error, Some(CommitErrorCode::Cancelled));
    assert!(completion.prepared.is_none());
    let terminal = completion.workspace.as_ref().expect("cancel add terminal");
    assert_terminal_workspace(&trusted, terminal);
    assert!(terminal.generation > checklist.workspace_generation);
    assert_eq!(fixture.mutation_argv(), expected_add_argv());
    assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
    let duplicate = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await;
    assert_eq!(duplicate.error, Some(CommitErrorCode::StaleAuthority));

    let fixture = command_stub::PolicyFixture::service(COMMIT_STAGED);
    fixture.expect_mutation_result(
        "commit",
        COMMIT_MUTATION_ARGS,
        Some(COMMIT_AFTER),
        Some(GitWorkspaceErrorCode::Cancelled),
        true,
    );
    let (_workspace, trusted) = fixture.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("cancel commit checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("cancel commit prepared");
    let completion = trusted
        .commit(prepared.id, "test: cancel".into(), CancellationToken::new())
        .await;
    assert_eq!(
        completion.outcome,
        CommitOutcome::Failed(CommitErrorCode::Cancelled)
    );
    let terminal = completion
        .workspace
        .as_ref()
        .expect("cancel commit terminal");
    assert_terminal_workspace(&trusted, terminal);
    assert!(terminal.generation > checklist.workspace_generation);
    assert_eq!(fixture.mutation_argv(), expected_commit_argv());
    assert_eq!(fixture.mutation_inputs(), vec![b"test: cancel".to_vec()]);
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
