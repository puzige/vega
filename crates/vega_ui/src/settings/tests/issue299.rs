use super::*;
use crate::conversation_stream::RunningMessageDefault;
use vega_conversation::types::PendingMessageBehavior;

fn mount(
    cx: &mut TestAppContext,
    path: std::path::PathBuf,
) -> (Entity<SettingsView>, WindowHandle<SettingsHarness>) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        cx.set_global(RunningMessageDefault::default());
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path), cx));
    let root = view.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.0), px(700.0)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(window, "settings-nav-general", cx);
    (view, window)
}

fn settle(cx: &mut TestAppContext, view: &Entity<SettingsView>) {
    for _ in 0..150 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| !view.provider_management.saving) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("preferences did not settle");
}

fn click(window: WindowHandle<SettingsHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn issue299_settings_persist_default_preserve_unrelated_config_and_reject_conflict(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().unwrap();
    let path = owned.path().join("config.toml");
    let config = AppConfig::default();
    config.save_to(&path).unwrap();
    let (view, window) = mount(cx, path.clone());
    let saved_events = Arc::new(AtomicUsize::new(0));
    let captured = saved_events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            captured.fetch_add(1, Ordering::SeqCst);
        })
        .detach()
    });
    let mut external = config.clone();
    external.agent.turn_limit = 19;
    external.save_to(&path).unwrap();
    click(window, "settings-running-message-steer", cx);
    settle(cx, &view);
    let saved = config::read_from(&path).unwrap();
    assert_eq!(saved.agent.running_message_behavior, "steer");
    assert_eq!(saved.agent.turn_limit, 19);
    assert_eq!(
        cx.update(|cx| cx.global::<RunningMessageDefault>().0),
        PendingMessageBehavior::Steer
    );
    assert_eq!(saved_events.load(Ordering::SeqCst), 1);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-running-message-steer-selected")
            .is_some()
    );
    let mut external = saved;
    external.agent.running_message_behavior = "queue".into();
    external.save_to(&path).unwrap();
    click(window, "settings-running-message-queue", cx);
    settle(cx, &view);
    assert!(view.read_with(cx, |view, _| view.error.as_deref()
        == Some("配置已更改，请重新读取")));
    assert_eq!(saved_events.load(Ordering::SeqCst), 1);
    assert_eq!(
        cx.update(|cx| cx.global::<RunningMessageDefault>().0),
        PendingMessageBehavior::Steer
    );
    cx.update(|cx| cx.set_global(RunningMessageDefault(PendingMessageBehavior::Queue)));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .config
            .agent
            .running_message_behavior
            .clone()),
        "queue"
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-running-message-queue-selected")
            .is_some()
    );
}
