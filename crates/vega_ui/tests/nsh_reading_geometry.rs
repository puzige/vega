use nsh_platform_gpui::host_regressions;

type GeometryCase = (&'static str, fn(std::sync::Arc<gpui_kit::TextSystem>));

fn main() {
    let platform = gpui_kit::platform::current_platform(true);
    let system = std::sync::Arc::new(gpui_kit::TextSystem::new(platform.text_system()));
    assert!(!system.all_font_names().is_empty());
    let cases: [GeometryCase; 24] = [
        ("composed_zero_motion_boundaries_revoke_geometry_leases", host_regressions::composed_zero_motion_boundaries_revoke_geometry_leases),
        ("composed_owner_and_mouse_latch_keep_original_mounted_path", host_regressions::composed_owner_and_mouse_latch_keep_original_mounted_path),
        ("composed_route_and_native_scope_retirement_revoke_authority", host_regressions::composed_route_and_native_scope_retirement_revoke_authority),
        ("composed_generation_visibility_and_drop_retire_route_owner", host_regressions::composed_generation_visibility_and_drop_retire_route_owner),
        ("composed_real_offsets_preserve_mixed_axis_remainder_and_units", host_regressions::composed_real_offsets_preserve_mixed_axis_remainder_and_units),
        ("composed_terminal_delta_and_system_momentum_do_not_add_glide", host_regressions::composed_terminal_delta_and_system_momentum_do_not_add_glide),
        ("anchor_real_caret_once_core_application", host_regressions::anchor_moves_real_caret_once_and_reports_core_application),
        ("anchor_actual_inset_extent", host_regressions::anchor_clamps_to_actual_inset_extent),
        ("anchor_invalid_foreign_noop", host_regressions::anchor_rejects_invalid_foreign_and_noop_requests),
        ("anchor_stale_resize_follow_scroll_aba", host_regressions::anchor_rejects_stale_resize_follow_and_scroll_aba),
        ("anchor_detached_reused_nodes", host_regressions::anchor_rejects_detached_and_reused_scroll_nodes),
        ("anchor_active_input_motion_geometry", host_regressions::anchor_rejects_active_input_motion_and_unsupported_geometry),
        ("real_scrollbar_fade_and_scroll_aba", host_regressions::geometry_survives_real_scrollbar_fade_but_rejects_scroll_aba),
        ("shaped_wrap_affinity_bidi_utf16", host_regressions::geometry_uses_shaped_wrap_affinity_bidi_and_utf16),
        ("clipped_ranges_hits_unclipped_carets", host_regressions::geometry_clips_ranges_and_hits_but_preserves_unclipped_carets),
        ("scroll_resize_origin_scale_repaint_aba", host_regressions::geometry_revokes_scroll_resize_origin_scale_and_repaint_aba),
        ("layout_content_mount_selection_retirement", host_regressions::geometry_rejects_layout_content_mount_selection_and_surface_retirement),
        ("supported_transforms_clips", host_regressions::geometry_accepts_translation_uniform_scale_and_rejects_unsupported_clips),
        ("opacity_atomic_ranges_and_replace", host_regressions::opacity_rejects_invalid_ranges_atomically_and_full_replaces),
        ("opacity_preserves_exact_identities", host_regressions::opacity_keeps_snapshot_layout_and_geometry_exact),
        ("opacity_real_clusters_and_background", host_regressions::opacity_uses_native_cluster_origins_and_original_background_shape),
        ("opacity_retirement_and_aba", host_regressions::opacity_retires_selection_content_mount_stop_and_surface_aba),
        ("opacity_cross_text_and_forged_snapshot", host_regressions::opacity_cross_text_publication_rejects_forged_snapshot_and_old_area_clear),
        ("opacity_bounded_weak_lifetime", host_regressions::opacity_publication_lifetime_is_bounded_and_does_not_retain_surface),
    ];
    let total = cases.len();
    let mut failed = 0;
    for (name, case) in cases {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(system.clone()))) {
            Ok(()) => println!("PASS {name}"),
            Err(_) => {
                failed += 1;
                eprintln!("FAIL {name}");
            }
        }
    }
    println!(
        "reading geometry and opacity: {} passed; {failed} failed; 0 ignored",
        total - failed
    );
    if failed != 0 {
        std::process::exit(1);
    }
}
