use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::{
    repeated_production_header_render_retains_unowned_mount_and_projection,
    revoked_thinking_root_rejects_open_retry_and_delayed_completion,
    stationary_thinking_hover_survives_fade_open_and_retry,
};

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    use vega_ui::conversation_stream as source;
    let cases: [MountedCase; 15] = [
        ("source_caret_reflow_preserves_position_affinity_and_signed_inset", source::source_caret_reflow_preserves_position_affinity_and_signed_inset),
        ("source_caret_trim_preserves_absolute_position_inside_paragraph", source::source_caret_trim_preserves_absolute_position_inside_paragraph),
        ("source_caret_removed_trim_clamps_forward_with_zero_inset", source::source_caret_removed_trim_clamps_forward_with_zero_inset),
        ("source_caret_replacement_does_not_restore_equal_offsets", source::source_caret_replacement_does_not_restore_equal_offsets),
        ("source_caret_owner_and_mount_aba_reject_old_geometry", source::source_caret_owner_and_mount_aba_reject_old_geometry),
        ("source_caret_manual_scroll_rejects_pending_and_delayed_follow", source::source_caret_manual_scroll_rejects_pending_and_delayed_follow),
        ("source_caret_follow_uses_actual_extent_after_append", source::source_caret_follow_uses_actual_extent_after_append),
        ("source_caret_selection_revokes_pending_reflow", source::source_caret_selection_revokes_pending_reflow),
        ("source_caret_new_projection_rejects_delayed_session_report", source::source_caret_new_projection_rejects_delayed_session_report),
        (
            "measured_focus_paints_wrapped_fragments_and_tracks_tab_reverse_and_escape",
            vega_ui::link_preview::measured_focus_paints_wrapped_fragments_and_tracks_tab_reverse_and_escape,
        ),
        (
            "stationary_thinking_hover_survives_fade_open_and_retry",
            stationary_thinking_hover_survives_fade_open_and_retry,
        ),
        (
            "revoked_thinking_root_rejects_open_retry_and_delayed_completion",
            revoked_thinking_root_rejects_open_retry_and_delayed_completion,
        ),
        (
            "repeated_production_header_render_retains_unowned_mount_and_projection",
            repeated_production_header_render_retains_unowned_mount_and_projection,
        ),
        (
            "mounted_native_recovery_history_ax_and_submit_close_production_loop",
            vega_ui::text_input::mounted_native_recovery_history_ax_and_submit_close_production_loop,
        ),
        (
            "mounted_rename_native_ack_ime_cancel_rebind_and_persistence",
            vega_ui::sidebar::mounted_rename_native_ack_ime_cancel_rebind_and_persistence,
        ),
    ];
    let total = cases.len();
    let mut passed = 0;
    let mut failed = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let platform = gpui_kit::platform::current_platform(true);
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            Some("mounted_thinking_reading_preview"),
            platform.text_system(),
        );
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "mounted reading integration requires actual platform fonts"
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
        "reading/editing integration: {passed} passed; {failed} failed; {not_executed} not executed; 0 ignored"
    );
    if result.is_err() {
        eprintln!("FAIL reading integration setup or teardown");
    }
    if failed != 0 || result.is_err() {
        std::process::exit(1);
    }
}
