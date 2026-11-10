use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::conversation_stream::{
    command_control_regressions, shell_recovery_mounted_regressions,
};

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    let cases: [MountedCase; 3] = [
        (
            "draft_summary_opens_separate_overlay_with_real_focus",
            shell_recovery_mounted_regressions::draft_summary_opens_separate_overlay_with_real_focus,
        ),
        (
            "pending_draft_close_escape_and_authority_keep_recovery_fenced",
            shell_recovery_mounted_regressions::pending_draft_close_escape_and_authority_keep_recovery_fenced,
        ),
        (
            "mounted_cancel_supersedes_read_and_retry_keeps_operation_authority",
            command_control_regressions::mounted_cancel_supersedes_read_and_retry_keeps_operation_authority,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_draft_shell_recovery"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted recovery integration requires actual platform fonts"
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
        "recovery integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if result.is_err() {
        eprintln!("FAIL recovery integration setup or teardown");
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
