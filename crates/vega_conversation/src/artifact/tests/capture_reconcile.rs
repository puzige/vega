use super::*;

#[tokio::test]
async fn artifact_strict_success_duplicate_and_non_candidates() {
    let repo = Repo::new();
    repo.write("artifact.txt", b"agent\n");
    let (_workspace, service, card) = captured_text_artifact(&repo, 7).await;
    assert_eq!(card.source, ArtifactSource::AgentArtifact);
    assert!(card.current_file_id.is_some());

    let duplicate = service
        .capture(
            &write_call("call-1", "artifact.txt", 6),
            &write_result("call-1", "artifact.txt", 6, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.id, card.id);
    assert_eq!(service.cards().len(), 1);

    let conflict = service
        .capture(
            &write_call("call-1", "other.txt", 1),
            &write_result("call-1", "other.txt", 1, false),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code(), GitWorkspaceErrorCode::ArtifactConflict);
    assert_eq!(service.cards().len(), 1);

    let same_length_different_body = service
        .capture(
            &write_call_with_fingerprint("call-1", "artifact.txt", 6, 'b'),
            &write_result("call-1", "artifact.txt", 6, false),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        same_length_different_body.code(),
        GitWorkspaceErrorCode::ArtifactConflict
    );

    for (call, result) in [
        (
            write_call("checkpoint-call", "artifact.txt", 6),
            write_result("other-call", "artifact.txt", 6, false),
        ),
        (
            write_call("project-call", "artifact.txt", 6),
            write_result_for_scope(
                "other-project",
                THREAD_ID,
                "project-call",
                "artifact.txt",
                6,
                false,
            ),
        ),
        (
            write_call("thread-call", "artifact.txt", 6),
            write_result_for_scope(
                PROJECT_ID,
                "other-thread",
                "thread-call",
                "artifact.txt",
                6,
                false,
            ),
        ),
    ] {
        assert_eq!(
            service
                .capture(&call, &result, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::ArtifactConflict
        );
    }

    assert!(
        service
            .capture(
                &write_call("failed", "artifact.txt", 6),
                &failed_result(),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .is_none()
    );
    for (call_id, status) in [
        ("rejected", ToolCallStatus::Rejected),
        ("cancelled", ToolCallStatus::Cancelled),
    ] {
        assert!(
            service
                .capture(
                    &write_call(call_id, "artifact.txt", 6),
                    &rejected_or_cancelled_result(status),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(
        service
            .capture(
                &write_call("reused", "artifact.txt", 6),
                &write_result("reused", "artifact.txt", 6, true),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .is_none()
    );
    let edit = service
        .capture(
            &edit_call("edit-call", "artifact.txt"),
            &edit_result("edit-call", "artifact.txt", 6),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(edit.source, ArtifactSource::AgentArtifact);
    assert_eq!(service.cards().len(), 2);
    let bash_call = ToolCall {
        id: "bash".to_owned(),
        tool: "bash".to_owned(),
        input_json: r#"{"command":"true"}"#.to_owned(),
    };
    let bash_result = ToolResult {
        status: ToolCallStatus::Success,
        output: String::new(),
        reused: false,
        exit_code: Some(0),
        duration_ms: Some(1),
        truncated: Some(false),
        invalid: None,
    };
    assert!(
        service
            .capture(&bash_call, &bash_result, CancellationToken::new())
            .await
            .unwrap()
            .is_none()
    );
    let read_call = ToolCall {
        id: "read".to_owned(),
        tool: "read".to_owned(),
        input_json: r#"{"path":"artifact.txt"}"#.to_owned(),
    };
    assert!(
        service
            .capture(&read_call, &bash_result, CancellationToken::new())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn artifact_provenance_downgrades_once_and_aba_does_not_upgrade() {
    let repo = Repo::new();
    repo.write("artifact.txt", b"AAAA\n");
    let (workspace, service, card) = captured_text_artifact(&repo, 8).await;
    assert_eq!(card.source, ArtifactSource::AgentArtifact);

    repo.write("artifact.txt", b"BBBB\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let changed = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(changed[0].source, ArtifactSource::WorkspaceChange);

    repo.write("artifact.txt", b"AAAA\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let restored = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(restored[0].source, ArtifactSource::WorkspaceChange);
    assert!(restored[0].current_file_id.is_some());
}

#[tokio::test]
async fn artifact_rename_tracks_raw_path_and_delete_disables_actions() {
    let repo = Repo::new();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nbase\n",
    );
    repo.commit_all();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nagent\n",
    );
    let (workspace, service, card) = captured_text_artifact(&repo, 9).await;
    assert_eq!(card.source, ArtifactSource::AgentArtifact);

    fs::rename(
        repo.path().join("artifact.txt"),
        repo.path().join("renamed.txt"),
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let renamed = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(renamed[0].label, "renamed.txt");
    assert_eq!(renamed[0].source, ArtifactSource::AgentArtifact);
    assert!(renamed[0].current_file_id.is_some());

    fs::remove_file(repo.path().join("renamed.txt")).unwrap();
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let deleted = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(deleted[0].label, "renamed.txt");
    assert!(deleted[0].current_file_id.is_none());
    assert_eq!(deleted[0].source, ArtifactSource::WorkspaceChange);
    assert_eq!(
        service
            .preview(card.id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );

    repo.write("renamed.txt", b"replacement\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let recreated = service.reconcile(CancellationToken::new()).await.unwrap();
    assert!(recreated[0].current_file_id.is_none());
    assert_eq!(recreated[0].source, ArtifactSource::WorkspaceChange);
}

#[tokio::test]
async fn artifact_rename_old_path_collision_never_binds_replacement() {
    let repo = Repo::new();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nbase\n",
    );
    repo.commit_all();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nagent\n",
    );
    let (workspace, service, card) = captured_text_artifact(&repo, 91).await;
    assert_eq!(card.source, ArtifactSource::AgentArtifact);

    git(repo.path(), &["mv", "artifact.txt", "renamed.txt"]);
    repo.write("artifact.txt", b"unrelated replacement\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let reconciled = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(reconciled[0].label, "artifact.txt");
    assert_eq!(reconciled[0].source, ArtifactSource::WorkspaceChange);
    assert!(reconciled[0].current_file_id.is_none());
    assert_eq!(
        service
            .open_in(
                card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    assert_eq!(service.launch_attempts(), 0);
}
