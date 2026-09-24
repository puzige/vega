use super::*;

#[tokio::test]
async fn commit_proof_uses_explicit_new_oid_for_born_and_unborn_commits() {
    for unborn in [false, true] {
        let fixture = if unborn {
            command_stub::PolicyFixture::policy("sha1-unborn")
        } else {
            command_stub::PolicyFixture::new("staged")
        };
        let base = fixture.raw("head");
        fixture.expect_mutation(
            "commit",
            &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
            if unborn {
                "sha1-first"
            } else {
                "proof-committed"
            },
        );
        let (trusted, prepared) = fixture.prepared().await;
        let before = fixture.requests();
        if unborn {
            assert!(
                !before
                    .iter()
                    .any(|args| args.iter().any(|arg| arg == "ls-tree"))
            );
        }
        let completion = trusted
            .commit(
                prepared.id,
                "test: explicit immutable proof".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert!(completion.workspace.is_some());
        let new_oid = fixture.raw("head");
        assert_ne!(new_oid, base);
        let requests = fixture.requests();
        assert!(requests.iter().any(|args| {
            args.iter().any(|arg| arg == "rev-parse")
                && args
                    .iter()
                    .any(|arg| arg == format!("{new_oid}^@").as_str())
        }));
        assert!(
            requests
                .iter()
                .any(|args| args.iter().any(|arg| arg == "ls-tree")
                    && args.iter().any(|arg| arg == new_oid.as_str())
                    && !args.iter().any(|arg| arg == "HEAD"))
        );
        assert_eq!(
            fixture.mutation_argv(),
            expected_mutation_argv(
                b"commit",
                &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
            )
        );
        assert_eq!(
            fixture.raw("parents").split_whitespace().count(),
            usize::from(!unborn)
        );
        if !unborn {
            assert_eq!(fixture.raw("parents").trim(), base);
        }
        fixture.assert_mutations_drained();
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
    let fixture = command_stub::PolicyFixture::new("staged");
    fixture.proof_plan("root-swap");
    fixture.expect_mutation(
        "commit",
        &["--no-gpg-sign", "--file=-", "--cleanup=verbatim"],
        "proof-committed",
    );
    let (trusted, prepared) = fixture.prepared().await;
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
        fixture.mutation_argv(),
        expected_mutation_argv(
            b"commit",
            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
        )
    );
    assert_eq!(fixture.mutation_inputs().len(), 1);
    let root = fixture.path().to_path_buf();
    fs::remove_dir(&root).unwrap();
    fs::rename(root.with_extension("root-backup"), &root).unwrap();
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
    let fixture = command_stub::PolicyFixture::policy("intent-visible");
    let (workspace, trusted) = fixture.services().await;
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("workspace intent");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::IntentToAdd)
    );
    fixture.set_case("intent-deleted");
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("workspace hidden intent");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::IntentToAdd)
    );
    fixture.assert_no_mutation();
}

#[tokio::test]
async fn trusted_git_rejects_detached_and_operation_state() {
    let fixture = command_stub::PolicyFixture::policy("detached");
    let (_workspace, trusted) = fixture.services().await;
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::UnsafeRepository)
    );
    fixture.assert_no_mutation();
}
