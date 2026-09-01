use super::*;

pub(crate) fn scrub_fixture_git_environment(command: &mut Command) {
    let explicit_git_keys: Vec<OsString> = command
        .get_envs()
        .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in explicit_git_keys {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.as_os_str().as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
}

pub(crate) fn configure_fixture_git_environment(command: &mut Command) {
    scrub_fixture_git_environment(command);
    command
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
}

pub(crate) fn fixture_git_command(root: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new("/usr/bin/git");
    command.arg("-C").arg(root).args(args);
    configure_fixture_git_environment(&mut command);
    command
}

pub(crate) fn run_fixture_git(root: &std::path::Path, args: &[&str]) {
    let status = fixture_git_command(root, args)
        .status()
        .expect("fixture git spawn");
    assert!(status.success(), "fixture git failed: {args:?}");
}

#[test]
fn diff_controller_fixture_scrubs_hook_git_environment() {
    let sentinel = tempfile::tempdir().expect("fresh sentinel repo");
    run_fixture_git(
        sentinel.path(),
        &["init", "-q", "--initial-branch=sentinel"],
    );
    run_fixture_git(
        sentinel.path(),
        &["config", "--local", "user.name", "Vega Sentinel"],
    );
    run_fixture_git(
        sentinel.path(),
        &[
            "config",
            "--local",
            "user.email",
            "sentinel@example.invalid",
        ],
    );
    fs::write(sentinel.path().join("sentinel.txt"), "sentinel\n").expect("sentinel body");
    run_fixture_git(sentinel.path(), &["add", "--", "sentinel.txt"]);
    run_fixture_git(sentinel.path(), &["commit", "-q", "-m", "sentinel"]);

    let sentinel_ref = sentinel.path().join(".git/refs/heads/sentinel");
    let sentinel_index = sentinel.path().join(".git/index");
    let ref_before = fs::read(&sentinel_ref).expect("sentinel ref before");
    let index_before = fs::read(&sentinel_index).expect("sentinel index before");

    let fixture = tempfile::tempdir().expect("fresh isolated fixture repo");
    let run_poisoned = |args: &[&str]| {
        let mut command = Command::new("/usr/bin/git");
        command
            .arg("-C")
            .arg(fixture.path())
            .args(args)
            .env("GIT_DIR", sentinel.path().join(".git"))
            .env("GIT_WORK_TREE", sentinel.path())
            .env("GIT_INDEX_FILE", &sentinel_index);
        configure_fixture_git_environment(&mut command);
        let status = command.status().expect("poisoned fixture git spawn");
        assert!(status.success(), "poisoned fixture git failed: {args:?}");
    };

    run_poisoned(&["init", "-q", "--initial-branch=fixture"]);
    run_poisoned(&["config", "--local", "user.name", "Vega Fixture"]);
    run_poisoned(&["config", "--local", "user.email", "fixture@example.invalid"]);
    fs::write(fixture.path().join("fixture.txt"), "fixture\n").expect("fixture body");
    run_poisoned(&["add", "--", "fixture.txt"]);
    run_poisoned(&["commit", "-q", "-m", "fixture"]);

    assert!(fixture.path().join(".git").is_dir());
    assert!(fixture.path().join("fixture.txt").is_file());
    assert_eq!(
        fs::read(&sentinel_ref).expect("sentinel ref after"),
        ref_before
    );
    assert_eq!(
        fs::read(&sentinel_index).expect("sentinel index after"),
        index_before
    );
    assert_eq!(
        fs::read(sentinel.path().join("sentinel.txt")).expect("sentinel body after"),
        b"sentinel\n"
    );
    assert!(!sentinel.path().join("fixture.txt").exists());
}

pub(crate) fn diff_controller_repo() -> TempDir {
    let repo = tempfile::tempdir().expect("fresh diff controller repo");
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
    cx.set_global(Theme::light());
    cx.set_global(SettingsOpen(false));
    cx.set_global(SidebarCollapsed(false));
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

#[gpui::test]
async fn diff_controller_real_finish_drops_superseded_result_and_global_switch_closes_route(
    cx: &mut gpui::TestAppContext,
) {
    let repo = diff_controller_repo();
    let (service, snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
    let snapshot_generation = snapshot.generation;
    let file_id = snapshot.files[0].id;
    let (projection_sender, projection_receiver) = mpsc::sync_channel(1);
    run_diff_projection_worker(
        service.clone(),
        file_id,
        tokio_util::sync::CancellationToken::new(),
        projection_sender,
    );
    let projection = projection_receiver
        .recv()
        .expect("pending projection worker")
        .expect("pending projection");
    let store = Store::open(":memory:").expect("diff window memory store");
    store.migrate().expect("diff window migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 fixture root"),
        "diff",
        None,
    )
    .expect("diff window project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("diff window thread");
    let thread_id = thread.id.clone();
    let project_id = thread.project_id.clone();
    cx.update(|cx| install_diff_window_globals(store, thread, cx));
    let root = cx.new(VegaWindow::new);
    let view = cx.new(|cx| DiffView::new(thread_id.clone(), project_id.clone(), cx));
    let identity = root.update(cx, |root, _| {
        root.diff_controller
            .begin(thread_id, project_id, view.clone())
            .expect("diff route")
    });
    root.update(cx, |root, cx| {
        let active = root
            .diff_controller
            .active
            .as_mut()
            .expect("active diff route");
        assert_eq!(active.request_refresh(), DiffRefreshDecision::Start(1));
        assert_eq!(active.request_refresh(), DiffRefreshDecision::Coalesced);
        active.snapshot_generation = Some(snapshot_generation);
        let pending_fence = active
            .next_projection_fence(snapshot_generation, file_id)
            .expect("pending projection fence");
        active.pending_projection = Some(PendingDiffProjection {
            fence: pending_fence,
            result: Ok(projection),
        });
        root.finish_diff_refresh(
            &identity,
            1,
            DiffRefreshWorkerResult::Ready { service, snapshot },
            cx,
        );
        assert_eq!(view.read(cx).generation(), None, "R1 must not reach the UI");
        assert!(
            root.diff_controller
                .active
                .as_ref()
                .is_some_and(|active| active.pending_projection.is_some()),
            "R1 must not release a projection while R2 is outstanding"
        );
        assert_eq!(
            root.diff_controller
                .active
                .as_ref()
                .and_then(|active| active.refresh_in_flight),
            Some(2),
            "only the latest queued refresh may remain active"
        );
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| window_root)
            .expect("diff controller focus window")
    });
    cx.run_until_parked();
    assert!(root.read_with(cx, |root, _| {
        root.diff_controller
            .active
            .as_ref()
            .is_some_and(|active| !active.focus_pending)
    }));
    let focused = window
        .update(cx, |_, window, cx| {
            view.read(cx).focus_handle(cx).is_focused(window)
        })
        .expect("diff controller focus window");
    assert!(focused, "the visible DiffView must receive one-shot focus");
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    assert!(root.read_with(cx, |root, _| root.diff_controller.active.is_none()));

    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    let exhausted_view = cx.new(|cx| DiffView::new("thread".into(), "project".into(), cx));
    let exhausted_cancel = root.update(cx, |root, cx| {
        let identity = root
            .diff_controller
            .begin(
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .expect("current thread")
                    .id
                    .clone(),
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .expect("current thread")
                    .project_id
                    .clone(),
                exhausted_view.clone(),
            )
            .expect("exhausted route");
        let active = root
            .diff_controller
            .active
            .as_mut()
            .expect("exhausted active route");
        active.file_request_seq = u64::MAX;
        let cancel = active.cancel.clone();
        root.request_diff_projection(
            exhausted_view,
            &DiffProjectionRequested {
                thread_id: identity.thread_id,
                project_id: identity.project_id,
                generation: snapshot_generation,
                file_id,
            },
            cx,
        );
        assert!(root.diff_controller.active.is_none());
        cancel
    });
    assert!(exhausted_cancel.is_cancelled());
}

#[gpui::test]
async fn diff_controller_route_latest_poll_tool_and_cross_project_fences(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        cx.set_global(OpenedThread(Some(Thread {
            id: "thread-b".into(),
            project_id: "project-b".into(),
            title: String::new(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: String::new(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        })));
        vega_ui::init(cx);
    });
    let repo = diff_controller_repo();
    let (_, snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
    let file_id = snapshot.files[0].id;
    let first_view = cx.new(|cx| DiffView::new("thread-a".into(), "project-a".into(), cx));
    let second_view = cx.new(|cx| DiffView::new("thread-b".into(), "project-b".into(), cx));
    let mut controller = DiffController::default();
    let first_route = controller
        .begin("thread-a".into(), "project-a".into(), first_view)
        .expect("first route");
    let first_cancel = controller
        .active
        .as_ref()
        .expect("first active")
        .cancel
        .clone();
    let second_route = controller
        .begin("thread-b".into(), "project-b".into(), second_view)
        .expect("second route");
    assert!(first_cancel.is_cancelled());
    assert!(!controller.matches(&first_route));
    assert!(controller.matches(&second_route));
    assert!(
        controller
            .active
            .as_ref()
            .is_some_and(|active| active.focus_pending)
    );
    cx.update(|cx| {
        assert!(VegaWindow::diff_route_is_current(&second_route, cx));
        cx.set_global(SettingsOpen(true));
        assert!(!VegaWindow::diff_route_is_current(&second_route, cx));
        cx.set_global(SettingsOpen(false));
        let mut other = cx
            .global::<OpenedThread>()
            .0
            .clone()
            .expect("opened thread fixture");
        other.id = "thread-c".into();
        cx.set_global(OpenedThread(Some(other)));
        assert!(!VegaWindow::diff_route_is_current(&second_route, cx));
    });

    let active = controller.active.as_mut().expect("second active");
    active.snapshot_generation = Some(snapshot.generation);
    assert_eq!(
        active.request_refresh(),
        DiffRefreshDecision::Start(1),
        "initial/poll refresh starts one worker"
    );
    assert_eq!(
        active.request_refresh(),
        DiffRefreshDecision::Coalesced,
        "tool terminal coalesces while the poll refresh is active"
    );
    assert_eq!(active.refresh_request_seq, 2);
    assert_eq!(active.refresh_in_flight, Some(1));
    assert_eq!(active.queued_refresh_seq, Some(2));
    assert_eq!(
        active.complete_refresh(1),
        Some(DiffRefreshCompletion::Superseded(Some(2))),
        "the pre-terminal poll result is dropped and only queues R2"
    );
    assert_eq!(active.refresh_in_flight, Some(2));
    assert_eq!(
        active.complete_refresh(2),
        Some(DiffRefreshCompletion::Latest)
    );

    let older = active
        .next_projection_fence(snapshot.generation, file_id)
        .expect("older file request");
    let latest = active
        .next_projection_fence(snapshot.generation, file_id)
        .expect("latest file request");
    assert_eq!(
        active.projection_disposition(&older),
        DiffProjectionDisposition::Drop
    );
    assert_eq!(
        active.projection_disposition(&latest),
        DiffProjectionDisposition::Apply
    );
    assert_eq!(active.request_refresh(), DiffRefreshDecision::Start(3));
    assert_eq!(
        active.projection_disposition(&latest),
        DiffProjectionDisposition::Defer,
        "a projection waits for an in-flight refresh"
    );
    active.refresh_in_flight = None;
    assert_eq!(
        active.projection_disposition(&latest),
        DiffProjectionDisposition::Apply,
        "unchanged generation survives a newer completed refresh"
    );
    let mut wrong_project = latest.clone();
    wrong_project.route.project_id = "project-a".into();
    assert_eq!(
        active.projection_disposition(&wrong_project),
        DiffProjectionDisposition::Drop
    );
    active.snapshot_generation = Some(snapshot.generation + 1);
    assert_eq!(
        active.projection_disposition(&latest),
        DiffProjectionDisposition::Drop
    );
    assert_eq!(DIFF_REFRESH_INTERVAL, Duration::from_millis(750));
}
