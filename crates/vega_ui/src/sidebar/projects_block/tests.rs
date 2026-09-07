use super::*;
use gpui::TestAppContext;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(output.status.success(), "owned Git command failed");
}
fn wait_for_suffix(
    view: &Entity<ProjectsBlock>,
    id: &str,
    expected: Option<&str>,
    cx: &mut TestAppContext,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        view.update(cx, |view, cx| view.poll_branches(cx));
        cx.run_until_parked();
        let done = view.read_with(cx, |view, _| {
            let project = view.projects.iter().find(|p| p.id == id).unwrap();
            !view.branch_pending
                && view.branch_suffix(project) == expected
                && view.branches.contains_key(id)
        });
        if done {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "production sidebar suffix did not converge"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[gpui::test]
async fn production_sidebar_refreshes_real_checkout_and_rejects_removed_results(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().unwrap();
    let root = owned.path().canonicalize().unwrap();
    git(&root, &["init", "-b", "main"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Vega Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let store = Store::open(root.join("owned.sqlite")).unwrap();
    store.migrate().unwrap();
    let project = projects::create(
        store.conn(),
        root.to_str().unwrap(),
        "owned",
        Some("wrong-cached-value"),
    )
    .unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id.clone())));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(crate::settings::SettingsOpen(false));
    });
    let view = cx.new(ProjectsBlock::new);
    let visible = view.clone();
    let _window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| visible))
        .unwrap();
    wait_for_suffix(&view, &project.id, Some("main"), cx);
    git(&root, &["checkout", "-b", "other"]);
    // Periodic production poll discovers this actual external checkout, with no reload/label injection.
    wait_for_suffix(&view, &project.id, Some("other"), cx);
    view.update(cx, |view, cx| view.select_project(&project.id, cx));
    assert!(view.read_with(cx, |view, _| view.branch_suffix(&project).is_none()));
    wait_for_suffix(&view, &project.id, Some("other"), cx);
    git(&root, &["checkout", "--detach"]);
    view.update(cx, |view, cx| view.reload(cx));
    wait_for_suffix(&view, &project.id, Some("detached"), cx);
    std::fs::write(root.join(".git/HEAD"), "invalid HEAD\n").unwrap();
    view.update(cx, |view, cx| view.reload(cx));
    wait_for_suffix(&view, &project.id, None, cx);
    view.update(cx, |view, _| view.start_branch_refresh());
    view.update(cx, |view, cx| view.remove_project(&project.id, cx));
    std::thread::sleep(Duration::from_millis(50));
    view.update(cx, |view, cx| {
        view.poll_branches(cx);
        assert!(view.projects.is_empty());
        assert!(view.branches.is_empty());
    });
}

#[gpui::test]
async fn hidden_sidebar_cancels_pending_and_generation_path_guard(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().unwrap();
    let root = owned.path().canonicalize().unwrap();
    git(&root, &["init", "-b", "main"]);
    let store = Store::open(root.join("owned.sqlite")).unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), root.to_str().unwrap(), "owned", None).unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SidebarCollapsed(false));
    });
    let view = cx.new(ProjectsBlock::new);
    view.update(cx, |view, _| view.start_branch_refresh());
    cx.update(|cx| cx.set_global(SidebarCollapsed(true)));
    view.update(cx, |view, cx| {
        view.poll_branches(cx);
        assert!(!view.branch_pending);
        assert!(view.branches.is_empty());
    });
    // This narrow seam verifies identity/path validation; happy-path evidence above uses real filesystem reads.
    view.update(cx, |view, _| {
        view.branches.insert(
            project.id.clone(),
            (
                "different registered path".into(),
                ProjectBranchState::Branch("stale".into()),
            ),
        );
        assert!(view.branch_suffix(&project).is_none());
    });
}

#[gpui::test]
async fn r14_registration_completion_cannot_mutate_replaced_database_owner(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("first.sqlite")).unwrap();
    store.migrate().unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(None));
        cx.set_global(crate::settings::SettingsOpen(false));
    });
    let view = cx.new(ProjectsBlock::new);
    view.update(cx, |view, cx| view.register_path(dir.path(), cx));
    let replacement = Store::open(dir.path().join("replacement.sqlite")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(replacement)));
        cx.set_global(crate::navigation::TaskMutationState {
            epoch: 100,
            pending: 1,
        });
    });
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.projects.is_empty());
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            1
        );
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().epoch,
            100
        );
    });
    assert_eq!(
        vega_conversation::sidebar_organization::snapshot(
            &Store::open(dir.path().join("first.sqlite")).unwrap()
        )
        .unwrap()
        .projects
        .len(),
        1
    );
}
