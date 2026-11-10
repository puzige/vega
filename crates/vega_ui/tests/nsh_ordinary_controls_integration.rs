use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::conversation_stream::ordinary_controls_mounted_regressions as cases;

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    let cases: [MountedCase; 4] = [
        (
            "timestamp_native_narrow_height_and_passive_input",
            cases::timestamp_native_narrow_height_and_passive_input,
        ),
        (
            "production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch",
            cases::production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch,
        ),
        (
            "retry_and_same_attempt_fact_changes_reject_old_displayed_controls",
            cases::retry_and_same_attempt_fact_changes_reject_old_displayed_controls,
        ),
        (
            "route_settings_geometry_and_unmount_reject_captured_controls",
            cases::route_settings_geometry_and_unmount_reject_captured_controls,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_timestamps_and_ordinary_controls"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted ordinary controls require actual platform fonts"
            );
        });
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        cx.update(vega_ui::init);
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
        "ordinary controls integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if result.is_err() {
        eprintln!("FAIL ordinary controls integration setup or teardown");
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
