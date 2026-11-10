use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::conversation_stream::{
    install_scroll_body_access, scroll_body_factory,
    scroll_composition_integration::{self as cases, BodyFactory},
};

use vega_ui::nsh_scroll_group_regressions as group_cases;

type MountedCase = (&'static str, fn(&mut TestAppContext, BodyFactory));

fn main() {
    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("nested_reading_scroll_composition"),
        platform.text_system(),
    );
    cx.update(|cx| {
        assert!(
            !cx.text_system().all_font_names().is_empty(),
            "nested scroll integration requires actual platform fonts"
        );
    });
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    cx.update(install_scroll_body_access);
    let cases: [MountedCase; 16] = [
        (
            "no_target_surface_forwards_wheel_once_to_original_list",
            cases::no_target_surface_forwards_wheel_once_to_original_list,
        ),
        (
            "inner_surface_consumes_wheel_until_actual_edge",
            cases::inner_surface_consumes_wheel_until_actual_edge,
        ),
        (
            "partial_forward_remainder_reaches_original_list_exactly_once",
            cases::partial_forward_remainder_reaches_original_list_exactly_once,
        ),
        (
            "partial_reverse_remainder_reaches_original_list_exactly_once",
            cases::partial_reverse_remainder_reaches_original_list_exactly_once,
        ),
        (
            "mixed_axes_consume_horizontal_inner_and_forward_vertical_remainder",
            cases::mixed_axes_consume_horizontal_inner_and_forward_vertical_remainder,
        ),
        (
            "mixed_axes_partial_inner_remainder_does_not_replay_vertical_delta",
            cases::mixed_axes_partial_inner_remainder_does_not_replay_vertical_delta,
        ),
        (
            "actual_top_and_bottom_edges_transfer_only_outward_delta",
            cases::actual_top_and_bottom_edges_transfer_only_outward_delta,
        ),
        (
            "gesture_owner_survives_pointer_move_until_ended",
            cases::gesture_owner_survives_pointer_move_until_ended,
        ),
        (
            "terminal_phase_delta_reaches_original_list_once",
            cases::terminal_phase_delta_reaches_original_list_once,
        ),
        (
            "cancelled_gesture_releases_owner_without_residual_glide",
            cases::cancelled_gesture_releases_owner_without_residual_glide,
        ),
        (
            "ended_edge_transfer_cannot_start_inner_and_outer_glides",
            cases::ended_edge_transfer_cannot_start_inner_and_outer_glides,
        ),
        (
            "ordinary_group_partial_and_mixed_axes_reach_original_list_once",
            |cx, _| group_cases::ordinary_group_partial_and_mixed_axes_reach_original_list_once(cx),
        ),
        (
            "group_horizontal_fallback_and_line_units_use_actual_offsets",
            |cx, _| group_cases::group_horizontal_fallback_and_line_units_use_actual_offsets(cx),
        ),
        (
            "group_owner_survives_pointer_change_and_terminal_delta_has_no_glide",
            |cx, _| {
                group_cases::group_owner_survives_pointer_change_and_terminal_delta_has_no_glide(cx)
            },
        ),
        (
            "replaced_scope_and_detached_group_revoke_retained_remainder_consumer",
            |cx, _| {
                group_cases::replaced_scope_and_detached_group_revoke_retained_remainder_consumer(
                    cx,
                )
            },
        ),
        (
            "real_body_then_group_then_original_list_preserve_partial_remainder",
            group_cases::real_body_then_group_then_original_list_preserve_partial_remainder,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    for (name, case) in cases {
        eprintln!("RUN {name}");
        let result = catch_unwind(AssertUnwindSafe(|| case(&mut cx, scroll_body_factory)));
        match result {
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
    let not_executed = total - passed - failed;
    println!(
        "nested scroll integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if failed != 0 {
        std::process::exit(1);
    }
}
