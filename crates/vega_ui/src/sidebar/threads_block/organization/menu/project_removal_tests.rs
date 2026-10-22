use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    App, AppContext, Bounds, ElementId, Entity, InputEvent, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, TestAppContext,
    Window, WindowHandle, point, px, size,
};
use std::{cell::Cell, rc::Rc};

use tempfile::TempDir;
use vega_conversation::types::{
    CodexAdapterArgument, CodexAdapterKind, CodexApprovalPolicy, CodexExecutionSnapshot,
    CodexProfileReference, CodexReasoningEffort, CodexRunSettings, CodexSandboxMode,
    CodexSessionCreationState, CodexWorkspaceSnapshot, TaskBackend, Thread,
};
use vega_conversation::{codex_tasks, threads};
use vega_store::{Store, projects, threads as stored_threads};

use crate::settings::SettingsOpen;
use crate::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, Sidebar, SidebarCollapsed, SidebarWidth, VegaStore,
    project_worker_is_active, register_project_worker,
};

struct Fixture {
    directory: TempDir,
    project: projects::Project,
    retained: projects::Project,
    tasks: Vec<Thread>,
    retained_task: Thread,
}

struct Mounted {
    sidebar: Entity<Sidebar>,
    window: WindowHandle<Root>,
    project_notifications: Rc<Cell<usize>>,
}

fn with_store<R>(cx: &App, observe: impl FnOnce(&Store) -> R) -> R {
    observe(
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned fixture store"),
    )
}

fn fixture(cx: &mut TestAppContext, codex: bool, filler_count: usize) -> Fixture {
    let directory = tempfile::tempdir().expect("owned Sidebar fixture");
    let path = directory.path().canonicalize().expect("canonical fixture");
    let project_path = path.join("removed");
    let retained_path = path.join("retained");
    std::fs::create_dir(&project_path).expect("owned project directory");
    std::fs::create_dir(&retained_path).expect("owned retained directory");
    std::fs::write(
        project_path.join("keep.txt"),
        b"removed project local sentinel",
    )
    .expect("owned sentinel");
    std::fs::write(
        retained_path.join("keep.txt"),
        b"retained project local sentinel",
    )
    .expect("owned sentinel");
    let store = Store::open(path.join("sidebar.sqlite")).expect("owned file-backed Store");
    store.migrate().expect("fresh production migrations");
    let project = projects::create(
        store.conn(),
        project_path.to_str().expect("UTF-8 project"),
        "Removable project",
        None,
    )
    .expect("register target");
    let retained = projects::create(
        store.conn(),
        retained_path.to_str().expect("UTF-8 retained"),
        "Retained project",
        None,
    )
    .expect("register sibling");
    let native = threads::create_thread(&store, &project.id, "fixture-model", "readonly")
        .expect("Native task");
    threads::rename_thread(&store, &native.id, "Preserved Native history").expect("Native title");
    let second = if codex {
        let snapshot = CodexExecutionSnapshot {
            profile: CodexProfileReference {
                id: "fixture-profile".into(),
                display_name: "Fixture profile".into(),
            },
            adapter: CodexAdapterKind::CodexAcp,
            executable: project_path
                .join("adapter-never-executed")
                .to_string_lossy()
                .into_owned(),
            arguments: CodexAdapterArgument::from_argv(&["--fixture".into()])
                .expect("synthetic adapter arguments"),
            adapter_version: "1.2.3".into(),
            codex_version: "4.5.6".into(),
            settings: CodexRunSettings {
                model: Some("fixture-model".into()),
                model_provider: Some("fixture-provider".into()),
                reasoning_effort: Some(CodexReasoningEffort::High),
                sandbox_mode: CodexSandboxMode::ReadOnly,
                approval_policy: CodexApprovalPolicy::OnRequest,
            },
            workspace: CodexWorkspaceSnapshot {
                project_id: Some(project.id.clone()),
                worktree_id: None,
                canonical_working_directory: project_path.to_string_lossy().into_owned(),
                additional_directories: Vec::new(),
            },
        };
        let mut draft = threads::draft_thread(Some(&project.id), "fixture-model", "readonly");
        draft.backend = TaskBackend::Codex;
        draft.title = "Preserved Codex history".into();
        let task = codex_tasks::materialize_codex_draft(&store, &draft, &snapshot)
            .expect("valid Codex task");
        let CodexSessionCreationState::Intent { intent_id } =
            codex_tasks::begin_codex_session_creation(&store, &task.id)
                .expect("live session intent")
        else {
            panic!("intent state");
        };
        codex_tasks::confirm_codex_session_creation(
            &store,
            &task.id,
            &intent_id,
            "preserved-session",
        )
        .expect("confirmed history");
        task
    } else {
        let task = threads::create_thread(&store, &project.id, "fixture-model", "readonly")
            .expect("second Native task");
        threads::rename_thread(&store, &task.id, "Preserved second history").expect("second title");
        task
    };
    let retained_task = threads::create_thread(&store, &retained.id, "fixture-model", "readonly")
        .expect("sibling task");
    for (index, id) in [&native.id, &second.id, &retained_task.id]
        .into_iter()
        .enumerate()
    {
        store
            .conn()
            .execute(
                "UPDATE threads SET created_at = 100, updated_at = ?1 WHERE id = ?2",
                (100_000 + index as i64, id),
            )
            .expect("deterministic history ordering");
        store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES (?1, ?2, 1, 'assistant', 'text', 'Preserved history', 'done', 100)", (format!("message-{id}"), id)).expect("preserved message");
    }
    for index in 0..filler_count {
        let id = format!("filler-{index:03}");
        let title = format!("Older standalone task {index:03}");
        stored_threads::create_standalone(
            store.conn(),
            stored_threads::NewThread {
                id: &id,
                project_id: "",
                title: &title,
                mode: "execute",
                permission_mode: "readonly",
                model: "fixture-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: index as i64,
                updated_at: index as i64,
            },
        )
        .expect("long task list");
    }
    let tasks = [&native.id, &second.id]
        .into_iter()
        .map(|id| threads::get_thread(&store, id).expect("fresh fixture task"))
        .collect::<Vec<_>>();
    let retained_task = threads::get_thread(&store, &retained_task.id).expect("fresh sibling task");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::navigation::init(cx);
        cx.set_global(vega_theme::Theme::dark());
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(SidebarWidth(if filler_count == 0 {
            vega_theme::Layout::SIDEBAR_WIDTH
        } else {
            vega_theme::Layout::SIDEBAR_MIN_WIDTH
        }));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(SettingsOpen(false));
        cx.set_global(SelectedProject(Some(project.id.clone())));
        cx.set_global(OpenedThread(Some(tasks[1].clone())));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(NewTaskDefaults {
            model: "fixture-model".into(),
            permission_mode: "readonly".into(),
        });
        cx.set_global(VegaStore(Ok(store)));
    });
    Fixture {
        directory,
        project,
        retained,
        tasks,
        retained_task,
    }
}

fn mount(cx: &mut TestAppContext) -> Mounted {
    let sidebar = cx.update(|cx| cx.new(Sidebar::new));
    let view = sidebar.clone();
    let window = cx.open_window(size(px(960.), px(600.)), move |window, cx| {
        Root::new(view, window, cx).bordered(false)
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .expect("mounted Sidebar frame");
    cx.read(|cx| {
        let block = sidebar.read(cx).sessions_block.read(cx);
        assert!(
            block
                .organization
                .as_ref()
                .expect("real organization")
                .snapshot
                .is_some(),
            "file-backed asynchronous loader completed"
        );
        assert!(
            block.error.is_none(),
            "valid fixture organization load: {:?}",
            block.error
        );
        assert!(sidebar.read(cx).projects_block.read(cx).error.is_none());
    });
    let project_notifications = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let notifications = project_notifications.clone();
        let projects = sidebar.read(cx).projects_block.clone();
        cx.observe(&projects, move |_, _| {
            notifications.set(notifications.get() + 1);
        })
        .detach();
    });
    Mounted {
        sidebar,
        window,
        project_notifications,
    }
}

fn draw(window: &mut Window, cx: &mut App) {
    window.draw(cx).clear(cx);
}

fn frame(mounted: &Mounted, cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.update_window(mounted.window.into(), |_, window, cx| draw(window, cx))
        .expect("complete naturally invalidated frame");
}

fn move_pointer(window: &mut Window, position: Point<Pixels>, cx: &mut App) {
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
        cx,
    );
    draw(window, cx);
}

fn click(window: &mut Window, id: ElementId, cx: &mut App) {
    let target = window.find(id);
    assert!(target.visible(), "real menu target is visible: {target:?}");
    let position = target.bounds().center();
    move_pointer(window, position, cx);
    window.dispatch_event(
        MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
}

fn press(window: &mut Window, key: &str, cx: &mut App) {
    draw(window, cx);
    window.dispatch_keystroke(Keystroke::parse(key).expect("valid test key"), cx);
}

fn open_menu(mounted: &Mounted, fixture: &Fixture, keyboard: bool, cx: &mut TestAppContext) {
    frame(mounted, cx);
    cx.update_window(mounted.window.into(), |_, window, cx| {
        let trigger = ElementId::Name(format!("project-more-{}", fixture.project.id).into());
        assert_eq!(
            window.find(trigger.clone()).label(),
            Some("Removable project 项目操作")
        );
        if keyboard {
            let scope = mounted
                .sidebar
                .read(cx)
                .sessions_block
                .read(cx)
                .project_focuses
                .get(&fixture.project.id)
                .expect("real project actions focus")
                .clone();
            window.focus(&scope, cx);
            draw(window, cx);
            window.focus_next(cx);
            window.focus_next(cx);
            press(window, "enter", cx);
        } else {
            let header = window
                .find(ElementId::Name(
                    format!("project-header-{}", fixture.project.id).into(),
                ))
                .bounds();
            move_pointer(window, header.center(), cx);
            click(window, trigger, cx);
        }
    })
    .expect("dispatch existing mouse or keyboard menu entry");
    frame(mounted, cx);
    cx.update_window(mounted.window.into(), |_, window, cx| {
        let menu = window.find("organization-menu");
        assert!(menu.visible(), "real project menu is mounted");
        let block = mounted.sidebar.read(cx).sessions_block.read(cx);
        let org = block.organization.as_ref().expect("organization");
        assert!(org.menu_focus.contains_focused(window, cx), "menu receives keyboard focus");
        assert!(matches!(&org.menu, Some(super::OrganizationMenu::Project(id)) if id == &fixture.project.id));
        assert_eq!(block.organization_menu_items()[0].0, "移除项目");
        assert!(matches!(&block.organization_menu_items()[0].1, super::MenuCommand::RemoveProject(id) if id == &fixture.project.id));
        assert!(window.find("organization-menu-0").visible());
    }).expect("existing mouse or keyboard menu entry");
}

fn activate_remove(mounted: &Mounted, keyboard: bool, cx: &mut TestAppContext) {
    let notifications = mounted.project_notifications.get();
    cx.update_window(mounted.window.into(), |_, window, cx| {
        if keyboard {
            press(window, "down", cx);
            press(window, "up", cx);
            press(window, "enter", cx);
        } else {
            click(window, ElementId::from("organization-menu-0"), cx);
        }
    })
    .expect("real remove menu action");
    cx.run_until_parked();
    assert!(
        mounted.project_notifications.get() > notifications,
        "the real remove result notifies before the first post-action paint"
    );
    frame(mounted, cx);
    cx.read(|cx| {
        assert!(
            mounted
                .sidebar
                .read(cx)
                .sessions_block
                .read(cx)
                .organization
                .as_ref()
                .expect("organization")
                .menu
                .is_none()
        )
    });
}

fn project_error(mounted: &Mounted, cx: &TestAppContext) -> String {
    cx.read(|cx| {
        mounted
            .sidebar
            .read(cx)
            .projects_block
            .read(cx)
            .error
            .clone()
            .expect("real remove failure")
    })
}

fn error_node(window: &Window, message: &str) -> gpui_kit::test::ElementSnapshot {
    let id = ElementId::Name(format!("sidebar-error-{message}").into());
    let matches = gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|node| node.path().last() == Some(&id))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "failure is rendered exactly once");
    window.find(id)
}

fn assert_fixed_error(mounted: &Mounted, message: &str, cx: &mut TestAppContext) -> Bounds<Pixels> {
    cx.update_window(mounted.window.into(), |_, window, _| {
        let error = error_node(window, message);
        let sidebar = window.find("sidebar").bounds();
        assert_eq!(
            sidebar.size.width,
            px(vega_theme::Layout::SIDEBAR_MIN_WIDTH)
        );
        assert_eq!(window.viewport_size(), size(px(960.), px(600.)));
        let scroll = window.find("sidebar-scroll").bounds();
        let bounds = error.bounds();
        assert!(
            error.visible(),
            "error must be visible without searching the long list: {error:?}"
        );
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        assert!(bounds.left() >= sidebar.left() && bounds.right() <= sidebar.right());
        assert!(bounds.top() >= sidebar.top() && bounds.bottom() <= sidebar.bottom());
        assert!(
            !error.path().contains(&ElementId::from("sidebar-scroll")),
            "failure belongs outside the scrolling task subtree"
        );
        assert!(
            bounds.bottom() <= scroll.top() || bounds.top() >= scroll.bottom(),
            "fixed error must not overlap the task viewport"
        );
        bounds
    })
    .expect("real error geometry")
}

fn scroll_list(mounted: &Mounted, delta: f32, cx: &mut TestAppContext) {
    cx.update_window(mounted.window.into(), |_, window, cx| {
        let bounds = window.find("sidebar-scroll").bounds();
        let position = point(bounds.left() + px(16.), bounds.bottom() - px(16.));
        window.dispatch_event(
            gpui_kit::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            gpui_kit::ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                modifiers: Default::default(),
                touch_phase: gpui_kit::TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
        draw(window, cx);
    })
    .expect("real Sidebar wheel input");
    frame(mounted, cx);
}

fn assert_unchanged(fixture: &Fixture, cx: &TestAppContext) {
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0,
            Some(fixture.project.id.clone())
        );
        assert_eq!(
            cx.global::<OpenedThread>().0,
            Some(fixture.tasks[1].clone())
        );
        with_store(cx, |store| {
            assert_eq!(
                projects::find(store.conn(), &fixture.project.id).expect("target registration"),
                Some(fixture.project.clone())
            );
            for task in &fixture.tasks {
                assert_eq!(
                    threads::get_thread(store, &task.id).expect("preserved bound task"),
                    *task
                );
            }
        });
    });
}

fn assert_files(fixture: &Fixture) {
    assert!(fixture.directory.path().is_dir());
    assert_eq!(
        std::fs::read(std::path::Path::new(&fixture.project.path).join("keep.txt"))
            .expect("target files preserved"),
        b"removed project local sentinel"
    );
    assert_eq!(
        std::fs::read(std::path::Path::new(&fixture.retained.path).join("keep.txt"))
            .expect("sibling files preserved"),
        b"retained project local sentinel"
    );
}

fn assert_removed(mounted: &Mounted, fixture: &Fixture, cx: &mut TestAppContext) {
    frame(mounted, cx);
    cx.read(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none(), "real removed event clears project route");
        assert!(cx.global::<OpenedThread>().0.is_none(), "real removed event clears stale opened task");
        assert!(mounted.sidebar.read(cx).projects_block.read(cx).error.is_none(), "success clears old error");
        with_store(cx, |store| {
            assert!(projects::find(store.conn(), &fixture.project.id).expect("target removed").is_none());
            assert_eq!(projects::find(store.conn(), &fixture.retained.id).expect("sibling registration"), Some(fixture.retained.clone()));
            assert_eq!(threads::get_thread(store, &fixture.retained_task.id).expect("sibling task unchanged"), fixture.retained_task);
            assert_eq!(store.conn().query_row("SELECT COUNT(*) FROM sidebar_project_order WHERE project_id = ?1", [&fixture.project.id], |row| row.get::<_, i64>(0)).expect("removed order"), 0);
            assert_eq!(store.conn().query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| row.get::<_, i64>(0)).expect("foreign keys"), 0);
            for task in &fixture.tasks {
                let mut expected = task.clone();
                expected.project_id.clear();
                assert_eq!(threads::get_thread(store, &task.id).expect("standalone history preserved"), expected);
                assert_eq!(store.conn().query_row("SELECT COUNT(*) FROM messages WHERE thread_id = ?1 AND content = 'Preserved history'", [&task.id], |row| row.get::<_, i64>(0)).expect("preserved message"), 1);
            }
        });
    });
    cx.update_window(mounted.window.into(), |_, window, _| {
        assert!(window.try_find(ElementId::Name(format!("project-header-{}", fixture.project.id).into())).is_none());
        assert!(window.find(ElementId::Name(format!("project-header-{}", fixture.retained.id).into())).visible());
        for task in &fixture.tasks {
            let row = window.within("organization-recents").find(ElementId::Name(format!("standalone-thread-row-{}", task.id).into()));
            assert!(row.visible(), "preserved task is discoverable in recents");
            assert_eq!(row.label(), Some(task.title.as_str()));
            let rows = gpui_kit::base::test_support::snapshots(window).into_iter().filter(|node| node.label() == Some(task.title.as_str())).collect::<Vec<_>>();
            assert_eq!(rows.len(), 1, "retained task has one mounted projection");
        }
        assert!(gpui_kit::base::test_support::snapshots(window).iter().all(|node| !matches!(node.path().last(), Some(ElementId::Name(name)) if name.starts_with("sidebar-error-"))), "success leaves no error or empty error surface");
    }).expect("mounted retained-task projections");
    assert_files(fixture);
}

#[gpui_kit::test]
fn mouse_project_menu_removes_codex_and_native_registration_but_keeps_history(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx, true, 0);
    let mounted = mount(cx);
    let before = cx.read(|cx| {
        with_store(cx, |store| {
            codex_tasks::codex_task_identity(store, &fixture.tasks[1].id)
                .expect("original Codex identity")
        })
    });
    open_menu(&mounted, &fixture, false, cx);
    activate_remove(&mounted, false, cx);
    assert_removed(&mounted, &fixture, cx);
    cx.read(|cx| {
        with_store(cx, |store| {
            assert_eq!(
                codex_tasks::codex_task_identity(store, &fixture.tasks[1].id)
                    .expect("read retained Codex history"),
                before
            )
        })
    });
}

#[gpui_kit::test]
fn keyboard_project_menu_obeys_cross_window_worker_ownership_until_last_token_drops(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx, false, 0);
    let owner = mount(cx);
    let mounted = mount(cx);
    let token = cx
        .update_window(owner.window.into(), |_, _, cx| {
            register_project_worker(&fixture.project.id, cx)
        })
        .expect("worker acquired by another window");
    let worker_owner = token.clone();
    open_menu(&mounted, &fixture, true, cx);
    activate_remove(&mounted, true, cx);
    assert_unchanged(&fixture, cx);
    assert!(project_error(&mounted, cx).contains("仍在运行"));
    drop(token);
    cx.read(|cx| {
        assert!(
            project_worker_is_active(&fixture.project.id, cx),
            "remaining worker owner is still live"
        )
    });
    open_menu(&mounted, &fixture, true, cx);
    activate_remove(&mounted, true, cx);
    assert_unchanged(&fixture, cx);
    drop(worker_owner);
    cx.read(|cx| {
        assert!(
            !project_worker_is_active(&fixture.project.id, cx),
            "last real token has exited"
        )
    });
    open_menu(&mounted, &fixture, true, cx);
    activate_remove(&mounted, true, cx);
    assert_removed(&mounted, &fixture, cx);
}

#[gpui_kit::test]
fn worker_refusal_stays_fixed_and_visible_while_long_sidebar_tasks_scroll(cx: &mut TestAppContext) {
    let fixture = fixture(cx, false, 40);
    let mounted = mount(cx);
    let token = cx.update(|cx| register_project_worker(&fixture.project.id, cx));
    open_menu(&mounted, &fixture, false, cx);
    activate_remove(&mounted, false, cx);
    assert_unchanged(&fixture, cx);
    let message = project_error(&mounted, cx);
    assert!(message.contains("仍在运行"));
    let before = assert_fixed_error(&mounted, &message, cx);
    scroll_list(&mounted, -140., cx);
    cx.read(|cx| {
        assert!(
            mounted.sidebar.read(cx).sidebar_scroll.offset().y < px(0.),
            "actual long-list scroll moved"
        )
    });
    let after = assert_fixed_error(&mounted, &message, cx);
    assert!((after.top() - before.top()).abs() <= px(0.5));
    assert!((after.bottom() - before.bottom()).abs() <= px(0.5));
    assert_files(&fixture);
    drop(token);
}

#[gpui_kit::test]
fn rejected_delete_in_scrolled_sidebar_shows_one_fixed_error_and_retry_reclaims_space(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx, false, 40);
    let mounted = mount(cx);
    let initial_viewport = cx
        .update_window(mounted.window.into(), |_, window, _| {
            window.find("sidebar-scroll").bounds()
        })
        .expect("no-error task viewport");
    cx.read(|cx| with_store(cx, |store| store.conn().execute_batch("CREATE TRIGGER reject_sidebar_project_delete BEFORE DELETE ON projects BEGIN SELECT RAISE(ABORT, 'injected sidebar deletion failure'); END;").expect("owned deletion failure injection")));
    open_menu(&mounted, &fixture, false, cx);
    scroll_list(&mounted, -60., cx);
    cx.read(|cx| {
        assert!(
            mounted.sidebar.read(cx).sidebar_scroll.offset().y < px(0.),
            "menu removal is attempted from a scrolled list"
        )
    });
    activate_remove(&mounted, true, cx);
    assert_unchanged(&fixture, cx);
    let message = project_error(&mounted, cx);
    assert!(
        message.starts_with("项目移除失败：")
            && message.contains("injected sidebar deletion failure")
    );
    let before = assert_fixed_error(&mounted, &message, cx);
    scroll_list(&mounted, -120., cx);
    let after = assert_fixed_error(&mounted, &message, cx);
    assert!((after.top() - before.top()).abs() <= px(0.5));
    cx.read(|cx| {
        with_store(cx, |store| {
            store
                .conn()
                .execute_batch("DROP TRIGGER reject_sidebar_project_delete")
                .expect("release injected fault")
        })
    });
    scroll_list(&mounted, 10_000., cx);
    open_menu(&mounted, &fixture, true, cx);
    activate_remove(&mounted, true, cx);
    scroll_list(&mounted, 10_000., cx);
    assert_removed(&mounted, &fixture, cx);
    let restored = cx
        .update_window(mounted.window.into(), |_, window, _| {
            window.find("sidebar-scroll").bounds()
        })
        .expect("error-free viewport restored");
    assert!(
        (restored.size.height - initial_viewport.size.height).abs() <= px(0.5),
        "absent errors reserve no fixed-area space"
    );
}
