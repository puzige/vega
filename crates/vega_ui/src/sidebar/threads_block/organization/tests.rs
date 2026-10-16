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
            preferences.sort,
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
