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
