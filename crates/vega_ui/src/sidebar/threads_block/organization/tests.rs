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
    assert_eq!(menu_items.len(), 2, "first project can move down");
    assert_eq!(menu_items[1].0, "下移");
    assert_eq!(menu_items[0].0, "移除项目");
    assert!(matches!(
        &menu_items[0].1,
        MenuCommand::RemoveProject(id) if id == "p"
    ));
    assert!(!absent(&f, cx, "organization-menu-0"));
    assert!(!absent(&f, cx, "organization-menu-1"));
    assert!(absent(&f, cx, "organization-menu-2"));
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
async fn project_tasks_grow_lazily_and_independently(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture_with_size(cx, 1200., 400.);
    cx.run_until_parked();
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    conversation::set_thread_pinned(&store, &f.first.id, true).unwrap();
    for _ in 0..15 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
        conversation::create_thread(&store, "q", "model", "confirm").unwrap();
    }
    let newest_p = conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    conversation::set_thread_status(&store, &newest_p.id, ThreadStatus::Archived).unwrap();
    conversation::set_thread_pinned(&store, &f.other.id, true).unwrap();
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
    assert_eq!(p_ids.len(), 19);
    assert_eq!(q_ids.len(), 15);
    let window = |project_id: &str, cx: &gpui_kit::TestAppContext| {
        sessions(&f, cx).read_with(cx, |block, _| {
            block
                .organization
                .as_ref()
                .unwrap()
                .project_threads_visible
                .get(project_id)
                .copied()
                .unwrap()
        })
    };
    assert_eq!(window("p", cx), 5);
    assert_eq!(window("q", cx), 5);
    assert!(!absent(&f, cx, "project-p-show-more"));
    assert!(!absent(&f, cx, "project-q-show-more"));
    assert!(absent(&f, cx, "project-p-show-less"));
    assert!(absent(&f, cx, "project-q-show-less"));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.first.id)));
    assert!(absent(&f, cx, format!("project-thread-row-{}", f.other.id)));
    assert!(absent(
        &f,
        cx,
        format!("project-thread-row-{}", newest_p.id)
    ));

    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let mounted = |ids: &[String], cx: &mut gpui_kit::TestAppContext| {
        ids.iter()
            .filter(|id| !absent(&f, cx, format!("project-thread-row-{id}")))
            .count()
    };
    let scroll_to_bottom = |visual: &mut gpui_kit::VisualTestContext| {
        let viewport = visual.debug_bounds("sidebar-scroll").unwrap();
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.),
                gpui_kit::px(-100_000.),
            )),
            modifiers: gpui_kit::Modifiers::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    };
    let scroll_to_top = |visual: &mut gpui_kit::VisualTestContext| {
        let viewport = visual.debug_bounds("sidebar-scroll").unwrap();
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.),
                gpui_kit::px(100_000.),
            )),
            modifiers: gpui_kit::Modifiers::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    };

    // The first project's header stays inside this short viewport, so the real
    // collapse control is reachable without scrolling.
    click(&f, cx, "project-header-p");
    assert_eq!(mounted(&p_ids, cx), 0);
    assert_eq!(window("p", cx), 5, "a collapsed project keeps its window");
    assert_eq!(mounted(&q_ids, cx), 5);

    scroll_to_bottom(&mut visual);
    assert_eq!(window("p", cx), 5, "a collapsed project never grows");
    assert_eq!(window("q", cx), 10);

    scroll_to_top(&mut visual);
    click(&f, cx, "project-header-p");
    assert_eq!(mounted(&p_ids, cx), 5, "reopening resumes the window");
    assert_eq!(mounted(&q_ids, cx), 10);

    scroll_to_bottom(&mut visual);
    assert_eq!(window("q", cx), q_ids.len());
    assert_eq!(window("p", cx), 10);

    scroll_to_bottom(&mut visual);
    assert_eq!(window("p", cx), 15);

    scroll_to_bottom(&mut visual);
    assert_eq!(mounted(&p_ids, cx), p_ids.len());
    assert_eq!(mounted(&q_ids, cx), q_ids.len());
    assert!(absent(
        &f,
        cx,
        format!("project-thread-row-{}", newest_p.id)
    ));
    assert!(absent(&f, cx, "project-p-show-more"));
    assert!(absent(&f, cx, "project-p-show-less"));

    let preferences = snapshot(&f).preferences;
    for (project_id, ids) in [("p", p_ids.as_slice()), ("q", q_ids.as_slice())] {
        let expected: Vec<String> = super::projections::sorted_threads(
            &state
                .threads
                .iter()
                .filter(|thread| {
                    thread.project_id == project_id
                        && thread.status == ThreadStatus::Active
                        && !thread.pinned
                })
                .cloned()
                .collect::<Vec<_>>(),
            &preferences,
        )
        .into_iter()
        .map(|thread| thread.id)
        .collect();
        let mut painted = ids.to_vec();
        painted.sort_by_key(|id| bounds(&f, cx, format!("project-thread-row-{id}")).top());
        assert_eq!(painted, expected);
    }

    sessions(&f, cx).update(cx, |block, _| {
        block
            .organization
            .as_mut()
            .unwrap()
            .project_threads_visible
            .insert("removed-project".into(), 10);
    });
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert!(sessions(&f, cx).read_with(cx, |block, _| {
        !block
            .organization
            .as_ref()
            .unwrap()
            .project_threads_visible
            .contains_key("removed-project")
    }));
}

#[gpui_kit::test]
async fn project_task_show_more_appends_one_page_per_activation(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture_with_size(cx, 1200., 500.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    for _ in 0..12 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    for _ in 0..12 {
        conversation::create_standalone_thread(&store, "model", "confirm").unwrap();
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let p_ids: Vec<String> = snapshot(&f)
        .threads
        .into_iter()
        .filter(|thread| {
            thread.project_id == "p" && thread.status == ThreadStatus::Active && !thread.pinned
        })
        .map(|thread| thread.id)
        .collect();
    assert_eq!(p_ids.len(), 17);
    let mounted = |cx: &mut gpui_kit::TestAppContext| {
        p_ids
            .iter()
            .filter(|id| !absent(&f, cx, format!("project-thread-row-{id}")))
            .count()
    };
    let window = |cx: &gpui_kit::TestAppContext| {
        sessions(&f, cx).read_with(cx, |block, _| {
            block
                .organization
                .as_ref()
                .unwrap()
                .project_threads_visible
                .get("p")
                .copied()
                .unwrap()
        })
    };
    assert_eq!(window(cx), 5);
    assert_eq!(mounted(cx), 5);
    assert!(!absent(&f, cx, "project-p-show-more"));
    assert!(absent(&f, cx, "project-p-show-less"));
    let control = bounds(&f, cx, "project-p-show-more");
    let label = bounds(&f, cx, "project-p-progressive-label");
    assert_eq!(
        f32::from(label.left() - control.left()),
        Layout::SIDEBAR_ROW_INSET
    );

    let has_page = |cx: &gpui_kit::TestAppContext| {
        sessions(&f, cx).read_with(cx, |block, _| {
            block
                .organization
                .as_ref()
                .unwrap()
                .has_project_task_page("p")
        })
    };
    let grow = |cx: &mut gpui_kit::TestAppContext| {
        let grown = sessions(&f, cx).update(cx, |block, cx| {
            let grown = block
                .organization
                .as_mut()
                .unwrap()
                .grow_project_task_page("p");
            if grown {
                cx.notify();
            }
            grown
        });
        cx.run_until_parked();
        grown
    };
    click(&f, cx, "project-p-show-more");
    assert_eq!(window(cx), 10);
    assert_eq!(mounted(cx), 10);
    assert!(has_page(cx));

    assert!(grow(cx));
    assert_eq!(window(cx), 15);
    assert_eq!(mounted(cx), 15);
    assert!(has_page(cx));

    assert!(grow(cx));
    assert_eq!(window(cx), p_ids.len());
    assert_eq!(mounted(cx), p_ids.len());
    assert!(!has_page(cx));
    assert!(absent(&f, cx, "project-p-show-more"));
    assert!(absent(&f, cx, "project-p-show-less"));

    assert!(!grow(cx));
    assert_eq!(window(cx), p_ids.len());
}

#[gpui_kit::test]
async fn project_tasks_fill_a_tall_viewport_until_complete(cx: &mut gpui_kit::TestAppContext) {
    let f = fixture_with_size(cx, 1200., 2600.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    for _ in 0..12 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let project_ids: Vec<String> = snapshot(&f)
        .threads
        .into_iter()
        .filter(|thread| {
            thread.project_id == "p" && thread.status == ThreadStatus::Active && !thread.pinned
        })
        .map(|thread| thread.id)
        .collect();
    assert_eq!(project_ids.len(), 17);
    for id in &project_ids {
        assert!(!absent(&f, cx, format!("project-thread-row-{id}")));
    }
    assert!(absent(&f, cx, "project-p-show-more"));
    assert!(absent(&f, cx, "project-p-show-less"));
}

#[gpui_kit::test]
async fn project_task_page_growth_stops_when_all_rows_are_loaded(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    for _ in 0..5 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();

    let sessions = sessions(&f, cx);
    let _ = sessions.update(cx, |block, cx| block.grow_lazy_lists(cx));
    assert!(!sessions.read_with(cx, |block, _| block.has_lazy_list_growth()));
    assert!(absent(&f, cx, "project-p-show-more"));
}

fn create_reorder_threads(
    store: &Store,
    project: Option<&str>,
    count: usize,
    pinned: bool,
) -> Vec<String> {
    (0..count)
        .map(|index| {
            let thread = match project {
                Some(project) => conversation::create_thread(store, project, "model", "confirm"),
                None => conversation::create_standalone_thread(store, "model", "confirm"),
            }
            .unwrap();
            conversation::set_thread_pinned(store, &thread.id, pinned).unwrap();
            store
                .conn()
                .execute(
                    "UPDATE threads SET updated_at=?1,created_at=?2 WHERE id=?3",
                    (9000 - index as i64, 1000 + index as i64, &thread.id),
                )
                .unwrap();
            thread.id
        })
        .collect()
}

fn scope_order(
    state: &SidebarOrganizationSnapshot,
    project: Option<&str>,
    pinned: bool,
) -> Vec<String> {
    service::sorted_threads(&state.threads, &state.preferences)
        .into_iter()
        .filter(|thread| thread.pinned == pinned && (pinned || thread.project_binding() == project))
        .map(|thread| thread.id)
        .collect()
}

fn start_row_drag(f: &Fixture, cx: &mut gpui_kit::TestAppContext, selector: impl Into<String>) {
    let row = bounds(f, cx, selector);
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(row.center(), None, Default::default());
    visual.simulate_mouse_down(row.center(), MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        row.center() + point(px(12.), px(0.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    cx.run_until_parked();
    assert!(cx.update(|cx| cx.has_active_drag()));
    assert!(!absent(f, cx, "sidebar-reorder-preview"));
}

fn move_drag(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    position: gpui_kit::Point<gpui_kit::Pixels>,
) {
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(position, Some(MouseButton::Left), Default::default());
    cx.run_until_parked();
}

fn release_drag(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    position: gpui_kit::Point<gpui_kit::Pixels>,
) {
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_up(position, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(!cx.update(|cx| cx.has_active_drag()));
    assert!(sessions(f, cx).read_with(cx, |block, _| {
        !block.organization.as_ref().unwrap().reorder.gesture
    }));
}

fn drop_row(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    source: impl Into<String>,
    target: impl Into<String>,
    after: bool,
) {
    start_row_drag(f, cx, source);
    let target = bounds(f, cx, target);
    let position = point(
        target.center().x,
        target.top() + target.size.height * if after { 0.75 } else { 0.25 },
    );
    move_drag(f, cx, position);
    release_drag(f, cx, position);
}

fn assert_reorder_navigation_unchanged(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        assert!(cx.global::<PendingDeleteConfirm>().0.is_none());
    });
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );
    assert!(
        sessions(f, cx).read_with(cx, |block, _| block.editing.is_none()
            && block.actions_open.is_none())
    );
}

#[gpui_kit::test]
async fn sidebar_reorder_pointer_moves_projects_and_all_thread_scopes(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 1400.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('r','owned-r','r',0,0)", []).unwrap();
    let pinned = create_reorder_threads(&store, None, 3, true);
    let recent = create_reorder_threads(&store, None, 3, false);
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let original = snapshot(&f);
    let project_tasks = scope_order(&original, Some("p"), false);

    drop_row(&f, cx, "project-header-r", "project-header-p", false);
    assert_eq!(snapshot(&f).project_order, ["r", "p", "q"]);
    assert!(bounds(&f, cx, "project-header-r").top() < bounds(&f, cx, "project-header-p").top());
    drop_row(&f, cx, "project-header-r", "project-header-q", true);
    assert_eq!(snapshot(&f).project_order, ["p", "q", "r"]);

    for (ids, prefix, project, pinned_scope) in [
        (project_tasks, "project-thread-row-", Some("p"), false),
        (pinned, "pinned-thread-row-", None, true),
        (recent, "standalone-thread-row-", None, false),
    ] {
        drop_row(
            &f,
            cx,
            format!("{prefix}{}", ids[2]),
            format!("{prefix}{}", ids[0]),
            false,
        );
        let mut expected = ids.clone();
        expected.remove(2);
        expected.insert(0, ids[2].clone());
        assert_eq!(scope_order(&snapshot(&f), project, pinned_scope), expected);
        assert!(
            bounds(&f, cx, format!("{prefix}{}", ids[2])).top()
                < bounds(&f, cx, format!("{prefix}{}", ids[0])).top()
        );
        drop_row(
            &f,
            cx,
            format!("{prefix}{}", ids[2]),
            format!(
                "{prefix}{}",
                ids.iter().rev().find(|id| **id != ids[2]).unwrap()
            ),
            true,
        );
        expected.remove(0);
        expected.push(ids[2].clone());
        assert_eq!(scope_order(&snapshot(&f), project, pinned_scope), expected);
        assert_reorder_navigation_unchanged(&f, cx);
    }
    let saved = snapshot(&f);
    let mut saved_threads = saved.threads.clone();
    let mut original_threads = original.threads.clone();
    saved_threads.sort_by(|a, b| a.id.cmp(&b.id));
    original_threads.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(saved_threads, original_threads);
    assert_eq!(saved.collapsed, original.collapsed);
    assert_eq!(saved.memberships, original.memberships);
    assert_eq!(saved.preferences.sort, SidebarTaskSort::Manual);
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_eq!(snapshot(&f), saved);
}

#[gpui_kit::test]
async fn sidebar_reorder_rejects_cross_scope_blank_and_cancel_without_clicks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 1400.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let pinned = create_reorder_threads(&store, None, 7, true);
    let recent = create_reorder_threads(&store, None, 2, false);
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let before = snapshot(&f);
    let source = format!("project-thread-row-{}", f.first.id);
    for target in [
        source.clone(),
        format!("project-thread-row-{}", f.other.id),
        format!("pinned-thread-row-{}", pinned[0]),
        format!("standalone-thread-row-{}", recent[0]),
        "project-header-q".into(),
        "organization-header-projects".into(),
    ] {
        drop_row(&f, cx, source.clone(), target, false);
        assert_eq!(snapshot(&f), before);
        assert_reorder_navigation_unchanged(&f, cx);
    }
    start_row_drag(&f, cx, &source);
    move_drag(&f, cx, point(px(1000.), px(20.)));
    release_drag(&f, cx, point(px(1000.), px(20.)));
    assert_eq!(snapshot(&f), before);

    start_row_drag(&f, cx, &source);
    let target = bounds(&f, cx, format!("project-thread-row-{}", f.other.id)).center();
    move_drag(&f, cx, target);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(!cx.update(|cx| cx.has_active_drag()));
    release_drag(&f, cx, target);
    assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert_eq!(snapshot(&f), before);
    assert_reorder_navigation_unchanged(&f, cx);

    start_row_drag(&f, cx, format!("pinned-thread-row-{}", pinned[0]));
    let project_row = bounds(&f, cx, "project-header-p");
    let hidden_row = bounds(&f, cx, format!("pinned-thread-row-{}", pinned[6]));
    assert!(hidden_row.bottom() > bounds(&f, cx, "organization-pinned-scroll").bottom());
    move_drag(&f, cx, project_row.center());
    release_drag(&f, cx, project_row.center());
    assert_eq!(
        snapshot(&f),
        before,
        "clipped pinned rows cannot receive a drop over projects"
    );
    assert_reorder_navigation_unchanged(&f, cx);
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    click(&f, cx, format!("project-thread-row-{}", f.other.id));
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        f.other.id
    );
}

#[gpui_kit::test]
async fn sidebar_reorder_keyboard_menus_restore_manual_order_and_keep_endpoints_valid(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let original = snapshot(&f);
    let ids = scope_order(&original, Some("p"), false);
    let source = ids.last().unwrap().clone();
    let actions = sessions(&f, cx).read_with(cx, |block, _| block.organization_actions(&source));
    assert_eq!(
        actions
            .iter()
            .map(|(label, _)| label.as_str())
            .collect::<Vec<_>>(),
        ["上移"]
    );
    let focus =
        sessions(&f, cx).read_with(cx, |block, _| block.thread_action_focuses[&source].clone());
    f.window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(f.window.into(), "enter up enter");
    cx.run_until_parked();
    let mut expected = ids.clone();
    let end = expected.len() - 1;
    expected.swap(end - 1, end);
    assert_eq!(scope_order(&snapshot(&f), Some("p"), false), expected);
    let manual = snapshot(&f).preferences.manual_thread_order;
    for (keys, sort) in [
        ("enter", SidebarTaskSort::Updated),
        ("down enter", SidebarTaskSort::Created),
        ("down down enter", SidebarTaskSort::Manual),
    ] {
        click(&f, cx, "organization-session-sort");
        cx.simulate_keystrokes(f.window.into(), keys);
        cx.run_until_parked();
        assert_eq!(snapshot(&f).preferences.sort, sort);
        assert_eq!(snapshot(&f).preferences.manual_thread_order, manual);
    }
    assert_eq!(scope_order(&snapshot(&f), Some("p"), false), expected);
    assert_eq!(
        sessions(&f, cx).read_with(cx, |block, _| block.organization_actions(&expected[0]))[0].0,
        "下移"
    );
    click(&f, cx, "project-more-q");
    cx.simulate_keystrokes(f.window.into(), "down enter");
    cx.run_until_parked();
    assert_eq!(snapshot(&f).project_order, ["q", "p"]);
    click(&f, cx, "project-more-q");
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(absent(&f, cx, "organization-menu"));
    assert_reorder_navigation_unchanged(&f, cx);
}

#[gpui_kit::test]
async fn sidebar_reorder_after_visible_boundary_preserves_unrendered_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let extra = create_reorder_threads(&store, Some("p"), 8, false);
    conversation::set_thread_status(&store, &extra[0], ThreadStatus::Archived).unwrap();
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let before = snapshot(&f);
    let ids = scope_order(&before, Some("p"), false);
    assert_eq!(ids.len(), 13);
    assert!(absent(&f, cx, format!("project-thread-row-{}", ids[6])));
    drop_row(
        &f,
        cx,
        format!("project-thread-row-{}", ids[0]),
        format!("project-thread-row-{}", ids[4]),
        true,
    );
    let mut expected = ids.clone();
    expected.remove(0);
    expected.insert(4, ids[0].clone());
    let after = snapshot(&f);
    assert_eq!(scope_order(&after, Some("p"), false), expected);
    assert_eq!(after.threads, before.threads);
    assert_eq!(
        after.preferences.manual_thread_order.len(),
        after.threads.len()
    );
    assert!(absent(&f, cx, format!("project-thread-row-{}", ids[6])));
    click(&f, cx, "project-p-show-more");
    assert!(!absent(&f, cx, format!("project-thread-row-{}", ids[6])));
    assert_eq!(
        sessions(&f, cx).read_with(cx, |block, _| block
            .organization
            .as_ref()
            .unwrap()
            .project_threads_visible["p"]),
        10
    );
    assert!(
        bounds(&f, cx, format!("project-thread-row-{}", ids[0])).top()
            < bounds(&f, cx, format!("project-thread-row-{}", ids[6])).top()
    );
    assert_reorder_navigation_unchanged(&f, cx);
}

fn has_reorder_line(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    target: gpui_kit::Bounds<gpui_kit::Pixels>,
    after: bool,
) -> bool {
    f.window
        .update(cx, |_, window, cx| {
            let scale = window.scale_factor();
            let color: gpui_kit::Hsla = theme(cx).colors.accent.into();
            let top = if after {
                target.bottom() - px(Layout::SIDEBAR_REORDER_LINE_HEIGHT)
            } else {
                target.top()
            };
            window.painted_quads().iter().any(|quad| {
                (quad.bounds.top().as_f32() / scale - f32::from(top)).abs() < 0.2
                    && (quad.bounds.size.height.as_f32() / scale
                        - Layout::SIDEBAR_REORDER_LINE_HEIGHT)
                        .abs()
                        < 0.2
                    && (quad.bounds.size.width.as_f32() / scale - f32::from(target.size.width))
                        .abs()
                        < 0.2
                    && quad.background.as_solid() == Some(color)
            })
        })
        .unwrap()
}

#[gpui_kit::test]
async fn sidebar_reorder_feedback_and_trailing_buttons_preserve_row_geometry(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 960., 900.);
    for dark in [false, true] {
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            })
        });
        cx.run_until_parked();
        let before = snapshot(&f);
        let ids = scope_order(&before, Some("p"), false);
        let source = ids.last().unwrap();
        let target_id = &ids[0];
        let selector = format!("project-thread-row-{target_id}");
        let original_geometry = bounds(&f, cx, &selector);
        start_row_drag(&f, cx, format!("project-thread-row-{source}"));
        let target = bounds(&f, cx, &selector);
        move_drag(
            &f,
            cx,
            point(target.center().x, target.top() + target.size.height * 0.25),
        );
        assert!(has_reorder_line(&f, cx, target, false));
        assert_eq!(bounds(&f, cx, &selector), original_geometry);
        let invalid_target = bounds(&f, cx, format!("project-thread-row-{}", f.other.id));
        move_drag(
            &f,
            cx,
            point(
                invalid_target.center().x,
                invalid_target.top() + invalid_target.size.height * 0.25,
            ),
        );
        assert!(!has_reorder_line(&f, cx, invalid_target, false));
        assert!(!has_reorder_line(&f, cx, target, false));
        move_drag(
            &f,
            cx,
            point(target.center().x, target.top() + target.size.height * 0.75),
        );
        assert!(has_reorder_line(&f, cx, target, true));
        assert!(!has_reorder_line(&f, cx, target, false));
        let button = bounds(
            &f,
            cx,
            format!(
                "project-thread-{}-{target_id}",
                if dark { "archive" } else { "pin" }
            ),
        );
        move_drag(&f, cx, button.center());
        release_drag(&f, cx, button.center());
        let mut expected = ids.clone();
        expected.pop();
        expected.insert(1, source.clone());
        let after = snapshot(&f);
        assert_eq!(scope_order(&after, Some("p"), false), expected);
        assert_eq!(after.threads, before.threads);
        assert_eq!(
            bounds(&f, cx, &selector).size.height,
            original_geometry.size.height
        );
        let settled_geometry = bounds(&f, cx, &selector);
        assert!(!has_reorder_line(&f, cx, settled_geometry, true));
        assert_reorder_navigation_unchanged(&f, cx);
    }
}

#[gpui_kit::test]
async fn sidebar_reorder_save_failures_preserve_committed_order_and_allow_retry(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let original = snapshot(&f);
    let ids = scope_order(&original, Some("p"), false);
    let source = format!("project-thread-row-{}", ids.last().unwrap());
    let target = format!("project-thread-row-{}", ids[0]);
    sessions(&f, cx).update(cx, |block, cx| {
        block.organization.as_mut().unwrap().pending = true;
        cx.notify();
    });
    drop_row(&f, cx, &source, &target, false);
    assert_eq!(snapshot(&f), original);
    assert!(sessions(&f, cx).read_with(cx, |block, _| {
        block.error.as_ref().unwrap().contains("正在保存")
    }));
    sessions(&f, cx).update(cx, |block, cx| {
        let org = block.organization.as_mut().unwrap();
        org.pending = false;
        org.refresh_queued = false;
        cx.notify();
    });
    store
        .conn()
        .execute(
            "UPDATE sidebar_organization SET revision=revision+1 WHERE singleton=1",
            [],
        )
        .unwrap();
    let conflict = snapshot(&f);
    drop_row(&f, cx, &source, &target, false);
    assert_eq!(snapshot(&f), conflict);
    assert!(sessions(&f, cx).read_with(cx, |block, _| {
        block
            .error
            .as_ref()
            .unwrap()
            .contains("organization changed")
    }));
    store.conn().execute_batch("CREATE TRIGGER owned_reorder_failure BEFORE UPDATE ON sidebar_organization BEGIN SELECT RAISE(FAIL, 'owned save failure'); END;").unwrap();
    drop_row(&f, cx, &source, &target, false);
    assert_eq!(snapshot(&f), conflict);
    assert!(sessions(&f, cx).read_with(cx, |block, _| {
        block.error.as_ref().unwrap().contains("owned save failure")
    }));
    assert!(bounds(&f, cx, &target).top() < bounds(&f, cx, &source).top());
    store
        .conn()
        .execute_batch("DROP TRIGGER owned_reorder_failure;")
        .unwrap();
    drop_row(&f, cx, &source, &target, false);
    let mut expected = ids.clone();
    expected.pop();
    expected.insert(0, ids.last().unwrap().clone());
    assert_eq!(scope_order(&snapshot(&f), Some("p"), false), expected);
    assert!(sessions(&f, cx).read_with(cx, |block, _| block.error.is_none()));
    assert_reorder_navigation_unchanged(&f, cx);
}
