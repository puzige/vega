use super::*;
use gpui_kit::TestAppContext;

fn write_head(root: &Path, head: &str) {
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/HEAD"), head).unwrap();
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
        std::thread::yield_now();
    }
}
#[gpui_kit::test]
async fn production_sidebar_refreshes_head_metadata_and_rejects_removed_results(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().unwrap();
    let root = owned.path().canonicalize().unwrap();
    write_head(&root, "ref: refs/heads/main\n");
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
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| visible))
        .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("projects-section-label-项目").is_some());
    assert!(
        visual
            .debug_bounds("projects-section-label-PROJECTS")
            .is_none()
    );
    wait_for_suffix(&view, &project.id, Some("main"), cx);
    write_head(&root, "ref: refs/heads/other\n");
    cx.executor().advance_clock(Duration::from_millis(1999));
    view.update(cx, |view, cx| view.poll_branches(cx));
    assert_eq!(
        view.read_with(cx, |view, _| view
            .branch_suffix(&project)
            .map(str::to_owned)),
        Some("main".into())
    );
    cx.executor().advance_clock(Duration::from_millis(1));
    wait_for_suffix(&view, &project.id, Some("other"), cx);
    view.update(cx, |view, cx| view.select_project(&project.id, cx));
    assert!(view.read_with(cx, |view, _| view.branch_suffix(&project).is_none()));
    wait_for_suffix(&view, &project.id, Some("other"), cx);
    write_head(&root, "1111111111111111111111111111111111111111\n");
    view.update(cx, |view, cx| view.reload(cx));
    wait_for_suffix(&view, &project.id, Some("detached"), cx);
    std::fs::write(root.join(".git/HEAD"), "invalid HEAD\n").unwrap();
    view.update(cx, |view, cx| view.reload(cx));
    wait_for_suffix(&view, &project.id, None, cx);
    view.update(cx, |view, _| view.start_branch_refresh());
    let deadline = Instant::now() + Duration::from_secs(5);
    let completion = loop {
        if let Some(completion) = view.read_with(cx, |view, _| {
            view.branch_service.as_ref().unwrap().take_completion()
        }) {
            break completion;
        }
        assert!(
            Instant::now() < deadline,
            "pending branch read did not complete"
        );
        std::thread::yield_now();
    };
    view.update(cx, |view, cx| view.remove_project(&project.id, cx));
    view.update(cx, |view, cx| {
        view.poll_branches(cx);
        assert_ne!(completion.generation, view.branch_generation);
        view.branch_completion = Some(completion);
        view.branch_probe = true;
        view.branch_rendered = true;
        view.poll_branches(cx);
        assert!(view.branch_completion.is_none());
        assert!(view.projects.is_empty());
        assert!(view.branches.is_empty());
    });
}

#[gpui_kit::test]
async fn hidden_sidebar_cancels_pending_and_generation_path_guard(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().unwrap();
    let root = owned.path().canonicalize().unwrap();
    write_head(&root, "ref: refs/heads/main\n");
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
