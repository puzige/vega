use gpui_kit::{prelude::*, *};
use separator::{background, drag_region};
use vega_theme::theme;
use vega_ui::nsh::Surface;

#[path = "controller.rs"]
mod controller;
#[path = "controller_regressions.rs"]
mod controller_regressions;
#[path = "mounted_regressions.rs"]
mod mounted_regressions;
#[path = "separator.rs"]
mod separator;

pub(super) fn run() {
    use mounted_regressions as cases;
    super::run_cases(&[
        (
            "five_pixel_region_and_background_deliver_pointer_and_wheel_once",
            cases::five_pixel_region_and_background_deliver_pointer_and_wheel_once,
        ),
        (
            "resize_theme_and_clip_retain_surface_and_native_paint",
            cases::resize_theme_and_clip_retain_surface_and_native_paint,
        ),
        (
            "unmount_stops_and_releases_surface_before_rebuild",
            cases::unmount_stops_and_releases_surface_before_rebuild,
        ),
        (
            "displayed_extents_preserve_clamps_defaults_and_raw_drag",
            controller_regressions::displayed_extents_preserve_clamps_defaults_and_raw_drag,
        ),
        (
            "drag_release_cancel_and_window_resize_preserve_reveal",
            controller_regressions::drag_release_cancel_and_window_resize_preserve_reveal,
        ),
        (
            "visibility_thresholds_and_fullscreen_suppress_dock_separators",
            controller_regressions::visibility_thresholds_and_fullscreen_suppress_dock_separators,
        ),
    ]);
}
