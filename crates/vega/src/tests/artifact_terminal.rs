use super::*;

pub(crate) fn artifact_controller_repo() -> TempDir {
    let repo = tempfile::tempdir().expect("fresh artifact controller repo");
    run_fixture_git(repo.path(), &["init", "-q"]);
    run_fixture_git(
        repo.path(),
        &["config", "--local", "user.name", "Vega Test"],
    );
    run_fixture_git(
        repo.path(),
        &["config", "--local", "user.email", "vega@example.invalid"],
    );
    fs::write(repo.path().join("base.txt"), "base\n").expect("artifact fixture base");
    run_fixture_git(repo.path(), &["add", "--", "base.txt"]);
    run_fixture_git(repo.path(), &["commit", "-q", "-m", "base"]);
    repo
}

pub(crate) fn artifact_write_call(call_id: &str, path: &str, bytes: u64) -> ToolCall {
    ToolCall {
        id: call_id.to_owned(),
        tool: "write".to_owned(),
        input_json: format!(
            r#"{{"audit_version":"write_edit_v1","tool":"write","path":"{path}","content_bytes":{bytes},"fingerprint_v1":"{}"}}"#,
            "a".repeat(64)
        ),
    }
}

pub(crate) fn artifact_write_result(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    path: &str,
    bytes: u64,
    reused: bool,
) -> ToolResult {
    ToolResult {
        status: vega_conversation::types::ToolCallStatus::Success,
        output: vega_tools::WriteSuccessOutput {
            path: path.to_owned(),
            bytes_written: bytes,
            checkpoint_ref: vega_tools::CheckpointIds::new(project_id, thread_id, call_id)
                .expect("artifact checkpoint ids")
                .checkpoint_ref(),
        }
        .to_json()
        .expect("artifact result JSON"),
        reused,
        exit_code: None,
        duration_ms: None,
        truncated: (!reused).then_some(false),
        invalid: None,
    }
}

pub(crate) fn receive_artifact_terminal(
    workspace: Arc<GitWorkspaceService>,
    service: Arc<ArtifactService>,
    job: ArtifactTerminalJob,
) -> Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode> {
    let (sender, receiver) = mpsc::sync_channel(1);
    run_artifact_terminal_worker(
        workspace,
        service,
        job,
        tokio_util::sync::CancellationToken::new(),
        sender,
    );
    receiver.recv().expect("artifact terminal result")
}

pub(crate) fn artifact_capture_work(
    service: &ArtifactService,
    call: ToolCall,
    result: ToolResult,
) -> ArtifactTerminalWork {
    let call_id = call.id.clone();
    let candidate = service
        .prepare_capture(&call, &result)
        .expect("strict artifact terminal")
        .expect("eligible artifact terminal");
    ArtifactTerminalWork::Capture { call_id, candidate }
}

#[test]
fn artifact_controller_terminal_refresh_captures_and_bash_reconciles_downgrade() {
    const PROJECT: &str = "artifact-project";
    const THREAD: &str = "artifact-thread";
    let repo = artifact_controller_repo();
    fs::write(repo.path().join("artifact.txt"), "agent\n").expect("agent artifact body");
    let workspace =
        Arc::new(GitWorkspaceService::new(repo.path()).expect("artifact controller workspace"));
    let service = Arc::new(
        ArtifactService::new(workspace.clone(), PROJECT.into(), THREAD.into(), 1)
            .expect("artifact controller service"),
    );
    let first = receive_artifact_terminal(
        workspace.clone(),
        service.clone(),
        ArtifactTerminalJob {
            sequence: 1,
            work: artifact_capture_work(
                &service,
                artifact_write_call("write-1", "artifact.txt", 6),
                artifact_write_result(PROJECT, THREAD, "write-1", "artifact.txt", 6, false),
            ),
        },
    )
    .expect("strict terminal capture");
    let (_, captured) = first.1.captured.expect("eligible terminal card");
    assert_eq!(
        captured.source,
        vega_conversation::types::ArtifactSource::AgentArtifact
    );
    assert!(captured.preview_available);

    fs::write(repo.path().join("artifact.txt"), "human\n").expect("later workspace mutation");
    let reconciled = receive_artifact_terminal(
        workspace,
        service,
        ArtifactTerminalJob {
            sequence: 2,
            work: ArtifactTerminalWork::Refresh,
        },
    )
    .expect("bash terminal reconciliation");
    assert!(reconciled.1.captured.is_none());
    assert_eq!(reconciled.1.cards.len(), 1);
    assert_eq!(
        reconciled.1.cards[0].source,
        vega_conversation::types::ArtifactSource::WorkspaceChange
    );
}

#[gpui::test]
async fn artifact_controller_real_batch_pairing_conflict_overflow_and_route_cancel(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    let store = Store::open(":memory:").expect("artifact window memory store");
    store.migrate().expect("artifact window migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 artifact root"),
        "artifact",
        None,
    )
    .expect("artifact project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("artifact thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let route_cancel = root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("artifact route");
        root.artifact_controller
            .active
            .as_mut()
            .expect("active artifact route")
            .agent_generation = Some(1);
        root.artifact_controller
            .active
            .as_ref()
            .expect("active artifact route")
            .cancel
            .clone()
    });
    let original = artifact_write_call("reused-id", "artifact.txt", 6);
    let conflicting = artifact_write_call("reused-id", "other.txt", 1);
    root.update(cx, |root, cx| {
        // This is the same ordering as the real AgentBatch loop: observe
        // before ownership moves into ConversationStream.
        root.observe_artifact_event(
            1,
            &stream,
            &ConversationEvent::ToolCallProposed {
                call: original.clone(),
            },
            cx,
        );
        root.observe_artifact_event(
            1,
            &stream,
            &ConversationEvent::ToolCallProposed { call: conflicting },
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .and_then(|active| active.proposals.get("reused-id"))
                .is_some_and(|proposal| proposal.call.is_none()),
            "a reused id with different safe audit data is corrupt"
        );
        root.artifact_controller
            .active
            .as_mut()
            .expect("active artifact route")
            .terminal_sequence = u64::MAX;
        root.observe_artifact_event(
            1,
            &stream,
            &ConversationEvent::ToolCallFinished {
                call_id: "reused-id".into(),
                result: artifact_write_result(
                    &thread.project_id,
                    &thread.id,
                    "reused-id",
                    "artifact.txt",
                    6,
                    false,
                ),
            },
            cx,
        );
        assert!(root.artifact_controller.active.is_none());
    });
    assert!(route_cancel.is_cancelled(), "checked overflow closes route");

    fs::write(repo.path().join("artifact.txt"), "agent\n")
        .expect("first conflicting artifact body");
    fs::write(repo.path().join("other.txt"), "x").expect("second conflicting artifact body");
    let first_call = artifact_write_call("fifo-conflict", "artifact.txt", 6);
    let second_call = artifact_write_call("fifo-conflict", "other.txt", 1);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: first_call.clone(),
            },
            cx,
        )
    });
    let (identity, workspace, service, first_job, second_job, conflict_cancel) =
        root.update(cx, |root, _| {
            let identity = root
                .artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("replacement artifact route");
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("conflict artifact route");
            let first_job = ArtifactTerminalJob {
                sequence: 1,
                work: artifact_capture_work(
                    &active.service,
                    first_call.clone(),
                    artifact_write_result(
                        &thread.project_id,
                        &thread.id,
                        "fifo-conflict",
                        "artifact.txt",
                        6,
                        false,
                    ),
                ),
            };
            let second_job = ArtifactTerminalJob {
                sequence: 2,
                work: artifact_capture_work(
                    &active.service,
                    second_call,
                    artifact_write_result(
                        &thread.project_id,
                        &thread.id,
                        "fifo-conflict",
                        "other.txt",
                        1,
                        false,
                    ),
                ),
            };
            active.terminal_in_flight = Some(1);
            (
                identity,
                active.workspace.clone(),
                active.service.clone(),
                first_job,
                second_job,
                active.cancel.clone(),
            )
        });
    let first_result = receive_artifact_terminal(workspace, service, first_job)
        .expect("first FIFO candidate capture");
    let card = root.update(cx, |root, cx| {
        root.finish_artifact_terminal(&identity, Ok(first_result), cx);
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("route after first FIFO capture");
        active.terminal_queue.push_back(second_job);
        active.terminal_queue.push_back(ArtifactTerminalJob {
            sequence: 3,
            work: ArtifactTerminalWork::Refresh,
        });
        active
            .cards
            .values()
            .next()
            .cloned()
            .expect("first FIFO card inserted adjacent to tool")
    });
    let ArtifactTerminalDispatch {
        identity: next_identity,
        workspace,
        service,
        job,
        cancel,
    } = root
        .update(cx, |root, _| root.take_next_artifact_terminal())
        .expect("production FIFO takes conflict before refresh");
    assert_eq!(job.sequence, 2);
    let (sender, receiver) = mpsc::sync_channel(1);
    run_artifact_terminal_worker(workspace, service, job, cancel, sender);
    let conflict = receiver.recv().expect("FIFO conflict worker result");
    assert!(matches!(
        conflict,
        Err(GitWorkspaceErrorCode::ArtifactConflict)
    ));
    root.update(cx, |root, cx| {
        assert_eq!(
            root.artifact_controller
                .active
                .as_ref()
                .expect("route before conflict completion")
                .terminal_queue
                .len(),
            1,
            "later FIFO work remains queued until conflict closes the route"
        );
        root.finish_artifact_terminal(&next_identity, conflict, cx);
    });
    assert!(conflict_cancel.is_cancelled());
    assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
    assert!(card.read_with(cx, |card, _| {
        card.projection().current_file_id.is_none()
            && !card.projection().preview_available
            && card.inline_error_code() == Some(GitWorkspaceErrorCode::ArtifactConflict)
    }));
    let cap_cancel = root.update(cx, |root, cx| {
        root.artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("proposal cap route");
        root.artifact_controller
            .active
            .as_mut()
            .expect("proposal cap active route")
            .agent_generation = Some(1);
        let id = "i".repeat(120);
        let exact = ToolCall {
            input_json: "x".repeat(64 * 1024 - id.len() - "write".len()),
            id,
            tool: "write".into(),
        };
        root.observe_artifact_event(
            1,
            &stream,
            &ConversationEvent::ToolCallProposed {
                call: exact.clone(),
            },
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.proposals.contains_key(&exact.id))
        );
        let mut plus_one = exact;
        plus_one.input_json.push('x');
        let cancel = root
            .artifact_controller
            .active
            .as_ref()
            .expect("proposal cap route before plus one")
            .cancel
            .clone();
        root.observe_artifact_event(
            1,
            &stream,
            &ConversationEvent::ToolCallProposed { call: plus_one },
            cx,
        );
        cancel
    });
    assert!(cap_cancel.is_cancelled());
    assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
    root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream, repo.path().to_path_buf())
            .expect("settings artifact route");
    });
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
}
