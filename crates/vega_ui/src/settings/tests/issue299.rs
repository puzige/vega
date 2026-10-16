use super::*;
use crate::conversation_stream::RunningMessageDefault;

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
