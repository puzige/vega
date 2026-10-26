use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui_kit::{AppContext, TestAppContext, WindowHandle};
use vega_conversation::types::StartupError;

use super::StartupView;

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-startup-window-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated root");
        let database = root.join("vega.db");
        let config = root.join("config.toml");
        let store = vega_store::Store::open(&database).expect("isolated database");
        store.migrate().expect("migrate fixture");
        store
            .conn()
            .pragma_update(None, "user_version", i32::MAX)
            .expect("unsupported schema fixture");
        Self {
            root,
            database,
            config,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn draw(window: WindowHandle<StartupView>, cx: &mut TestAppContext) {
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .expect("draw real startup window");
}

fn wait_for_failure(window: WindowHandle<StartupView>, cx: &mut TestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        cx.run_until_parked();
        if !window
            .read_with(cx, |view, _| view.running)
            .expect("startup state")
        {
            break;
        }
        assert!(Instant::now() < deadline, "startup worker must finish");
        cx.background_executor
            .advance_clock(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(1));
    }
    draw(window, cx);
    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.error, Some(StartupError::StorageUnavailable));
            assert!(view.retry_focus.is_focused(window));
            assert!(!cx.has_global::<vega_ui::sidebar::VegaStore>());
            assert!(!cx.has_global::<vega_ui::settings::SettingsOpen>());
            assert!(!cx.has_global::<crate::app_agent::AppMcpSettings>());
            assert!(!cx.has_global::<crate::automation_controller::AutomationGlobal>());
        })
        .expect("restricted failure state");
}

#[gpui_kit::test]
fn startup_failure_retains_gate_and_keyboard_retry_without_opening_workspace(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::apply_appearance(None, cx);
    });
    let window = cx.add_window(|window, cx| {
        let mut view = StartupView::idle(cx);
        view.fixture_paths = Some((fixture.database.clone(), fixture.config.clone()));
        view.start(window, cx);
        assert!(view.running);
        assert!(view.quit_focus.is_focused(window));
        view.move_focus(window, cx);
        assert!(view.quit_focus.is_focused(window));
        view
    });
    draw(window, cx);
    wait_for_failure(window, cx);
    assert!(!fixture.config.exists());
    cx.simulate_keystrokes(window.into(), "tab");
    window
        .update(cx, |view, window, _| {
            assert!(view.quit_focus.is_focused(window))
        })
        .expect("tab reaches quit");
    draw(window, cx);
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |view, window, _| {
            assert!(view.retry_focus.is_focused(window))
        })
        .expect("shift-tab reaches retry");
    draw(window, cx);
    cx.simulate_keystrokes(window.into(), "escape");
    wait_for_failure(window, cx);
    for key in ["enter", "space"] {
        cx.dispatch_keystroke(
            window.into(),
            gpui_kit::Keystroke::parse(key).expect("activation key"),
        );
        window
            .update(cx, |view, window, _| {
                assert!(view.running);
                assert!(view.error.is_none());
                assert!(view.quit_focus.is_focused(window));
            })
            .expect("keyboard retry starts actual worker");
        draw(window, cx);
        cx.simulate_keystrokes(window.into(), "tab shift-tab escape");
        window
            .update(cx, |view, window, _| {
                assert!(view.quit_focus.is_focused(window))
            })
            .expect("running gate excludes retry");
        wait_for_failure(window, cx);
        assert!(!fixture.config.exists());
    }
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close startup fixture");
    cx.run_until_parked();
}
