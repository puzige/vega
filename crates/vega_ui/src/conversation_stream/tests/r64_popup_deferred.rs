//! R64 §5 A1: `gpui_kit::deferred` must not move or resize a popup.
//!
//! Contract source: `docs/vega-r64-popup-border-clipping.md` §4 (R1/R3) and
//! §5 (A1). The defect is a **paint-order** one: `Style::paint` draws an
//! element's border *after* its children, so the composer card's own 1px
//! `border_subtle` border crossed every popup mounted inside the card.
//! `gpui_kit::deferred(...).with_priority(2)` fixes it by delaying the
//! painting until after the ancestors while keeping layout in the current
//! tree, so the one thing that must not change is the layer's geometry.
//!
//! ## What these tests pin
//!
//! For each of the four layers R64 R1 names, the open layer's `debug_selector`
//! bounds are **pixel-identical** to the pre-change baseline. The constants
//! below were measured on `master @ 0af289c` (the R64 baseline, before the
//! `deferred` wrap) with a temporary dump of the same four selectors on the
//! same harness, and re-measured after the wrap: every value matched exactly.
//! They are recorded as literals rather than derived, because the A1 claim is
//! precisely "these numbers did not change" — a relative assertion would miss
//! a uniform shift of every layer.
//!
//! ## What these tests cannot pin
//!
//! The paint **order** itself (A2). The GPUI test platform has no headless
//! renderer, so `debug_bounds` is the only geometry the harness exposes and no
//! test can observe which quad was drawn last. A2's evidence is the native
//! pixel scan of R64 §5 A3/A4, not anything in this file — see the R64 report.

use super::*;
use gpui_kit::{Bounds, Modifiers, Pixels, VisualTestContext};

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
    store
        .conn()
        .execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) \
             VALUES(?1,'/tmp/r64-bound','bound',0,0)",
            [PROJECT_BINDING],
        )
        .expect("owned bound project");
    let ids = std::iter::once(PROJECT_BINDING.to_string())
        .chain(projects.iter().map(|(path, name)| {
            vega_store::projects::create(store.conn(), path, name, None)
                .expect("owned utility project")
                .id
        }))
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
) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let b = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(b.center(), Modifiers::default());
    visual.run_until_parked();
}

/// R64 A1: a layer's bounds must equal the recorded baseline exactly.
///
/// Exact comparison is the contract: A1 is "逐像素相同", so an off-by-one is a
/// failure, not noise. The four components are compared separately so a
/// failure names which edge moved.
fn assert_baseline(
    actual: Bounds<Pixels>,
    expected: (f32, f32, f32, f32),
    label: &str,
    selector: &str,
) {
    let (x, y, w, h) = (
        f32::from(actual.left()),
        f32::from(actual.top()),
        f32::from(actual.size.width),
        f32::from(actual.size.height),
    );
    let (ex, ey, ew, eh) = expected;
    assert_eq!(
        (x, y, w, h),
        (ex, ey, ew, eh),
        "R64 A1: {label} (`{selector}`) must keep its pre-`deferred` bounds. \
         Expected x={ex} y={ey} w={ew} h={eh} (baseline master @ 0af289c), \
         got x={x} y={y} w={w} h={h}. `deferred` delays painting only; if this \
         moved, the wrap changed layout."
    );
}

fn three_tiers() -> Vec<String> {
    ["low", "medium", "high"]
        .iter()
        .map(|tier| (*tier).to_string())
        .collect()
}

fn twenty_models() -> Vec<String> {
    (0..20).map(|index| format!("model-{index:02}")).collect()
}

/// Opens the model picker's catalog and capability projection through the same
/// production appliers the R59/R61 suites use, so the trigger really opens.
fn open_picker_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _events) = open_controller_stream(cx, thread_id);
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(twenty_models(), cx);
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.efforts = three_tiers();
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();
    (window, stream)
}

/// R64 A1 / R62 R8: the permission picker keeps the geometry it had before the
/// `deferred` wrap — anchored to the chip's top edge with `left_0()` (R61's
/// anchoring technique), 350px wide, 188px tall for the three rows.
#[gpui_kit::test]
async fn r64_permission_picker_bounds_match_the_baseline(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r64-a1-permission");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    click(window, "composer-permission-status", cx);
    assert_baseline(
        bounds(window, "composer-permission-picker", cx),
        (445.0, 826.5, 350.0, 188.0),
        "the permission picker",
        "composer-permission-picker",
    );
}

/// R64 A1 / R59 R1 / R61 R1: the level-one slider card keeps the geometry it
/// had before the `deferred` wrap.
#[gpui_kit::test]
async fn r64_picker_slider_layer_bounds_match_the_baseline(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r64-a1-slider");
    click(window, "composer-model", cx);
    assert_baseline(
        bounds(window, "composer-thinking-slider", cx),
        (832.5, 905.0, 254.5, 109.0),
        "the tier slider layer",
        "composer-thinking-slider",
    );
}

/// R64 A1 / R59 R2 / R61 R1: the level-two model list keeps the geometry it had
/// before the `deferred` wrap, including its `COMPOSER_PICKER_MAX_HEIGHT` bound.
#[gpui_kit::test]
async fn r64_picker_list_layer_bounds_match_the_baseline(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r64-a1-list");
    click(window, "composer-model", cx);
    click(window, "thinking-slider-title", cx);
    assert_baseline(
        bounds(window, "composer-model-menu", cx),
        (737.0, 694.0, 350.0, 320.0),
        "the model list layer",
        "composer-model-menu",
    );
}

/// R64 A1 / R49 §2.5 / R62 R10: the utility-bar project menu keeps the geometry
/// it had before the `deferred` wrap.
///
/// The measured width is the chip wrapper's, not `MENU_MAX_WIDTH`: the menu
/// carries `.w(350).max_w_full()` and its containing block is the 40px-wide
/// folder chip, so `max_w_full` is what binds. R64 R2 forbids touching that
/// anchoring, and this test is where a future change to it would show up.
#[gpui_kit::test]
async fn r64_project_menu_bounds_match_the_baseline(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r64-a1-project");
    install_utility_globals(
        cx,
        &[("/tmp/r64-alpha", "alpha"), ("/tmp/r64-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    click(window, "composer-utility-project-chip", cx);
    assert_baseline(
        bounds(window, "composer-utility-project-menu", cx),
        (433.5, 711.5, 40.0, 215.0),
        "the utility-bar project menu",
        "composer-utility-project-menu",
    );
}
