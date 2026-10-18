use super::*;

#[path = "tests/archive_controls.rs"]
mod archive_controls;
#[path = "tests/menu_keyboard.rs"]
mod menu_keyboard;
#[path = "tests/reorder_neighbors.rs"]
mod reorder_neighbors;

struct MountedSidebar {
    sidebar: Entity<Sidebar>,
    draft: Entity<TextInput>,
}

impl Render for MountedSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| {
                cx.set_global(SettingsOpen(false));
                cx.set_global(SelectedProject(None));
                cx.set_global(OpenedThread(None));
                cx.notify();
            }))
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
    window: gpui_kit::WindowHandle<gpui_kit::component::Root>,
    first: Thread,
    other: Thread,
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
    let first = create_fixture_thread(&store, "p-a", Some("p"), false, false, 9000);
    for (id, at, unread) in [
        ("p-b", 8999, false),
        ("p-c", 8998, true),
        ("p-d", 8997, false),
        ("p-e", 8996, false),
    ] {
        create_fixture_thread(&store, id, Some("p"), false, unread, at);
    }
    let other = create_fixture_thread(&store, "q-a", Some("q"), false, true, 8000);
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        crate::init(cx);
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
            move |window, cx| {
                let view = cx.new(|_| MountedSidebar {
                    sidebar: s,
                    draft: d,
                });
                cx.new(|cx| gpui_kit::component::Root::new(view, window, cx).bordered(false))
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

fn create_fixture_thread(
    store: &Store,
    id: &str,
    project: Option<&str>,
    pinned: bool,
    unread: bool,
    at: i64,
) -> Thread {
    let row = vega_store::threads::NewThread {
        id,
        project_id: project.unwrap_or_default(),
        title: id,
        mode: "ask",
        permission_mode: "confirm",
        model: "model",
        status: "active",
        pinned,
        unread,
        created_at: at,
        updated_at: at,
    };
    if project.is_some() {
        vega_store::threads::create(store.conn(), row).unwrap();
    } else {
        vega_store::threads::create_standalone(store.conn(), row).unwrap();
    }
    let threads = match project {
        Some(project) => conversation::list_threads(store, project, None).unwrap(),
        None => conversation::list_standalone_threads(store, None).unwrap(),
    };
    threads.into_iter().find(|thread| thread.id == id).unwrap()
}

fn create_reorder_threads(
    store: &Store,
    project: Option<&str>,
    count: usize,
    pinned: bool,
) -> Vec<String> {
    (0..count)
        .map(|index| {
            let prefix = if pinned { "pinned" } else { "recent" };
            let id = format!("{prefix}-{index}");
            create_fixture_thread(store, &id, project, pinned, false, 7000 - index as i64).id
        })
        .collect()
}

fn keystroke(f: &Fixture, cx: &mut gpui_kit::TestAppContext, key: &str) {
    cx.simulate_keystrokes(f.window.into(), key);
    cx.run_until_parked();
}

fn assert_rendered_order(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    prefix: &str,
    expected: &[&str],
) {
    let rows: Vec<_> = expected
        .iter()
        .map(|id| bounds(f, cx, format!("{prefix}thread-row-{id}")))
        .collect();
    for pair in rows.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top(), "{expected:?}");
    }
}

fn assert_saved_order(f: &Fixture, project: Option<&str>, pinned: bool, expected: &[&str]) {
    let state = snapshot(f);
    let actual: Vec<_> = service::sorted_threads(&state.threads, &state.preferences)
        .into_iter()
        .filter(|thread| thread.pinned == pinned && (pinned || thread.project_binding() == project))
        .map(|thread| thread.id)
        .collect();
    assert_eq!(actual, expected);
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
