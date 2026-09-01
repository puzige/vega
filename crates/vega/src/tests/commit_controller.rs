use super::*;

#[test]
fn commit_worker_terminal_releases_exact_owner_after_window_drop() {
    let actions = TrustedActionCoordinator::default();
    let lease = actions
        .acquire(TrustedActionKind::Commit, 7, 11)
        .expect("commit lease");
    let alive = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    mark_commit_worker_terminal(done.clone(), alive, actions.clone(), lease);
    assert!(done.load(Ordering::Acquire));
    assert!(!actions.is_busy());

    let lease = actions
        .acquire(TrustedActionKind::Commit, 8, 12)
        .expect("fresh commit lease");
    let alive = Arc::new(AtomicBool::new(true));
    let done = Arc::new(AtomicBool::new(false));
    mark_commit_worker_terminal(done.clone(), alive.clone(), actions.clone(), lease);
    assert!(actions.is_busy(), "live window owns UI reconciliation");
    alive.store(false, Ordering::Release);
    assert!(done.load(Ordering::Acquire));
    assert!(actions.release(lease), "Drop exact terminal cleanup");
}

#[test]
fn commit_terminal_and_window_cleanup_are_seqcst_race_safe() {
    for generation in 1..=1_000 {
        let actions = TrustedActionCoordinator::default();
        let lease = actions
            .acquire(TrustedActionKind::Commit, generation, 1)
            .expect("race lease");
        let alive = Arc::new(AtomicBool::new(true));
        let done = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|scope| {
            scope.spawn({
                let actions = actions.clone();
                let alive = alive.clone();
                let done = done.clone();
                let barrier = barrier.clone();
                move || {
                    barrier.wait();
                    mark_commit_worker_terminal(done, alive, actions, lease);
                }
            });
            scope.spawn({
                let actions = actions.clone();
                let alive = alive.clone();
                let done = done.clone();
                let barrier = barrier.clone();
                move || {
                    barrier.wait();
                    alive.store(false, Ordering::SeqCst);
                    if done.load(Ordering::SeqCst) {
                        let _ = actions.release(lease);
                    }
                }
            });
            barrier.wait();
        });
        assert!(!actions.is_busy(), "iteration {generation} leaked lease");
        let fresh = actions
            .acquire(TrustedActionKind::Commit, generation, 2)
            .expect("fresh race lease");
        assert!(
            !actions.release(lease),
            "stale terminal released fresh lease"
        );
        assert!(actions.release(fresh));
    }
}

#[test]
fn commit_runtime_failure_is_typed_and_recovery_backoff_is_bounded() {
    let failed = build_commit_runtime_with(|| Err(std::io::Error::other("fixture")));
    assert!(matches!(failed, Err(CommitErrorCode::SpawnFailed)));
    let terminal = CommitWorkerResult::RuntimeUnavailable(CommitErrorCode::SpawnFailed);
    assert!(commit_result_has_authoritative_workspace(
        CommitPhase::Preparing,
        &terminal
    ));
    assert!(commit_result_has_authoritative_workspace(
        CommitPhase::Committing,
        &terminal
    ));
    assert!(commit_result_reconciliation(&terminal).is_none());

    let mut delay = Duration::from_millis(25);
    for expected in [50, 100, 200, 400, 800, 1000, 1000] {
        delay = next_commit_recovery_backoff(delay);
        assert_eq!(delay, Duration::from_millis(expected));
    }

    let attempts = std::cell::Cell::new(0_u8);
    let mut waits = Vec::new();
    let recovered = build_commit_recovery_runtime_with(
        || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            if attempt < 3 {
                Err(CommitErrorCode::SpawnFailed)
            } else {
                build_commit_runtime()
            }
        },
        |delay| waits.push(delay),
    );
    assert!(recovered.is_ok());
    assert_eq!(attempts.get(), 3);
    assert_eq!(
        waits,
        [Duration::from_millis(25), Duration::from_millis(50)]
    );

    let attempts = std::cell::Cell::new(0_u8);
    let mut waits = Vec::new();
    assert!(matches!(
        build_commit_recovery_runtime_with(
            || {
                attempts.set(attempts.get() + 1);
                Err(CommitErrorCode::SpawnFailed)
            },
            |delay| waits.push(delay),
        ),
        Err(CommitErrorCode::SpawnFailed)
    ));
    assert_eq!(attempts.get(), 6);
    assert_eq!(waits.len(), 5, "the terminal failure is bounded");
}

#[gpui::test]
async fn commit_controller_retiring_fence_is_first_wins_and_holds_owner(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let repo = diff_controller_repo();
    let thread = Thread {
        id: "commit-thread".into(),
        project_id: "commit-project".into(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: String::new(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    };
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service =
        Arc::new(TrustedGitService::new(repo.path(), workspace).expect("trusted commit service"));
    let lease = TrustedActionToken {
        generation: 1,
        kind: TrustedActionKind::Commit,
        owner_epoch: 1,
        request_sequence: 1,
    };
    let identity = CommitRouteIdentity {
        epoch: 1,
        thread_id: thread.id,
        project_id: thread.project_id,
        stream: stream.clone(),
        panel,
    };
    let mut active = ActiveCommitRoute {
        identity,
        service,
        lease,
        next_sequence: 0,
        phase: CommitPhase::Checklist,
        snapshot: None,
        prepared: None,
        focus_pending: false,
        pending: None,
        cancel: None,
        terminal_done: None,
    };
    let (fence, cancel, _) = CommitController::begin_fence(
        &mut active,
        CommitPhase::Checklist,
        None,
        CommitFenceAuthority::None,
    )
    .expect("checklist owner fence");
    let mut controller = CommitController {
        next_epoch: 1,
        active: Some(active),
        retiring: None,
    };
    assert_eq!(controller.retire_or_close(), None);
    assert!(cancel.is_cancelled());
    assert!(controller.active.is_none());
    assert!(controller.retiring.is_some());
    assert!(matches!(
        controller.claim(&fence),
        CommitClaim::Retiring(active)
            if active.lease == lease && active.identity.stream == stream
    ));
    assert!(matches!(controller.claim(&fence), CommitClaim::Stale));
}

#[gpui::test]
async fn commit_controller_binds_exact_snapshot_and_overflow_is_zero_work(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let repo = diff_controller_repo();
    let thread = Thread {
        id: "commit-capability-thread".into(),
        project_id: "commit-capability-project".into(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: String::new(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    };
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service =
        Arc::new(TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let first = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("first checklist")
    });
    let second = runtime
        .block_on(service.open_checklist(tokio_util::sync::CancellationToken::new()))
        .expect("second checklist");
    assert_ne!(first.id, second.id);
    let identity = CommitRouteIdentity {
        epoch: 1,
        thread_id: thread.id,
        project_id: thread.project_id,
        stream,
        panel,
    };
    let lease = TrustedActionToken {
        generation: 1,
        kind: TrustedActionKind::Commit,
        owner_epoch: 1,
        request_sequence: 1,
    };
    let mut active = ActiveCommitRoute {
        identity,
        service,
        lease,
        next_sequence: 0,
        phase: CommitPhase::Checklist,
        snapshot: Some(first.id),
        prepared: None,
        focus_pending: false,
        pending: None,
        cancel: None,
        terminal_done: None,
    };
    let (wrong, _, _) = CommitController::begin_fence(
        &mut active,
        CommitPhase::Preparing,
        None,
        CommitFenceAuthority::Snapshot(second.id),
    )
    .expect("wrong capability fixture");
    let mut controller = CommitController {
        next_epoch: 1,
        active: Some(active),
        retiring: None,
    };
    assert!(matches!(controller.claim(&wrong), CommitClaim::Stale));
    let active = controller.active.as_mut().expect("active retained");
    active.pending = None;
    active.cancel = None;
    active.next_sequence = u64::MAX;
    let phase = active.phase;
    assert!(
        CommitController::begin_fence(
            active,
            CommitPhase::Preparing,
            None,
            CommitFenceAuthority::Snapshot(first.id),
        )
        .is_none()
    );
    assert!(active.phase == phase);
    assert!(active.pending.is_none());
    assert!(active.cancel.is_none());
}

#[gpui::test]
async fn commit_controller_same_id_entity_aba_is_stale_and_worker_recovers_authority(
    cx: &mut gpui::TestAppContext,
) {
    let repo = artifact_controller_repo();
    let store = Store::open(":memory:").expect("commit window memory store");
    store.migrate().expect("commit window migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 commit root"),
        "commit",
        None,
    )
    .expect("commit project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("commit thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let old_stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let fresh_stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let old_panel = old_stream.read_with(cx, |stream, _| stream.commit_panel());
    let old_identity = CommitRouteIdentity {
        epoch: 1,
        thread_id: thread.id.clone(),
        project_id: thread.project_id.clone(),
        stream: old_stream,
        panel: old_panel,
    };
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.stream_view = Some((thread.id.clone(), fresh_stream));
        assert!(!root.commit_route_is_current(&old_identity, cx));
    });
    let fresh_diff = cx.new(|cx| DiffView::new(thread.id.clone(), thread.project_id.clone(), cx));
    root.update(cx, |root, _| {
        assert!(
            root.diff_controller
                .begin(
                    thread.id.clone(),
                    thread.project_id.clone(),
                    fresh_diff.clone(),
                )
                .is_some()
        );
    });

    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service =
        Arc::new(TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let stale = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        let stale = service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("stale checklist");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("replacement checklist");
        stale
    });
    let result = run_commit_prepare_worker(
        service,
        stale.id,
        Vec::new(),
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        None,
    );
    let reconciliation = match result {
        CommitWorkerResult::Prepare(
            CommitPrepareCompletion {
                prepared: None,
                workspace: Some(_),
                error: Some(CommitErrorCode::StaleAuthority),
            },
            reconciliation,
        ) => reconciliation,
        _ => panic!("stale capability must return typed prepare completion"),
    };
    root.update(cx, |root, cx| {
        root.apply_commit_workspace_reconciliation(&old_identity, &reconciliation, cx);
    });
    assert_eq!(
        fresh_diff.read_with(cx, |view, _| view.generation()),
        None,
        "old same-id stream completion cannot overwrite fresh Diff route"
    );
}
