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
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("organization.db")).unwrap();
    store.migrate().unwrap();
    for id in ["p", "q"] {
        store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES(?1,?2,?1,0,?3)", (id, dir.path().join(id).to_string_lossy().to_string(), if id == "p" { 2 } else {1})).unwrap();
    }
    let first = conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    for _ in 0..5 {
        conversation::create_thread(&store, "p", "model", "confirm").unwrap();
    }
    let other = conversation::create_thread(&store, "q", "model", "confirm").unwrap();
    store.conn().execute("UPDATE threads SET created_at = 9000000000000, updated_at = 9000000000000 WHERE id = ?1", [&other.id]).unwrap();
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
                    gpui_kit::Bounds::centered(None, gpui_kit::size(px(1200.), px(1100.)), cx),
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
fn snapshot(f: &Fixture) -> SidebarOrganizationSnapshot {
    service::snapshot(&Store::open(f.dir.path().join("organization.db")).unwrap()).unwrap()
}
fn create_group(f: &Fixture, cx: &mut gpui_kit::TestAppContext, name: &str) -> String {
    click(f, cx, "organization-new-group");
    cx.simulate_input(f.window.into(), name);
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    snapshot(f)
        .groups
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .id
        .clone()
}
#[gpui_kit::test]
async fn mounted_sidebar_views_more_group_edit_menu_move_restart_and_navigation_guard(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    let before_escape = snapshot(&f);
    click(&f, cx, "organization-filter");
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(
        f.sidebar.read_with(cx, |s, cx| s
            .sessions_block
            .read(cx)
            .organization
            .as_ref()
            .unwrap()
            .menu
            .is_none()),
        "Escape must close the filter menu under the VegaWindow action binding"
    );
    assert_eq!(snapshot(&f), before_escape);
    cx.update(|cx| {
        assert!(
            cx.global::<SettingsOpen>().0,
            "menu Escape must not close settings"
        );
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
    });
    assert_eq!(
        f.draft.read_with(cx, |d, _| d.text().to_string()),
        "owned unsent draft"
    );
    assert_eq!(snapshot(&f).threads.len(), 7);
    click(&f, cx, "project-more-p");
    assert_eq!(
        f.sidebar.read_with(cx, |s, cx| s
            .sessions_block
            .read(cx)
            .organization
            .as_ref()
            .unwrap()
            .more["p"]),
        10
    );
    click(&f, cx, "organization-filter");
    cx.simulate_keystrokes(f.window.into(), "down enter");
    cx.run_until_parked();
    assert_eq!(
        snapshot(&f).preferences.project_view,
        SidebarProjectView::Timeline
    );
    click(&f, cx, "organization-filter");
    click(&f, cx, "organization-menu-3");
    assert_eq!(snapshot(&f).preferences.sort, SidebarTaskSort::Created);
    assert!(
        snapshot(&f)
            .threads
            .iter()
            .find(|t| t.id == f.other.id)
            .unwrap()
            .unread
    );
    click(&f, cx, "organization-groups");
    click(&f, cx, "organization-new-group");
    cx.simulate_input(f.window.into(), "cancelled");
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(snapshot(&f).groups.is_empty());
    let group = create_group(&f, cx, "Review");
    click(&f, cx, format!("group-menu-{group}"));
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(
        f.sidebar.read_with(cx, |s, cx| s
            .sessions_block
            .read(cx)
            .organization
            .as_ref()
            .unwrap()
            .menu
            .is_none()),
        "Escape must close the group menu under the VegaWindow action binding"
    );
    cx.update(|cx| assert!(cx.global::<SettingsOpen>().0));
    click(&f, cx, format!("group-menu-{group}"));
    click(&f, cx, "organization-menu-7");
    assert_eq!(snapshot(&f).groups[0].color, SidebarGroupColor::Blue);
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    cx.simulate_keystrokes(f.window.into(), "up enter");
    cx.run_until_parked();
    assert_eq!(snapshot(&f).memberships[0].thread_id, f.other.id);
    assert_eq!(snapshot(&f).memberships[0].group_id, group);
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
    });
    assert_eq!(
        f.draft.read_with(cx, |d, _| d.text().to_string()),
        "owned unsent draft"
    );
    click(&f, cx, format!("group-menu-{group}"));
    click(&f, cx, "organization-menu-1");
    cx.simulate_keystrokes(f.window.into(), "cmd-a");
    cx.simulate_input(f.window.into(), "Renamed");
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert_eq!(snapshot(&f).groups[0].name, "Renamed");
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    click(&f, cx, format!("thread-action-{}-1", f.other.id));
    cx.simulate_keystrokes(f.window.into(), "cmd-a");
    cx.simulate_input(f.window.into(), "Other project title");
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert_eq!(
        snapshot(&f)
            .threads
            .iter()
            .find(|t| t.id == f.other.id)
            .unwrap()
            .title,
        "Other project title"
    );
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    click(&f, cx, format!("thread-action-{}-3", f.other.id));
    assert!(
        !snapshot(&f)
            .threads
            .iter()
            .find(|t| t.id == f.other.id)
            .unwrap()
            .unread
    );
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    click(&f, cx, format!("thread-action-{}-3", f.other.id));
    cx.update(|cx| {
        cx.set_global(crate::navigation::DraftNavigationGuard {
            task: f.first.id.clone(),
            input: f.draft.downgrade(),
            cached_tasks: 100,
            cached_bytes: 0,
        })
    });
    click(&f, cx, format!("thread-row-{}", f.other.id));
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        )
    });
    assert!(
        snapshot(&f)
            .threads
            .iter()
            .find(|t| t.id == f.other.id)
            .unwrap()
            .unread
    );
    cx.update(|cx| {
        cx.update_global::<crate::navigation::DraftNavigationGuard, _>(|g, _| g.cached_tasks = 0)
    });
    click(&f, cx, format!("thread-row-{}", f.other.id));
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.other.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("q"));
    });
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    click(&f, cx, format!("thread-action-{}-2", f.other.id));
    assert_eq!(snapshot(&f).memberships[0].group_id, group);
    click(&f, cx, "organization-archive");
    click(&f, cx, format!("thread-actions-{}", f.other.id));
    click(&f, cx, format!("thread-action-{}-2", f.other.id));
    click(&f, cx, "organization-archive");
    assert_eq!(snapshot(&f).memberships[0].group_id, group);
    click(&f, cx, "organization-collapse-all");
    assert!(
        snapshot(&f)
            .collapsed
            .contains(&SidebarCollapseTarget::Group(group.clone()))
    );
    let restarted = cx.new(Sidebar::new);
    cx.run_until_parked();
    assert_eq!(
        restarted.read_with(cx, |s, cx| s
            .sessions_block
            .read(cx)
            .organization
            .as_ref()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
            .preferences
            .view),
        SidebarView::Groups
    );
    click(&f, cx, format!("group-menu-{group}"));
    click(&f, cx, "organization-menu-9");
    assert!(snapshot(&f).groups.is_empty());
    assert_eq!(snapshot(&f).threads.len(), 7);
    assert!(snapshot(&f).memberships.is_empty());
}

fn drag(f: &Fixture, cx: &mut gpui_kit::TestAppContext, source: String, target: String) {
    let source: &'static str = Box::leak(source.into_boxed_str());
    let target: &'static str = Box::leak(target.into_boxed_str());
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let from = visual.debug_bounds(source).unwrap().center();
    let to = visual.debug_bounds(target).unwrap().center();
    visual.simulate_mouse_down(from, MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        from + point(px(12.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    visual.update(|_, cx| assert!(cx.has_active_drag(), "pointer must start a real drag"));
    visual.simulate_mouse_move(to, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(to, MouseButton::Left, Default::default());
    cx.run_until_parked();
}
#[gpui_kit::test]
async fn mounted_sidebar_real_drag_and_conflict_retry_preserve_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    click(&f, cx, "project-collapse-p");
    drag(&f, cx, "project-header-q".into(), "project-header-p".into());
    assert_eq!(
        snapshot(&f).project_order.first().map(String::as_str),
        Some("q")
    );
    click(&f, cx, "organization-groups");
    let first = create_group(&f, cx, "First");
    let second = create_group(&f, cx, "Second");
    drag(
        &f,
        cx,
        format!("group-header-{second}"),
        format!("group-header-{first}"),
    );
    assert_eq!(snapshot(&f).groups[0].id, second);
    drag(
        &f,
        cx,
        format!("thread-row-{}", f.other.id),
        format!("group-empty-{second}"),
    );
    assert_eq!(snapshot(&f).memberships[0].group_id, second);
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        )
    });
    let unchanged = snapshot(&f)
        .threads
        .iter()
        .find(|t| t.id == f.other.id)
        .unwrap()
        .clone();
    drag(
        &f,
        cx,
        format!("thread-row-{}", f.other.id),
        "organization-ungrouped".into(),
    );
    assert!(
        !snapshot(&f)
            .memberships
            .iter()
            .any(|m| m.thread_id == f.other.id),
        "ungrouped heading must accept drag-out"
    );
    drag(
        &f,
        cx,
        format!("thread-row-{}", f.other.id),
        format!("group-empty-{second}"),
    );
    assert!(
        snapshot(&f)
            .memberships
            .iter()
            .any(|m| m.thread_id == f.other.id && m.group_id == second)
    );
    let existing_ungrouped = snapshot(&f)
        .threads
        .into_iter()
        .filter(|t| t.project_id == "p")
        .min_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        })
        .unwrap();
    drag(
        &f,
        cx,
        format!("thread-row-{}", f.other.id),
        format!("thread-row-{}", existing_ungrouped.id),
    );
    assert!(
        !snapshot(&f)
            .memberships
            .iter()
            .any(|m| m.thread_id == f.other.id),
        "existing ungrouped row must accept drag-out"
    );
    assert_eq!(
        snapshot(&f)
            .threads
            .iter()
            .find(|t| t.id == f.other.id)
            .unwrap(),
        &unchanged,
        "drag-out must preserve project and all task metadata"
    );
    cx.update(|cx| {
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.first.id
        );
    });
    assert_eq!(
        f.draft.read_with(cx, |d, _| d.text().to_string()),
        "owned unsent draft"
    );
    click(&f, cx, "organization-new-group");
    cx.simulate_input(f.window.into(), "Retry");
    let external = Store::open(f.dir.path().join("organization.db")).unwrap();
    service::apply(
        &external,
        snapshot(&f).revision,
        SidebarOrganizationAction::SetGroupColor {
            group_id: first.clone(),
            color: SidebarGroupColor::Red,
        },
    )
    .unwrap();
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert!(!snapshot(&f).groups.iter().any(|g| g.name == "Retry"));
    assert!(f.sidebar.read_with(cx, |s, cx| {
        s.sessions_block
            .read(cx)
            .error
            .as_ref()
            .is_some_and(|e| e.contains("organization changed"))
    }));
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert!(snapshot(&f).groups.iter().any(|g| g.name == "Retry"));
    assert_eq!(snapshot(&f).threads.len(), 7);
}

#[gpui_kit::test]
async fn mounted_sidebar_group_task_creation_uses_atomic_service_identity(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    click(&f, cx, "organization-groups");
    let group = create_group(&f, cx, "Task creation");
    let before = snapshot(&f);
    click(&f, cx, format!("group-menu-{group}"));
    click(&f, cx, "organization-menu-0");
    let after = snapshot(&f);
    assert_eq!(after.threads.len(), before.threads.len() + 1);
    let opened = cx.update(|cx| cx.global::<OpenedThread>().0.clone().unwrap());
    assert_eq!(opened.project_id, "p");
    assert!(!before.threads.iter().any(|t| t.id == opened.id));
    assert!(
        after
            .memberships
            .iter()
            .any(|m| m.thread_id == opened.id && m.group_id == group)
    );
    assert_eq!(
        f.draft.read_with(cx, |d, _| d.text().to_string()),
        "owned unsent draft"
    );
}

// The deterministic GPUI input helpers drain every event. This narrow handler-level
// race retains the real worker/file DB while staging two actions before its ack.
#[gpui_kit::test]
async fn organization_ack_preserves_newer_editor_and_does_not_steal_later_route(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    click(&f, cx, "organization-groups");
    click(&f, cx, "organization-new-group");
    cx.simulate_input(f.window.into(), "Submitted");
    let block = f.sidebar.read_with(cx, |s, _| s.sessions_block.clone());
    block.update(cx, |block, cx| {
        block.commit_group(cx);
        block
            .organization
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .input
            .update(cx, |input, cx| input.set_text("Newer unsaved text", cx));
    });
    cx.run_until_parked();
    assert_eq!(snapshot(&f).groups[0].name, "Submitted");
    assert_eq!(
        block.read_with(cx, |block, cx| block
            .organization
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .input
            .read(cx)
            .text()
            .to_string()),
        "Newer unsaved text"
    );
    let group = snapshot(&f).groups[0].id.clone();
    block.update(cx, |block, cx| {
        block.new_group_task(group.clone(), cx);
        block.new_group_task(group, cx);
        block.open_thread(&f.other.id, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        snapshot(&f).threads.len(),
        8,
        "duplicate preflight must remain single-flight"
    );
    cx.update(|cx| {
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("q"));
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.other.id
        );
    });
    let group = snapshot(&f).groups[0].id.clone();
    block.update(cx, |block, cx| block.new_group_task(group, cx));
    f.window
        .update(cx, |root, window, cx| {
            root.sidebar.update(cx, |sidebar, cx| {
                sidebar.open_settings(
                    &MouseUpEvent {
                        position: point(px(0.), px(0.)),
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                    },
                    window,
                    cx,
                )
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(snapshot(&f).threads.len(), 9);
    cx.update(|cx| {
        assert!(cx.global::<SettingsOpen>().0);
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.other.id
        );
    });
}

#[gpui_kit::test]
async fn r14_production_folder_registration_reveals_dedupes_and_scopes_new_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    click(&f, cx, "organization-groups");
    let group = create_group(&f, cx, "preserved manual group");
    let first = f.dir.path().join("first-folder");
    let second = f.dir.path().join("second-folder");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    cx.update(|cx| cx.set_global(SidebarCollapsed(true)));
    let register = |path: &Path, cx: &mut gpui_kit::TestAppContext| {
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
    assert_eq!(state.preferences.view, SidebarView::Projects);
    assert_eq!(
        state.preferences.project_view,
        SidebarProjectView::ByProject
    );
    assert!(state.groups.iter().any(|g| g.id == group));
    assert!(state.threads.iter().all(|t| t.project_id != first_id));
    click(&f, cx, format!("project-collapse-{first_id}"));
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
            .filter(|t| t.project_id == second_id)
            .count(),
        1
    );
    assert_eq!(
        state
            .threads
            .iter()
            .filter(|t| t.project_id == first_id)
            .count(),
        0
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
        f.draft.read_with(cx, |d, _| d.text().to_owned()),
        "owned unsent draft"
    );
    let reopened = Store::open(f.dir.path().join("organization.db")).unwrap();
    assert_eq!(service::snapshot(&reopened).unwrap(), state);
}
