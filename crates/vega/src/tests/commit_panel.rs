#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
enum CapturedCommitEvent {
    Prepare(CommitPrepareRequested),
    Draft(CommitDraftRequested),
    Commit(CommitRequested),
    Close,
}
#[gpui::test]
async fn commit_panel_accepts_canonical_mixed_staged_and_unstaged_identity(
    cx: &mut gpui::TestAppContext,
) {
    let repo = diff_controller_repo();
    run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
    fs::write(
        repo.path().join("tracked.rs"),
        "fn base() {}\nfn changed() {}\nfn later() {}\n",
    )
    .expect("mixed worktree update");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service = TrustedGitService::new(repo.path(), workspace.clone()).expect("service");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let checklist = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("mixed checklist")
    });
    assert_eq!(checklist.staged.len(), 1);
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.staged[0].file_id, checklist.optional[0].file_id);
    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    panel.update(cx, |panel, cx| {
        assert!(panel.request_open(cx));
        assert!(panel.apply_checklist(checklist, cx));
    });
}

#[gpui::test]
async fn commit_panel_real_key_handlers_are_scoped_and_first_wins(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        vega_ui::init(cx);
    });
    let repo = diff_controller_repo();
    run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
    fs::write(repo.path().join("optional.rs"), "fn optional() {}\n").expect("optional fixture");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service =
        Arc::new(TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let checklist = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("checklist")
    });
    assert!(!checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 1);

    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    let events = Arc::new(Mutex::new(Vec::<CapturedCommitEvent>::new()));
    let window_events = events.clone();
    let root = panel.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                let events_prepare = window_events.clone();
                let events_draft = window_events.clone();
                let events_commit = window_events.clone();
                let events_close = window_events.clone();
                cx.new(|cx| {
                    cx.subscribe(&root, move |_, _, event: &CommitPrepareRequested, _| {
                        events_prepare
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Prepare(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, event: &CommitDraftRequested, _| {
                        events_draft
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Draft(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, event: &CommitRequested, _| {
                        events_commit
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Commit(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, _event: &CommitPanelClosed, _| {
                        events_close
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Close);
                    })
                    .detach();
                    CommitPanelHarness { panel: root }
                })
            })
        })
        .expect("commit key window");
    window
        .update(cx, |_, window, cx| {
            assert!(panel.update(cx, |panel, cx| panel.request_open(cx)));
            assert!(panel.update(cx, |panel, cx| {
                panel.apply_checklist(checklist.clone(), cx)
            }));
            let focus = panel.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        })
        .expect("open checklist");

    // Space at Cancel is inert; Tab skips the forced staged row and lands
    // on the sole optional worktree row.
    cx.simulate_keystrokes(window.into(), "space tab space cmd-enter cmd-enter");
    let prepare = events
        .lock()
        .expect("events")
        .iter()
        .filter_map(|event| match event {
            CapturedCommitEvent::Prepare(request) => Some(request.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(prepare.len(), 1, "prepare is exact first-wins");
    assert_eq!(prepare[0].selected.len(), 1, "optional Space toggles once");
    let completion = runtime.block_on(service.prepare(
        prepare[0].snapshot_id,
        prepare[0].selected.clone(),
        tokio_util::sync::CancellationToken::new(),
    ));
    let prepared = completion.prepared.expect("prepared authority");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_prepare(prepare[0].operation_id, Ok(prepared.clone()), cx)
    }));
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.focused_control()),
        vega_ui::commit_panel::CommitPanelFocus::Cancel
    );

    // Editor Enter remains newline and emits no draft. Generate Enter and
    // Space each emit exactly once; repeating the same key while pending
    // cannot duplicate the operation.
    cx.simulate_keystrokes(window.into(), "tab enter");
    assert!(
        panel
            .read_with(cx, |panel, cx| panel.commit_message(cx))
            .contains('\n')
    );
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        0
    );
    cx.simulate_keystrokes(window.into(), "tab enter enter space");
    let first_draft = events
        .lock()
        .expect("events")
        .iter()
        .find_map(|event| match event {
            CapturedCommitEvent::Draft(request) => Some(request.clone()),
            _ => None,
        })
        .expect("Enter draft");
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        1
    );
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let draft = runtime
        .block_on(service.draft(
            prepared.id,
            "mock".into(),
            provider,
            tokio_util::sync::CancellationToken::new(),
        ))
        .expect("mock draft");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_draft(first_draft.operation_id, Ok(draft), cx)
    }));
    cx.simulate_keystrokes(window.into(), "space space");
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        2,
        "Generate Space is first-wins"
    );
    let second_draft = events
        .lock()
        .expect("events")
        .iter()
        .filter_map(|event| match event {
            CapturedCommitEvent::Draft(request) => Some(request.clone()),
            _ => None,
        })
        .nth(1)
        .expect("Space draft");
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let draft = runtime
        .block_on(service.draft(
            prepared.id,
            "mock".into(),
            provider,
            tokio_util::sync::CancellationToken::new(),
        ))
        .expect("second mock draft");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_draft(second_draft.operation_id, Ok(draft), cx)
    }));
    cx.simulate_keystrokes(window.into(), "tab cmd-enter cmd-enter escape escape");
    let events = events.lock().expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Commit(_)))
            .count(),
        1,
        "commit is exact first-wins"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Close))
            .count(),
        1,
        "Esc close is exact first-wins"
    );
    let commit = events.iter().find_map(|event| match event {
        CapturedCommitEvent::Commit(request) => Some(request),
        _ => None,
    });
    assert!(commit.is_some_and(|request| request.prepared_id == prepared.id));
}

#[gpui::test]
async fn commit_app_production_handlers_reconcile_before_release_across_close_and_routes_s6_controller(
    cx: &mut gpui::TestAppContext,
) {
    let repo = diff_controller_repo();
    let store = Store::open(":memory:").expect("commit production store");
    store.migrate().expect("commit production migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 commit root"),
        "commit-production",
        None,
    )
    .expect("commit production project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("commit production thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
    let panel_root = panel.clone();
    let panel_window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| CommitPanelHarness { panel: panel_root })
            })
        })
        .expect("commit production panel window");
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let probe = Arc::new(CommitTestProbe::default());
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.commit_provider_override = Some(provider.clone());
        root.commit_test_probe = Some(probe.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_artifact_route(&thread, stream.clone(), cx);
        root.ensure_branch_route(&thread, stream.clone(), cx);
        root.open_workspace_diff(
            stream.clone(),
            &OpenWorkspaceDiffRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
            },
            cx,
        );
        cx.subscribe(&panel, |this, panel, request, cx| {
            this.request_commit_prepare(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&panel, |this, panel, request, cx| {
            this.request_commit_draft(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&panel, |this, panel, request, cx| {
            this.request_commit_execute(panel.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&panel, |this, panel, request, cx| {
            this.commit_panel_closed(panel.clone(), request, cx);
        })
        .detach();
    });
    let (branch_service, branch_selector, artifact_service) = root.read_with(cx, |root, _| {
        let branch = root
            .branch_controller
            .active
            .as_ref()
            .expect("initial branch route");
        let artifacts = root
            .artifact_controller
            .active
            .as_ref()
            .expect("initial artifact route");
        (
            branch.service.clone(),
            branch.identity.selector.clone(),
            artifacts.service.clone(),
        )
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("commit production runtime");
    branch_selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
    });
    let initial_branch_error = runtime
        .block_on(branch_service.refresh(tokio_util::sync::CancellationToken::new()))
        .expect_err("dirty initial branch state");
    assert_eq!(
        initial_branch_error.code(),
        GitWorkspaceErrorCode::BranchDirty
    );
    runtime
        .block_on(artifact_service.reconcile(tokio_util::sync::CancellationToken::new()))
        .expect("initial artifact reconciliation");
    branch_selector.update(cx, |selector, cx| {
        selector.apply_error(initial_branch_error.code(), cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.diff_controller
                .active
                .as_ref()
                .is_some_and(|active| active.view.read(cx).generation().is_some())
        })
    });
    root.update(cx, |root, cx| {
        root.open_commit_panel(
            stream.clone(),
            &OpenCommitPanelRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
            },
            cx,
        );
    });
    for _ in 0..400 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if panel.read_with(cx, |panel, _| panel.stage())
            == vega_ui::commit_panel::CommitPanelStage::Checklist
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::Checklist,
        "second checklist controller_open={} lease_busy={}",
        root.read_with(cx, |root, _| root.commit_controller.is_open()),
        root.read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    panel_window
        .update(cx, |_, window, cx| {
            let focus = panel.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        })
        .expect("focus first checklist");
    cx.simulate_keystrokes(panel_window.into(), "tab space cmd-enter cmd-enter");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::Preparing
    );
    let cached_clean = fixture_git_command(repo.path(), &["diff", "--cached", "--quiet"])
        .status()
        .expect("inspect prepare mutation")
        .success();
    assert!(!cached_clean, "prepare worker established owned B");
    assert_eq!(probe.prepare_workers.load(Ordering::SeqCst), 1);
    assert_eq!(
        root.read_with(cx, |root, _| {
            root.commit_controller
                .active
                .as_ref()
                .map(|active| active.next_sequence)
        }),
        Some(2),
        "repeated prepare ingress starts one production fence"
    );
    cx.simulate_keystrokes(panel_window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            !root.commit_controller.is_open() && !root.trusted_actions.is_busy()
        })
    });
    root.read_with(cx, |root, cx| {
        let diff = root
            .diff_controller
            .active
            .as_ref()
            .expect("diff survives prepare close");
        assert!(diff.view.read(cx).generation().is_some());
        let branch = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch survives prepare close");
        assert_eq!(
            branch.identity.selector.read(cx).snapshot_generation(),
            None,
            "dirty prepare invalidates the clean-only branch snapshot"
        );
        let artifacts = root
            .artifact_controller
            .active
            .as_ref()
            .expect("artifact survives prepare close");
        assert!(artifacts.terminal_in_flight.is_none());
    });

    // Reopen against owned B, prepare without another add, enter a real
    // message through TextInput, and close while commit owns the lease.
    root.update(cx, |root, cx| {
        root.open_commit_panel(
            stream.clone(),
            &OpenCommitPanelRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
            },
            cx,
        );
    });
    for _ in 0..400 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if panel.read_with(cx, |panel, _| panel.stage())
            == vega_ui::commit_panel::CommitPanelStage::Checklist
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::Checklist,
        "reopen state controller_open={} lease_busy={}",
        root.read_with(cx, |root, _| root.commit_controller.is_open()),
        root.read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    panel_window
        .update(cx, |_, window, cx| {
            let focus = panel.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        })
        .expect("focus second checklist");
    probe
        .trace
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
    cx.simulate_keystrokes(panel_window.into(), "tab cmd-enter cmd-enter");
    for _ in 0..400 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if panel.read_with(cx, |panel, _| panel.stage())
            == vega_ui::commit_panel::CommitPanelStage::CommitReady
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::CommitReady,
        "prepare ready controller_open={} lease_busy={}",
        root.read_with(cx, |root, _| root.commit_controller.is_open()),
        root.read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    assert_eq!(probe.prepare_workers.load(Ordering::SeqCst), 2);
    assert_eq!(
        probe
            .trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_slice(),
        [
            "workspace_candidate",
            "branch_result",
            "artifact_result",
            "workspace_final",
            "ui_diff",
            "ui_branch",
            "ui_artifact",
            "panel_terminal",
        ],
        "Prepare consumers must precede CommitReady and retain the lease"
    );
    cx.simulate_keystrokes(panel_window.into(), "tab tab enter enter");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::Drafting
    );
    pump_test_app(cx, |cx| {
        panel.read_with(cx, |panel, cx| {
            panel.stage() == vega_ui::commit_panel::CommitPanelStage::CommitReady
                && panel.commit_message(cx) == "feat: generated"
        })
    });
    assert_eq!(probe.draft_workers.load(Ordering::SeqCst), 1);
    assert_eq!(provider.requests().len(), 1, "draft provider is exact once");
    probe
        .trace
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
    let terminal_before_commit = probe.terminal_applications.load(Ordering::SeqCst);
    probe.drop_commit_sender.store(true, Ordering::SeqCst);
    cx.simulate_keystrokes(panel_window.into(), "tab cmd-enter cmd-enter");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        vega_ui::commit_panel::CommitPanelStage::Committing
    );
    let commit_count = fixture_git_command(repo.path(), &["rev-list", "--count", "HEAD"])
        .output()
        .expect("inspect commit mutation");
    assert!(commit_count.status.success());
    assert_eq!(commit_count.stdout, b"2\n");
    cx.simulate_keystrokes(panel_window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            !root.commit_controller.is_open() && !root.trusted_actions.is_busy()
        })
    });
    assert_eq!(probe.commit_workers.load(Ordering::SeqCst), 1);
    assert_eq!(
        probe.terminal_applications.load(Ordering::SeqCst),
        terminal_before_commit + 1,
        "disconnected completion applies exactly one accepted terminal"
    );
    let trace = probe
        .trace
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    assert_eq!(
        trace
            .iter()
            .filter(|event| **event == "workspace_final")
            .count(),
        2,
        "the dropped result is followed by one authoritative recovery"
    );
    for event in ["ui_diff", "ui_branch", "ui_artifact", "panel_terminal"] {
        assert_eq!(
            trace
                .iter()
                .filter(|candidate| **candidate == event)
                .count(),
            1,
            "visible terminal event is exact once: {event}"
        );
    }
    let panel_terminal = trace
        .iter()
        .position(|event| *event == "panel_terminal")
        .expect("accepted panel terminal trace");
    assert!(
        ["ui_diff", "ui_branch", "ui_artifact"]
            .into_iter()
            .all(
                |event| trace.iter().position(|candidate| *candidate == event)
                    < Some(panel_terminal)
            ),
        "visible consumers precede the panel terminal"
    );
    assert_eq!(
        trace.last(),
        Some(&"lease_release"),
        "exact shared lease release remains the final action"
    );
    let status = fixture_git_command(repo.path(), &["status", "--porcelain=v1"])
        .output()
        .expect("post-commit status");
    assert!(status.status.success());
    assert!(status.stdout.is_empty(), "commit leaves repository clean");
    let post_commit_branch = runtime
        .block_on(branch_service.refresh(tokio_util::sync::CancellationToken::new()))
        .expect("post-commit branch service refresh");
    root.read_with(cx, |root, cx| {
        assert!(
            root.diff_controller
                .active
                .as_ref()
                .is_some_and(|active| active.view.read(cx).generation().is_some())
        );
        assert!(
            root.branch_controller
                .active
                .as_ref()
                .is_some_and(
                    |active| active.identity.selector.read(cx).snapshot_generation()
                        == Some(post_commit_branch.generation)
                )
        );
        assert!(root.artifact_controller.active.is_some());
    });

    let actions = root.read_with(cx, |root, _| root.trusted_actions.clone());
    root.update(cx, |root, _| root.window_terminal_cleanup());
    pump_test_app(cx, |_| !actions.is_busy());
    panel_window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close commit production panel window");
    cx.run_until_parked();
}
