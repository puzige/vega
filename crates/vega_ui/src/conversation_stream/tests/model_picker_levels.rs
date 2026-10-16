//! Two-level model picker: one state machine, one level on screen, anchored to
//! the model trigger.
//!
//! Contract sources: `docs/vega-r59-two-level-model-picker.md` §3 (the
//! two-level drill-down) and `docs/vega-r61-picker-trigger-anchoring.md` §3
//! (the trigger anchoring that replaced R59's composer-column anchoring).
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame — no test-only seam decides any branch, and no synthetic
//! keyboard event is used to prove focus (AGENTS.md: GPUI focus cannot be
//! driven that way; the structural assertions below are the authority).
//!
//! The defects these tests pin:
//! - R59 D1 the slider card floated inside the model list;
//! - R59 D2 the unbounded menu covered the R49 utility bar;
//! - R59 D3 the card's rows collided;
//! - R61 D1 the R59 column anchoring left the card a whole utility-bar height
//!   away from the model button;
//! - R61 D2 the same anchoring right-aligned the card to the 768px container
//!   edge instead of to the trigger.
//!
//! R61 accepts that a trigger-anchored card overlaps the R49 utility bar: the
//! trigger has ~61px of clearance and the R57-frozen card needs 84px, so the
//! overlap is geometric (R61 §1). The tests below assert the **hug** instead of
//! the clearance, and pin the accepted overlap so re-introducing the column
//! anchoring has to re-open the decision (R61 R5).

use super::*;
use gpui_kit::{Bounds, Modifiers, Pixels, VisualTestContext};

/// The `permission_thread()` fixture's durable project binding. The utility
/// bar's visibility predicate requires the shared selection to match it.
const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store plus the selection global the composer's project context
/// resolves through, so the R49 utility bar is genuinely mounted.
fn install_utility_globals(cx: &mut TestAppContext, selected: Option<&str>) {
    let store = vega_store::Store::open(":memory:").expect("owned picker store");
    store.migrate().expect("owned picker migrations");
    for (path, name) in [("/tmp/r59-alpha", "alpha"), ("/tmp/r59-beta", "beta")] {
        vega_store::projects::create(store.conn(), path, name, None).expect("owned picker project");
    }
    cx.update(|cx| {
        cx.set_global(crate::sidebar::VegaStore(Ok(store)));
        cx.set_global(crate::sidebar::SelectedProject(selected.map(str::to_owned)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
}

/// Opens a stream whose catalog and capability projection are already
/// installed, so the picker can be opened through its production trigger.
fn open_picker_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
    models: Vec<String>,
    efforts: Vec<String>,
    with_utility_bar: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _events) = open_controller_stream(cx, thread_id);
    install_utility_globals(cx, with_utility_bar.then_some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(models, cx);
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.efforts = efforts;
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();
    (window, stream)
}

fn twenty_models() -> Vec<String> {
    (0..20).map(|index| format!("model-{index:02}")).collect()
}

fn three_tiers() -> Vec<String> {
    ["low", "medium", "high"]
        .iter()
        .map(|tier| (*tier).to_string())
        .collect()
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Option<Bounds<Pixels>> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).debug_bounds(selector)
}

fn is_mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    mounted(window, selector, cx).is_some()
}

fn level(stream: &Entity<ConversationStream>, cx: &mut TestAppContext) -> ModelPickerLevel {
    stream.read_with(cx, |stream, _| stream.model_picker_level)
}

// ---- R1/A1: the model button shows only the slider ------------------------

// ---- R2/A2: the slider title opens the model list -------------------------

// ---- R3/A3: the two levels are mutually exclusive -------------------------

// ---- R4: two independent layers, never card-inside-card -------------------

/// Compares two rendered coordinates with the sub-pixel tolerance the layout
/// engine's rounding needs.
fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: actual {actual} vs expected {expected}"
    );
}

// ---- R61 A1: the card hugs the trigger (and overlaps the utility bar) -----

// ---- R6/D3: no row overlap inside the card --------------------------------

// ---- R7: the trigger label while adjusting a tier -------------------------

// ---- R3/A4: what a model pick does ---------------------------------------
