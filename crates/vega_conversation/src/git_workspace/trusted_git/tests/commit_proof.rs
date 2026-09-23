use super::*;

#[tokio::test]
async fn commit_proof_uses_explicit_new_oid_for_born_and_unborn_commits() {
    for unborn in [false, true] {
        let (repo, _read_dir, _mutation_dir, trusted, prepared, read_log, mutation_argv, base) =
            prepared_with_proof_plan(unborn, "pass").await;
        let completion = trusted
            .commit(
                prepared.id,
                "test: explicit immutable proof".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert!(completion.workspace.is_some());
        let new_oid = run_git_output(repo.path(), &["rev-parse", "HEAD"])
            .strip_suffix(b"\n")
            .expect("new oid newline")
            .to_vec();
        assert_ne!(new_oid, base);
        let invocations = read_invocations(&read_log);
        if unborn {
            assert!(
                !invocations.iter().any(|invocation| {
                    invocation.first().is_some_and(|phase| phase == b"pre")
                        && invocation.iter().any(|arg| arg == b"ls-tree")
                }),
                "unborn A performed a HEAD tree read"
            );
        }
        let mut parent_arg = new_oid.clone();
        parent_arg.extend_from_slice(b"^@");
        assert!(invocations.iter().any(|invocation| {
            invocation.first().is_some_and(|phase| phase == b"post")
                && invocation.iter().any(|arg| arg == b"rev-parse")
                && invocation.iter().any(|arg| arg == &parent_arg)
        }));
        assert!(invocations.iter().any(|invocation| {
            invocation.first().is_some_and(|phase| phase == b"post")
                && invocation.iter().any(|arg| arg == b"ls-tree")
                && invocation.iter().any(|arg| arg == &new_oid)
                && !invocation.iter().any(|arg| arg == b"HEAD")
        }));
        assert_eq!(
            fs::read(mutation_argv).expect("commit argv"),
            expected_mutation_argv(
                b"commit",
                &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
            )
        );
        let parents = run_git_output(repo.path(), &["rev-list", "--parents", "-n", "1", "HEAD"]);
        let fields: Vec<_> = parents
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect();
        assert_eq!(fields.len(), if unborn { 1 } else { 2 });
        if !unborn {
            assert_eq!(fields[1], base);
        }
    }
}

// Keep each fault independently discoverable so nextest can distribute the matrix.
async fn assert_commit_proof_fault_after_one_commit(plan: &str, expected: CommitErrorCode) {
    let fixture = command_stub::PolicyFixture::new("staged");
    fixture.proof_plan(plan);
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "proof-committed",
    );
    let (trusted, prepared) = fixture.prepared().await;
    let completion = trusted
        .commit(
            prepared.id,
            "test: proof must fail closed".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        completion.outcome,
        CommitOutcome::Failed(expected),
        "proof plan {plan}"
    );
    assert!(completion.workspace.is_some(), "{plan} terminal refresh");
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        ),
        "proof plan {plan}"
    );
    assert_eq!(fixture.mutation_inputs().len(), 1, "proof plan {plan}");
    let duplicate = trusted
        .commit(
            prepared.id,
            "test: duplicate proof".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        duplicate.outcome,
        CommitOutcome::Failed(CommitErrorCode::StaleAuthority),
        "proof plan {plan}"
    );
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        )
    );
}

#[tokio::test]
async fn real_git_object_missing_proof_consumes_prepared_after_one_commit() {
    let plan = "object-missing";
    let expected = CommitErrorCode::GitFailed;
    let (_repo, _read_dir, mutation_dir, trusted, prepared, _read_log, mutation_argv, _base) =
        prepared_with_proof_plan(false, plan).await;
    let completion = trusted
        .commit(
            prepared.id,
            "test: proof must fail closed".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        completion.outcome,
        CommitOutcome::Failed(expected),
        "proof plan {plan}"
    );
    assert!(completion.workspace.is_some(), "{plan} terminal refresh");
    assert_eq!(
        fs::read(&mutation_argv).expect("one commit argv"),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        ),
        "proof plan {plan}"
    );
    assert_eq!(
        fs::read(mutation_dir.path().join("mutation-attempts"))
            .expect("one commit process attempt"),
        b"x",
        "proof plan {plan}"
    );
    let duplicate = trusted
        .commit(
            prepared.id,
            "test: duplicate proof".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        duplicate.outcome,
        CommitOutcome::Failed(CommitErrorCode::StaleAuthority),
        "proof plan {plan}"
    );
    assert_eq!(
        fs::read(mutation_argv).expect("still one commit"),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        )
    );
}

#[tokio::test]
async fn commit_proof_rejects_zero_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("zero-parent", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_wrong_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("wrong-parent", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_two_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("two-parent", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_tree_diff_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("tree-diff", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_malformed_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit(
        "malformed-parent",
        CommitErrorCode::MalformedOutput,
    )
    .await;
}

#[tokio::test]
async fn commit_proof_rejects_short_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("short-parent", CommitErrorCode::MalformedOutput)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_mixed_parent_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("mixed-parent", CommitErrorCode::MalformedOutput)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_object_missing_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("object-missing", CommitErrorCode::GitFailed).await;
}

#[tokio::test]
async fn commit_proof_rejects_ref_moved_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("ref-moved", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_ref_deleted_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("ref-deleted", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_ref_renamed_after_one_commit() {
    assert_commit_proof_fault_after_one_commit("ref-renamed", CommitErrorCode::ChangedDuringRead)
        .await;
}

#[tokio::test]
async fn commit_proof_rejects_root_identity_swap_after_exactly_one_commit() {
    let (repo, read_dir, mutation_dir, trusted, prepared, _read_log, mutation_argv, _base) =
        prepared_with_proof_plan(false, "root-swap").await;
    let completion = trusted
        .commit(
            prepared.id,
            "test: root swap".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        completion.outcome,
        CommitOutcome::Failed(CommitErrorCode::ChangedDuringRead)
    );
    assert!(completion.workspace.is_none());
    assert_eq!(
        fs::read(mutation_argv).expect("one root-swap commit"),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        )
    );
    assert_eq!(
        fs::read(mutation_dir.path().join("mutation-attempts"))
            .expect("one root-swap process attempt"),
        b"x"
    );

    let root = repo.path().to_path_buf();
    let backup = read_dir.path().join("root-backup");
    fs::remove_dir(&root).expect("remove exact empty replacement root");
    fs::rename(backup, root).expect("restore exact fixture root");
}

#[tokio::test]
async fn commit_third_capture_mismatch_consumes_prepared_and_spawns_zero_commit() {
    for change in ["status", "index", "ref", "operation"] {
        let fixture = command_stub::PolicyFixture::new("staged");
        let (_workspace, trusted) = fixture.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await
            .prepared
            .expect("prepared through real policy");
        fixture.assert_no_mutation();
        match change {
            "status" => fixture.set_case("status-drift"),
            "index" => fixture.set_case("index-drift"),
            "ref" => fixture.set_case("ref-drift"),
            "operation" => fixture.operation_marker(),
            _ => unreachable!(),
        }
        let completion = trusted
            .commit(
                prepared.id,
                "test: must not execute".into(),
                CancellationToken::new(),
            )
            .await;
        assert!(
            matches!(completion.outcome, CommitOutcome::Failed(_)),
            "{change} drift"
        );
        assert!(completion.workspace.is_some(), "{change} terminal refresh");
        fixture.assert_no_mutation();
        let stale = trusted
            .commit(
                prepared.id,
                "test: duplicate".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            stale.outcome,
            CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
        );
        fixture.assert_no_mutation();
    }
}

#[tokio::test]
async fn commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit() {
    let (repo, _recorder, trusted, prepared, argv, _input) = staged_service_with_recorder().await;
    fs::write(repo.path().join("tracked.txt"), "changed after B\n").expect("status drift");
    let completion = trusted
        .commit(
            prepared.id,
            "test: must not execute".into(),
            CancellationToken::new(),
        )
        .await;
    assert!(
        matches!(completion.outcome, CommitOutcome::Failed(_)),
        "status drift"
    );
    assert!(completion.workspace.is_some(), "status terminal refresh");
    assert!(!argv.exists(), "status drift spawned commit");
    let stale = trusted
        .commit(
            prepared.id,
            "test: duplicate".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        stale.outcome,
        CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
    );
    assert!(!argv.exists(), "duplicate spawned commit");
}

#[tokio::test]
async fn commit_message_byte_bounds_and_exact_stdin_are_enforced() {
    let fixture = command_stub::PolicyFixture::new("staged");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "proof-committed",
    );
    let (trusted, prepared) = fixture.prepared().await;
    for invalid in [
        String::new(),
        "nul\0body".into(),
        "x".repeat(MESSAGE_LIMIT + 1),
    ] {
        let completion = trusted
            .commit(prepared.id, invalid, CancellationToken::new())
            .await;
        assert_eq!(
            completion.outcome,
            CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
        );
        fixture.assert_no_mutation();
    }
    let exact = "x".repeat(MESSAGE_LIMIT);
    let completion = trusted
        .commit(prepared.id, exact.clone(), CancellationToken::new())
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    assert_eq!(fixture.mutation_inputs(), vec![exact.as_bytes().to_vec()]);

    let fixture = command_stub::PolicyFixture::new("staged");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "proof-committed",
    );
    let (trusted, prepared) = fixture.prepared().await;
    let multibyte_exact = "é".repeat(MESSAGE_LIMIT / 2);
    let multibyte_plus_one = format!("{multibyte_exact}x");
    assert_eq!(multibyte_exact.len(), MESSAGE_LIMIT);
    assert_eq!(multibyte_plus_one.len(), MESSAGE_LIMIT + 1);
    let rejected = trusted
        .commit(prepared.id, multibyte_plus_one, CancellationToken::new())
        .await;
    assert_eq!(
        rejected.outcome,
        CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
    );
    fixture.assert_no_mutation();
    let committed = trusted
        .commit(
            prepared.id,
            multibyte_exact.clone(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(committed.outcome, CommitOutcome::Committed);
    assert_eq!(
        fixture.mutation_inputs(),
        vec![multibyte_exact.as_bytes().to_vec()]
    );

    let fixture = command_stub::PolicyFixture::new("staged");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "proof-committed",
    );
    let (trusted, prepared) = fixture.prepared().await;
    let newline_message = "subject\n\nbody\n";
    let committed = trusted
        .commit(
            prepared.id,
            newline_message.into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(committed.outcome, CommitOutcome::Committed);
    assert_eq!(
        fixture.mutation_inputs(),
        vec![newline_message.as_bytes().to_vec()]
    );
}

#[tokio::test]
async fn owned_prepare_accepts_exact_b_published_by_ordinary_poll() {
    let fixture = command_stub::PolicyFixture::service("add-selected");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "add-staged-after",
    );
    let (workspace, trusted) = fixture.services().await;
    let trusted = Arc::new(trusted);
    let a = workspace
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .generation;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let selected = vec![checklist.optional[0].file_id];
    let checklist_id = checklist.id;
    let mut gate = fixture.arm_gate(command_stub::GateTarget::Mutation);
    let worker = tokio::spawn({
        let trusted = trusted.clone();
        async move {
            trusted
                .prepare(checklist_id, selected, CancellationToken::new())
                .await
        }
    });
    gate.wait_entered().await;
    let observed_b = workspace
        .refresh(CancellationToken::new())
        .await
        .expect("ordinary B poll");
    assert_ne!(observed_b.generation, a);
    gate.release();
    let completion = worker.await.expect("prepare task");
    assert!(completion.prepared.is_some());
    assert_eq!(
        completion
            .workspace
            .as_ref()
            .map(|snapshot| snapshot.generation),
        Some(observed_b.generation)
    );
    let after = workspace
        .refresh(CancellationToken::new())
        .await
        .expect("post completion poll");
    assert_eq!(after.generation, observed_b.generation);
    assert_eq!(
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"add",
            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
        )
    );
    assert_eq!(fixture.mutation_inputs(), vec![b"tracked.txt\0".to_vec()]);
}

#[tokio::test]
async fn owned_prepare_rejects_a_to_b_to_a_without_capability() {
    let fixture = command_stub::PolicyFixture::service("add-selected");
    fixture.expect_mutation(
        "add",
        &["-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        "add-staged-after",
    );
    let (workspace, trusted) = fixture.services().await;
    let trusted = Arc::new(trusted);
    let a = workspace
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .generation;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let selected = vec![checklist.optional[0].file_id];
    let checklist_id = checklist.id;
    let mut gate = fixture.arm_gate(command_stub::GateTarget::Mutation);
    let worker = tokio::spawn({
        let trusted = trusted.clone();
        async move {
            trusted
                .prepare(checklist_id, selected, CancellationToken::new())
                .await
        }
    });
    gate.wait_entered().await;
    let b = workspace
        .refresh(CancellationToken::new())
        .await
        .expect("B poll");
    assert_ne!(b.generation, a);
    fixture.set_case("add-selected");
    let aba = workspace
        .refresh(CancellationToken::new())
        .await
        .expect("ABA poll");
    assert_ne!(aba.generation, b.generation);
    gate.release();
    let completion = worker.await.expect("prepare task");
    assert!(completion.prepared.is_none());
    assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    assert!(completion.workspace.is_some());
}

#[tokio::test]
async fn trusted_git_rejects_intent_to_add_and_hidden_delete_form() {
    let repo = Repo::new();
    fs::write(repo.path().join("intent.txt"), "intent\n").expect("intent");
    run_git(repo.path(), &["add", "-N", "intent.txt"]);
    let (workspace, trusted) = repo.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("workspace intent");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::IntentToAdd)
    );
    fs::remove_file(repo.path().join("intent.txt")).expect("remove intent");
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("workspace hidden intent");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::IntentToAdd)
    );
}

#[tokio::test]
async fn trusted_git_rejects_detached_and_operation_state() {
    let repo = Repo::new();
    run_git(repo.path(), &["checkout", "--detach", "-q"]);
    let (_workspace, trusted) = repo.services().await;
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::UnsafeRepository)
    );
}
