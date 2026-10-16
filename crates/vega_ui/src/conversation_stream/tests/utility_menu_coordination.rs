//! C1–C3: production Composer utility-menu coordination.
//!
//! These tests mount the real `ConversationStream`, drive the actual chip
//! mouse/key paths, and inspect the painted branch rows rather than inferring
//! state from the selector model alone.

use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};

fn mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn branch_selector(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    stream.read_with(cx, |stream, _| stream.branch_selector())
}

fn leaked(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn set_theme(cx: &mut TestAppContext, dark: bool) {
    cx.update(|cx| {
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
        cx.refresh_windows();
    });
    cx.run_until_parked();
}

fn painted_quads(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) -> (Vec<Quad>, f32) {
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("utility-menu window must remain open while reading painted quads")
}

fn same_colour(actual: Hsla, expected: Rgba) -> bool {
    let actual = Rgba::from(actual);
    [
        (actual.r, expected.r),
        (actual.g, expected.g),
        (actual.b, expected.b),
        (actual.a, expected.a),
    ]
    .into_iter()
    .all(|(actual, expected)| (actual - expected).abs() <= 1.0 / 255.0)
}

fn quad_matches_bounds(
    quad: &Quad,
    target: gpui_kit::Bounds<gpui_kit::Pixels>,
    scale: f32,
) -> bool {
    let left = quad.bounds.left().as_f32() / scale;
    let top = quad.bounds.top().as_f32() / scale;
    let right = quad.bounds.right().as_f32() / scale;
    let bottom = quad.bounds.bottom().as_f32() / scale;
    let tolerance = 0.2;
    (left - target.left().as_f32()).abs() <= tolerance
        && (right - target.right().as_f32()).abs() <= tolerance
        && (top - target.top().as_f32()).abs() <= tolerance
        && (bottom - target.bottom().as_f32()).abs() <= tolerance
}

fn row_has_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    expected: Rgba,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some_and(|solid| {
            same_colour(solid, expected) && quad_matches_bounds(quad, target, scale)
        })
    })
}

fn row_has_any_solid_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some() && quad_matches_bounds(quad, target, scale)
    })
}

fn assert_popup_state(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    expected: Option<&'static str>,
    cx: &mut TestAppContext,
) {
    let project = mounted(window, "composer-utility-project-menu", cx);
    let branch = mounted(window, "branch-selector-popup", cx);
    assert!(
        !(project && branch),
        "at most one utility popup may be mounted"
    );
    match expected {
        Some("project") => assert!(project && !branch, "project popup must be the only popup"),
        Some("branch") => assert!(branch && !project, "branch popup must be the only popup"),
        None => assert!(!project && !branch, "both utility popups must be closed"),
        Some(other) => panic!("unknown expected popup {other}"),
    }
    let (project_open, branch_open) = stream.read_with(cx, |stream, cx| {
        (
            stream.utility_projects_open,
            stream
                .branch_selector
                .read_with(cx, |selector, _| selector.is_open()),
        )
    });
    assert_eq!(
        project, project_open,
        "project mounted state must match entity state"
    );
    assert_eq!(
        branch, branch_open,
        "branch mounted state must match entity state"
    );
}
