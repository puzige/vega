use super::*;

struct MountedSidebar {
    sidebar: Entity<Sidebar>,
    draft: Entity<TextInput>,
}

impl Render for MountedSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .on_action(|_: &CloseSettings, _, cx| cx.set_global(SettingsOpen(false)))
            .size_full()
            .flex()
            .child(self.sidebar.clone())
            .child(div().flex_1().child(self.draft.clone()))
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    sidebar: Entity<Sidebar>,
    draft: Entity<TextInput>,
    window: gpui_kit::WindowHandle<MountedSidebar>,
    first: Thread,
    other: Thread,
}

fn fixture(cx: &mut gpui_kit::TestAppContext) -> Fixture {
    fixture_with_size(cx, 1200., 760.)
}

fn fixture_with_size(cx: &mut gpui_kit::TestAppContext, width: f32, height: f32) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("organization.db")).unwrap();
    store.migrate().unwrap();
    for id in ["p", "q"] {
        store
            .conn()
            .execute(
                "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES(?1,?2,?1,0,?3)",
                (
                    id,
                    dir.path().join(id).to_string_lossy().to_string(),
                    if id == "p" { 2 } else { 1 },
                ),
            )
            .unwrap();
    }
    let first = conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    for _ in 0..5 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    let other = conversation::create_thread(&store, "q", "model", "confirm").unwrap();
    store
        .conn()
        .execute(
            "UPDATE threads SET created_at = 9000000000000, updated_at = 9000000000000 WHERE id = ?1",
            [&other.id],
        )
        .unwrap();
    conversation::update_thread(
        &store,
        &other.id,
        &ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        crate::init(cx);
        // Match the application-level binding and fallback handler: otherwise
        // Escape reaches raw key_down and hides a real action-dispatch defect.
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "escape",
            CloseSettings,
            Some("VegaWindow"),
        )]);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some("p".into())));
        cx.set_global(OpenedThread(Some(first.clone())));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(SettingsOpen(false));
    });
    let sidebar = cx.new(Sidebar::new);
    let draft = cx.new(|cx| TextInput::new(cx, "Draft", false));
    draft.update(cx, |input, cx| input.set_text("owned unsent draft", cx));
    let s = sidebar.clone();
    let d = draft.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                    gpui_kit::Bounds::centered(None, gpui_kit::size(px(width), px(height)), cx),
                )),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| MountedSidebar {
                    sidebar: s,
                    draft: d,
                })
            },
        )
        .unwrap()
    });
    cx.run_until_parked();
    Fixture {
        dir,
        sidebar,
        draft,
        window,
        first,
        other,
    }
}

fn click(f: &Fixture, cx: &mut gpui_kit::TestAppContext, selector: impl Into<String>) {
    let selector: &'static str = Box::leak(selector.into().into_boxed_str());
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
}

fn bounds(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    selector: impl Into<String>,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    let selector: &'static str = Box::leak(selector.into().into_boxed_str());
    gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn absent(f: &Fixture, cx: &mut gpui_kit::TestAppContext, selector: impl Into<String>) -> bool {
    let selector: &'static str = Box::leak(selector.into().into_boxed_str());
    gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .is_none()
}

fn snapshot(f: &Fixture) -> SidebarOrganizationSnapshot {
    service::snapshot(&Store::open(f.dir.path().join("organization.db")).unwrap()).unwrap()
}

fn sessions(f: &Fixture, cx: &gpui_kit::TestAppContext) -> Entity<ThreadsBlock> {
    f.sidebar
        .read_with(cx, |sidebar, _| sidebar.sessions_block.clone())
}

#[gpui_kit::test]
async fn r15_route_guard_preserves_draft_and_switches_project_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    assert_eq!(snapshot(&f).threads.len(), 7);
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );

    cx.update(|cx| {
        cx.set_global(crate::navigation::DraftNavigationGuard {
            task: f.first.id.clone(),
            input: f.draft.downgrade(),
            cached_tasks: 100,
            cached_bytes: 0,
        })
    });
    click(&f, cx, format!("project-thread-row-{}", f.other.id));
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        assert!(
            cx.global::<crate::navigation::NavigationState>()
                .error
                .is_some()
        );
    });
    assert!(
        snapshot(&f)
            .threads
            .iter()
            .find(|thread| thread.id == f.other.id)
            .unwrap()
            .unread,
        "blocked navigation must not visit or mutate the destination task"
    );
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );

    cx.update(|cx| {
        cx.update_global::<crate::navigation::DraftNavigationGuard, _>(|guard, _| {
            guard.cached_tasks = 0
        })
    });
    click(&f, cx, format!("project-thread-row-{}", f.other.id));
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.other.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("q"));
    });
    assert!(
        !snapshot(&f)
            .threads
            .iter()
            .find(|thread| thread.id == f.other.id)
            .unwrap()
            .unread
    );
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );
}

#[gpui_kit::test]
async fn r15_project_registration_reveals_dedupes_and_scopes_new_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let first = f.dir.path().join("first-folder");
    let second = f.dir.path().join("second-folder");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    cx.update(|cx| cx.set_global(SidebarCollapsed(true)));

    let register = |path: &std::path::Path, cx: &mut gpui_kit::TestAppContext| {
        f.sidebar.update(cx, |sidebar, cx| {
            sidebar
                .projects_block
                .update(cx, |projects, cx| projects.register_path(path, cx));
        });
        cx.run_until_parked();
    };
    register(&first, cx);
    let first_id = cx.update(|cx| {
        assert!(!cx.global::<SidebarCollapsed>().0);
        cx.global::<SelectedProject>().0.clone().unwrap()
    });
    let state = snapshot(&f);
    assert!(state.projects.iter().any(|project| project.id == first_id));
    assert!(
        state
            .threads
            .iter()
            .all(|thread| thread.project_id != first_id)
    );

    click(&f, cx, format!("project-header-{first_id}"));
    assert!(
        snapshot(&f)
            .collapsed
            .contains(&SidebarCollapseTarget::Project(first_id.clone()))
    );
    register(&first.join("."), cx);
    assert_eq!(snapshot(&f).projects.len(), state.projects.len());
    assert!(
        !snapshot(&f)
            .collapsed
            .contains(&SidebarCollapseTarget::Project(first_id.clone()))
    );

    register(&second, cx);
    let second_id = cx.update(|cx| cx.global::<SelectedProject>().0.clone().unwrap());
    assert_ne!(first_id, second_id);
    click(&f, cx, "sidebar-new-task");
    let state = snapshot(&f);
    assert_eq!(
        state
            .threads
            .iter()
            .filter(|thread| thread.project_id == second_id)
            .count(),
        1
    );
    assert!(state.memberships.is_empty());
    let selection = cx.update(|cx| cx.global::<SelectedProject>().0.clone());
    register(&f.dir.path().join("missing-folder"), cx);
    assert_eq!(snapshot(&f), state);
    assert_eq!(
        cx.update(|cx| cx.global::<SelectedProject>().0.clone()),
        selection
    );
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_owned()),
        "owned unsent draft"
    );
    let reopened = Store::open(f.dir.path().join("organization.db")).unwrap();
    assert_eq!(service::snapshot(&reopened).unwrap(), state);
}

#[gpui_kit::test]
async fn r15_sidebar_has_only_projects_and_standalone_sessions(cx: &mut gpui_kit::TestAppContext) {
    // This exercises the mounted route at the product's default 1200×760
    // viewport. Legacy group/filter selectors must no longer be in the tree.
    let f = fixture(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("organization-groups").is_none());
    assert!(visual.debug_bounds("organization-filter").is_none());
    assert!(visual.debug_bounds("organization-collapse-all").is_none());
    assert!(visual.debug_bounds("organization-new-session").is_some());
    assert!(visual.debug_bounds("organization-recents-empty").is_some());
    assert!(visual.debug_bounds("organization-add-project").is_some());
    assert!(visual.debug_bounds("project-header-p").is_some());
    assert!(visual.debug_bounds("project-add-p").is_some());
    assert!(visual.debug_bounds("project-more-p").is_some());

    let project_row = bounds(&f, cx, "project-header-p");
    let project_folder = bounds(&f, cx, "project-folder-p-open");
    let project_label = bounds(&f, cx, "organization-project-p");
    let task_title = bounds(&f, cx, format!("project-thread-row-title-{}", f.first.id));
    assert!(absent(&f, cx, "project-folder-p-closed"));
    assert!(
        (f32::from(project_folder.left() - project_row.left()) - 8.0).abs() <= 1.0,
        "folder must start at the content inset without a Chevron slot"
    );
    assert!(
        f32::from(project_label.left() - task_title.left()).abs() <= 1.0,
        "project label and child task title must share the same content column"
    );
    assert_eq!(f32::from(bounds(&f, cx, "project-add-p").size.width), 24.0);
    assert_eq!(f32::from(bounds(&f, cx, "project-more-p").size.width), 24.0);

    click(&f, cx, "organization-new-session");
    let standalone_id = cx.update(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none());
        cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()
    });
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let standalone_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE project_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(standalone_count, 1);
    let standalone_selector: &'static str =
        Box::leak(format!("standalone-thread-row-{standalone_id}").into_boxed_str());
    assert!(visual.debug_bounds(standalone_selector).is_some());
    assert!(visual.debug_bounds("organization-recents-empty").is_none());

    click(&f, cx, "project-add-p");
    let project_tasks: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE project_id = 'p'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(project_tasks, 7);
    assert_eq!(
        store
            .conn()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM threads WHERE project_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    let project_task_id = cx.update(|cx| {
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()
    });
    let project_selector: &'static str =
        Box::leak(format!("project-thread-row-{project_task_id}").into_boxed_str());
    let standalone_project_selector: &'static str =
        Box::leak(format!("standalone-thread-row-{project_task_id}").into_boxed_str());
    assert!(visual.debug_bounds(project_selector).is_some());
    assert!(visual.debug_bounds(standalone_project_selector).is_none());

    click(&f, cx, "project-header-p");
    assert!(
        snapshot(&f)
            .collapsed
            .contains(&SidebarCollapseTarget::Project("p".into()))
    );
    let _ = bounds(&f, cx, "project-folder-p-closed");
    assert!(absent(&f, cx, "project-folder-p-open"));
    assert!(absent(
        &f,
        cx,
        format!("project-thread-row-title-{}", f.first.id)
    ));
    click(&f, cx, "project-header-p");
    let _ = bounds(&f, cx, "project-folder-p-open");
    let _ = bounds(&f, cx, format!("project-thread-row-title-{}", f.first.id));
}

#[gpui_kit::test]
async fn r26_sidebar_projects_each_task_once_and_reveals_contextual_actions(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let pinned_standalone =
        conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    conversation::set_thread_pinned(&store, &pinned_standalone.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let pinned = bounds(&f, cx, "organization-section-pinned");
    let projects = bounds(&f, cx, "organization-section-projects");
    let recents = bounds(&f, cx, "organization-section-recents");
    assert!(pinned.top() < projects.top() && projects.top() < recents.top());

    assert!(!absent(
        &f,
        cx,
        format!("pinned-thread-row-{}", pinned_standalone.id)
    ));
    assert!(absent(
        &f,
        cx,
        format!("standalone-thread-row-{}", pinned_standalone.id)
    ));
    assert!(!absent(&f, cx, format!("pinned-thread-row-{}", f.first.id)));
    assert!(!absent(
        &f,
        cx,
        format!("pinned-thread-row-project-{}", f.first.id)
    ));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
    assert!(!absent(
        &f,
        cx,
        format!("standalone-thread-row-{}", recent.id)
    ));
    assert!(absent(&f, cx, format!("pinned-thread-row-{}", recent.id)));

    // Selected project rows remain quiet, while all controls stay mounted.
    assert!(!absent(&f, cx, "project-add-p"));
    assert!(!absent(&f, cx, "project-more-p"));
    assert!(!absent(&f, cx, "project-actions-p-rest"));
    assert!(!absent(&f, cx, "organization-projects-actions-rest"));
    assert!(!absent(&f, cx, "organization-recents-actions-rest"));
    assert!(!absent(
        &f,
        cx,
        format!("thread-actions-state-{}-rest", recent.id)
    ));
    assert!(!absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
    let projects_actions_rest = bounds(&f, cx, "organization-projects-actions-rest");
    let project_actions_rest = bounds(&f, cx, "project-actions-p-rest");
    let task_actions_rest = bounds(&f, cx, format!("thread-actions-state-{}-rest", recent.id));

    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let projects_header = visual.debug_bounds("organization-header-projects").unwrap();
    visual.simulate_mouse_move(
        projects_header.center(),
        None,
        gpui_kit::Modifiers::default(),
    );
    assert!(!absent(&f, cx, "organization-projects-actions-visible"));
    assert_eq!(
        bounds(&f, cx, "organization-projects-actions-visible").size,
        projects_actions_rest.size
    );

    let project_row = bounds(&f, cx, "project-header-p");
    visual.simulate_mouse_move(project_row.center(), None, gpui_kit::Modifiers::default());
    assert!(!absent(&f, cx, "project-actions-p-visible"));
    assert_eq!(
        bounds(&f, cx, "project-actions-p-visible").size,
        project_actions_rest.size
    );

    let task_row = bounds(&f, cx, format!("standalone-thread-row-{}", recent.id));
    visual.simulate_mouse_move(task_row.center(), None, gpui_kit::Modifiers::default());
    assert!(!absent(
        &f,
        cx,
        format!("thread-actions-state-{}-visible", recent.id)
    ));
    assert_eq!(
        bounds(
            &f,
            cx,
            format!("thread-actions-state-{}-visible", recent.id)
        )
        .size,
        task_actions_rest.size
    );
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));

    visual.simulate_click(project_row.center(), gpui_kit::Modifiers::default());
    visual.simulate_mouse_move(
        gpui_kit::point(gpui_kit::px(900.), gpui_kit::px(700.)),
        None,
        gpui_kit::Modifiers::default(),
    );
    assert!(!absent(&f, cx, "project-actions-p-rest"));
    f.window
        .update(cx, |_, window, cx| window.focus_next(cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!absent(&f, cx, "project-actions-p-visible"));

    let recents_focus =
        sessions(&f, cx).read_with(cx, |block, _| block.recents_header_focus.clone());
    f.window
        .update(cx, |_, window, cx| recents_focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!absent(&f, cx, "organization-recents-actions-visible"));

    let task_focus = sessions(&f, cx).read_with(cx, |block, _| {
        block.thread_action_focuses.get(&recent.id).unwrap().clone()
    });
    visual.simulate_mouse_move(
        gpui_kit::point(gpui_kit::px(900.), gpui_kit::px(700.)),
        None,
        gpui_kit::Modifiers::default(),
    );
    assert!(!absent(
        &f,
        cx,
        format!("thread-actions-state-{}-rest", recent.id)
    ));
    assert!(!absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
    f.window
        .update(cx, |_, window, cx| task_focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!absent(
        &f,
        cx,
        format!("thread-actions-state-{}-visible", recent.id)
    ));
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));

    click(&f, cx, "project-more-p");
    assert!(!absent(&f, cx, "organization-menu"));
    assert!(!absent(&f, cx, "project-actions-p-visible"));
}

#[gpui_kit::test]
async fn r27_sidebar_rows_share_title_origin_and_keep_stable_metadata_columns(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    store
        .conn()
        .execute("UPDATE projects SET name = 'P' WHERE id = 'p'", [])
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE projects SET name = 'a-very-long-project-name' WHERE id = 'q'",
            [],
        )
        .unwrap();
    let pinned_standalone =
        conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    conversation::set_thread_pinned(&store, &f.other.id, true).unwrap();
    conversation::set_thread_pinned(&store, &pinned_standalone.id, true).unwrap();
    conversation::update_thread(
        &store,
        &f.other.id,
        &ThreadUpdate {
            unread: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let child = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && !thread.pinned)
        .unwrap();
    let pinned_row = bounds(&f, cx, format!("pinned-thread-row-{}", f.first.id));
    let pinned_title = bounds(&f, cx, format!("pinned-thread-row-title-{}", f.first.id));
    let child_row = bounds(&f, cx, format!("project-thread-row-{}", child.id));
    let child_title = bounds(&f, cx, format!("project-thread-row-title-{}", child.id));
    let recent_row = bounds(&f, cx, format!("standalone-thread-row-{}", recent.id));
    let recent_title = bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id));
    for (row, title) in [
        (pinned_row, pinned_title),
        (child_row, child_title),
        (recent_row, recent_title),
    ] {
        assert_eq!(
            f32::from(title.left() - row.left()),
            Layout::SIDEBAR_NAV_CONTENT_INSET
        );
    }
    assert_eq!(pinned_title.left(), child_title.left());
    assert_eq!(child_title.left(), recent_title.left());
    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-pin-{}", f.first.id)
    ));
    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-pin-{}", pinned_standalone.id)
    ));

    let project_row = bounds(&f, cx, "project-header-p");
    let folder = bounds(&f, cx, "project-folder-p-open");
    let label = bounds(&f, cx, "organization-project-p");
    assert_eq!(f32::from(folder.left() - project_row.left()), 8.0);
    assert_eq!(f32::from(label.left() - folder.right()), 8.0);
    assert_eq!(f32::from(label.left() - project_row.left()), 32.0);

    let short_metadata = bounds(&f, cx, format!("pinned-thread-row-project-{}", f.first.id));
    let long_metadata = bounds(&f, cx, format!("pinned-thread-row-project-{}", f.other.id));
    assert_eq!(
        f32::from(short_metadata.size.width),
        Layout::SIDEBAR_PROJECT_METADATA_WIDTH
    );
    assert_eq!(short_metadata.size.width, long_metadata.size.width);
    assert_eq!(short_metadata.left(), long_metadata.left());

    let rest_tail = bounds(&f, cx, format!("thread-actions-state-{}-rest", f.first.id));
    let long_tail = bounds(&f, cx, format!("thread-actions-state-{}-rest", f.other.id));
    assert_eq!(
        f32::from(rest_tail.size.width),
        Layout::SIDEBAR_ACTIONS_WIDTH
    );
    assert_eq!(rest_tail.left(), long_tail.left());
    let rest_title = pinned_title;
    let rest_metadata = short_metadata;
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(pinned_row.center(), None, gpui_kit::Modifiers::default());
    let hover_title = bounds(&f, cx, format!("pinned-thread-row-title-{}", f.first.id));
    let hover_metadata = bounds(&f, cx, format!("pinned-thread-row-project-{}", f.first.id));
    let hover_tail = bounds(
        &f,
        cx,
        format!("thread-actions-state-{}-visible", f.first.id),
    );
    assert_eq!(rest_title.origin, hover_title.origin);
    assert_eq!(rest_metadata.origin, hover_metadata.origin);
    assert_eq!(rest_tail.origin, hover_tail.origin);
}

#[gpui_kit::test]
async fn r26_empty_pinned_section_is_absent(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    assert!(absent(&f, cx, "organization-section-pinned"));
    assert!(!absent(&f, cx, "organization-section-projects"));
    assert!(!absent(&f, cx, "organization-section-recents"));
}

#[gpui_kit::test]
async fn r26_pin_mutation_persists_and_reprojects_between_pinned_and_project(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    click(&f, cx, format!("pinned-thread-actions-{}", f.first.id));
    click(&f, cx, format!("pinned-thread-action-{}-0", f.first.id));
    assert!(
        !snapshot(&f)
            .threads
            .iter()
            .find(|thread| thread.id == f.first.id)
            .unwrap()
            .pinned
    );
    assert!(absent(&f, cx, format!("pinned-thread-row-{}", f.first.id)));
    assert!(!absent(
        &f,
        cx,
        format!("project-thread-row-{}", f.first.id)
    ));

    click(&f, cx, format!("project-thread-actions-{}", f.first.id));
    click(&f, cx, format!("project-thread-action-{}-0", f.first.id));
    assert!(
        snapshot(&f)
            .threads
            .iter()
            .find(|thread| thread.id == f.first.id)
            .unwrap()
            .pinned
    );
    assert!(!absent(&f, cx, format!("pinned-thread-row-{}", f.first.id)));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
}

#[gpui_kit::test]
async fn r15_archive_filter_hides_and_restores_standalone_task(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    click(&f, cx, "organization-new-session");
    let standalone_id = cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone());
    let row_selector: &'static str =
        Box::leak(format!("standalone-thread-row-{standalone_id}").into_boxed_str());
    let actions_selector: &'static str =
        Box::leak(format!("standalone-thread-actions-{standalone_id}").into_boxed_str());
    let archive_selector: &'static str =
        Box::leak(format!("standalone-thread-action-{standalone_id}-2").into_boxed_str());

    click(&f, cx, actions_selector);
    assert!(
        gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(archive_selector)
            .is_some()
    );
    click(&f, cx, archive_selector);
    let archived = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.id == standalone_id)
        .unwrap();
    assert_eq!(archived.status, ThreadStatus::Archived);
    assert!(
        gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(row_selector)
            .is_none(),
        "archived tasks are hidden by default"
    );

    click(&f, cx, "organization-session-sort");
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("organization-menu-0").is_some());
    assert!(visual.debug_bounds("organization-menu-1").is_some());
    assert!(visual.debug_bounds("organization-menu-2").is_some());
    assert!(visual.debug_bounds("organization-menu-3").is_none());
    click(&f, cx, "organization-menu-2");
    assert!(
        visual.debug_bounds(row_selector).is_some(),
        "show archived must restore the task to its sessions position"
    );

    click(&f, cx, actions_selector);
    click(&f, cx, archive_selector);
    let restored = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.id == standalone_id)
        .unwrap();
    assert_eq!(restored.status, ThreadStatus::Active);
    assert!(visual.debug_bounds(row_selector).is_some());

    let project_task_id = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && thread.status == ThreadStatus::Active)
        .unwrap()
        .id;
    let project_actions_selector: &'static str =
        Box::leak(format!("project-thread-actions-{project_task_id}").into_boxed_str());
    let project_archive_selector: &'static str =
        Box::leak(format!("project-thread-action-{project_task_id}-2").into_boxed_str());
    click(&f, cx, project_actions_selector);
    click(&f, cx, project_archive_selector);
    assert_eq!(
        snapshot(&f)
            .threads
            .into_iter()
            .find(|thread| thread.id == project_task_id)
            .unwrap()
            .status,
        ThreadStatus::Archived
    );
    click(&f, cx, project_actions_selector);
    click(&f, cx, project_archive_selector);
    assert_eq!(
        snapshot(&f)
            .threads
            .into_iter()
            .find(|thread| thread.id == project_task_id)
            .unwrap()
            .status,
        ThreadStatus::Active
    );
}

#[gpui_kit::test]
async fn r15_sidebar_mounts_at_minimum_viewport(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture_with_size(cx, 960., 600.);
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("organization-new-session").is_some());
    assert!(visual.debug_bounds("organization-add-project").is_some());
    assert!(visual.debug_bounds("project-header-p").is_some());
}
