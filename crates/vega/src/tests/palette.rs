use super::*;
use vega_ui::sidebar::SelectedProject;

struct PaletteHarness {
    root: Entity<VegaWindow>,
}
impl Render for PaletteHarness {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}

#[gpui_kit::test]
async fn production_root_palette_escape_preserves_composer_and_settings_action(
    cx: &mut gpui_kit::TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("vega.db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        data.path().to_str().unwrap(),
        "owned palette",
        None,
    )
    .unwrap();
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text("preserve my draft", cx));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.model_selection_config_override = Some(data.path().join("config.toml"));
    });
    let root_view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| PaletteHarness { root: root_view })
        })
        .unwrap()
    });
    cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
    window
        .update(cx, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx)
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let sidebar = visual.debug_bounds("toggle-sidebar").unwrap();
    let search = visual.debug_bounds("titlebar-search-button").unwrap();
    let back = visual.debug_bounds("navigation-back").unwrap();
    let forward = visual.debug_bounds("navigation-forward").unwrap();
    assert_eq!(f32::from(search.left() - sidebar.right()), 4.0);
    assert_eq!(f32::from(back.left() - search.right()), 4.0);
    assert_eq!(f32::from(forward.left() - back.right()), 4.0);
    assert!(visual.debug_bounds("sidebar-search-button").is_none());
    visual.simulate_click(search.center(), Default::default());
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_some())
    });
    cx.simulate_keystrokes(window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_none())
    });
    cx.update(|cx| {
        cx.set_global(SidebarCollapsed(true));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    let mut hidden_visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let hidden_sidebar = hidden_visual.debug_bounds("toggle-sidebar").unwrap();
    let hidden_search = hidden_visual
        .debug_bounds("titlebar-search-button")
        .unwrap();
    let hidden_back = hidden_visual.debug_bounds("navigation-back").unwrap();
    let hidden_forward = hidden_visual.debug_bounds("navigation-forward").unwrap();
    assert_eq!(
        f32::from(hidden_search.left() - hidden_sidebar.right()),
        4.0
    );
    assert_eq!(f32::from(hidden_back.left() - hidden_search.right()), 4.0);
    assert_eq!(f32::from(hidden_forward.left() - hidden_back.right()), 4.0);
    assert!(
        hidden_visual
            .debug_bounds("sidebar-search-button")
            .is_none()
    );
    hidden_visual.simulate_click(hidden_search.center(), Default::default());
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_some())
    });
    cx.simulate_keystrokes(window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_none())
    });
    cx.simulate_keystrokes(window.into(), "cmd-k");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_some())
    });
    cx.simulate_keystrokes(window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_none())
    });
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "preserve my draft"
    );
    assert!(
        window
            .update(cx, |_, window, cx| input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window))
            .unwrap()
    );
    assert_eq!(
        root.read_with(cx, |root, _| root
            .stream_view
            .as_ref()
            .map(|(_, view)| view.clone())),
        Some(stream.clone())
    );
    std::fs::write(
        data.path().join("AGENTS.md"),
        "# Owned preview\n\nFile stays read-only.",
    )
    .unwrap();
    cx.simulate_keystrokes(window.into(), "cmd-k a g e n t s . m d");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            !root.palette.search_busy
                && root
                    .palette
                    .view
                    .as_ref()
                    .is_some_and(|view| view.read(cx).query() == "agents.md")
        })
    });
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.palette.view.is_none() && root.workspace_has_preview()
        })
    });
    assert_eq!(
        root.read_with(cx, |root, _| root
            .stream_view
            .as_ref()
            .map(|(_, view)| view.clone())),
        Some(stream.clone())
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "preserve my draft"
    );
    assert_eq!(
        std::fs::read_to_string(data.path().join("AGENTS.md")).unwrap(),
        "# Owned preview\n\nFile stays read-only."
    );
    let store = Store::open(&path).unwrap();
    let destination =
        vega_conversation::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
    vega_store::threads::rename(store.conn(), &destination.id, "Palette destination", 200).unwrap();
    cx.simulate_keystrokes(window.into(), "cmd-k d e s t i n a t i o n");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            !root.palette.search_busy
                && root
                    .palette
                    .view
                    .as_ref()
                    .is_some_and(|view| view.read(cx).query() == "destination")
        })
    });
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == destination.id)
        })
    });
    cx.simulate_keystrokes(window.into(), "cmd-k s e t t i n g s");
    cx.run_until_parked();
    assert_eq!(
        root.read_with(cx, |root, cx| root
            .palette
            .view
            .as_ref()
            .map(|view| view.read(cx).query().to_string())),
        Some("settings".into())
    );
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert!(root.read_with(cx, |root, _| root.palette.view.is_none()));
    cx.simulate_keystrokes(window.into(), "cmd-k a g e n t s . m d");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            !root.palette.search_busy
                && root
                    .palette
                    .view
                    .as_ref()
                    .is_some_and(|view| view.read(cx).query() == "agents.md")
        })
    });
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.palette.view.is_none()
                && !cx.global::<SettingsOpen>().0
                && root.workspace_has_preview()
        })
    });
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "preserve my draft"
    );
    // Native Open dialog cancellation preserves the underlying route.
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.simulate_keystrokes(window.into(), "cmd-o");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(cx.global::<SettingsOpen>().0);
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(project.id.as_str())
        );
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().map(|t| &t.id),
            Some(&destination.id)
        );
    });
    let picked = data.path().join("picked-project");
    std::fs::create_dir(&picked).unwrap();
    cx.simulate_keystrokes(window.into(), "cmd-o");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![picked.clone()]));
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<SelectedProject>()
                .0
                .as_ref()
                .is_some_and(|id| id != &project.id)
        })
    });
    let selected = vega_store::projects::find_by_path(
        store.conn(),
        picked.canonicalize().unwrap().to_str().unwrap(),
    )
    .unwrap()
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(selected.id.as_str())
        );
        assert!(cx.global::<OpenedThread>().0.is_none());
        assert!(!cx.global::<SettingsOpen>().0);
    });
    // Selecting a previously registered folder reuses its row and activates it.
    cx.simulate_keystrokes(window.into(), "cmd-o");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![data.path().to_path_buf()]));
    pump_test_app(cx, |cx| {
        cx.update(|cx| cx.global::<SelectedProject>().0.as_deref() == Some(project.id.as_str()))
    });
    assert_eq!(
        vega_store::projects::list(store.conn(), vega_store::projects::ProjectSort::Name)
            .unwrap()
            .len(),
        2
    );
    let before = vega_store::threads::list_by_project(store.conn(), &project.id, None)
        .unwrap()
        .len();
    cx.simulate_keystrokes(window.into(), "cmd-n");
    pump_test_app(cx, |cx| {
        cx.update(|cx| cx.global::<OpenedThread>().0.is_some())
    });
    assert_eq!(
        vega_store::threads::list_by_project(store.conn(), &project.id, None)
            .unwrap()
            .len(),
        before + 1
    );
    let created = cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone());
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &created)
        })
    });
    cx.simulate_keystrokes(window.into(), "cmd-n");
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id != created)
        })
    });
    assert_eq!(
        vega_store::threads::list_by_project(store.conn(), &project.id, None)
            .unwrap()
            .len(),
        before + 2
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "preserve my draft"
    );
}
