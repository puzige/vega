#[allow(unused_imports)]
use super::*;

#[test]
fn branch_controller_shared_lease_is_first_wins_and_aba_safe() {
    let actions = TrustedActionCoordinator::default();
    let first = actions
        .acquire(TrustedActionKind::BranchSwitch, 7, 1)
        .expect("first owner");
    assert!(
        actions
            .acquire(TrustedActionKind::ArtifactOpen, 8, 1)
            .is_none(),
        "a second trusted action cannot overlap"
    );
    let mut forged = first;
    forged.generation += 1;
    assert!(!actions.release(forged));
    assert!(actions.is_busy());
    assert!(actions.release(first));
    let second = actions
        .acquire(TrustedActionKind::Commit, 7, 2)
        .expect("new generation");
    assert_ne!(first, second);
    assert!(!actions.release(first), "stale A cannot release B");
    assert!(actions.is_busy());
    assert!(actions.release(second));
}

#[gpui::test]
async fn branch_controller_route_and_active_guards_fail_closed(cx: &mut gpui::TestAppContext) {
    let repo = artifact_controller_repo();
    let store = Store::open(":memory:").expect("branch window memory store");
    store.migrate().expect("branch window migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch root"),
        "branch",
        None,
    )
    .expect("branch project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("current branch route");
        assert!(VegaWindow::branch_route_is_current(&active.identity, cx));
        assert_eq!(active.identity.stream, stream);
        assert_eq!(active.identity.selector, stream.read(cx).branch_selector());
        assert!(root.branch_guards_clear(&stream, cx));

        let lease = root
            .trusted_actions
            .acquire(TrustedActionKind::Commit, 99, 1)
            .expect("future commit lease");
        assert!(!root.branch_guards_clear(&stream, cx));
        assert!(root.trusted_actions.release(lease));

        let (generation, _) =
            root.agent_controller
                .begin(thread.id.clone(), stream.clone(), None, None);
        assert!(!root.branch_guards_clear(&stream, cx));
        let _ = root
            .agent_controller
            .finish(generation, &thread.id, &stream)
            .expect("finish guard run");

        stream.update(cx, |stream, cx| {
            stream.apply_plan(
                Plan {
                    id: "pending-branch-plan".into(),
                    thread_id: thread.id.clone(),
                    content: "Inspect before switch".into(),
                    status: PlanStatus::Pending,
                    review_note: None,
                    reviewed_at: None,
                },
                cx,
            );
        });
        assert!(!root.branch_guards_clear(&stream, cx));

        cx.set_global(SettingsOpen(true));
        assert!(!VegaWindow::branch_route_is_current(
            &root
                .branch_controller
                .active
                .as_ref()
                .expect("route before settings close")
                .identity,
            cx,
        ));
    });
}

#[gpui::test]
async fn branch_controller_guard_change_after_preflight_starts_zero_execute(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "other"]);
    let store = Store::open(":memory:").expect("branch preflight store");
    store.migrate().expect("branch preflight migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch root"),
        "branch",
        None,
    )
    .expect("branch project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let root = cx.new(VegaWindow::new);
    let (identity, service) = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch route");
        (active.identity.clone(), active.service.clone())
    });
    let list_fence = BranchListFence {
        route: identity.clone(),
        sequence: 1,
    };
    let (list_sender, list_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        service.clone(),
        list_fence,
        tokio_util::sync::CancellationToken::new(),
        list_sender,
    );
    let (_, snapshot) = list_receiver.recv().expect("branch snapshot output");
    let snapshot = snapshot.expect("branch snapshot");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("switch target")
        .id;
    let operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot.clone(), cx));
        selector
            .begin_switch(snapshot.generation, target, cx)
            .expect("switch operation")
    });
    let prepare_fence = BranchPrepareFence {
        route: identity,
        sequence: 1,
        snapshot_generation: snapshot.generation,
        branch_id: target,
        operation_id: operation,
    };
    root.update(cx, |root, _| {
        let active = root
            .branch_controller
            .active
            .as_mut()
            .expect("active preflight route");
        active.switch_sequence = 1;
        active.prepare_fence = Some(prepare_fence.clone());
        active.switch_cancel = Some(tokio_util::sync::CancellationToken::new());
    });
    let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
    run_branch_prepare_worker(
        service,
        prepare_fence.clone(),
        tokio_util::sync::CancellationToken::new(),
        prepare_sender,
    );
    let (_, permit) = prepare_receiver.recv().expect("preflight output");
    let permit = permit.expect("valid preflight permit");
    root.update(cx, |root, cx| {
        let competing = root
            .trusted_actions
            .acquire(TrustedActionKind::Commit, 42, 1)
            .expect("guard changes after preflight");
        root.finish_branch_prepare(prepare_fence, Ok(permit), cx);
        assert!(
            root.branch_controller
                .active
                .as_ref()
                .is_some_and(|active| active.switch_fence.is_none()),
            "guard change starts zero execute"
        );
        assert_eq!(root.trusted_actions.active_token(), Some(competing));
        assert!(root.trusted_actions.release(competing));
    });
    let output = fixture_git_command(repo.path(), &["symbolic-ref", "--short", "HEAD"])
        .output()
        .expect("read current branch");
    assert!(output.status.success());
    assert_ne!(output.stdout, b"other\n", "preflight alone never mutates");
    assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
}

#[gpui::test]
async fn branch_controller_close_during_preflight_clears_exact_pending_then_reopens(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "preflight-close-target"]);
    let store = Store::open(":memory:").expect("branch close preflight store");
    store.migrate().expect("branch close preflight migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch root"),
        "branch",
        None,
    )
    .expect("branch project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let root = cx.new(VegaWindow::new);
    let (identity, service) = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch close preflight route");
        (active.identity.clone(), active.service.clone())
    });
    let (list_sender, list_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        service.clone(),
        BranchListFence {
            route: identity.clone(),
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        list_sender,
    );
    let snapshot = list_receiver
        .recv()
        .expect("close preflight list output")
        .1
        .expect("close preflight snapshot");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("close preflight target")
        .id;
    let operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot.clone(), cx));
        selector
            .begin_switch(snapshot.generation, target, cx)
            .expect("close preflight operation")
    });
    let fence = BranchPrepareFence {
        route: identity,
        sequence: 1,
        snapshot_generation: snapshot.generation,
        branch_id: target,
        operation_id: operation,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    root.update(cx, |root, _| {
        let active = root
            .branch_controller
            .active
            .as_mut()
            .expect("active close preflight route");
        active.switch_sequence = 1;
        active.prepare_fence = Some(fence.clone());
        active.switch_cancel = Some(cancel.clone());
    });
    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
    });
    root.update(cx, |root, cx| {
        root.branch_selector_closed(
            selector.clone(),
            &BranchSelectorClosed {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
            },
            cx,
        );
    });
    assert!(cancel.is_cancelled());
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        Some((operation, snapshot.generation, target))
    );

    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert!(
        !selector.read_with(cx, |selector, _| selector.is_pending()),
        "route close synchronously clears only its exact operation"
    );
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    let (fresh_identity, fresh_service) = root.update(cx, |root, cx| {
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("restored preflight route");
        (active.identity.clone(), active.service.clone())
    });

    let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
    run_branch_prepare_worker(service, fence.clone(), cancel, prepare_sender);
    let (_, result) = prepare_receiver.recv().expect("close preflight terminal");
    root.update(cx, |root, cx| root.finish_branch_prepare(fence, result, cx));
    assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
    let (fresh_sender, fresh_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        fresh_service,
        BranchListFence {
            route: fresh_identity,
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        fresh_sender,
    );
    let fresh_snapshot = fresh_receiver
        .recv()
        .expect("fresh preflight list")
        .1
        .expect("fresh preflight snapshot");
    let fresh_target = fresh_snapshot
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("fresh preflight target")
        .id;
    let fresh_operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx), "fresh list request is reusable");
        assert!(selector.apply_snapshot(fresh_snapshot.clone(), cx));
        selector
            .begin_switch(fresh_snapshot.generation, fresh_target, cx)
            .expect("fresh preflight operation")
    });
    root.update(cx, |root, cx| {
        root.request_branch_switch(
            selector.clone(),
            &BranchSwitchRequested {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                snapshot_generation: fresh_snapshot.generation,
                branch_id: fresh_target,
                operation_id: fresh_operation,
            },
            cx,
        );
        assert!(
            root.branch_controller
                .active
                .as_ref()
                .is_some_and(|active| active.prepare_fence.is_some())
        );
        root.close_branch_route(GitWorkspaceErrorCode::Cancelled, cx);
    });
}

#[gpui::test]
async fn branch_controller_close_cancels_owner_but_releases_only_after_cleanup(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "cancel-target"]);
    let store = Store::open(":memory:").expect("branch cancel store");
    store.migrate().expect("branch cancel migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch root"),
        "branch",
        None,
    )
    .expect("branch project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let root = cx.new(VegaWindow::new);
    let (identity, service) = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch cancel route");
        (active.identity.clone(), active.service.clone())
    });
    let list_fence = BranchListFence {
        route: identity.clone(),
        sequence: 1,
    };
    let (list_sender, list_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        service.clone(),
        list_fence,
        tokio_util::sync::CancellationToken::new(),
        list_sender,
    );
    let snapshot = list_receiver
        .recv()
        .expect("list output")
        .1
        .expect("list snapshot");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("cancel target")
        .id;
    let operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot.clone(), cx));
        selector
            .begin_switch(snapshot.generation, target, cx)
            .expect("cancel owner operation")
    });
    let prepare_fence = BranchPrepareFence {
        route: identity.clone(),
        sequence: 1,
        snapshot_generation: snapshot.generation,
        branch_id: target,
        operation_id: operation,
    };
    let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
    run_branch_prepare_worker(
        service.clone(),
        prepare_fence,
        tokio_util::sync::CancellationToken::new(),
        prepare_sender,
    );
    let permit = prepare_receiver
        .recv()
        .expect("prepare output")
        .1
        .expect("prepare permit");
    let cancel = tokio_util::sync::CancellationToken::new();
    let fence = root.update(cx, |root, cx| {
        let lease = root
            .trusted_actions
            .acquire(TrustedActionKind::BranchSwitch, identity.epoch, 1)
            .expect("branch owner lease");
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let fence = BranchSwitchFence {
            route: identity,
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
            lease,
        };
        let active = root
            .branch_controller
            .active
            .as_mut()
            .expect("branch owner route");
        active.switch_fence = Some(fence.clone());
        active.switch_cancel = Some(cancel.clone());
        fence
    });
    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
    });
    root.update(cx, |root, cx| {
        root.branch_selector_closed(
            selector.clone(),
            &BranchSelectorClosed {
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
            },
            cx,
        );
        assert!(cancel.is_cancelled());
        assert!(root.trusted_actions.is_busy(), "close cannot release owner");
    });
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        Some((operation, snapshot.generation, target))
    );
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
    root.update(cx, |root, _| {
        assert!(
            root.trusted_actions.is_busy(),
            "settings cannot release owner"
        );
    });
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    let fresh_service = root.update(cx, |root, cx| {
        root.ensure_branch_route(&thread, stream.clone(), cx);
        root.branch_controller
            .active
            .as_ref()
            .expect("restored owner route")
            .service
            .clone()
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    run_branch_switch_worker(service, permit, fence.clone(), cancel, sender);
    let (_, completion) = receiver.recv().expect("cancelled owner completion");
    assert!(matches!(
        completion.outcome,
        BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled)
    ));
    assert!(
        completion.snapshot.is_some(),
        "owner cancellation still returns authoritative refresh"
    );
    assert!(completion.snapshot.is_some());
    root.update(cx, |root, cx| {
        root.finish_branch_switch(fence, completion, cx);
        assert!(
            !root.trusted_actions.is_busy(),
            "cleanup completion releases exact owner"
        );
    });
    assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
    let (fresh_sender, fresh_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        fresh_service,
        BranchListFence {
            route: root.read_with(cx, |root, _| {
                root.branch_controller
                    .active
                    .as_ref()
                    .expect("fresh owner identity")
                    .identity
                    .clone()
            }),
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        fresh_sender,
    );
    let refreshed = fresh_receiver
        .recv()
        .expect("fresh owner list")
        .1
        .expect("fresh owner snapshot");
    let fresh_generation = refreshed.generation;
    let fresh_target = refreshed
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("fresh target after owner cleanup")
        .id;
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx), "selector reopens after cleanup");
        assert!(selector.apply_snapshot(refreshed, cx));
        assert!(
            selector
                .begin_switch(fresh_generation, fresh_target, cx)
                .is_some()
        );
    });
    assert!(!stream.read_with(cx, |stream, _| stream.has_active_agent()));
}

#[gpui::test]
async fn branch_controller_s6_controller_owner_success_applies_authority_then_releases(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "success-target"]);
    let store = Store::open(":memory:").expect("branch success store");
    store.migrate().expect("branch success migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch root"),
        "branch",
        None,
    )
    .expect("branch project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let root = cx.new(VegaWindow::new);
    let (identity, service) = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream.clone(), cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch success route");
        (active.identity.clone(), active.service.clone())
    });
    let (list_sender, list_receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        service.clone(),
        BranchListFence {
            route: identity.clone(),
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        list_sender,
    );
    let snapshot = list_receiver
        .recv()
        .expect("success list output")
        .1
        .expect("success list snapshot");
    let target = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "success-target")
        .expect("success target")
        .id;
    let operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot.clone(), cx));
        selector
            .begin_switch(snapshot.generation, target, cx)
            .expect("success owner operation")
    });
    let prepare_fence = BranchPrepareFence {
        route: identity.clone(),
        sequence: 1,
        snapshot_generation: snapshot.generation,
        branch_id: target,
        operation_id: operation,
    };
    let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
    run_branch_prepare_worker(
        service.clone(),
        prepare_fence,
        tokio_util::sync::CancellationToken::new(),
        prepare_sender,
    );
    let permit = prepare_receiver
        .recv()
        .expect("success prepare output")
        .1
        .expect("success permit");
    let fence = root.update(cx, |root, cx| {
        let lease = root
            .trusted_actions
            .acquire(TrustedActionKind::BranchSwitch, identity.epoch, 1)
            .expect("success owner lease");
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let fence = BranchSwitchFence {
            route: identity,
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
            lease,
        };
        let active = root
            .branch_controller
            .active
            .as_mut()
            .expect("success owner route");
        active.switch_fence = Some(fence.clone());
        fence
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    run_branch_switch_worker(
        service,
        permit,
        fence.clone(),
        tokio_util::sync::CancellationToken::new(),
        sender,
    );
    let (_, completion) = receiver.recv().expect("success owner completion");
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    assert!(completion.snapshot.is_some());
    let authoritative = completion
        .snapshot
        .clone()
        .expect("success authoritative snapshot");
    let duplicate_fence = fence.clone();
    let duplicate_completion = completion.clone();
    root.update(cx, |root, cx| {
        root.finish_branch_switch(fence, completion, cx);
        assert!(!root.trusted_actions.is_busy());
    });
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
    let output = fixture_git_command(repo.path(), &["symbolic-ref", "--short", "HEAD"])
        .output()
        .expect("read switched branch");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"success-target\n");

    let fresh_target = authoritative
        .branches
        .iter()
        .find(|branch| !branch.current)
        .expect("fresh switch target")
        .id;
    let fresh_operation = selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(authoritative.clone(), cx));
        selector
            .begin_switch(authoritative.generation, fresh_target, cx)
            .expect("fresh owner operation")
    });
    let preview_cancel = tokio_util::sync::CancellationToken::new();
    let open_cancel = tokio_util::sync::CancellationToken::new();
    let fresh_fence = root.update(cx, |root, cx| {
        root.ensure_artifact_route(&thread, stream.clone(), cx);
        let active_artifact = root
            .artifact_controller
            .active
            .as_mut()
            .expect("fresh artifact route");
        active_artifact.preview_cancel = Some(preview_cancel.clone());
        active_artifact.open_cancel = Some(open_cancel.clone());

        let lease = root
            .trusted_actions
            .acquire(
                TrustedActionKind::BranchSwitch,
                duplicate_fence.route.epoch,
                2,
            )
            .expect("fresh branch owner lease");
        let fresh = BranchSwitchFence {
            route: duplicate_fence.route.clone(),
            sequence: 2,
            snapshot_generation: authoritative.generation,
            branch_id: fresh_target,
            operation_id: fresh_operation,
            lease,
        };
        let active = root
            .branch_controller
            .active
            .as_mut()
            .expect("fresh branch route");
        active.switch_fence = Some(fresh.clone());
        active.switch_cancel = Some(tokio_util::sync::CancellationToken::new());

        root.finish_branch_switch(duplicate_fence, duplicate_completion, cx);
        assert!(
            root.branch_controller
                .active
                .as_ref()
                .is_some_and(|active| active.switch_fence.as_ref() == Some(&fresh)),
            "old duplicate cannot claim the fresh branch fence"
        );
        assert!(root.trusted_actions.is_busy());
        assert!(!preview_cancel.is_cancelled());
        assert!(!open_cancel.is_cancelled());
        fresh
    });
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        Some((fresh_operation, authoritative.generation, fresh_target,)),
        "old terminal cannot clear the fresh operation token"
    );
    root.update(cx, |root, cx| {
        root.finish_branch_switch(
            fresh_fence,
            BranchSwitchCompletion {
                outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                snapshot: Some(authoritative),
            },
            cx,
        );
        assert!(!root.trusted_actions.is_busy());
    });
}

#[gpui::test]
async fn branch_selector_real_projection_keyboard_first_wins_and_visible_range(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "aaa-selector"]);
    run_fixture_git(repo.path(), &["branch", "zzz-selector"]);
    let store = Store::open(":memory:").expect("branch selector interaction store");
    store
        .migrate()
        .expect("branch selector interaction migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 branch selector root"),
        "branch",
        None,
    )
    .expect("branch selector project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("branch selector thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let root = cx.new(VegaWindow::new);
    let (identity, service) = root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.ensure_branch_route(&thread, stream, cx);
        let active = root
            .branch_controller
            .active
            .as_ref()
            .expect("branch selector interaction route");
        (active.identity.clone(), active.service.clone())
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    run_branch_list_worker(
        service,
        BranchListFence {
            route: identity,
            sequence: 1,
        },
        tokio_util::sync::CancellationToken::new(),
        sender,
    );
    let snapshot = receiver
        .recv()
        .expect("branch selector interaction list")
        .1
        .expect("branch selector interaction snapshot");
    let current = snapshot
        .branches
        .iter()
        .find(|branch| branch.current)
        .expect("current branch")
        .id;
    let switchable = snapshot
        .branches
        .iter()
        .filter(|branch| !branch.current)
        .map(|branch| branch.id)
        .collect::<Vec<_>>();
    assert_eq!(switchable.len(), 2);

    let window_selector = selector.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| window_selector)
            .expect("branch selector interaction window")
    });
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot.clone(), cx));
        assert!(
            selector
                .begin_switch(snapshot.generation, current, cx)
                .is_none(),
            "current branch is never activatable"
        );
    });
    window
        .update(cx, |selector, window, cx| {
            let focus = selector.focus_handle(cx);
            window.focus(&focus, cx);
        })
        .expect("focus branch selector");
    cx.run_until_parked();
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.focused_branch()),
        Some(switchable[0])
    );
    cx.simulate_keystrokes(window.into(), "up");
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.focused_branch()),
        Some(switchable[0]),
        "up does not wrap before first switchable row"
    );
    cx.simulate_keystrokes(window.into(), "down down");
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.focused_branch()),
        Some(switchable[1]),
        "down skips current and does not wrap past the end"
    );
    cx.simulate_keystrokes(window.into(), "enter");
    let pending = selector
        .read_with(cx, |selector, _| selector.pending_key())
        .expect("Enter activates focused branch");
    cx.simulate_keystrokes(window.into(), "space");
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        Some(pending),
        "Space cannot replace a pending first winner"
    );
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        Some(pending),
        "Esc closes visibility without forging terminal cleanup"
    );

    selector.update(cx, |selector, cx| {
        assert!(selector.clear_pending(pending.0, pending.1, pending.2, cx));
        assert!(selector.request_open(cx));
        let template = snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("large-list template")
            .clone();
        let large = BranchSnapshot {
            generation: snapshot.generation,
            branches: vec![template.clone(); vega_ui::branch_selector::BRANCH_LIMIT],
        };
        assert!(selector.apply_snapshot(large, cx));
        let visible = selector.visible_rows(4_321..4_329);
        assert_eq!(visible.len(), 8);
        assert_eq!(visible.first().map(|row| row.0), Some(4_321));
        assert_eq!(visible.last().map(|row| row.0), Some(4_328));
        assert_eq!(vega_ui::branch_selector::BRANCH_ROW_HEIGHT, 24.0);
    });
    cx.simulate_keystrokes(window.into(), "space");
    let space_pending = selector
        .read_with(cx, |selector, _| selector.pending_key())
        .expect("Space activates focused branch");
    selector.update(cx, |selector, cx| {
        assert!(selector.clear_pending(space_pending.0, space_pending.1, space_pending.2, cx,));
        let template = snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("over-limit template")
            .clone();
        let too_large = BranchSnapshot {
            generation: snapshot.generation,
            branches: vec![template; vega_ui::branch_selector::BRANCH_LIMIT + 1],
        };
        assert!(!selector.apply_snapshot(too_large, cx));
    });
}
