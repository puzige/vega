use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::conversation_stream::message_anchor_mounted_regressions;

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    let cases: [MountedCase; 6] = [
        (
            "short_preview_uses_actual_natural_height_and_original_shadow",
            message_anchor_mounted_regressions::short_preview_uses_actual_natural_height_and_original_shadow,
        ),
        (
            "narrow_long_cjk_and_emoji_preview_clamps_three_native_lines",
            message_anchor_mounted_regressions::narrow_long_cjk_and_emoji_preview_clamps_three_native_lines,
        ),
        (
            "mark_states_keep_native_dimensions_and_semantic_colors",
            message_anchor_mounted_regressions::mark_states_keep_native_dimensions_and_semantic_colors,
        ),
        (
            "stable_frames_and_changed_font_theme_content_width_retain_surfaces",
            message_anchor_mounted_regressions::stable_frames_and_changed_font_theme_content_width_retain_surfaces,
        ),
        (
            "clipped_passive_leaves_forward_input_once_and_skip_tab_focus",
            message_anchor_mounted_regressions::clipped_passive_leaves_forward_input_once_and_skip_tab_focus,
        ),
        (
            "unmount_stops_both_surfaces_and_remount_creates_new_identity",
            message_anchor_mounted_regressions::unmount_stops_both_surfaces_and_remount_creates_new_identity,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_message_anchor_pixels"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted message anchors require actual platform fonts"
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
        "message anchor integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if result.is_err() {
        eprintln!("FAIL message anchor integration setup or teardown");
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
