use super::*;

/// Completes the refresh currently in flight on the active route with an
/// injected worker result.
///
/// The refresh is still started through the production entry points
/// (`schedule_diff_refresh*` / `retry_workspace_diff`), so their routing,
/// progress intent and one-refresh bookkeeping stay covered. Only the worker's
/// terminal `Ready`/`Failed` value is supplied here instead of waiting for a
/// real `/usr/bin/git` child. The request-sequence fence makes this
/// deterministic: a later real-worker result for the same sequence is dropped
/// by `ActiveDiffRoute::complete_refresh`, so no assertion depends on a Git
/// child winning a wall-clock race — which is what made the previous
/// real-worker pump flaky under parallel load.
fn finish_injected_refresh(
    root: &Entity<VegaWindow>,
    identity: &DiffRouteIdentity,
    result: DiffRefreshWorkerResult,
    cx: &mut gpui_kit::TestAppContext,
) {
    root.update(cx, |root, cx| {
        let request_seq = root
            .diff_controller
            .active
            .as_ref()
            .and_then(|active| active.refresh_in_flight)
            .expect("in-flight refresh sequence");
        root.finish_diff_refresh(identity, request_seq, result, cx);
    });
}

pub(crate) fn run_fixture_git(root: &std::path::Path, args: &[&str]) {
    assert!(
        fixture_git_command(root, args).status().unwrap().success(),
        "fixture Git {args:?}"
    );
}

pub(crate) fn diff_controller_repo() -> ControllerRepo {
    let repo = ControllerRepo::for_current_test(
        include_str!("controller-git-fixtures.json"),
        "fresh diff controller repo",
    );
    run_fixture_git(repo.path(), &["init", "-q"]);
    run_fixture_git(
        repo.path(),
        &["config", "--local", "user.name", "Vega Test"],
    );
    run_fixture_git(
        repo.path(),
        &["config", "--local", "user.email", "vega@example.invalid"],
    );
    fs::write(repo.path().join("tracked.rs"), "fn base() {}\n").expect("fixture base");
    run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
    run_fixture_git(repo.path(), &["commit", "-q", "-m", "base"]);
    fs::write(
        repo.path().join("tracked.rs"),
        "fn base() {}\nfn changed() {}\n",
    )
    .expect("fixture change");
    repo
}

pub(crate) fn receive_refresh(
    service: Option<Arc<GitWorkspaceService>>,
    root: Option<PathBuf>,
) -> (Arc<GitWorkspaceService>, WorkspaceSnapshot) {
    let (sender, receiver) = mpsc::sync_channel(1);
    run_diff_refresh_worker(
        service,
        root,
        tokio_util::sync::CancellationToken::new(),
        sender,
    );
    match receiver.recv().expect("refresh worker result") {
        DiffRefreshWorkerResult::Ready { service, snapshot } => (service, snapshot),
        DiffRefreshWorkerResult::Failed(code) => panic!("refresh failed: {}", code.as_str()),
    }
}

pub(crate) fn install_diff_window_globals(store: Store, thread: Thread, cx: &mut App) {
    gpui_kit::init(cx);
    cx.set_global(Theme::light());
    cx.set_global(SettingsOpen(false));
    cx.set_global(SidebarCollapsed(false));
    cx.set_global(SidebarWidth(Layout::SIDEBAR_WIDTH));
    cx.set_global(vega_ui::sidebar::SelectedProject(Some(
        thread.project_id.clone(),
    )));
    cx.set_global(OpenedThread(Some(thread)));
    cx.set_global(PendingDeleteConfirm(None));
    cx.set_global(vega_ui::sidebar::ProjectsCollapsed(false));
    cx.set_global(vega_ui::sidebar::SessionsCollapsed(false));
    cx.set_global(VegaStore(Ok(store)));
    vega_ui::init(cx);
}

#[test]
fn diff_controller_worker_preserves_unchanged_generation_and_rejects_stale_file() {
    let repo = diff_controller_repo();
    let (service, first) = receive_refresh(None, Some(repo.path().to_path_buf()));
    assert_eq!(first.files.len(), 1);
    let old_file = first.files[0].id;

    let (service, unchanged) = receive_refresh(Some(service), None);
    assert_eq!(unchanged.generation, first.generation);
    assert_eq!(unchanged.files[0].id, old_file);

    fs::write(
        repo.path().join("tracked.rs"),
        "fn base() {}\nfn changed_again() {}\n",
    )
    .expect("second fixture change");
    let (service, changed) = receive_refresh(Some(service), None);
    assert_ne!(changed.generation, unchanged.generation);

    let (sender, receiver) = mpsc::sync_channel(1);
    run_diff_projection_worker(
        service,
        old_file,
        tokio_util::sync::CancellationToken::new(),
        sender,
    );
    assert_eq!(
        receiver
            .recv()
            .expect("stale projection result")
            .expect_err("old file capability must fail"),
        GitWorkspaceErrorCode::StaleGeneration
    );
}

#[gpui_kit::test]
async fn diff_refresh_intents_keep_content_during_background_and_retry(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = diff_controller_repo();
    let store = Store::open(":memory:").expect("diff refresh intent store");
    store.migrate().expect("diff refresh intent migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 refresh intent root"),
        "diff-refresh-intent",
        None,
    )
    .expect("diff refresh intent project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("diff refresh intent thread");
    let thread_id = thread.id.clone();
    let project_id = thread.project_id.clone();
    cx.update(|cx| install_diff_window_globals(store, thread, cx));

    let root = cx.new(VegaWindow::new);
    let view = cx.new(|cx| DiffView::new(thread_id.clone(), project_id.clone(), cx));
    let identity = root.update(cx, |root, _| {
        root.diff_controller
            .begin(
                thread_id.clone(),
                project_id.clone(),
                view.clone(),
                DiffFocusIntent::PreserveCurrent,
            )
            .expect("diff refresh intent route")
    });

    // Initial: an explicit (progress) intent started through the production
    // entry point. One real, synchronous refresh supplies the non-empty
    // snapshot; the terminal result is injected through `finish_diff_refresh`.
    let (service, initial_snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
    let initial_generation = initial_snapshot.generation;
    let initial_head = initial_snapshot.head.clone();
    root.update(cx, |root, cx| {
        root.schedule_diff_refresh_with_progress(&identity, true, cx);
    });
    assert!(view.read_with(cx, |view, _| {
        view.is_refreshing() && view.is_refresh_progress_visible()
    }));
    finish_injected_refresh(
        &root,
        &identity,
        DiffRefreshWorkerResult::Ready {
            service: service.clone(),
            snapshot: initial_snapshot.clone(),
        },
        cx,
    );
    assert!(view.read_with(cx, |view, _| {
        !view.is_refreshing() && view.generation().is_some() && view.row_count() > 0
    }));
    let initial_stats = view
        .read_with(cx, |view, _| view.snapshot_stats())
        .expect("initial diff stats");
    assert_eq!(initial_stats.file_count, 1);

    // Background: no progress marker, old content and stats stay visible.
    root.update(cx, |root, cx| root.schedule_diff_refresh(&identity, cx));
    let background_state = view.read_with(cx, |view, _| {
        (
            view.is_refreshing(),
            view.is_refresh_progress_visible(),
            view.row_count(),
            view.snapshot_stats(),
        )
    });
    assert!(background_state.0);
    assert!(
        !background_state.1,
        "background refresh has no progress marker"
    );
    assert_eq!(background_state.2, 1, "the old file row remains visible");
    assert_eq!(background_state.3, Some(initial_stats.clone()));

    // Typed failure keeps the last snapshot and its stats.
    finish_injected_refresh(
        &root,
        &identity,
        DiffRefreshWorkerResult::Failed(GitWorkspaceErrorCode::GitFailed),
        cx,
    );
    assert_eq!(
        view.read_with(cx, |view, _| {
            (
                view.refresh_error(),
                view.row_count(),
                view.snapshot_stats(),
                view.is_refreshing(),
            )
        }),
        (
            Some(GitWorkspaceErrorCode::GitFailed),
            1,
            Some(initial_stats.clone()),
            false,
        )
    );

    // Retry: explicit progress again, then success clears the error.
    root.update(cx, |root, cx| {
        root.retry_workspace_diff(
            view.clone(),
            &DiffRetryRequested {
                thread_id: identity.thread_id.clone(),
                project_id: identity.project_id.clone(),
            },
            cx,
        );
    });
    assert!(view.read_with(cx, |view, _| {
        view.is_refreshing() && view.is_refresh_progress_visible()
    }));
    finish_injected_refresh(
        &root,
        &identity,
        DiffRefreshWorkerResult::Ready {
            service: service.clone(),
            snapshot: initial_snapshot.clone(),
        },
        cx,
    );
    assert!(view.read_with(cx, |view, _| {
        !view.is_refreshing() && view.refresh_error().is_none()
    }));

    // Clean-empty snapshot: a background refresh keeps the old row until the
    // terminal result swaps in a zero-file snapshot with 0/0/0 stats.
    let clean_snapshot = WorkspaceSnapshot {
        generation: initial_generation + 1,
        head: initial_head,
        files: Vec::new(),
        stats: WorkspaceStats {
            file_count: 0,
            additions: WorkspaceLineCount::Known(0),
            deletions: WorkspaceLineCount::Known(0),
        },
    };
    root.update(cx, |root, cx| root.schedule_diff_refresh(&identity, cx));
    assert!(view.read_with(cx, |view, _| {
        view.is_refreshing() && !view.is_refresh_progress_visible() && view.row_count() == 1
    }));
    finish_injected_refresh(
        &root,
        &identity,
        DiffRefreshWorkerResult::Ready {
            service,
            snapshot: clean_snapshot,
        },
        cx,
    );
    assert!(view.read_with(cx, |view, _| {
        !view.is_refreshing()
            && view.refresh_error().is_none()
            && view.row_count() == 0
            && view.snapshot_stats().is_some_and(|stats| {
                stats.file_count == 0
                    && stats.additions == WorkspaceLineCount::Known(0)
                    && stats.deletions == WorkspaceLineCount::Known(0)
            })
    }));
}
