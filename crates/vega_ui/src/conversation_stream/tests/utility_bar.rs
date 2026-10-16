//! R49 composer utility bar: one state, one production test.
//!
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame's debug bounds — no test-only seam decides any branch. The
//! project rows come from the same `vega_store::projects` query the sidebar's
//! project block uses.

use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};

/// The `permission_thread()` fixture's durable project binding. The utility
/// bar's visibility predicate requires the shared selection to match it.
const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store with the same project rows the sidebar renders, plus the
/// selection global the composer's project context resolves through.
fn install_utility_globals(
    cx: &mut TestAppContext,
    projects: &[(&str, &str)],
    selected: Option<&str>,
) -> Vec<String> {
    let store = vega_store::Store::open(":memory:").expect("owned utility store");
    store.migrate().expect("owned utility migrations");
    let ids = projects
        .iter()
        .map(|(path, name)| {
            vega_store::projects::create(store.conn(), path, name, None)
                .expect("owned utility project")
                .id
        })
        .collect();
    cx.update(|cx| {
        cx.set_global(crate::sidebar::VegaStore(Ok(store)));
        cx.set_global(crate::sidebar::SelectedProject(selected.map(str::to_owned)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    ids
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let bounds = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: expected {expected}±1px, got {actual}px"
    );
}

/// Reads the actual production scene rather than inferring paint from debug
/// bounds. GPUI stores quad bounds in device pixels, so the helper converts
/// them back to logical pixels using the window's scale factor.
fn painted_quads(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) -> (Vec<Quad>, f32) {
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("utility-bar window must remain open while reading painted quads")
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

fn has_overlay_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    dark: bool,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let expected = if dark {
        vega_theme::DARK.bg_utility_chip_overlay
    } else {
        vega_theme::LIGHT.bg_utility_chip_overlay
    };
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        let Some(solid) = quad.background.as_solid() else {
            return false;
        };
        if !same_colour(solid, expected) {
            return false;
        }
        quad_matches_chip_bounds(quad, target, scale)
            && quad_has_radius(quad, scale, Layout::COMPOSER_UTILITY_CHIP_RADIUS)
    })
}

/// A fill is only the chip's fill when the painted quad is the chip's own
/// bounds.  Ancestor surfaces are intentionally not accepted: a test that
/// only checks containment would let a broad utility-bar background conceal a
/// missing chip overlay.
fn quad_matches_chip_bounds(
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

fn quad_has_radius(quad: &Quad, scale: f32, expected: f32) -> bool {
    let expected = expected * scale;
    let tolerance = 0.75;
    [
        quad.corner_radii.top_left.as_f32(),
        quad.corner_radii.top_right.as_f32(),
        quad.corner_radii.bottom_right.as_f32(),
        quad.corner_radii.bottom_left.as_f32(),
    ]
    .into_iter()
    .all(|actual| (actual - expected).abs() <= tolerance)
}

/// Finds any opaque/solid quad at exactly the chip's bounds.  The rest state
/// must have none, regardless of its color, so replacing the shared overlay
/// with an unrelated theme color cannot evade the state test.
fn has_any_chip_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some() && quad_matches_chip_bounds(quad, target, scale)
    })
}

fn move_pointer(
    window: WindowHandle<StreamHarness>,
    point: gpui_kit::Point<gpui_kit::Pixels>,
    cx: &mut TestAppContext,
) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(point, None, Modifiers::default());
    // The test platform compares the incoming position with the previous
    // event; dispatching the same move twice also lets any hover invalidation
    // scheduled by the first event settle before the scene is read.
    visual.simulate_mouse_move(point, None, Modifiers::default());
    visual.run_until_parked();
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
