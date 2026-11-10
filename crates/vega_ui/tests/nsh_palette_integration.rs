use gpui_kit::{TestAppContext, TestDispatcher};
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::command_palette::{
    marked_palette_keys_leave_controller_and_focus_unchanged,
    ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events,
    retired_palette_requests_cannot_mutate_mounted_controller,
};

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn report(message: impl std::fmt::Display) {
    let mut output = io::stderr().lock();
    writeln!(output, "{message}").expect("palette regression report");
}

fn main() {
    let cases: [MountedCase; 3] = [
        (
            "marked_palette_keys_leave_controller_and_focus_unchanged",
            marked_palette_keys_leave_controller_and_focus_unchanged,
        ),
        (
            "ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events",
            ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events,
        ),
        (
            "retired_palette_requests_cannot_mutate_mounted_controller",
            retired_palette_requests_cannot_mutate_mounted_controller,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_production_command_palette"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted palette integration requires actual platform fonts"
            );
        });
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        for (name, case) in cases {
            report(format_args!("RUN {name}"));
            match catch_unwind(AssertUnwindSafe(|| case(&mut cx))) {
                Ok(()) => {
                    passed += 1;
                    report(format_args!("PASS {name}"));
                }
                Err(_) => {
                    failed += 1;
                    report(format_args!("FAIL {name}"));
                    break;
                }
            }
        }
    }));
    let unrun = total - passed - failed;
    report(format_args!(
        "palette integration: {passed} passed; {failed} failed; {unrun} unrun; 0 ignored"
    ));
    if result.is_err() {
        report(
            "FAIL palette integration setup or teardown; case counts above exclude infrastructure failure",
        );
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
