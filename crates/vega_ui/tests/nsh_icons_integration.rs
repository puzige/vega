use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::icons::host_regressions;

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    let cases: [MountedCase; 3] = [
        (
            "shared_icons_retain_native_surface_across_frames_and_theme",
            host_regressions::shared_icons_retain_native_surface_across_frames_and_theme,
        ),
        (
            "shared_icon_clip_motion_and_opacity_preserve_parent_input",
            host_regressions::shared_icon_clip_motion_and_opacity_preserve_parent_input,
        ),
        (
            "shared_tooltip_shortcut_toggle_retains_native_layout_and_accessibility",
            host_regressions::shared_tooltip_shortcut_toggle_retains_native_layout_and_accessibility,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_shared_icons_and_tooltip"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted icon integration requires actual platform fonts"
            );
        });
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        for (name, case) in cases {
            eprintln!("RUN {name}");
            match catch_unwind(AssertUnwindSafe(|| case(&mut cx))) {
                Ok(()) => {
                    passed += 1;
                    println!("PASS {name}");
                }
                Err(_) => {
                    failed += 1;
                    eprintln!("FAIL {name}");
                    break;
                }
            }
        }
    }));
    let not_executed = total - passed - failed;
    println!(
        "icon integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if result.is_err() {
        eprintln!("FAIL icon integration setup or teardown");
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
