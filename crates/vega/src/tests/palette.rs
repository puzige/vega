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

fn assert_titlebar_control_grid(visual: &mut gpui_kit::VisualTestContext) {
    let header = visual.debug_bounds("main-header").unwrap();
    let title = visual.debug_bounds("main-header-title").unwrap();
    assert!((f32::from(title.center().y - header.center().y)).abs() <= 0.5);
    if let Some(sidebar) = visual.debug_bounds("sidebar") {
        let new_task = visual.debug_bounds("sidebar-new-task").unwrap();
        let scroll = visual.debug_bounds("sidebar-scroll").unwrap();
        let footer = visual.debug_bounds("sidebar-settings-surface").unwrap();
        assert_eq!(f32::from(new_task.top() - sidebar.top()), 64.0);
        assert_eq!(f32::from(new_task.left() - sidebar.left()), 12.0);
        assert_eq!(f32::from(scroll.top() - sidebar.top()), 108.0);
        assert_eq!(f32::from(sidebar.bottom() - footer.bottom()), 12.0);
        assert_eq!(f32::from(footer.top() - scroll.bottom()), 12.0);
    }
    let controls = [
        ("toggle-sidebar", "titlebar-sidebar-icon"),
        ("titlebar-search-button", "titlebar-search-icon"),
        ("navigation-back", "titlebar-back-icon"),
        ("navigation-forward", "titlebar-forward-icon"),
    ];
    let mut previous: Option<gpui_kit::Bounds<gpui_kit::Pixels>> = None;
    for (surface_selector, icon_selector) in controls {
        let surface = visual.debug_bounds(surface_selector).unwrap();
        let icon = visual.debug_bounds(icon_selector).unwrap();
        assert!(
            f32::from(surface.center().y - header.center().y).abs() <= 0.5,
            "{surface_selector} center {:?} must align with header {:?}",
            surface.center().y,
            header.center().y
        );
        assert_eq!(
            f32::from(surface.size.width),
            Layout::TITLEBAR_CONTROL_SIZE,
            "unexpected width for {surface_selector}"
        );
        assert_eq!(
            f32::from(surface.size.height),
            Layout::TITLEBAR_CONTROL_SIZE,
            "unexpected height for {surface_selector}"
        );
        assert_eq!(f32::from(icon.size.width), 16.0);
        assert_eq!(f32::from(icon.size.height), 16.0);
        assert_eq!(icon.center(), surface.center());
        if let Some(previous_surface) = previous {
            assert_eq!(
                f32::from(surface.left() - previous_surface.right()),
                Layout::TITLEBAR_CONTROL_GAP
            );
            assert_eq!(
                f32::from(surface.center().x - previous_surface.center().x),
                Layout::TITLEBAR_CONTROL_SIZE + Layout::TITLEBAR_CONTROL_GAP
            );
            assert_eq!(surface.center().y, previous_surface.center().y);
        }
        previous = Some(surface);
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
    cx.update(|cx| {
        let state = cx.global::<vega_ui::navigation::NavigationState>();
        assert!(!state.back);
        assert!(!state.forward);
    });
    // Exercise the production root across both palettes and responsive edges.
    for theme in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        cx.update(|cx| cx.set_global(theme));
        for (width, height) in [
            (1403., 860.),
            (1200., 760.),
            (960., 600.),
            (1229., 860.),
            (1230., 860.),
        ] {
            window
                .update(cx, |_, window, _| {
                    window.resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)))
                })
                .unwrap();
            cx.run_until_parked();
            let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
            assert_titlebar_control_grid(&mut visual);
        }
    }
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let search = visual.debug_bounds("titlebar-search-button").unwrap();
    assert_titlebar_control_grid(&mut visual);
    assert!(visual.debug_bounds("sidebar-search-button").is_none());
    visual.simulate_click(search.center(), Default::default());
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_some())
    });
    cx.simulate_keystrokes(window.into(), "escape");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.palette.view.is_none())
    });
    let sidebar = visual.debug_bounds("toggle-sidebar").unwrap();
    visual.simulate_click(sidebar.center(), Default::default());
    pump_test_app(cx, |cx| cx.update(|cx| cx.global::<SidebarCollapsed>().0));
    let mut hidden_visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let hidden_search = hidden_visual
        .debug_bounds("titlebar-search-button")
        .unwrap();
    assert_titlebar_control_grid(&mut hidden_visual);
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
    let hidden_sidebar = hidden_visual.debug_bounds("toggle-sidebar").unwrap();
    hidden_visual.simulate_click(hidden_sidebar.center(), Default::default());
    pump_test_app(cx, |cx| cx.update(|cx| !cx.global::<SidebarCollapsed>().0));
    assert_titlebar_control_grid(&mut gpui_kit::VisualTestContext::from_window(
        window.into(),
        cx,
    ));
    window
        .update(cx, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx)
        })
        .unwrap();
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
    cx.update(|cx| {
        let state = cx.global::<vega_ui::navigation::NavigationState>();
        assert!(state.back);
        assert!(!state.forward);
    });
    let mut enabled_visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert_titlebar_control_grid(&mut enabled_visual);
    let back = enabled_visual.debug_bounds("navigation-back").unwrap();
    enabled_visual.simulate_click(back.center(), Default::default());
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|opened| opened.id == thread.id)
        })
    });
    cx.update(|cx| {
        let state = cx.global::<vega_ui::navigation::NavigationState>();
        assert!(!state.back);
        assert!(state.forward);
    });
    let mut forward_visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert_titlebar_control_grid(&mut forward_visual);
    let forward = forward_visual.debug_bounds("navigation-forward").unwrap();
    forward_visual.simulate_click(forward.center(), Default::default());
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|opened| opened.id == destination.id)
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
        // R69 intentional change: the home route no longer leaves
        // `OpenedThread` empty. It now installs the window's lazy draft (R1),
        // bound to the project that was just selected (R2). The property this
        // assertion used to protect — "the picker did not create a durable
        // task" — is now proven by the store row count instead, which is
        // checked below; a draft is by definition not a row.
        let opened = cx.global::<OpenedThread>().0.clone();
        assert!(
            opened
                .as_ref()
                .is_some_and(|thread| thread.project_id == selected.id),
            "home route must hold a draft bound to the newly selected project"
        );
        assert_eq!(
            vega_store::threads::list_by_project(store.conn(), &selected.id, None)
                .unwrap()
                .len(),
            0,
            "selecting a project must not insert a task row"
        );
        assert!(!cx.global::<SettingsOpen>().0);
    });
    // The empty home draft retains the same titlebar and sidebar geometry.
    assert_titlebar_control_grid(&mut gpui_kit::VisualTestContext::from_window(
        window.into(),
        cx,
    ));
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
    // R69 intentional change: ⌘N navigates to the home draft route (R14). It
    // no longer INSERTs a row, which is exactly what used to pile up empty
    // `未命名任务` rows in the sidebar.
    assert_eq!(
        vega_store::threads::list_by_project(store.conn(), &project.id, None)
            .unwrap()
            .len(),
        before,
        "⌘N must not insert a task row"
    );
    let created = cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone());
    assert!(
        root.read_with(cx, |root, _| root.is_draft_route(&created)),
        "⌘N must land on the unpersisted draft route"
    );
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &created)
        })
    });
    // R69 R4: a second ⌘N reuses the same draft id, so the composer draft
    // text keyed by that id is never stranded.
    cx.simulate_keystrokes(window.into(), "cmd-n");
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == created)
        })
    });
    assert_eq!(
        vega_store::threads::list_by_project(store.conn(), &project.id, None)
            .unwrap()
            .len(),
        before,
        "re-entering the draft route must not insert a row either"
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "preserve my draft"
    );
}
