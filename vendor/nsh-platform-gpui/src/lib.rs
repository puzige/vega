//! GPUI backend for nsh: apps drawn by GPUI (GPUI Fast), on macOS, Windows
//! and Linux alike, for embedding in GPUI and GPUI Kit apps (standalone
//! apps run on each OS's own backend: AppKit, Win32, GTK 4).
//!
//! - Each node is an [`view::NshView`], a retained view the core placed at
//!   its frame. A surface is one GPUI element ([`render::SurfaceElement`])
//!   that paints the whole tree with GPUI's primitives (quads, shadows,
//!   paths, glyphs, images): shadows and background, the content (shader,
//!   text, image, canvas), the children, the border, then the items the view
//!   hosts (text editors, headers).
//! - The engine runs on its own thread ([`nsh_engine::Engine`]); each
//!   commit is one closure sent to GPUI's main thread ([`rt`],
//!   [`surface`]), which notifies the surface's view.
//! - Text is shaped by GPUI's text system on the engine thread and broken
//!   into lines by nsh; the main thread paints the glyphs it placed
//!   ([`text`]).
//! - Native animations and JS frame requests tick on the window's frames
//!   while something animates.
//!
//! [`AppView`] is one surface as a GPUI view (`Entity<AppView>`), to put
//! anywhere in a GPUI app's element tree; [`run_app`] opens one in a
//! window, as the development host does. The input, scrolling, transition and editing logic is the
//! Windows backend's (its virtual key codes included, [`keys`]). Off the
//! desktop this crate is empty.

#![cfg(any(target_os = "linux", target_os = "macos", windows))]

mod access;
#[cfg(feature = "dev")]
mod agent;
mod animation;
mod app;
mod canvas;
mod chrome;
mod decor;
#[cfg(feature = "dev")]
pub mod dev;
#[allow(missing_docs)]
pub mod editing;
mod events;
mod file_drop;
mod geom;
mod image;
mod input;
#[cfg(feature = "vega-host-regressions")]
#[path = "app/integration_support.rs"]
mod integration_support;
mod keymap;
mod keys;
mod mount;
mod native;
mod navigation;
#[allow(missing_docs)]
pub mod reading;
mod render;
mod rt;
mod scroll;
mod scroll_scope;
mod selectable_view;
mod shader;
mod stats;
mod surface;
#[cfg(feature = "dev")]
mod synth;
mod system;
mod text;
mod text_input;
mod transition;
mod view;

// Windows of GPUI's test platform, which `shot` turns on.
#[cfg(all(test, feature = "shot"))]
mod tests;

#[cfg(feature = "vega-host-regressions")]
#[allow(missing_docs)]
pub mod host_regressions {
    pub use crate::input::scroll_host_tests::{
        composed_zero_motion_boundaries_revoke_geometry_leases,
        composed_owner_and_mouse_latch_keep_original_mounted_path,
        composed_route_and_native_scope_retirement_revoke_authority,
        composed_generation_visibility_and_drop_retire_route_owner,
        composed_real_offsets_preserve_mixed_axis_remainder_and_units,
        composed_terminal_delta_and_system_momentum_do_not_add_glide,
    };
    pub use crate::reading::anchor_host_tests::{
        anchor_clamps_to_actual_inset_extent,
        anchor_moves_real_caret_once_and_reports_core_application,
        anchor_rejects_active_input_motion_and_unsupported_geometry,
        anchor_rejects_detached_and_reused_scroll_nodes,
        anchor_rejects_invalid_foreign_and_noop_requests,
        anchor_rejects_stale_resize_follow_and_scroll_aba,
    };
    pub use crate::reading::geometry_host_tests::{
        geometry_accepts_translation_uniform_scale_and_rejects_unsupported_clips,
        geometry_clips_ranges_and_hits_but_preserves_unclipped_carets,
        geometry_rejects_layout_content_mount_selection_and_surface_retirement,
        geometry_revokes_scroll_resize_origin_scale_and_repaint_aba,
        geometry_survives_real_scrollbar_fade_but_rejects_scroll_aba,
        geometry_uses_shaped_wrap_affinity_bidi_and_utf16,
    };
    pub use crate::reading::host_tests::{
        pending_clipboard_write_is_revoked_by_takeover_aba_and_stop,
        single_text_install_preserves_direction_and_rejects_retired_snapshots,
        snapshots_reject_selection_aba_and_distinct_content_objects,
        takeover_is_fail_closed_after_detach_and_scoped_to_surface,
    };
    pub use crate::reading::opacity_host_tests::{
        opacity_cross_text_publication_rejects_forged_snapshot_and_old_area_clear,
        opacity_keeps_snapshot_layout_and_geometry_exact,
        opacity_publication_lifetime_is_bounded_and_does_not_retain_surface,
        opacity_rejects_invalid_ranges_atomically_and_full_replaces,
        opacity_retires_selection_content_mount_stop_and_surface_aba,
        opacity_uses_native_cluster_origins_and_original_background_shape,
    };
    pub use crate::selectable_view::host_tests::{
        exact_endpoints_preserve_direction_and_reject_stale_or_split_surrogates,
        mounted_cross_text_copy_uses_exact_content_and_rejects_replacement,
        selection_listener_tracks_mount_clear_and_reentrant_retirement,
    };
    pub use crate::text_input::host_tests::{
        edit_transaction_preserves_reversed_utf16_and_secure_input_is_not_observed,
        focus_loss_and_recycled_node_reject_old_policy,
        hooks_only_image_clipboard_enables_native_paste_menu,
        host_focus_gain_notifies_current_registration_once_and_loss_retires_it,
        marked_keys_never_reach_shortcuts_or_waiting_keys,
        native_clipping_and_explicit_same_text_composition_replacement_are_exact,
        native_deletes_cut_and_accessibility_report_real_ranges,
        native_marked_update_commit_cancel_and_reversed_surrogates_are_exact,
        native_menu_history_survives_empty_model_history_and_redo_uses_policy_once,
        native_menu_redo_rejects_composition_stale_registration_and_secure_history,
        native_menu_undo_calls_policy_once_and_unhandled_history_fails_closed,
        native_menu_undo_excludes_composition_without_queueing_policy,
        native_menu_undo_rechecks_target_after_declining_policy,
        native_menu_undo_rejects_stale_focus_mount_registration_and_window,
        native_opaque_history_is_not_reported_as_a_guessed_mutation,
        native_partial_and_overlapping_composition_ranges_preserve_untouched_text,
        native_paste_full_image_budget_uses_original_borrowed_files,
        native_paste_none_and_refusal_do_not_fabricate_original_or_edit,
        native_paste_preserves_original_entries_and_opaque_metadata, native_paste_through_hooks,
        native_replacements_preserve_operation_ranges_and_same_text_serials,
        native_same_text_paste_is_explicit_and_secret_snapshots_are_absent,
        native_selection_and_reregistration_have_no_mutation,
        ownerless_waiting_key_never_reaches_new_field_policy,
        paste_completion_blur_preserves_engine_accounting_and_rejects_stale_value,
        paste_is_one_native_transaction_and_attachment_only_is_not_lost,
        retirement_and_replacement_callbacks_cannot_rebind,
        stale_paste_and_callback_rebinding_cancel_without_native_edit,
        unknown_native_mutations_retire_without_diff_or_fallback,
        visual_history_context_uses_soft_wrap_and_current_viewport,
    };
}

pub use events::scroll_bridge;
pub use events::scroll_bridge::{ScrollRemainder, ScrollRoute};
pub use gpui;
pub use input::ScrollOutcome;
#[cfg(feature = "vega-host-regressions")]
pub use integration_support::{IntegrationScrollObservation, IntegrationTextObservation};
pub use nsh_base::{Theme, ThemeError};
pub use nsh_core::{
    element::{AdaptiveColor, Color, Hsla, Rgba},
    platform::{Appearance, Locale},
};
pub use nsh_engine::{
    App, AppError, AppErrorKind, Bundle, ByteSize, CacheLocation, CacheOptions, CachePolicy,
    Limits, PackageInfo, Root,
};
/// Signed packages: verifying, unpacking and accepting them
/// (`docs/dev/signed-packages.md`).
#[cfg(feature = "js")]
pub use nsh_engine::{LoadedPackage, accept_package, load_package, package, verify_package};
pub use scroll_scope::ScrollScope;

#[cfg(feature = "dev")]
pub use crate::app::set_root;
pub use crate::{
    app::{AppOptions, AppSource, AppView, CloseRequest, open_app_window, run_app, set_ui_sets},
    chrome::titlebar_options,
    image::AssetBase,
    native::{ComponentContext, ComponentFactory, Components, NativeComponent},
};
