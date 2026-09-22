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
    for _ in 0..4 {
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
async fn a8_project_menu_detaches_tasks_into_one_standalone_projection(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let project_path = f.dir.path().join("p");
    std::fs::create_dir(&project_path).unwrap();
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let project_tasks = conversation::list_threads(&store, "p", None).unwrap();
    assert_eq!(project_tasks.len(), 5);
    let archived = project_tasks
        .iter()
        .find(|thread| thread.id != f.first.id)
        .unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    conversation::set_thread_status(&store, &archived.id, ThreadStatus::Archived).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    assert_eq!(
        snapshot(&f)
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        vec!["p", "q"]
    );
    click(&f, cx, "project-more-p");
    let menu_items = sessions(&f, cx).read_with(cx, |block, _| block.organization_menu_items());
    assert_eq!(menu_items.len(), 1, "project menu has one action");
    assert_eq!(menu_items[0].0, "移除项目");
    assert!(matches!(
        &menu_items[0].1,
        MenuCommand::RemoveProject(id) if id == "p"
    ));
    assert!(!absent(&f, cx, "organization-menu-0"));
    assert!(absent(&f, cx, "organization-menu-1"));
    click(&f, cx, "organization-menu-0");
    cx.run_until_parked();

    let state = snapshot(&f);
    assert_eq!(
        state
            .projects
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["q"]
    );
    assert_eq!(
        state.threads.len(),
        6,
        "all five target tasks and the unrelated task survive"
    );
    assert!(project_path.is_dir(), "removal preserves local files");
    assert_eq!(
        state
            .threads
            .iter()
            .find(|thread| thread.id == f.other.id)
            .unwrap()
            .project_id,
        "q",
        "unrelated task remains bound to its project"
    );
    for thread in state
        .threads
        .iter()
        .filter(|thread| thread.id != f.other.id)
    {
        assert!(
            thread.is_standalone(),
            "removed-project tasks are genuinely unbound"
        );
        let pinned = format!("pinned-thread-row-{}", thread.id);
        let recent = format!("standalone-thread-row-{}", thread.id);
        let project = format!("project-thread-row-{}", thread.id);
        let visible = usize::from(!absent(&f, cx, &pinned))
            + usize::from(!absent(&f, cx, &recent))
            + usize::from(!absent(&f, cx, &project));
        assert_eq!(
            visible,
            usize::from(thread.status == ThreadStatus::Active),
            "task {} has exactly its expected projection",
            thread.id
        );
    }
    assert!(absent(&f, cx, "project-header-p"));
    assert!(!absent(&f, cx, "project-header-q"));
    f.sidebar.read_with(cx, |sidebar, cx| {
        assert!(sidebar.projects_block.read(cx).error.is_none());
        assert!(sidebar.sessions_block.read(cx).error.is_none());
    });
    cx.update(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert!(cx.global::<OpenedThread>().0.is_none());
    });

    click(&f, cx, format!("pinned-thread-row-{}", f.first.id));
    cx.update(|cx| {
        let opened = cx.global::<OpenedThread>().0.as_ref().unwrap();
        assert_eq!(opened.id, f.first.id);
        assert!(opened.is_standalone());
        assert!(cx.global::<SelectedProject>().0.is_none());
    });
}

#[gpui_kit::test]
async fn r15_route_guard_preserves_draft_and_switches_project_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    assert_eq!(snapshot(&f).threads.len(), 6);
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
    // R69 intentional change: [新建任务] navigates to the home draft route (R14)
    // instead of eagerly INSERTing. The scoping property this assertion
    // protected — "the new task belongs to the newly selected project, not the
    // previous one" — is now carried by `SelectedProject`, which the draft
    // binds to at construction (R2). The row itself appears on first submit.
    assert!(
        state
            .threads
            .iter()
            .all(|thread| thread.project_id != second_id),
        "the eager create path must not insert a row anymore"
    );
    assert_eq!(
        cx.update(|cx| cx.global::<SelectedProject>().0.clone()),
        Some(second_id.clone()),
        "the new task's project scope is the newly selected project"
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
    // R48: the project row keeps no leading inset, so the folder icon starts on
    // the row's own leading edge (the shared label column) without a Chevron
    // slot. The text column is reached by icon + gap instead.
    assert!(
        f32::from(project_folder.left() - project_row.left()) <= 1.0,
        "folder must start at the row's leading edge without a Chevron slot"
    );
    assert!(
        f32::from(project_label.left() - task_title.left()).abs() <= 1.0,
        "project label and child task title must share the same content column"
    );
    assert_eq!(f32::from(bounds(&f, cx, "project-add-p").size.width), 24.0);
    assert_eq!(f32::from(bounds(&f, cx, "project-more-p").size.width), 24.0);

    click(&f, cx, "organization-new-session");
    // R69 intentional change: the SESSIONS plus button navigates to the home
    // draft route (R14) rather than INSERTing a standalone row, so there is
    // nothing for the sidebar to list yet. In this sidebar-only harness the
    // route stays `OpenedThread == None` (the window installs the draft on its
    // next frame); the durable row appears on first submit, whose sidebar-level
    // proof is the window test `r69_a3_first_submit_materializes_the_draft_under_its_own_id`.
    cx.update(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert!(
            cx.global::<OpenedThread>().0.is_none(),
            "the new-task entry must not eagerly create a task"
        );
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
    assert_eq!(standalone_count, 0);
    assert!(
        visual.debug_bounds("organization-recents-empty").is_some(),
        "with no durable standalone row the recents section stays empty"
    );

    click(&f, cx, "project-add-p");
    let project_tasks: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE project_id = 'p'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // R69: five fixture rows for project `p`, unchanged by the new-task click.
    assert_eq!(project_tasks, 5);
    assert_eq!(
        store
            .conn()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM threads WHERE project_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
    cx.update(|cx| {
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        assert!(
            cx.global::<OpenedThread>().0.is_none(),
            "the project-scoped new-task entry must not eagerly create a task either"
        );
    });
    let project_selector: &'static str =
        Box::leak(format!("project-thread-row-{}", f.first.id).into_boxed_str());
    let standalone_project_selector: &'static str =
        Box::leak(format!("standalone-thread-row-{}", f.first.id).into_boxed_str());
    if !absent(&f, cx, "project-p-show-more") {
        click(&f, cx, "project-p-show-more");
    }
    assert!(
        visual.debug_bounds(project_selector).is_some(),
        "the project's existing durable rows stay listed"
    );
    assert!(absent(&f, cx, standalone_project_selector));

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
    assert!(absent(
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
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
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
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
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
async fn r32_pinned_and_recents_rows_align_to_headings_while_project_children_keep_the_grid(
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
    let pinned_heading = bounds(&f, cx, "organization-section-label-Pinned");
    let recents_heading = bounds(&f, cx, "organization-section-label-Recents");
    let child_row = bounds(&f, cx, format!("project-thread-row-{}", child.id));
    let child_title = bounds(&f, cx, format!("project-thread-row-title-{}", child.id));
    let recent_row = bounds(&f, cx, format!("standalone-thread-row-{}", recent.id));
    let recent_title = bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id));
    for id in [&f.first.id, &f.other.id, &pinned_standalone.id] {
        assert_eq!(
            bounds(&f, cx, format!("pinned-thread-row-title-{id}")).left(),
            pinned_heading.left()
        );
    }
    assert_eq!(f32::from(pinned_title.left() - pinned_row.left()), 8.0);
    assert_eq!(recent_title.left(), recents_heading.left());
    assert_eq!(recent_title.left(), recent_row.left());
    assert_eq!(
        f32::from(child_title.left() - child_row.left()),
        Layout::SIDEBAR_ROW_INSET
    );
    assert_ne!(child_title.left(), recent_title.left());
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
    // R48: the folder icon shares the section label column (row leading edge);
    // the project name sits one ladder step further at the text column.
    assert_eq!(f32::from(folder.left() - project_row.left()), 0.0);
    assert_eq!(f32::from(label.left() - folder.right()), 8.0);
    assert_eq!(
        f32::from(label.left() - project_row.left()),
        Layout::SIDEBAR_ROW_INSET
    );

    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-project-{}", f.first.id)
    ));
    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-project-{}", f.other.id)
    ));

    let rest_tail = bounds(&f, cx, format!("thread-actions-state-{}-rest", f.first.id));
    let long_tail = bounds(&f, cx, format!("thread-actions-state-{}-rest", f.other.id));
    assert_eq!(
        f32::from(rest_tail.size.width),
        Layout::SIDEBAR_ACTIONS_WIDTH
    );
    assert_eq!(rest_tail.left(), long_tail.left());
    let rest_title = pinned_title;
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(pinned_row.center(), None, gpui_kit::Modifiers::default());
    let hover_title = bounds(&f, cx, format!("pinned-thread-row-title-{}", f.first.id));
    let hover_tail = bounds(
        &f,
        cx,
        format!("thread-actions-state-{}-visible", f.first.id),
    );
    assert_eq!(rest_title.origin, hover_title.origin);
    assert_eq!(rest_tail.origin, hover_tail.origin);

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_eq!(
        bounds(&f, cx, format!("pinned-thread-row-title-{}", f.first.id)).left(),
        bounds(&f, cx, "organization-section-label-Pinned").left()
    );
    assert_eq!(
        bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id)).left(),
        bounds(&f, cx, "organization-section-label-Recents").left()
    );
    assert_eq!(
        f32::from(
            bounds(&f, cx, format!("project-thread-row-title-{}", child.id)).left()
                - bounds(&f, cx, format!("project-thread-row-{}", child.id)).left()
        ),
        Layout::SIDEBAR_ROW_INSET
    );
    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-project-{}", f.first.id)
    ));
    assert_eq!(
        bounds(
            &f,
            cx,
            format!("thread-actions-state-{}-visible", f.first.id)
        )
        .origin,
        hover_tail.origin
    );
}

#[gpui_kit::test]
async fn r33_production_task_rows_are_quiet_and_keep_stable_actions(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let child = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && !thread.pinned)
        .unwrap();
    for thread_id in [&f.first.id, &child.id, &recent.id] {
        assert!(absent(&f, cx, format!("thread-timestamp-{thread_id}")));
    }
    assert!(absent(
        &f,
        cx,
        format!("pinned-thread-row-project-{}", f.first.id)
    ));

    let row = bounds(&f, cx, format!("standalone-thread-row-{}", recent.id));
    let title_rest = bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id));
    let tail_rest = bounds(&f, cx, format!("thread-actions-state-{}-rest", recent.id));
    let trigger_rest = bounds(&f, cx, format!("standalone-thread-actions-{}", recent.id));
    assert_eq!(
        f32::from(tail_rest.size.width),
        Layout::SIDEBAR_ACTIONS_WIDTH
    );

    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(row.center(), None, gpui_kit::Modifiers::default());
    let title_hover = bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id));
    let tail_hover = bounds(
        &f,
        cx,
        format!("thread-actions-state-{}-visible", recent.id),
    );
    let trigger_hover = bounds(&f, cx, format!("standalone-thread-actions-{}", recent.id));
    assert_eq!(title_rest.origin, title_hover.origin);
    assert_eq!(tail_rest, tail_hover);
    assert_eq!(trigger_rest, trigger_hover);
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));

    drop(visual);
    click(&f, cx, format!("standalone-thread-actions-{}", recent.id));
    assert_eq!(
        sessions(&f, cx).read_with(cx, |block, _| block.actions_open.clone()),
        Some(recent.id.clone())
    );
    assert_eq!(
        bounds(&f, cx, format!("standalone-thread-actions-{}", recent.id)),
        trigger_rest
    );
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
}

#[gpui_kit::test]
async fn r33_project_tasks_expand_independently_and_preserve_mounted_state(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    for _ in 0..3 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    let newest_p = conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    conversation::set_thread_status(&store, &newest_p.id, ThreadStatus::Archived).unwrap();
    conversation::set_thread_pinned(&store, &f.other.id, true).unwrap();
    for _ in 0..6 {
        conversation::create_thread(&store, "q", "model", "confirm").unwrap();
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let state = snapshot(&f);
    let active_ids = |project_id: &str| {
        state
            .threads
            .iter()
            .filter(|thread| {
                thread.project_id == project_id
                    && thread.status == ThreadStatus::Active
                    && !thread.pinned
            })
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>()
    };
    let p_ids = active_ids("p");
    let q_ids = active_ids("q");
    assert_eq!(p_ids.len(), 7);
    assert_eq!(q_ids.len(), 6);
    let mounted = |ids: &[String], cx: &mut gpui_kit::TestAppContext| {
        ids.iter()
            .filter(|id| !absent(&f, cx, format!("project-thread-row-{id}")))
            .count()
    };
    assert_eq!(mounted(&p_ids, cx), 5);
    assert_eq!(mounted(&q_ids, cx), 5);
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.other.id)));
    assert!(absent(
        &f,
        cx,
        format!("project-thread-row-{}", newest_p.id)
    ));

    let more = bounds(&f, cx, "project-p-show-more");
    let more_label = bounds(&f, cx, "project-p-progressive-label");
    assert_eq!(
        f32::from(more_label.left() - more.left()),
        Layout::SIDEBAR_ROW_INSET
    );
    click(&f, cx, "project-p-show-more");
    assert_eq!(mounted(&p_ids, cx), p_ids.len());
    assert_eq!(mounted(&q_ids, cx), 5);
    assert!(!absent(&f, cx, "project-p-show-less"));
    assert!(!absent(&f, cx, "project-q-show-more"));

    click(&f, cx, "project-header-p");
    assert_eq!(mounted(&p_ids, cx), 0);
    click(&f, cx, "project-header-p");
    assert_eq!(mounted(&p_ids, cx), p_ids.len());
    assert!(!absent(&f, cx, "project-p-show-less"));

    click(&f, cx, "project-p-show-less");
    assert_eq!(mounted(&p_ids, cx), 5);
    assert_eq!(mounted(&q_ids, cx), 5);

    sessions(&f, cx).update(cx, |block, _| {
        block
            .organization
            .as_mut()
            .unwrap()
            .project_threads_expanded
            .insert("removed-project".into());
    });
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert!(sessions(&f, cx).read_with(cx, |block, _| {
        !block
            .organization
            .as_ref()
            .unwrap()
            .project_threads_expanded
            .contains("removed-project")
    }));
}

#[gpui_kit::test]
async fn r28_sidebar_section_labels_use_title_case(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let mut headings = Vec::new();
    for label in ["Pinned", "Projects", "Recents"] {
        assert!(!absent(
            &f,
            cx,
            format!("organization-section-label-{label}")
        ));
        headings.push(bounds(
            &f,
            cx,
            format!("organization-section-label-{label}"),
        ));
    }
    assert!(headings[0].top() < headings[1].top() && headings[1].top() < headings[2].top());
    for label in ["PINNED", "PROJECTS", "RECENTS"] {
        assert!(absent(
            &f,
            cx,
            format!("organization-section-label-{label}")
        ));
    }
}

#[gpui_kit::test]
async fn r29_projects_expand_and_recents_has_no_progressive_control(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    click(&f, cx, "project-header-p");
    click(&f, cx, "project-header-q");
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let mut project_ids = vec!["p".to_string(), "q".to_string()];
    for index in 0..7 {
        let id = format!("r29-project-{index}");
        store
            .conn()
            .execute(
                "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES(?1,?2,?1,0,?3)",
                (
                    &id,
                    f.dir.path().join(&id).to_string_lossy().to_string(),
                    10 + index,
                ),
            )
            .unwrap();
        project_ids.push(id);
    }
    let mut recent_ids = Vec::new();
    for _ in 0..11 {
        recent_ids.push(
            conversation::create_standalone_thread(&store, "model", "confirm")
                .unwrap()
                .id,
        );
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    assert_eq!(
        project_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("project-header-{id}")))
            .count(),
        5
    );
    assert!(!absent(&f, cx, "organization-projects-show-more"));
    assert!(absent(&f, cx, "organization-projects-show-less"));
    let projects_control = bounds(&f, cx, "organization-projects-show-more");
    let projects_label = bounds(&f, cx, "organization-projects-progressive-label");
    assert_eq!(
        f32::from(projects_label.left() - projects_control.left()),
        Layout::SIDEBAR_ROW_INSET
    );
    // R50: the group gap is now the single explicit section pitch, not the
    // legacy shared `gap_2()` 8px. The old 8px here was the other half of the
    // defect R50 removes (Pinned→Projects measured 12px while this boundary
    // measured 8px for the same relationship); asserting the token keeps this
    // test's intent — sections expand independently in the outer scroller —
    // while pinning the corrected value.
    assert_eq!(
        f32::from(
            bounds(&f, cx, "organization-section-recents").top()
                - bounds(&f, cx, "organization-section-projects").bottom()
        ),
        Layout::SIDEBAR_SECTION_GAP
    );
    // Issue #57: Recents has no progressive control at all. It still starts at
    // the compact 10-row window; the outer scroller fills the rest (covered by
    // the dedicated r57 tests below).
    assert_eq!(
        recent_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("standalone-thread-row-{id}")))
            .count(),
        10
    );
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));

    click(&f, cx, "organization-projects-show-more");
    assert_eq!(
        project_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("project-header-{id}")))
            .count(),
        9
    );
    assert!(!absent(&f, cx, "organization-projects-show-less"));
    // Issue #57: Recents has no progressive control, so this scroll never
    // changes which Recents control exists — there is none to change.
    assert!(absent(&f, cx, "organization-recents-show-more"));
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let sidebar_scroll = visual.debug_bounds("sidebar-scroll").unwrap();
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: sidebar_scroll.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
            gpui_kit::px(0.),
            gpui_kit::px(-400.),
        )),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    cx.run_until_parked();
    click(&f, cx, "organization-projects-show-less");
    assert_eq!(
        project_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("project-header-{id}")))
            .count(),
        5
    );

    let projects_focus =
        sessions(&f, cx).read_with(cx, |block, _| block.projects_progressive_focus.clone());
    f.window
        .update(cx, |_, window, cx| projects_focus.focus(window, cx))
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "enter");
    assert!(!absent(&f, cx, "organization-projects-show-less"));

    // Issue #57: the Projects control still toggles by keyboard, and the
    // absence of a Recents control is unaffected by it.
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));
    assert!(!absent(&f, cx, "organization-projects-show-less"));
}

#[gpui_kit::test]
async fn r29_progressive_controls_are_absent_without_hidden_items(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    assert!(absent(&f, cx, "organization-projects-show-more"));
    assert!(absent(&f, cx, "organization-projects-show-less"));
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));
}

/// Issue #57 / T57-1, T57-2, T57-4: with a viewport tall enough to hold every
/// eligible Recents row, the list renders **all** of them with no `Show More`
/// control, and the painted order is the stable projection order.
#[gpui_kit::test]
async fn r57_recents_fill_a_tall_viewport_without_a_progressive_control(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 2600.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let mut recent_ids = Vec::new();
    for _ in 0..12 {
        recent_ids.push(
            conversation::create_standalone_thread(&store, "model", "confirm")
                .unwrap()
                .id,
        );
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    // R1: no progressive control exists for Recents, in either state.
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));

    // R3: the whole list fits, so nothing is withheld.
    assert_eq!(
        recent_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("standalone-thread-row-{id}")))
            .count(),
        recent_ids.len()
    );

    // R5: painted top-to-bottom order equals the stable sort projection.
    let preferences = snapshot(&f).preferences;
    let mut expected: Vec<String> = super::projections::sorted_threads(
        &snapshot(&f)
            .threads
            .into_iter()
            .filter(|thread| thread.is_standalone() && !thread.pinned)
            .collect::<Vec<_>>(),
        preferences.sort,
    )
    .into_iter()
    .map(|thread| thread.id)
    .collect();
    expected.sort_by_key(|id| bounds(&f, cx, format!("standalone-thread-row-{id}")).top());
    let mut painted: Vec<String> = recent_ids.clone();
    painted.sort_by_key(|id| bounds(&f, cx, format!("standalone-thread-row-{id}")).top());
    assert_eq!(painted, expected);
}

/// Issue #57 / T57-3: when the list does not fit, Recents starts at one page
/// and each reach of the outer scroller bottom appends exactly one more page —
/// never a `Show More` control, never a reorder.
#[gpui_kit::test]
async fn r57_recents_grow_one_page_per_scroll_to_bottom(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let mut recent_ids = Vec::new();
    for _ in 0..30 {
        recent_ids.push(
            conversation::create_standalone_thread(&store, "model", "confirm")
                .unwrap()
                .id,
        );
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let page = super::RECENTS_PAGE;
    let mounted = |cx: &mut gpui_kit::TestAppContext| {
        recent_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("standalone-thread-row-{id}")))
            .count()
    };
    // The window is short, so the initial content overflows and the lazy list
    // holds at the first page instead of filling.
    assert_eq!(mounted(cx), page);
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));

    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let viewport = visual.debug_bounds("sidebar-scroll").unwrap();
    let scroll_to_bottom = |visual: &mut gpui_kit::VisualTestContext| {
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            // A delta far past the scroll maximum clamps to the bottom, which
            // is the state the lazy list keys on.
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.),
                gpui_kit::px(-100_000.),
            )),
            modifiers: gpui_kit::Modifiers::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    };

    scroll_to_bottom(&mut visual);
    assert_eq!(mounted(cx), page * 2);
    scroll_to_bottom(&mut visual);
    assert_eq!(mounted(cx), page * 3);
    // At the end of the data the list stops growing and still has no control.
    scroll_to_bottom(&mut visual);
    assert_eq!(mounted(cx), recent_ids.len());
    assert!(absent(&f, cx, "organization-recents-show-more"));
    assert!(absent(&f, cx, "organization-recents-show-less"));

    // The appended pages kept the stable order: the newest row is on top.
    let preferences = snapshot(&f).preferences;
    let expected_first = super::projections::sorted_threads(
        &snapshot(&f)
            .threads
            .into_iter()
            .filter(|thread| thread.is_standalone() && !thread.pinned)
            .collect::<Vec<_>>(),
        preferences.sort,
    )
    .first()
    .map(|thread| thread.id.clone())
    .unwrap();
    let mut painted: Vec<String> = recent_ids.clone();
    painted.sort_by_key(|id| bounds(&f, cx, format!("standalone-thread-row-{id}")).top());
    assert_eq!(painted.first(), Some(&expected_first));
}

#[gpui_kit::test]
async fn r31_sidebar_typography_restores_compact_mounted_sizes(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let primary_height = bounds(&f, cx, "sidebar-new-task-label").size.height;
    for selector in [
        "organization-project-p".to_string(),
        format!("pinned-thread-row-title-{}", f.first.id),
        format!("standalone-thread-row-title-{}", recent.id),
    ] {
        assert_eq!(bounds(&f, cx, selector).size.height, primary_height);
    }

    let metadata_height = bounds(&f, cx, "sidebar-new-task-shortcut").size.height;
    for selector in [
        "organization-section-label-Pinned".to_string(),
        "organization-section-label-Projects".to_string(),
        "organization-section-label-Recents".to_string(),
    ] {
        let height = bounds(&f, cx, &selector).size.height;
        assert_eq!(height, metadata_height, "unexpected height for {selector}");
    }
    assert!(primary_height > metadata_height);
    assert!(!absent(&f, cx, "sidebar-settings-label"));
    assert!(!absent(&f, cx, "sidebar-settings-shortcut"));

    for selector in [
        "sidebar-new-task".to_string(),
        "sidebar-settings-surface".to_string(),
        "project-header-p".to_string(),
        format!("pinned-thread-row-{}", f.first.id),
        format!("standalone-thread-row-{}", recent.id),
    ] {
        assert_eq!(
            f32::from(bounds(&f, cx, selector).size.height),
            Typography::SIDEBAR_LINE_HEIGHT
        );
    }

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_eq!(
        bounds(&f, cx, "sidebar-new-task-label").size.height,
        primary_height
    );
    assert_eq!(
        bounds(&f, cx, "organization-section-label-Projects")
            .size
            .height,
        metadata_height
    );
    assert!(absent(&f, cx, format!("thread-timestamp-{}", recent.id)));
}

#[gpui_kit::test]
async fn r41_search_uses_shared_titlebar_controls_and_releases_navigation_row_space(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn assert_geometry(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
        let new_task = bounds(f, cx, "sidebar-new-task");
        let sidebar = bounds(f, cx, "toggle-sidebar");
        let search = bounds(f, cx, "titlebar-search-button");
        let back = bounds(f, cx, "navigation-back");
        assert!(search.bottom() < new_task.top());
        assert!(search.size.width < new_task.size.width);
        assert_eq!(f32::from(search.left() - sidebar.right()), 4.0);
        assert_eq!(f32::from(back.left() - search.right()), 4.0);
        assert!(absent(f, cx, "sidebar-search-button"));
        assert!(absent(f, cx, "sidebar-search"));
        let organization = bounds(f, cx, "sidebar-scroll");
        assert_eq!(f32::from(organization.top() - new_task.bottom()), 12.0);
    }

    let f = fixture(cx);
    assert_geometry(&f, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_geometry(&f, cx);

    cx.update(|cx| cx.set_global(SidebarWidth(Layout::SIDEBAR_MIN_WIDTH)));
    cx.run_until_parked();
    assert_geometry(&f, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    cx.run_until_parked();
    assert_geometry(&f, cx);
}

#[gpui_kit::test]
async fn r35_pinned_to_projects_gap_uses_one_extra_rhythm_step_across_themes(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn assert_geometry(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
        let pinned_row = bounds(f, cx, format!("pinned-thread-row-{}", f.first.id));
        let pinned_section = bounds(f, cx, "organization-section-pinned");
        let projects_header = bounds(f, cx, "organization-header-projects");

        assert_eq!(pinned_section.bottom(), pinned_row.bottom());
        assert_eq!(f32::from(projects_header.top() - pinned_row.bottom()), 12.0);
        assert_eq!(
            f32::from(pinned_row.size.height),
            Typography::SIDEBAR_LINE_HEIGHT
        );
        assert_eq!(f32::from(projects_header.size.height), 28.0);
        assert_eq!(
            projects_header.left(),
            bounds(f, cx, "organization-section-label-Projects").left()
        );
    }

    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    assert_geometry(&f, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_geometry(&f, cx);
}

#[gpui_kit::test]
async fn r35_projects_starts_without_a_pinned_only_spacer_across_themes(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn assert_geometry(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
        assert!(absent(f, cx, "organization-section-pinned"));
        let organization = bounds(f, cx, "sidebar-organization");
        let projects = bounds(f, cx, "organization-section-projects");
        let projects_header = bounds(f, cx, "organization-header-projects");

        assert_eq!(projects.top(), organization.top());
        assert_eq!(projects_header.top(), organization.top());
        assert_eq!(f32::from(projects_header.size.height), 28.0);
        assert_eq!(
            projects_header.left(),
            bounds(f, cx, "organization-section-label-Projects").left()
        );
    }

    let f = fixture(cx);
    assert_geometry(&f, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_geometry(&f, cx);
}

#[gpui_kit::test]
async fn r37_pinned_surfaces_keep_padding_inside_scroll_clips(cx: &mut gpui_kit::TestAppContext) {
    fn assert_geometry(
        f: &Fixture,
        recent: &Thread,
        child: &Thread,
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let heading = bounds(f, cx, "organization-section-label-Pinned");
        let recents_heading = bounds(f, cx, "organization-section-label-Recents");
        let selected_row = bounds(f, cx, format!("pinned-thread-row-{}", f.first.id));
        let selected_title = bounds(f, cx, format!("pinned-thread-row-title-{}", f.first.id));
        let rest_row = bounds(f, cx, format!("pinned-thread-row-{}", f.other.id));
        let rest_title = bounds(f, cx, format!("pinned-thread-row-title-{}", f.other.id));
        let recent_row = bounds(f, cx, format!("standalone-thread-row-{}", recent.id));
        let recent_title = bounds(f, cx, format!("standalone-thread-row-title-{}", recent.id));
        let project_row = bounds(f, cx, "project-header-p");
        let child_row = bounds(f, cx, format!("project-thread-row-{}", child.id));
        let child_title = bounds(f, cx, format!("project-thread-row-title-{}", child.id));
        let pinned_viewport = bounds(f, cx, "organization-pinned-scroll");
        let sidebar_viewport = bounds(f, cx, "sidebar-scroll");

        for (row, title) in [(selected_row, selected_title), (rest_row, rest_title)] {
            assert_eq!(title.left(), heading.left());
            assert_eq!(f32::from(title.left() - row.left()), 8.0);
            assert_eq!(row.right(), recent_row.right());
            assert_eq!(f32::from(row.size.height), Typography::SIDEBAR_LINE_HEIGHT);
            for viewport in [pinned_viewport, sidebar_viewport] {
                assert!(viewport.left() <= row.left());
                assert!(viewport.right() >= row.right());
            }
        }

        assert_eq!(selected_row.left(), rest_row.left());
        assert_eq!(selected_row.right(), rest_row.right());
        assert_eq!(recent_title.left(), recents_heading.left());
        assert_eq!(recent_title.left(), recent_row.left());
        assert_eq!(project_row.left(), recent_row.left());
        assert_eq!(project_row.right(), recent_row.right());
        assert_eq!(
            f32::from(child_title.left() - child_row.left()),
            Layout::SIDEBAR_ROW_INSET
        );

        let pinned_section = bounds(f, cx, "organization-section-pinned");
        let projects_header = bounds(f, cx, "organization-header-projects");
        assert_eq!(
            f32::from(projects_header.top() - pinned_section.bottom()),
            12.0
        );
    }

    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    conversation::set_thread_pinned(&store, &f.other.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let child = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && !thread.pinned)
        .unwrap();

    assert_geometry(&f, &recent, &child, cx);

    let rest_row = bounds(&f, cx, format!("pinned-thread-row-{}", f.other.id));
    let rest_title = bounds(&f, cx, format!("pinned-thread-row-title-{}", f.other.id));
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(rest_row.center(), None, gpui_kit::Modifiers::default());
    assert_eq!(
        bounds(&f, cx, format!("pinned-thread-row-{}", f.other.id)),
        rest_row
    );
    assert_eq!(
        bounds(&f, cx, format!("pinned-thread-row-title-{}", f.other.id)),
        rest_title
    );
    click(&f, cx, format!("pinned-thread-actions-{}", f.other.id));
    assert_eq!(
        bounds(&f, cx, format!("pinned-thread-row-{}", f.other.id)),
        rest_row
    );
    assert_eq!(
        bounds(&f, cx, format!("pinned-thread-row-title-{}", f.other.id)),
        rest_title
    );

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_geometry(&f, &recent, &child, cx);
}

#[gpui_kit::test]
async fn r38_organization_content_keeps_eight_pixel_edge_inset_across_themes_and_widths(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn assert_geometry(
        f: &Fixture,
        recent: &Thread,
        child: &Thread,
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let sidebar = bounds(f, cx, "sidebar");
        let sidebar_scroll = bounds(f, cx, "sidebar-scroll");
        let pinned_heading = bounds(f, cx, "organization-section-label-Pinned");
        let projects_heading = bounds(f, cx, "organization-section-label-Projects");
        let recents_heading = bounds(f, cx, "organization-section-label-Recents");
        let pinned_row = bounds(f, cx, format!("pinned-thread-row-{}", f.first.id));
        let pinned_title = bounds(f, cx, format!("pinned-thread-row-title-{}", f.first.id));
        let project_row = bounds(f, cx, "project-header-p");
        let child_row = bounds(f, cx, format!("project-thread-row-{}", child.id));
        let child_title = bounds(f, cx, format!("project-thread-row-title-{}", child.id));
        let recent_row = bounds(f, cx, format!("standalone-thread-row-{}", recent.id));
        let recent_title = bounds(f, cx, format!("standalone-thread-row-title-{}", recent.id));

        assert_eq!(
            f32::from(pinned_row.left() - sidebar.left()),
            8.0,
            "Pinned surface must keep an 8px application-edge inset"
        );
        assert_eq!(pinned_title.left(), pinned_heading.left());
        assert_eq!(pinned_title.left(), projects_heading.left());
        assert_eq!(pinned_title.left(), recents_heading.left());
        assert_eq!(pinned_title.left(), recent_title.left());
        assert_eq!(
            f32::from(child_title.left() - recent_title.left()),
            Layout::SIDEBAR_ROW_INSET
        );
        assert_eq!(project_row.left(), recent_row.left());
        assert_eq!(child_row.left(), recent_row.left());

        for row in [pinned_row, project_row, recent_row] {
            assert_eq!(row.right(), sidebar_scroll.right());
            assert_eq!(f32::from(row.size.height), Typography::SIDEBAR_LINE_HEIGHT);
        }
        assert_eq!(
            f32::from(pinned_title.left() - pinned_row.left()),
            8.0,
            "Pinned title padding must remain 8px inside the surface"
        );
    }

    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let child = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && !thread.pinned)
        .unwrap();

    assert_geometry(&f, &recent, &child, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_geometry(&f, &recent, &child, cx);

    cx.update(|cx| cx.set_global(SidebarWidth(Layout::SIDEBAR_MIN_WIDTH)));
    cx.run_until_parked();
    assert_geometry(&f, &recent, &child, cx);

    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    cx.run_until_parked();
    assert_geometry(&f, &recent, &child, cx);
}

#[gpui_kit::test]
async fn r42_mounted_sidebar_prefers_the_active_task_surface_in_light_and_dark(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn assert_child_active(f: &Fixture, row_prefix: &str, cx: &mut gpui_kit::TestAppContext) {
        assert!(!absent(f, cx, "project-persistent-surface-p-rest"));
        assert!(absent(f, cx, "project-persistent-surface-p-active"));
        assert!(!absent(
            f,
            cx,
            format!("{row_prefix}surface-{}-active", f.first.id)
        ));
        assert!(absent(
            f,
            cx,
            format!("{row_prefix}surface-{}-rest", f.first.id)
        ));
    }

    fn assert_project_only_active(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
        assert!(!absent(f, cx, "project-persistent-surface-p-active"));
        assert!(absent(f, cx, "project-persistent-surface-p-rest"));
        assert!(!absent(
            f,
            cx,
            format!("pinned-thread-row-surface-{}-rest", f.first.id)
        ));
    }

    let f = fixture(cx);

    assert_child_active(&f, "project-thread-row-", cx);
    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_child_active(&f, "project-thread-row-", cx);

    // Temporary project-row states keep their existing behavior without
    // restoring the suppressed persistent ancestor selection.
    let project_row = bounds(&f, cx, "project-header-p");
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(project_row.center(), None, gpui_kit::Modifiers::default());
    assert!(!absent(&f, cx, "project-actions-p-visible"));
    assert!(!absent(&f, cx, "project-persistent-surface-p-rest"));
    let project_actions_focus = sessions(&f, cx).read_with(cx, |block, _| {
        block.project_focuses.get("p").unwrap().clone()
    });
    f.window
        .update(cx, |_, window, cx| project_actions_focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!absent(&f, cx, "project-actions-p-visible"));
    assert!(!absent(&f, cx, "project-persistent-surface-p-rest"));

    click(&f, cx, "project-header-p");
    assert!(!absent(&f, cx, "project-folder-p-closed"));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
    assert!(!absent(&f, cx, "project-persistent-surface-p-rest"));
    click(&f, cx, "project-header-p");
    assert!(!absent(&f, cx, "project-folder-p-open"));
    assert_child_active(&f, "project-thread-row-", cx);

    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_child_active(&f, "pinned-thread-row-", cx);
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    cx.run_until_parked();
    assert_child_active(&f, "pinned-thread-row-", cx);

    cx.update(|cx| {
        cx.set_global(OpenedThread(None));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    assert_project_only_active(&f, cx);
    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_project_only_active(&f, cx);
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
    // R69 intentional change: the SESSIONS plus button no longer INSERTs a row
    // (R14) — it opens the home draft route, which is not a sidebar row until
    // first submit. This test's subject is the archive filter, so its fixture
    // row is created through the conversation layer directly and then loaded
    // through the sidebar's own production reload path.
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let standalone = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    cx.update(|cx| {
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(Some(standalone.clone())));
    });
    f.sidebar
        .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
    cx.run_until_parked();
    let standalone_id = standalone.id.clone();
    let row_selector: &'static str =
        Box::leak(format!("standalone-thread-row-{standalone_id}").into_boxed_str());
    let actions_selector: &'static str =
        Box::leak(format!("standalone-thread-actions-{standalone_id}").into_boxed_str());
    let archive_selector: &'static str =
        Box::leak(format!("standalone-thread-action-{standalone_id}-2").into_boxed_str());
    assert!(
        gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(row_selector)
            .is_some(),
        "the durable standalone row must be listed"
    );

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

/// R48 t1/t2/t5: the Sidebar indent ladder in the mounted production tree.
///
/// One state per test: the folder icon shares the section label column, the
/// project name shares the child text column, and the folder icon keeps that
/// column in both collapsed and expanded states.
#[gpui_kit::test]
async fn r48_project_folder_shares_the_section_label_column(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let folder = bounds(&f, cx, "project-folder-p-open");
    let label = bounds(&f, cx, "organization-section-label-Projects");
    assert!(
        (f32::from(folder.left() - label.left())).abs() <= 1.0,
        "folder icon ({}) must share the section label column ({})",
        f32::from(folder.left()),
        f32::from(label.left())
    );
}

#[gpui_kit::test]
async fn r48_project_name_shares_the_child_text_column(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let child = snapshot(&f)
        .threads
        .into_iter()
        .find(|thread| thread.project_id == "p" && !thread.pinned)
        .unwrap();
    let project_name = bounds(&f, cx, "organization-project-p");
    let child_row = bounds(&f, cx, format!("project-thread-row-{}", child.id));
    let child_title = bounds(&f, cx, format!("project-thread-row-title-{}", child.id));
    assert!(
        (f32::from(project_name.left() - child_title.left())).abs() <= 1.0,
        "project name ({}) must share the child text column ({})",
        f32::from(project_name.left()),
        f32::from(child_title.left())
    );
    assert_eq!(
        f32::from(child_title.left() - child_row.left()),
        Layout::SIDEBAR_ROW_INSET,
        "the child text column must be the ladder's text step"
    );
}

#[gpui_kit::test]
async fn r48_folder_icon_keeps_its_column_when_the_project_collapses(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let expanded = bounds(&f, cx, "project-folder-p-open");
    click(&f, cx, "project-header-p");
    let collapsed = bounds(&f, cx, "project-folder-p-closed");
    assert_eq!(
        collapsed.left(),
        expanded.left(),
        "collapsing a project must not move the folder icon column"
    );
}

/// R50 contract B, core executable proof: every adjacent section boundary in
/// `Pinned → Projects → Recents` must be **equal**.
///
/// Before R50 the body's shared `gap_2()` (8px) plus `render_pinned_pi`'s
/// `mb_1()` (4px) gave Pinned→Projects 12px while Projects→Recents stayed at
/// 8px. The two boundaries express the same relationship, so the reader sees
/// one of them as looser even though neither is individually wrong. This test
/// pins both the section-box gap and the text-ink band, so a future extra
/// margin, padding compensation or special case cannot reintroduce the 4px
/// difference without failing here.
#[gpui_kit::test]
async fn r50_sidebar_section_boundaries_share_one_pitch(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let recent = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let pinned = bounds(&f, cx, "organization-section-pinned");
    let projects = bounds(&f, cx, "organization-section-projects");
    let recents = bounds(&f, cx, "organization-section-recents");
    let pinned_to_projects = f32::from(projects.top() - pinned.bottom());
    let projects_to_recents = f32::from(recents.top() - projects.bottom());

    assert_eq!(
        pinned_to_projects, projects_to_recents,
        "Pinned→Projects ({pinned_to_projects}) and Projects→Recents \
         ({projects_to_recents}) must share one section pitch"
    );
    assert_eq!(
        pinned_to_projects,
        Layout::SIDEBAR_SECTION_GAP,
        "the shared section pitch must come from the explicit token, not a \
         leftover margin"
    );

    // The same invariant in the reader's terms: the empty band from the last
    // text of one section to the next section's label. Section boxes include
    // their label's internal leading, so equal boxes are necessary but not
    // sufficient — assert the visible band too.
    let pinned_title = bounds(&f, cx, format!("pinned-thread-row-title-{}", f.first.id));
    let projects_title = bounds(&f, cx, format!("project-thread-row-title-{}", f.other.id));
    let recents_title = bounds(&f, cx, format!("standalone-thread-row-title-{}", recent.id));
    let pinned_band = f32::from(
        bounds(&f, cx, "organization-section-label-Projects").top() - pinned_title.bottom(),
    );
    let projects_band = f32::from(
        bounds(&f, cx, "organization-section-label-Recents").top() - projects_title.bottom(),
    );
    assert_eq!(
        pinned_band, projects_band,
        "the visible band after the Pinned list ({pinned_band}) and after the \
         Projects list ({projects_band}) must read the same"
    );
    assert!(pinned_band > 0.0 && recents_title.top() > recents.top());
}

/// R50 contract B, second half: `render_pinned_pi` carries no extra margin.
///
/// Pinned is not a special section. Asserting the boundary directly (rather
/// than grepping the source) keeps the invariant tied to rendered geometry,
/// so any future re-introduction of the 4px step — margin, padding or
/// coordinate special-casing — fails here regardless of how it is written.
#[gpui_kit::test]
async fn r50_pinned_section_carries_no_extra_margin(cx: &mut gpui_kit::TestAppContext) {
    fn assert_no_extra_margin(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
        let pinned = bounds(f, cx, "organization-section-pinned");
        let pinned_row = bounds(f, cx, format!("pinned-thread-row-{}", f.first.id));
        let projects_header = bounds(f, cx, "organization-header-projects");
        // The section box ends where its last row ends — no trailing margin
        // inside the Pinned box itself.
        assert_eq!(pinned.bottom(), pinned_row.bottom());
        assert_eq!(
            f32::from(projects_header.top() - pinned_row.bottom()),
            Layout::SIDEBAR_SECTION_GAP,
            "the Pinned→Projects boundary must be exactly one section pitch"
        );
    }

    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_no_extra_margin(&f, cx);

    // The pitch is a layout token, not a theme-dependent value: it must hold
    // in dark mode too.
    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    cx.run_until_parked();
    assert_no_extra_margin(&f, cx);
}

/// #150 R9/R12: the row indicator is driven only by the real thread-liveness
/// projection. Nothing about `unread`, `updated_at` or selection may light it.
#[gpui_kit::test]
async fn issue150_row_indicator_follows_only_real_thread_liveness(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let running = format!("project-thread-running-{}", f.first.id);

    // C4/R12: nothing is running, so the tail slot stays empty — and the
    // opened/selected row alone must not imply liveness.
    assert!(absent(&f, cx, running.clone()));

    // C3: the projection turns the indicator on for exactly that thread.
    cx.update(|cx| set_thread_running(&f.first.id, true, cx));
    cx.run_until_parked();
    assert!(f32::from(bounds(&f, cx, running.clone()).size.width) > 0.0);

    // Releasing liveness restores the rest state exactly.
    cx.update(|cx| set_thread_running(&f.first.id, false, cx));
    cx.run_until_parked();
    assert!(absent(&f, cx, running.clone()));
}

/// #150 R10: the tail is a single slot. Production rows never render a resting
/// timestamp (`show_timestamp_at_rest = false`), so the running-row precedence
/// is asserted on the pure predicate in `sidebar/mod.rs`; this mounted case
/// pins the part the scene can prove — a running row still shows exactly one
/// tail indicator and never a `thread-timestamp-*` selector.
#[gpui_kit::test]
async fn issue150_running_row_shows_no_resting_timestamp(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let timestamp = format!("thread-timestamp-{}", f.first.id);
    cx.update(|cx| set_thread_running(&f.first.id, true, cx));
    cx.run_until_parked();
    assert!(absent(&f, cx, timestamp));
    assert!(
        f32::from(
            bounds(&f, cx, format!("project-thread-running-{}", f.first.id))
                .size
                .width
        ) > 0.0
    );
}

/// #150 R10: the indicator is inserted left of the action trigger inside the
/// right-aligned tail group, so appearing/disappearing never shifts the
/// trigger or the row.
#[gpui_kit::test]
async fn issue150_row_indicator_does_not_displace_the_row_or_trigger(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let row_selector = format!("project-thread-row-{}", f.first.id);
    let trigger_selector = format!("project-thread-actions-{}", f.first.id);
    let running = format!("project-thread-running-{}", f.first.id);

    let row_before = bounds(&f, cx, row_selector.clone());
    let trigger_before = bounds(&f, cx, trigger_selector.clone());

    cx.update(|cx| set_thread_running(&f.first.id, true, cx));
    cx.run_until_parked();
    assert!(f32::from(bounds(&f, cx, running.clone()).size.width) > 0.0);

    // C5: the row height is a token and the trigger keeps its column, so the
    // indicator is purely additive inside the existing slot.
    assert_eq!(bounds(&f, cx, row_selector.clone()), row_before);
    assert_eq!(bounds(&f, cx, trigger_selector.clone()), trigger_before);
    assert_eq!(
        f32::from(bounds(&f, cx, row_selector).size.height),
        Typography::SIDEBAR_LINE_HEIGHT
    );
}

/// #150 R4: a background run lights its own row even when another thread is
/// the opened route (Issue #67 concurrency).
#[gpui_kit::test]
async fn issue150_background_thread_lights_its_own_row(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let background = conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let background_indicator = format!("standalone-thread-running-{}", background.id);
    let opened_indicator = format!("project-thread-running-{}", f.first.id);
    assert!(absent(&f, cx, background_indicator.clone()));

    cx.update(|cx| set_thread_running(&background.id, true, cx));
    cx.run_until_parked();

    assert!(f32::from(bounds(&f, cx, background_indicator).size.width) > 0.0);
    assert!(
        absent(&f, cx, opened_indicator),
        "the opened row must not borrow another thread's liveness"
    );
}

/// #150 R12: without the projection installed the UI never invents liveness,
/// even for the opened row.
#[gpui_kit::test]
async fn issue150_missing_liveness_projection_shows_no_indicator(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    cx.update(|cx| assert!(!cx.has_global::<RunningThreadsGlobal>()));
    assert!(absent(
        &f,
        cx,
        format!("project-thread-running-{}", f.first.id)
    ));
}
