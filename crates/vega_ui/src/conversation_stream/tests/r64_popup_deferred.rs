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
//! Issue #58 explicitly adds a fourth permission row: that picker keeps its
//! recorded left edge, width and bottom anchor, growing upward by one row.
//!
//! ## Issue #100 re-baseline (2026-09-21)
//!
//! Issue #100 widened the Composer column from 736 to 768 to match the body
//! column. Every layer here is anchored to that column, so all four shifted by
//! exactly **16px** — half of the 32px the column grew — in the direction each
//! layer hangs: left-anchored layers move −16, right-anchored layers +16. The
//! heights, widths and the vertical geometry are unchanged. These are the
//! intentional consequence of #100, not a `deferred` regression (same rationale
//! as the R68 R13 note below): the contract is "`deferred` does not move
//! geometry", not "geometry is frozen forever".
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
///
/// `pub(super)`: the R68 suite mounts the same utility bar and reuses the
/// helpers below rather than growing a fourth copy of the fixture.
pub(super) const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store with the same project rows the sidebar renders, plus the
/// selection global the composer's project context resolves through.
pub(super) fn install_utility_globals(
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

pub(super) fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

pub(super) fn click(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) {
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

/// R64 A1 / I58-R7: preserve the original left edge, width and bottom anchor.
/// The fourth row adds 48.5px: height = 188 + 48.5 = 236.5, so the top moves
/// from 826.5 to 778 while bottom = 826.5 + 188 = 1014.5 stays fixed.
#[gpui_kit::test]
async fn r64_permission_picker_bounds_match_the_baseline(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r64-a1-permission");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    click(window, "composer-permission-status", cx);
    let picker = bounds(window, "composer-permission-picker", cx);
    assert_eq!(
        (
            f32::from(picker.left()),
            f32::from(picker.size.width),
            f32::from(picker.bottom())
        ),
        // Issue #100: the left edge moved 445.0 -> 429.0 because the Composer
        // column widened 736 -> 768 (left-anchored layers move by -16, half of
        // 32). The width and bottom anchor are unchanged.
        (429.0, 350.0, 1014.5),
        "I58 must retain the R64 width and bottom anchor (left edge re-baselined by issue #100)"
    );
    let added_row = bounds(window, "composer-permission-option-full_access", cx);
    assert_eq!(f32::from(added_row.size.height), 48.5);
    assert_eq!(
        (f32::from(picker.top()), f32::from(picker.size.height)),
        (778.0, 236.5),
        "I58 must grow upward by exactly the fourth row's 48.5px"
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
        // Issue #100: right-anchored to the model trigger, which moved +16 with
        // the 736 -> 768 column widening.
        (848.5, 905.0, 254.5, 109.0),
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
        // Issue #100: right-anchored to the model trigger (+16).
        (753.0, 694.0, 350.0, 320.0),
        "the model list layer",
        "composer-model-menu",
    );
}

/// R64 A1 / R49 §2.5 / R62 R10 / **R68 R12/R13**: the utility-bar project menu
/// keeps its `deferred`-era geometry except for the one width R68 changes on
/// purpose.
///
/// The **width** is now [`Layout::MENU_MAX_WIDTH`] (350), not the chip's. R64's
/// baseline recorded 40 here because the menu carried `.w(350).max_w_full()`
/// and its containing block was the 40px-wide folder chip, so `max_w_full` was
/// what bound; natively that clamped the popup to ~183px and truncated every
/// project name (R68 §4). R68 R13 removes the clamp, so **this `40 → 350` is
/// R68's intentional change, not a `deferred` regression**: R64 A1's contract
/// is "`deferred` does not move geometry", not "geometry is frozen forever"
/// (R68 R12 says exactly this). Every other component — x, y, height — is
/// unchanged, which is what still makes this test a `deferred` guard: the
/// upward anchoring and the R64 wrap are untouched (R68 R15).
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
        // Issue #100: the utility chip is left-anchored in the Composer column,
        // so it moved -16 with the 736 -> 768 widening.
        (417.5, 711.5, Layout::MENU_MAX_WIDTH, 215.0),
        "the utility-bar project menu (width updated by R68 R13)",
        "composer-utility-project-menu",
    );
}
