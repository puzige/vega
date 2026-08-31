#[allow(unused_imports)]
use super::*;

#[gpui::test]
async fn artifact_controller_agent_batch_generation_orphans_are_content_free_refreshes(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    let store = Store::open(":memory:").expect("artifact generation store");
    store.migrate().expect("artifact generation migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 artifact root"),
        "artifact",
        None,
    )
    .expect("artifact generation project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("artifact generation thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("artifact generation route");
    });
    let generation_a = root.update(cx, |root, _| {
        let (generation, _) =
            root.agent_controller
                .begin(thread.id.clone(), stream.clone(), None, None);
        root.begin_artifact_agent_generation(generation, &stream);
        generation
    });
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::Event(ConversationEvent::ToolCallProposed {
            call: artifact_write_call("same-id", "artifact.txt", 6),
        }))
        .expect("orphan proposal");
    sender
        .send(AgentUpdate::Finished(false))
        .expect("orphan terminal");
    let batch = drain_agent_updates(&receiver);
    assert!(root.update(cx, |root, cx| matches!(
        root.apply_agent_batch_ingress(generation_a, &thread.id, &stream, batch, cx),
        AgentBatchIngress::Finished { success: false, .. }
    )));

    let generation_b = root.update(cx, |root, _| {
        let (generation, _) =
            root.agent_controller
                .begin(thread.id.clone(), stream.clone(), None, None);
        root.begin_artifact_agent_generation(generation, &stream);
        root.artifact_controller
            .active
            .as_mut()
            .expect("active generation route")
            .terminal_in_flight = Some(999);
        generation
    });
    assert!(root.update(cx, |root, cx| matches!(
        root.apply_agent_batch_ingress(
            generation_a,
            &thread.id,
            &stream,
            AgentBatch {
                events: vec![ConversationEvent::ToolCallProposed {
                    call: artifact_write_call("stale-generation", "artifact.txt", 6),
                }],
                finished: None,
            },
            cx,
        ),
        AgentBatchIngress::Stale
    )));
    assert!(root.read_with(cx, |root, _| {
        root.artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| active.proposals.is_empty())
    }));
    let huge_output = "sensitive unrelated output".repeat(100_000);
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
            call_id: "same-id".into(),
            result: ToolResult {
                status: vega_conversation::types::ToolCallStatus::Success,
                output: huge_output,
                reused: false,
                exit_code: None,
                duration_ms: None,
                truncated: Some(false),
                invalid: None,
            },
        }))
        .expect("later same-id terminal");
    let batch = drain_agent_updates(&receiver);
    root.update(cx, |root, cx| {
        assert!(matches!(
            root.apply_agent_batch_ingress(generation_b, &thread.id, &stream, batch, cx),
            AgentBatchIngress::Running
        ));
        let active = root
            .artifact_controller
            .active
            .as_ref()
            .expect("active artifact generation");
        assert!(matches!(
            active.terminal_queue.back().map(|job| &job.work),
            Some(ArtifactTerminalWork::Refresh)
        ));
        assert!(active.service.cards().is_empty());
    });

    assert!(root.update(cx, |root, cx| matches!(
        root.apply_agent_batch_ingress(
            generation_b,
            &thread.id,
            &stream,
            AgentBatch {
                events: Vec::new(),
                finished: Some(false),
            },
            cx,
        ),
        AgentBatchIngress::Finished { success: false, .. }
    )));
    let generation_c = root.update(cx, |root, _| {
        let (generation, _) =
            root.agent_controller
                .begin(thread.id.clone(), stream.clone(), None, None);
        root.begin_artifact_agent_generation(generation, &stream);
        generation
    });
    root.update(cx, |root, cx| {
        assert!(matches!(
            root.apply_agent_batch_ingress(
                generation_c,
                &thread.id,
                &stream,
                AgentBatch {
                    events: vec![ConversationEvent::ToolCallProposed {
                        call: artifact_write_call("cancelled-id", "artifact.txt", 6),
                    }],
                    finished: None,
                },
                cx,
            ),
            AgentBatchIngress::Running
        ));
        root.cancel_active_agent(cx);
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.proposals.is_empty())
        );
        assert!(matches!(
            root.apply_agent_batch_ingress(
                generation_c,
                &thread.id,
                &stream,
                AgentBatch {
                    events: Vec::new(),
                    finished: Some(false),
                },
                cx,
            ),
            AgentBatchIngress::Finished { success: false, .. }
        ));
    });
    let generation_d = root.update(cx, |root, _| {
        let (generation, _) =
            root.agent_controller
                .begin(thread.id.clone(), stream.clone(), None, None);
        root.begin_artifact_agent_generation(generation, &stream);
        generation
    });
    root.update(cx, |root, cx| {
        assert!(matches!(
            root.apply_agent_batch_ingress(
                generation_d,
                &thread.id,
                &stream,
                AgentBatch {
                    events: vec![ConversationEvent::ToolCallFinished {
                        call_id: "cancelled-id".into(),
                        result: artifact_write_result(
                            &thread.project_id,
                            &thread.id,
                            "cancelled-id",
                            "artifact.txt",
                            6,
                            false,
                        ),
                    }],
                    finished: None,
                },
                cx,
            ),
            AgentBatchIngress::Running
        ));
        let active = root
            .artifact_controller
            .active
            .as_ref()
            .expect("active cancelled replacement generation");
        assert!(matches!(
            active.terminal_queue.back().map(|job| &job.work),
            Some(ArtifactTerminalWork::Refresh)
        ));
        assert!(active.service.cards().is_empty());
    });
}

#[gpui::test]
async fn artifact_controller_preview_open_latest_stale_and_max_fences(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    let late_branch_repo = artifact_controller_repo();
    run_fixture_git(
        late_branch_repo.path(),
        &["branch", "late-branch-callback-target"],
    );
    fs::write(repo.path().join("artifact.txt"), "agent\n").expect("preview artifact body");
    let store = Store::open(":memory:").expect("artifact fence memory store");
    store.migrate().expect("artifact fence migrations");
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
    let workspace =
        Arc::new(GitWorkspaceService::new(repo.path()).expect("artifact fence workspace"));
    let service = Arc::new(
        ArtifactService::new(
            workspace.clone(),
            thread.project_id.clone(),
            thread.id.clone(),
            1,
        )
        .expect("artifact fence service"),
    );
    let terminal = receive_artifact_terminal(
        workspace.clone(),
        service.clone(),
        ArtifactTerminalJob {
            sequence: 1,
            work: artifact_capture_work(
                &service,
                artifact_write_call("write-1", "artifact.txt", 6),
                artifact_write_result(
                    &thread.project_id,
                    &thread.id,
                    "write-1",
                    "artifact.txt",
                    6,
                    false,
                ),
            ),
        },
    )
    .expect("artifact fence capture");
    let (_, projection) = terminal.1.captured.expect("artifact fence card");
    let file_id = projection.current_file_id.expect("current artifact file");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("artifact preview runtime");
    let preview = runtime
        .block_on(service.preview(projection.id, tokio_util::sync::CancellationToken::new()))
        .expect("artifact preview");

    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let branch_identity = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("artifact test branch route");
        active.identity.clone()
    });
    let branch_service = Arc::new(
        BranchWorkspaceService::new(late_branch_repo.path())
            .expect("artifact test clean branch service"),
    );
    let (branch_sender, branch_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        branch_service,
        BranchListFence {
            route: branch_identity.clone(),
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        branch_sender,
    );
    let branch_snapshot = branch_receiver
        .recv()
        .expect("artifact test branch list")
        .1
        .expect("artifact test branch snapshot");
    let late_branch_id = branch_snapshot
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("artifact test branch target")
        .id;
    let late_operation = branch_identity.selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(branch_snapshot.clone(), cx));
        selector
            .begin_switch(branch_snapshot.generation, late_branch_id, cx)
            .expect("artifact test late operation")
    });
    let late_branch_fence = BranchSwitchFence {
        route: branch_identity,
        sequence: 1,
        snapshot_generation: branch_snapshot.generation,
        branch_id: late_branch_id,
        operation_id: late_operation,
        lease: TrustedActionToken {
            generation: 1,
            kind: TrustedActionKind::BranchSwitch,
            owner_epoch: 1,
            request_sequence: 1,
        },
    };
    root.update(cx, |root, _| {
        root.branch_controller
            .active
            .as_mut()
            .expect("artifact test active branch route")
            .switch_fence = Some(late_branch_fence.clone());
        assert!(root.branch_controller.claim_terminal(&late_branch_fence));
    });
    let card = cx.new(|cx| {
        ArtifactCard::new(
            thread.id.clone(),
            thread.project_id.clone(),
            projection.clone(),
            cx,
        )
    });
    assert!(!stream.update(cx, |stream, cx| {
        stream.apply_artifact_card("missing-call", card.clone(), cx)
    }));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant-before-artifact".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: artifact_write_call("write-1", "artifact.txt", 6),
            },
            cx,
        );
        assert!(stream.apply_artifact_card("write-1", card.clone(), cx));
        assert!(
            stream.apply_artifact_card("write-1", card.clone(), cx),
            "an identical duplicate is idempotent"
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: artifact_write_call("later-tool", "later.txt", 1),
            },
            cx,
        );
        assert!(stream.artifact_card_is_adjacent("write-1"));
    });
    let route = root.update(cx, |root, _| {
        let route = root
            .artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("artifact fence route");
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("active artifact fence route");
        active.workspace = workspace;
        active.service = service;
        active.cards.insert(projection.id, card.clone());
        route
    });
    let older_preview = ArtifactPreviewFence {
        route: route.clone(),
        sequence: 1,
        card_id: projection.id,
        file_id,
    };
    let latest_preview = ArtifactPreviewFence {
        sequence: 2,
        ..older_preview.clone()
    };
    let rows_before_preview = stream.read_with(cx, |stream, cx| stream.virtual_row_count(cx));
    let expected_preview_rows = preview.text().split_inclusive('\n').count();
    root.update(cx, |root, cx| {
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("active artifact preview fence");
        active.preview_sequence = 2;
        active.preview_fence = Some(latest_preview.clone());
        root.finish_branch_switch(
            late_branch_fence.clone(),
            BranchSwitchCompletion {
                outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                snapshot: None,
            },
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.preview_fence.as_ref() == Some(&latest_preview)),
            "old duplicate branch terminal cannot clear fresh preview fence"
        );
        root.finish_artifact_preview(older_preview, Ok(preview.clone()), cx);
        assert_eq!(card.read(cx).row_count(), 2, "stale preview is dropped");
        root.finish_artifact_preview(latest_preview, Ok(preview), cx);
        assert!(card.read(cx).row_count() > 2, "latest preview is applied");
        assert_eq!(
            stream.read(cx).virtual_row_count(cx),
            rows_before_preview + expected_preview_rows
        );
    });

    let older_open = ArtifactOpenFence {
        route: route.clone(),
        sequence: 1,
        card_id: projection.id,
        file_id,
        target: OpenInTarget::VisualStudioCode,
        lease: TrustedActionToken {
            generation: 1,
            kind: TrustedActionKind::ArtifactOpen,
            owner_epoch: route.epoch,
            request_sequence: 1,
        },
    };
    let latest_open = ArtifactOpenFence {
        sequence: 2,
        ..older_open.clone()
    };
    card.update(cx, |card, cx| {
        card.set_opening(Some(OpenInTarget::VisualStudioCode), cx)
    });
    root.update(cx, |root, cx| {
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("active artifact open fence");
        active.open_sequence = 2;
        active.open_fence = Some(latest_open.clone());
        root.finish_branch_switch(
            late_branch_fence,
            BranchSwitchCompletion {
                outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                snapshot: None,
            },
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.open_fence.as_ref() == Some(&latest_open)),
            "old duplicate branch terminal cannot clear fresh open fence"
        );
        root.finish_artifact_open(
            older_open,
            Ok(OpenInOutcome {
                card_id: projection.id,
                target: OpenInTarget::VisualStudioCode,
            }),
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.open_fence.as_ref() == Some(&latest_open)),
            "stale open completion cannot release the latest fence"
        );
        root.finish_artifact_open(
            latest_open,
            Ok(OpenInOutcome {
                card_id: projection.id,
                target: OpenInTarget::VisualStudioCode,
            }),
            cx,
        );
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.open_fence.is_none()),
            "latest open completion is accepted"
        );
    });

    let terminal_cancel = tokio_util::sync::CancellationToken::new();
    card.update(cx, |card, cx| {
        card.set_opening(Some(OpenInTarget::Terminal), cx)
    });
    root.update(cx, |root, cx| {
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("active terminal cancellation route");
        active.agent_generation = Some(7);
        active.terminal_in_flight = Some(999);
        active.open_cancel = Some(terminal_cancel.clone());
        active.open_fence = Some(ArtifactOpenFence {
            route: route.clone(),
            sequence: 3,
            card_id: projection.id,
            file_id,
            target: OpenInTarget::Terminal,
            lease: TrustedActionToken {
                generation: 3,
                kind: TrustedActionKind::ArtifactOpen,
                owner_epoch: route.epoch,
                request_sequence: 3,
            },
        });
        root.observe_artifact_event(
            7,
            &route.stream,
            &ConversationEvent::ToolCallFinished {
                call_id: "bash-terminal".into(),
                result: ToolResult {
                    status: vega_conversation::types::ToolCallStatus::Success,
                    output: "unrelated raw output".repeat(100_000),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        let active = root
            .artifact_controller
            .active
            .as_ref()
            .expect("terminal cancellation keeps route");
        assert!(active.open_fence.is_none());
        assert!(matches!(
            active.terminal_queue.back().map(|job| &job.work),
            Some(ArtifactTerminalWork::Refresh)
        ));
    });
    assert!(terminal_cancel.is_cancelled());
    assert_eq!(card.read_with(cx, |card, _| card.row_count()), 3);

    let open_starts = ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst);
    card.update(cx, |card, cx| {
        card.set_opening(Some(OpenInTarget::Cursor), cx)
    });
    let cancel = root.update(cx, |root, cx| {
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("active artifact max fence");
        active.open_sequence = u64::MAX;
        let cancel = active.cancel.clone();
        root.request_artifact_open(
            card.clone(),
            &ArtifactOpenRequested {
                thread_id: route.thread_id.clone(),
                project_id: route.project_id.clone(),
                card_id: projection.id,
                file_id,
                target: OpenInTarget::Cursor,
            },
            cx,
        );
        cancel
    });
    assert!(cancel.is_cancelled());
    assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
    assert_eq!(
        ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        open_starts,
        "checked overflow cannot start an Open worker"
    );
    assert_eq!(card.read_with(cx, |card, _| card.row_count()), 3);
    assert!(card.read_with(cx, |card, _| card.projection().current_file_id.is_none()));

    let removed_card = cx.new(|cx| {
        ArtifactCard::new(
            thread.id.clone(),
            thread.project_id.clone(),
            projection.clone(),
            cx,
        )
    });
    let removed_cancel = root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("selected-project route");
        let active = root
            .artifact_controller
            .active
            .as_mut()
            .expect("selected-project active route");
        active.cards.insert(projection.id, removed_card.clone());
        active.cancel.clone()
    });
    cx.update(|cx| {
        cx.set_global(vega_ui::sidebar::SelectedProject(None));
    });
    cx.run_until_parked();
    assert!(removed_cancel.is_cancelled());
    assert!(removed_card.read_with(cx, |card, _| card.projection().current_file_id.is_none()));
    removed_card.update(cx, |card, cx| card.set_opening(Some(OpenInTarget::Zed), cx));
    root.update(cx, |root, cx| {
        root.request_artifact_open(
            removed_card.clone(),
            &ArtifactOpenRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id,
                target: OpenInTarget::Zed,
            },
            cx,
        );
    });
    assert_eq!(
        ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        open_starts,
        "removed project cannot start an Open worker"
    );
    assert_eq!(removed_card.read_with(cx, |card, _| card.row_count()), 3);

    let active_none_card = cx.new(|cx| {
        ArtifactCard::new(
            thread.id.clone(),
            thread.project_id.clone(),
            projection.clone(),
            cx,
        )
    });
    let preview_starts = ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst);
    root.update(cx, |root, cx| {
        root.request_artifact_preview(
            active_none_card.clone(),
            &ArtifactPreviewRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id,
            },
            cx,
        );
        root.request_artifact_open(
            active_none_card.clone(),
            &ArtifactOpenRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id,
                target: OpenInTarget::DefaultApplication,
            },
            cx,
        );
        root.request_artifact_open(
            active_none_card.clone(),
            &ArtifactOpenRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id,
                target: OpenInTarget::DefaultApplication,
            },
            cx,
        );
    });
    assert!(active_none_card.read_with(cx, |card, _| {
        card.projection().current_file_id.is_none()
            && !card.projection().preview_available
            && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
    }));
    assert_eq!(
        ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        preview_starts
    );
    assert_eq!(
        ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        open_starts
    );

    cx.update(|cx| {
        cx.set_global(vega_ui::sidebar::SelectedProject(Some(
            thread.project_id.clone(),
        )));
    });
    let owned_card = cx.new(|cx| {
        ArtifactCard::new(
            thread.id.clone(),
            thread.project_id.clone(),
            projection.clone(),
            cx,
        )
    });
    let foreign_card = cx.new(|cx| {
        ArtifactCard::new(
            thread.id.clone(),
            thread.project_id.clone(),
            projection.clone(),
            cx,
        )
    });
    root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream, repo.path().to_path_buf())
            .expect("ownership mismatch route");
        root.artifact_controller
            .active
            .as_mut()
            .expect("ownership mismatch active route")
            .cards
            .insert(projection.id, owned_card.clone());
    });
    root.update(cx, |root, cx| {
        root.request_artifact_open(
            foreign_card.clone(),
            &ArtifactOpenRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id,
                target: OpenInTarget::RevealInFinder,
            },
            cx,
        );
    });
    assert!(foreign_card.read_with(cx, |card, _| {
        card.projection().current_file_id.is_none()
            && !card.projection().preview_available
            && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
    }));

    let (_, other_snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
    let mismatched_file_id = other_snapshot.files[0].id;
    assert_ne!(mismatched_file_id, file_id);
    root.update(cx, |root, cx| {
        root.request_artifact_preview(
            owned_card.clone(),
            &ArtifactPreviewRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                card_id: projection.id,
                file_id: mismatched_file_id,
            },
            cx,
        );
    });
    assert!(owned_card.read_with(cx, |card, _| {
        card.projection().current_file_id.is_none()
            && !card.projection().preview_available
            && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
    }));
    assert_eq!(
        ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        preview_starts
    );
    assert_eq!(
        ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
        open_starts
    );
}
