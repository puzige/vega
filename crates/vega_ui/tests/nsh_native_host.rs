macro_rules! native_regressions {
    ($($name:ident),* $(,)?) => {$(
        #[test]
        fn $name() { nsh_platform_gpui::host_regressions::$name(); }
    )*};
}

native_regressions!(
    single_text_install_preserves_direction_and_rejects_retired_snapshots,
    native_replacements_preserve_operation_ranges_and_same_text_serials,
    native_selection_and_reregistration_have_no_mutation,
    native_marked_update_commit_cancel_and_reversed_surrogates_are_exact,
    native_partial_and_overlapping_composition_ranges_preserve_untouched_text,
    native_deletes_cut_and_accessibility_report_real_ranges,
    unknown_native_mutations_retire_without_diff_or_fallback,
    native_clipping_and_explicit_same_text_composition_replacement_are_exact,
    native_same_text_paste_is_explicit_and_secret_snapshots_are_absent,
    native_opaque_history_is_not_reported_as_a_guessed_mutation,
    native_menu_history_survives_empty_model_history_and_redo_uses_policy_once,
    native_menu_redo_rejects_composition_stale_registration_and_secure_history,
    native_menu_undo_calls_policy_once_and_unhandled_history_fails_closed,
    native_menu_undo_excludes_composition_without_queueing_policy,
    native_menu_undo_rejects_stale_focus_mount_registration_and_window,
    native_menu_undo_rechecks_target_after_declining_policy,
    native_paste_preserves_original_entries_and_opaque_metadata,
    native_paste_none_and_refusal_do_not_fabricate_original_or_edit,
    native_paste_full_image_budget_uses_original_borrowed_files,
);

#[test]
fn exact_endpoints_preserve_direction_and_reject_stale_or_split_surrogates() {
    nsh_platform_gpui::host_regressions::exact_endpoints_preserve_direction_and_reject_stale_or_split_surrogates();
}

#[test]
fn selection_listener_tracks_mount_clear_and_reentrant_retirement() {
    nsh_platform_gpui::host_regressions::selection_listener_tracks_mount_clear_and_reentrant_retirement();
}

#[test]
fn paste_is_one_native_transaction_and_attachment_only_is_not_lost() {
    nsh_platform_gpui::host_regressions::paste_is_one_native_transaction_and_attachment_only_is_not_lost();
}

#[test]
fn paste_completion_blur_preserves_engine_accounting_and_rejects_stale_value() {
    nsh_platform_gpui::host_regressions::paste_completion_blur_preserves_engine_accounting_and_rejects_stale_value();
}

#[test]
fn hooks_only_image_clipboard_enables_native_paste_menu() {
    nsh_platform_gpui::host_regressions::hooks_only_image_clipboard_enables_native_paste_menu();
}

#[test]
fn stale_paste_and_callback_rebinding_cancel_without_native_edit() {
    nsh_platform_gpui::host_regressions::stale_paste_and_callback_rebinding_cancel_without_native_edit();
}

#[test]
fn visual_history_context_uses_soft_wrap_and_current_viewport() {
    nsh_platform_gpui::host_regressions::visual_history_context_uses_soft_wrap_and_current_viewport(
    );
}

#[test]
fn marked_keys_never_reach_shortcuts_or_waiting_keys() {
    nsh_platform_gpui::host_regressions::marked_keys_never_reach_shortcuts_or_waiting_keys();
}

#[test]
fn reversed_utf16_edit_is_atomic_and_secure_input_is_not_observed() {
    nsh_platform_gpui::host_regressions::edit_transaction_preserves_reversed_utf16_and_secure_input_is_not_observed();
}

#[test]
fn ownerless_waiting_key_never_reaches_new_field_policy() {
    nsh_platform_gpui::host_regressions::ownerless_waiting_key_never_reaches_new_field_policy();
}

#[test]
fn retirement_and_replacement_callbacks_cannot_rebind() {
    nsh_platform_gpui::host_regressions::retirement_and_replacement_callbacks_cannot_rebind();
}

#[test]
fn pending_clipboard_write_is_revoked_by_takeover_aba_and_stop() {
    nsh_platform_gpui::host_regressions::pending_clipboard_write_is_revoked_by_takeover_aba_and_stop();
}

#[test]
fn host_focus_gain_notifies_current_registration_once_and_loss_retires_it() {
    nsh_platform_gpui::host_regressions::host_focus_gain_notifies_current_registration_once_and_loss_retires_it();
}

#[test]
fn focus_loss_and_recycled_node_reject_old_policy() {
    nsh_platform_gpui::host_regressions::focus_loss_and_recycled_node_reject_old_policy();
}

#[test]
fn takeover_is_fail_closed_after_detach_and_scoped_to_surface() {
    nsh_platform_gpui::host_regressions::takeover_is_fail_closed_after_detach_and_scoped_to_surface(
    );
}

#[test]
fn snapshots_reject_selection_aba_and_distinct_content_objects() {
    nsh_platform_gpui::host_regressions::snapshots_reject_selection_aba_and_distinct_content_objects();
}

#[test]
fn mounted_cross_text_copy_uses_exact_content_and_rejects_replacement() {
    nsh_platform_gpui::host_regressions::mounted_cross_text_copy_uses_exact_content_and_rejects_replacement();
}
