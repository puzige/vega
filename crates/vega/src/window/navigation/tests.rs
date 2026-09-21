use super::{DRAFT_BYTES, HISTORY_LIMIT};
use crate::window::VegaWindow;
use gpui_kit::prelude::*;
use gpui_kit::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, size};
use gpui_kit::{Entity, EntityInputHandler, Focusable, Modifiers, TestAppContext, px};
use std::time::Duration;
use vega_conversation::types::{Thread, ThreadStatus};
use vega_store::Store;
use vega_ui::navigation::NavigationState;
use vega_ui::settings::{CloseSettings, SettingsOpen};
use vega_ui::sidebar::{OpenedThread, SelectedProject, SidebarCollapsed};

struct Fixture {
    data: tempfile::TempDir,
    store: Store,
    first: Thread,
    second: Thread,
    root: Entity<VegaWindow>,
    window: WindowHandle<VegaWindow>,
}
fn pump(cx: &mut TestAppContext, mut ready: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..400 {
        cx.executor().advance_clock(Duration::from_millis(25));
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("navigation did not settle");
}
fn fixture(cx: &mut TestAppContext) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let database = data.path().join("vega.db");
    let config = data.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        data.path().to_str().unwrap(),
        "owned navigation",
        None,
    )
    .unwrap();
    let first =
        vega_conversation::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
    let second =
        vega_conversation::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
    vega_conversation::threads::rename_thread(&store, &second.id, "Destination").unwrap();
    cx.update(|cx| {
        crate::tests::install_diff_window_globals(
            Store::open(&database).unwrap(),
            first.clone(),
            cx,
        )
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config)
    });
    let view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| view,
        )
        .unwrap()
    });
    cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
    pump(cx, |cx| {
        root.read_with(cx, |root, _| root.stream_view.is_some())
    });
    Fixture {
        data,
        store,
        first,
        second,
        root,
        window,
    }
}
fn editor(f: &Fixture, cx: &mut TestAppContext) -> Entity<vega_ui::text_input::TextInput> {
    f.root.read_with(cx, |root, cx| {
        root.stream_view
            .as_ref()
            .unwrap()
            .1
            .read(cx)
            .composer_input()
    })
}
fn focus_editor(f: &Fixture, cx: &mut TestAppContext) {
    let input = editor(f, cx);
    f.window
        .update(cx, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx)
        })
        .unwrap();
    cx.run_until_parked();
}
fn current(f: &Fixture, cx: &mut TestAppContext) -> Option<String> {
    f.root.read_with(cx, |_, cx| {
        cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone())
    })
}
fn settled(f: &Fixture, id: &str, cx: &mut TestAppContext) {
    pump(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            !root.navigation.pending
                && super::task_mutation_state(cx).pending == 0
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|t| t.id == id)
                && root
                    .stream_view
                    .as_ref()
                    .is_some_and(|(task, _)| task == id)
        })
    });
}
fn palette_second(f: &Fixture, cx: &mut TestAppContext) {
    focus_editor(f, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-k d e s t i n a t i o n");
    pump(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            !root.palette.search_busy
                && root
                    .palette
                    .view
                    .as_ref()
                    .is_some_and(|view| view.read(cx).query() == "destination")
        })
    });
    cx.simulate_keystrokes(f.window.into(), "enter");
    settled(f, &f.second.id, cx);
}
fn click(f: &Fixture, selector: &'static str, cx: &mut TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .expect("visible navigation control");
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn navigation_real_root_palette_mouse_shortcuts_and_settings_preserve_drafts(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    editor(&f, cx).update(cx, |input, cx| input.set_text("first unsent 中文", cx));
    palette_second(&f, cx);
    editor(&f, cx).update(cx, |input, cx| input.set_text("second unsent", cx));
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.navigation.entries.len()),
        2
    );
    click(&f, "navigation-back", cx);
    settled(&f, &f.first.id, cx);
    assert_eq!(
        editor(&f, cx).read_with(cx, |input, _| input.text().to_owned()),
        "first unsent 中文"
    );
    focus_editor(&f, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-]");
    settled(&f, &f.second.id, cx);
    assert_eq!(
        editor(&f, cx).read_with(cx, |input, _| input.text().to_owned()),
        "second unsent"
    );
    // Actual platform text-input composition must keep bracket shortcuts in the editor.
    focus_editor(&f, cx);
    let input = editor(&f, cx);
    f.window
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "拼音", Some(0..2), window, cx);
            })
        })
        .unwrap();
    cx.simulate_keystrokes(f.window.into(), "cmd-[");
    cx.run_until_parked();
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
    assert!(input.read_with(cx, |input, _| input.is_composing()));
    input.update(cx, |input, cx| input.set_text("second unsent", cx));
    // Collapsed layout still exposes the actual mouse control.
    cx.update(|cx| {
        cx.set_global(SidebarCollapsed(true));
        cx.refresh_windows();
    });
    click(&f, "navigation-back", cx);
    settled(&f, &f.first.id, cx);
    // Settings global is an existing production route entry; close uses the real global handler.
    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    f.window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(CloseSettings), cx)
        })
        .unwrap();
    pump(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            !root.navigation.pending && !cx.global::<SettingsOpen>().0
        })
    });
    settled(&f, &f.first.id, cx);
    assert_eq!(
        editor(&f, cx).read_with(cx, |input, _| input.text().to_owned()),
        "first unsent 中文"
    );
    assert!(
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    );
    assert!(f.data.path().join("vega.db").is_file());
}

#[gpui_kit::test]
async fn navigation_real_root_skips_archived_and_capacity_refusal_precedes_visit(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    palette_second(&f, cx);
    // A nonempty oversized live draft must never be silently truncated or evicted.
    let text = "one more unsent draft".to_string();
    f.root.update(cx, |root, _| {
        root.navigation
            .drafts
            .insert("retained-owned-task".into(), "x".repeat(DRAFT_BYTES));
    });
    let input = editor(&f, cx);
    input.update(cx, |input, cx| input.set_text(&text, cx));
    let before = vega_store::threads::find(f.store.conn(), &f.first.id)
        .unwrap()
        .unwrap();
    focus_editor(&f, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-[");
    cx.run_until_parked();
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
    assert_eq!(
        input.read_with(cx, |input, _| input.text().len()),
        text.len()
    );
    let after = vega_store::threads::find(f.store.conn(), &f.first.id)
        .unwrap()
        .unwrap();
    assert_eq!(before.updated_at, after.updated_at);
    assert!(
        f.root
            .read_with(cx, |_, cx| cx.global::<NavigationState>().error.is_some())
    );
    input.update(cx, |input, cx| input.clear(cx));
    // Archive the only back target through the real conversation service.
    vega_conversation::threads::set_thread_status(&f.store, &f.first.id, ThreadStatus::Archived)
        .unwrap();
    click(&f, "navigation-back", cx);
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            !root.navigation.pending && root.navigation.entries.len() == 1
        })
    });
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
    assert_eq!(
        vega_store::threads::find(f.store.conn(), &f.first.id)
            .unwrap()
            .unwrap()
            .status,
        "archived"
    );
    assert!(
        !f.root
            .read_with(cx, |_, cx| cx.global::<NavigationState>().back)
    );
}

#[gpui_kit::test]
async fn navigation_coalesces_attributes_bounds_history_and_rejects_stale_result(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let third = vega_conversation::threads::create_thread(
        &f.store,
        &f.first.project_id,
        "model",
        "confirm",
    )
    .unwrap();
    palette_second(&f, cx);
    let count = f
        .root
        .read_with(cx, |root, _| root.navigation.entries.len());
    cx.update(|cx| {
        let mut thread = cx.global::<OpenedThread>().0.clone().unwrap();
        thread.title = "attribute refresh".into();
        cx.set_global(OpenedThread(Some(thread)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.navigation.entries.len()),
        count
    );
    // Start the actual worker, then move before polling; its completion must not apply.
    f.root.update(cx, |root, cx| root.navigate(false, cx));
    cx.update(|cx| {
        cx.set_global(SelectedProject(Some(third.project_id.clone())));
        cx.set_global(OpenedThread(Some(third.clone())));
        cx.refresh_windows();
    });
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| !root.navigation.pending)
    });
    assert_eq!(current(&f, cx), Some(third.id.clone()));
    // Bounds are a supplemental invariant; all entries come through production reconciliation.
    for index in 0..105 {
        cx.update(|cx| {
            cx.set_global(OpenedThread(None));
            cx.set_global(SelectedProject(Some(format!("owned-logical-{index}"))));
        });
        f.root.update(cx, |root, cx| root.sync_navigation(cx));
    }
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.navigation.entries.len()),
        HISTORY_LIMIT
    );
}

#[gpui_kit::test]
async fn navigation_settings_returns_to_empty_without_database(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update(|cx| {
        cx.set_global(OpenedThread(None));
        cx.set_global(SelectedProject(None));
        cx.refresh_windows();
    });
    // R69 intentional change: the home route now renders the real composer
    // over an unpersisted draft thread (R1), so `stream_view` is mounted here
    // rather than `None`. The property this assertion protected — "the home
    // route needs no database" — is preserved: the draft is reached with the
    // store global still healthy, and the steps below re-prove it with the
    // store replaced by an error.
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft.is_some() && root.stream_view.is_some()
        })
    });
    let draft = f
        .root
        .read_with(cx, |root, _| {
            root.draft.as_ref().map(|draft| draft.id.clone())
        })
        .expect("home route installs its draft");
    cx.update(|cx| {
        cx.set_global(vega_ui::sidebar::VegaStore(Err(
            "owned unavailable database".into(),
        )));
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    f.window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(CloseSettings), cx)
        })
        .unwrap();
    pump(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            !root.navigation.pending && !cx.global::<SettingsOpen>().0
        })
    });
    // R69 R4/R7: returning from Settings lands back on the same draft id, and
    // the route renders without touching the (now unavailable) store — no
    // hydration ran, so no controller error bar appeared.
    assert_eq!(current(&f, cx), Some(draft.clone()));
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.draft.as_ref().map(|d| d.id.clone())),
        Some(draft)
    );
    assert_eq!(f.root.read_with(cx, |root, _| root.navigation.cursor), 1);
}

#[gpui_kit::test]
async fn navigation_resolver_rejects_real_sidebar_delete_before_ack(cx: &mut TestAppContext) {
    let f = fixture(cx);
    palette_second(&f, cx);
    f.root.update(cx, |root, cx| root.navigate(false, cx));
    // The real Sidebar confirmation commits deletion before resolver acknowledgement.
    cx.update(|cx| {
        cx.set_global(vega_ui::sidebar::PendingDeleteConfirm(Some(
            f.first.clone(),
        )))
    });
    f.root.update(cx, |root, cx| {
        root.sidebar
            .update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx))
    });
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| !root.navigation.pending)
    });
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
    assert!(
        vega_store::threads::find(f.store.conn(), &f.first.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.root.read_with(cx, |root, _| root.navigation.cursor), 1);
    click(&f, "navigation-back", cx);
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            !root.navigation.pending && root.navigation.entries.len() == 1
        })
    });
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
}

#[gpui_kit::test]
async fn navigation_draft_growth_during_read_only_resolution_does_not_record_visit(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    palette_second(&f, cx);
    vega_conversation::threads::update_thread(
        &f.store,
        &f.first.id,
        &vega_conversation::types::ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let before = vega_store::threads::find(f.store.conn(), &f.first.id)
        .unwrap()
        .unwrap();
    let input = editor(&f, cx);
    input.update(cx, |input, cx| input.clear(cx));
    f.root.update(cx, |root, cx| {
        root.navigation
            .drafts
            .insert("retained-budget".into(), "x".repeat(DRAFT_BYTES));
        root.navigate(false, cx);
    });
    input.update(cx, |input, cx| {
        input.set_text("typed while resolver was pending", cx)
    });
    pump(cx, |cx| {
        f.root.read_with(cx, |root, _| !root.navigation.pending)
    });
    assert_eq!(current(&f, cx), Some(f.second.id.clone()));
    let after = vega_store::threads::find(f.store.conn(), &f.first.id)
        .unwrap()
        .unwrap();
    assert!(after.unread);
    assert_eq!(before.updated_at, after.updated_at);
    assert_eq!(f.root.read_with(cx, |root, _| root.navigation.cursor), 1);
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "typed while resolver was pending"
    );
}

#[gpui_kit::test]
async fn navigation_consecutive_shortcuts_survive_dropped_composer_focus(cx: &mut TestAppContext) {
    let f = fixture(cx);
    editor(&f, cx).update(cx, |input, cx| input.set_text("Alpha draft", cx));
    palette_second(&f, cx);
    editor(&f, cx).update(cx, |input, cx| input.set_text("Beta draft", cx));
    focus_editor(&f, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-[");
    settled(&f, &f.first.id, cx);
    assert_eq!(
        editor(&f, cx).read_with(cx, |input, _| input.text().to_owned()),
        "Alpha draft"
    );
    // No focus call or mouse click between these real application keystrokes.
    cx.simulate_keystrokes(f.window.into(), "cmd-]");
    settled(&f, &f.second.id, cx);
    assert_eq!(
        editor(&f, cx).read_with(cx, |input, _| input.text().to_owned()),
        "Beta draft"
    );
}

#[gpui_kit::test]
async fn r14_real_root_sidebar_pointer_then_keyboard_preserves_toggle_and_editor_focus(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    cx.update(|cx| {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "cmd-b",
            vega_ui::sidebar::ToggleSidebar,
            None,
        )]);
        cx.set_global(SidebarCollapsed(true));
        cx.refresh_windows();
    });
    editor(&f, cx).update(cx, |input, cx| input.set_text("owned sidebar draft", cx));
    focus_editor(&f, cx);
    click(&f, "toggle-sidebar", cx);
    assert!(!cx.update(|cx| cx.global::<SidebarCollapsed>().0));
    cx.simulate_keystrokes(f.window.into(), "space");
    cx.run_until_parked();
    assert!(
        cx.update(|cx| cx.global::<SidebarCollapsed>().0),
        "Space after pointer reveal must collapse"
    );
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert!(
        !cx.update(|cx| cx.global::<SidebarCollapsed>().0),
        "Enter after layout relocation must reveal"
    );
    focus_editor(&f, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-b");
    cx.run_until_parked();
    assert!(cx.update(|cx| cx.global::<SidebarCollapsed>().0));
    let input = editor(&f, cx);
    f.window
        .update(cx, |_, window, cx| {
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            assert_eq!(input.read(cx).text(), "owned sidebar draft");
        })
        .unwrap();
}

#[gpui_kit::test]
async fn r14_current_head_loads_without_selector_click_and_refreshes_hidden_sidebar(
    cx: &mut TestAppContext,
) {
    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let result = std::process::Command::new("/usr/bin/git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(result.status.success(), "owned Git setup/probe failed");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    }
    let data = tempfile::tempdir().unwrap();
    let owned = data.path().canonicalize().unwrap();
    let normal = owned.join("normal");
    let linked = owned.join("linked");
    let plain = owned.join("plain");
    for path in [&normal, &plain] {
        std::fs::create_dir(path).unwrap();
    }
    git(&normal, &["init", "-b", "actual-current"]);
    git(
        &normal,
        &[
            "-c",
            "user.name=Vega Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "owned",
        ],
    );
    git(
        &normal,
        &[
            "worktree",
            "add",
            "-b",
            "linked-current",
            linked.to_str().unwrap(),
        ],
    );
    let database = owned.join("branches.sqlite");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let mut routes = Vec::new();
    for path in [&normal, &linked, &plain] {
        let project = vega_store::projects::create(
            store.conn(),
            path.to_str().unwrap(),
            "owned",
            Some("wrong-cache"),
        )
        .unwrap();
        routes.push(
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .unwrap(),
        );
    }
    cx.update(|cx| {
        crate::tests::install_diff_window_globals(
            Store::open(&database).unwrap(),
            routes[0].clone(),
            cx,
        );
        cx.set_global(SidebarCollapsed(true));
    });
    let root = cx.new(VegaWindow::new);
    let view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| view,
        )
        .unwrap()
    });
    let wait_label = |label: &str, cx: &mut TestAppContext| {
        let selector: &'static str = Box::leak(format!("branch-current-{label}").into_boxed_str());
        pump(cx, |cx| {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.debug_bounds(selector).is_some()
        });
        root.read_with(cx, |root, cx| {
            let selector = root
                .stream_view
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .branch_selector();
            assert!(!selector.read(cx).is_open());
            assert!(!selector.read(cx).is_pending());
        });
    };
    wait_label("actual-current", cx);
    assert_eq!(
        git(&normal, &["branch", "--show-current"]),
        "actual-current"
    );
    git(&normal, &["checkout", "-b", "external-current"]);
    wait_label("external-current", cx);
    assert!(cx.update(|cx| cx.global::<SidebarCollapsed>().0));
    let visit = |thread: &Thread, cx: &mut TestAppContext| {
        cx.update(|cx| {
            cx.set_global(SelectedProject(Some(thread.project_id.clone())));
            cx.set_global(OpenedThread(Some(thread.clone())));
            cx.refresh_windows();
        });
    };
    visit(&routes[1], cx);
    wait_label("linked-current", cx);
    assert_eq!(
        git(&normal, &["branch", "--show-current"]),
        "external-current"
    );
    assert_eq!(
        git(&linked, &["branch", "--show-current"]),
        "linked-current"
    );
    git(&linked, &["checkout", "--detach"]);
    wait_label("detached", cx);
    assert!(git(&linked, &["branch", "--show-current"]).is_empty());
    visit(&routes[2], cx);
    pump(cx, |cx| {
        if !root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &routes[2].id)
        }) {
            return false;
        }
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.debug_bounds("branch-current-读取分支…").is_none()
            && visual.debug_bounds("branch-current-分支暂不可用").is_none()
            && visual.debug_bounds("branch-current-detached").is_none()
            && visual
                .debug_bounds("branch-current-非 Git 文件夹")
                .is_none()
    });
    visit(&routes[0], cx);
    wait_label("external-current", cx);
    assert_eq!(
        git(&normal, &["branch", "--show-current"]),
        "external-current"
    );
}
