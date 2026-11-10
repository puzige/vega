use gpui_kit::{TestAppContext, TestDispatcher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use vega_ui::settings::{
    SettingsView, changed_content_font_theme_and_width_reject_retired_measurements,
    narrow_fields_wrap_labels_and_preserve_natural_height,
    passive_pixels_forward_parent_input_once_and_skip_cross_surface_focus,
};

type MountedCase = (&'static str, fn(&mut TestAppContext));

fn main() {
    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("mounted_settings_tail"),
        platform.text_system(),
    );
    cx.update(|cx| {
        assert!(
            !cx.text_system().all_font_names().is_empty(),
            "mounted Settings integration requires actual platform fonts"
        );
    });
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let cases: [MountedCase; 4] = [
        (
            "narrow_fields_wrap_labels_and_preserve_natural_height",
            narrow_fields_wrap_labels_and_preserve_natural_height,
        ),
        (
            "changed_content_font_theme_and_width_reject_retired_measurements",
            changed_content_font_theme_and_width_reject_retired_measurements,
        ),
        (
            "passive_pixels_forward_parent_input_once_and_skip_cross_surface_focus",
            passive_pixels_forward_parent_input_once_and_skip_cross_surface_focus,
        ),
        (
            "queued_general_commands_are_revoked_on_close",
            SettingsView::queued_general_commands_are_revoked_on_close,
        ),
    ];
    let total = cases.len();
    for (name, case) in cases {
        if catch_unwind(AssertUnwindSafe(|| case(&mut cx))).is_err() {
            eprintln!("FAIL {name}; remaining cases not executed; 0 ignored");
            std::process::exit(1);
        }
        println!("PASS {name}");
    }
    println!("Settings integration: {total} passed; 0 failed; 0 ignored");
}
