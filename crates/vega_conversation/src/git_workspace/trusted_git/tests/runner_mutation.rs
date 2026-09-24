use super::*;

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
