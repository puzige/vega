use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[path = "mounted_entry.rs"]
mod window;

fn main() {
    window::run();
}

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn run_cases(cases: &[MountedCase]) {
    let mut passed = 0;
    let mut failed = 0;
    let setup = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_workspace_passive_separators"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted workspace integration requires actual platform fonts"
            );
        });
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        for (name, case) in cases {
            eprintln!("RUN {name}");
            match catch_unwind(AssertUnwindSafe(|| case(&mut cx))) {
                Ok(()) => {
                    passed += 1;
                    eprintln!("PASS {name}");
                }
                Err(_) => {
                    failed += 1;
                    eprintln!("FAIL {name}");
                    break;
                }
            }
        }
    }));
    let not_executed = cases.len() - passed - failed;
    eprintln!(
        "workspace integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if setup.is_err() {
        eprintln!("FAIL workspace integration setup or teardown");
    }
    if failed != 0 || setup.is_err() {
        std::process::exit(1);
    }
}
