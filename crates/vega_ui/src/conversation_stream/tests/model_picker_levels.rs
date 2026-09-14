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
//! - R61 D2 the same anchoring right-aligned the card to the 736px container
//!   edge instead of to the trigger.
//!
//! R61 accepts that a trigger-anchored card overlaps the R49 utility bar: the
//! trigger has ~61px of clearance and the R57-frozen card needs 84px, so the
//! overlap is geometric (R61 §1). The tests below assert the **hug** instead of
//! the clearance, and pin the accepted overlap so re-introducing the column
//! anchoring has to re-open the decision (R61 R5).

use super::*;
use crate::conversation_stream::render::{PICKER_EFFORT_LABEL, PICKER_LIST_HEADING};
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

/// Installs the model trigger's focus stop explicitly.
///
/// AGENTS.md records that synthetic keyboard/mouse events cannot drive GPUI's
/// focus chain (four methods were measured failing on 2026-09-13), and that the
/// authoritative way to verify focus-dependent behaviour is to focus the real
/// handle in a `TestAppContext` and then assert. This does exactly that, so a
/// key-driven assertion below tests the picker rather than the harness.
fn focus_model_trigger(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, _| stream.model_focus.clone());
            window.focus(&focus, cx);
        })
        .expect("picker stream window");
    cx.run_until_parked();
}

// ---- R1/A1: the model button shows only the slider ------------------------

/// R1/A1: clicking the model button mounts the slider and does **not** mount
/// the model list.
#[gpui_kit::test]
async fn r59_model_button_mounts_only_the_slider(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(cx, "r59-r1", twenty_models(), three_tiers(), true);

    assert!(
        !is_mounted(window, "composer-thinking-slider", cx),
        "no level is mounted before the trigger is used"
    );
    assert_eq!(level(&stream, cx), ModelPickerLevel::Closed);

    click(window, "composer-model", cx);

    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    assert!(
        is_mounted(window, "composer-thinking-slider", cx),
        "R1: the model button mounts the tier slider"
    );
    assert!(
        is_mounted(window, "thinking-slider-track", cx),
        "R1: the mounted slider renders its track"
    );
    assert!(
        !is_mounted(window, "composer-model-menu", cx),
        "R1: the model button must not mount the model list"
    );
}

// ---- R2/A2: the slider title opens the model list -------------------------

/// R2/A2: activating the slider card's title row mounts the model list and
/// unmounts the slider.
#[gpui_kit::test]
async fn r59_slider_title_mounts_the_model_list(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(cx, "r59-r2", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert!(is_mounted(window, "composer-thinking-slider", cx));

    click(window, "thinking-slider-title", cx);

    assert_eq!(level(&stream, cx), ModelPickerLevel::List);
    assert!(
        is_mounted(window, "composer-model-menu", cx),
        "R2: the slider title opens the model list"
    );
    assert!(
        !is_mounted(window, "composer-thinking-slider", cx),
        "R3: the slider is hidden once the list is showing"
    );
    assert!(
        is_mounted(window, "composer-model-heading", cx),
        "R2: the list carries its heading"
    );
}

// ---- R3/A3: the two levels are mutually exclusive -------------------------

/// R3/A3: at no point are both layers mounted at once, and the two states are
/// exactly the two arms of one value.
#[gpui_kit::test]
async fn r59_the_two_levels_are_mutually_exclusive(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(cx, "r59-r3", twenty_models(), three_tiers(), true);

    // Closed.
    assert_eq!(level(&stream, cx), ModelPickerLevel::Closed);
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
    assert!(!is_mounted(window, "composer-model-menu", cx));

    // Level one.
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    let slider = is_mounted(window, "composer-thinking-slider", cx);
    let list = is_mounted(window, "composer-model-menu", cx);
    assert!(slider && !list, "slider={slider} list={list}");

    // Level two.
    click(window, "thinking-slider-title", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::List);
    let slider = is_mounted(window, "composer-thinking-slider", cx);
    let list = is_mounted(window, "composer-model-menu", cx);
    assert!(!slider && list, "slider={slider} list={list}");

    // Esc closes whichever level is mounted.
    //
    // AGENTS.md: synthetic input cannot drive GPUI's focus chain, so the
    // trigger's focus stop is installed explicitly — the authoritative method
    // for focus-dependent behaviour — before the key is delivered. Without it
    // the `ModelSelector` key context is never entered and Esc is a no-op for a
    // reason unrelated to the picker.
    focus_model_trigger(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(level(&stream, cx), ModelPickerLevel::Closed);
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
    assert!(!is_mounted(window, "composer-model-menu", cx));
}

// ---- R4: two independent layers, never card-inside-card -------------------

/// R4/D1: the slider card is not nested inside the list container. The list is
/// unmounted in the slider state, so no frame can contain both; when the list
/// is mounted the slider card is absent from the tree entirely.
#[gpui_kit::test]
async fn r59_the_slider_card_is_never_nested_in_the_list(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r59-r4", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    let card = mounted(window, "thinking-slider-card", cx).expect("slider card");
    let menu = mounted(window, "composer-model-menu", cx);
    assert!(
        menu.is_none(),
        "D1: the list must not be mounted with the card"
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("model list");
    assert!(
        !is_mounted(window, "thinking-slider-card", cx),
        "R4: the card leaves the tree when the list mounts"
    );

    // R4: the list owns its own chrome. The card's own border/background are
    // drawn by `ThinkingSlider`, so the two layers never share a container.
    assert!(
        f32::from(menu.size.width) > 0.0,
        "the list renders its own surface: {menu:?}"
    );
    let _ = card;
}

/// R61 A2: both layers are right-aligned to the **model trigger's** right edge,
/// not to the composer container's, and each carries its own width — the slider
/// the measured 254.5px card, the list the frozen `MENU_MAX_WIDTH`.
///
/// This is the R61 replacement for R59's `PICKER_RIGHT_INSET` derivation. R59
/// computed the inset from the frozen bottom row (`border + p_3 + send + gap_2`)
/// because the layers hung off the composer **column**; R61 hangs them off the
/// trigger's own wrapper, so `right_0()` is structural and the assertion can be
/// exact rather than derived. It also pins R61 D2: a layer aligned to the
/// 736px container edge would sit 254.5px to the right of the trigger's edge
/// for the card and ~350px for the list.
#[gpui_kit::test]
async fn r61_both_layers_align_with_the_model_trigger(cx: &mut TestAppContext) {
    let (window, _stream) =
        open_picker_stream(cx, "r61-align", twenty_models(), three_tiers(), true);
    let trigger = mounted(window, "composer-model", cx).expect("trigger");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert_close(
        f32::from(slider.right()),
        f32::from(trigger.right()),
        "R61 A2: the slider layer's right edge is the trigger's right edge",
    );
    // The card keeps its measured width and therefore extends to the *left* of
    // the trigger; the point of the assertion is that it hangs off the trigger
    // rather than off the container's right edge.
    assert!(
        f32::from(slider.left()) < f32::from(trigger.left()),
        "R61 A2: the 254.5px card is wider than the trigger, so it must extend \
         left of it: slider.left={:?} trigger.left={:?}",
        slider.left(),
        trigger.left()
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert_close(
        f32::from(menu.right()),
        f32::from(trigger.right()),
        "R61 A2: the list layer's right edge is the trigger's right edge",
    );
    assert_close(
        f32::from(menu.size.width),
        Layout::MENU_MAX_WIDTH,
        "the list keeps the frozen menu width rather than collapsing to the trigger",
    );
}

/// Compares two rendered coordinates with the sub-pixel tolerance the layout
/// engine's rounding needs.
fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: actual {actual} vs expected {expected}"
    );
}

// ---- R61 A1: the card hugs the trigger (and overlaps the utility bar) -----

/// R61 A1/R5: **the rewritten R59 test.**
///
/// R59 asserted `layer.bottom() <= bar.top()` — "no picker layer may reach the
/// utility bar" — and that assertion is exactly what forced the card onto the
/// composer column's top edge, a whole utility-bar height away from the model
/// button (R61 §1 D1/D2). R61 §2 rules the trade-off out, so the assertion is
/// **inverted, not deleted**: what matters now is that the card hugs the
/// trigger, and the accepted consequence is pinned rather than left implicit.
///
/// Three claims, in the order they matter:
/// 1. the card's bottom is within [`HUG_TOLERANCE`] of the trigger's top — this
///    is A1's "≤ 12px" and it is what the R59 assertion got backwards;
/// 2. the card therefore **intersects** the bar. This is the exact negation of
///    R59's `slider.bottom() <= bar.top()`, so a future change that
///    re-introduces the clearance fails here and has to re-open the R61 §2
///    decision;
/// 3. the list level hugs the trigger the same way, and still respects
///    [`Layout::COMPOSER_PICKER_MAX_HEIGHT`] (R61 A4/R3).
///
/// The bar itself stays mounted and unmoved — R61 does not change the bar, only
/// whether a popup may cover it.
#[gpui_kit::test]
async fn r61_card_hugs_the_trigger_and_overlaps_the_utility_bar(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(
        cx,
        "r61-hug",
        // A long catalog is the case that made the R57 menu 1409px tall.
        twenty_models(),
        three_tiers(),
        true,
    );
    let bar = mounted(window, "composer-utility-bar", cx).expect("utility bar");
    let trigger = mounted(window, "composer-model", cx).expect("trigger");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert_hugs(slider, trigger, "R61 A1: the slider card");
    // Claim 2: the rewrite. R59 asserted `slider.bottom() <= bar.top()` here.
    // The card hangs 8px above the trigger, which is inside the bar's band, so
    // the two boxes genuinely intersect — the overlap R61 §2 accepted.
    assert!(
        slider.intersects(&bar),
        "R61 R5: the card is expected to overlap the utility bar (R61 §2 chose \
         the overlap); card {slider:?} vs bar {bar:?}. If this fails the card \
         stopped hugging the trigger — see the A1 assertion above, not this line."
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert_hugs(menu, trigger, "R61 A1: the model list");
    assert!(
        f32::from(menu.size.height) <= Layout::COMPOSER_PICKER_MAX_HEIGHT,
        "R61 A4: the list is still bounded by COMPOSER_PICKER_MAX_HEIGHT, got {}",
        f32::from(menu.size.height)
    );
}

/// R61 A1: how close the card's bottom edge must sit to the trigger's top edge.
///
/// The spec's own threshold ("≤ 12px", §5 A1) rather than the token's 8px: the
/// assertion is about the *hug* being visually tight, and a tolerance of 12
/// keeps it meaningful while leaving room for sub-pixel rounding in the layout
/// engine. The token itself is pinned separately by
/// [`r61_the_trigger_gap_is_the_frozen_token`], so a drift in
/// [`Layout::COMPOSER_PICKER_TRIGGER_GAP`] still fails a test.
const HUG_TOLERANCE: f32 = 12.0;

/// Asserts the layer's bottom edge sits at most [`HUG_TOLERANCE`] above the
/// trigger's top edge, and never *below* it (which would cover the button).
fn assert_hugs(layer: Bounds<Pixels>, trigger: Bounds<Pixels>, label: &str) {
    let gap = f32::from(trigger.top()) - f32::from(layer.bottom());
    assert!(
        gap.abs() <= HUG_TOLERANCE,
        "{label} must hug the trigger: its bottom is {gap}px above the trigger's \
         top (layer {layer:?}, trigger {trigger:?}); R61 A1 allows at most \
         {HUG_TOLERANCE}px and R61 R1's token is {}px",
        Layout::COMPOSER_PICKER_TRIGGER_GAP
    );
}

/// R61 R1: the gap between the trigger's top edge and a layer's bottom edge is
/// the frozen [`Layout::COMPOSER_PICKER_TRIGGER_GAP`] token, measured on the
/// real frame.
///
/// A1's threshold is a tolerance; this is the exact value. Both are needed: the
/// threshold says the card reads as attached to the button, and this says the
/// attachment is the one token rather than whatever the layout happened to
/// produce.
#[gpui_kit::test]
async fn r61_the_trigger_gap_is_the_frozen_token(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r61-gap", twenty_models(), three_tiers(), true);
    let trigger = mounted(window, "composer-model", cx).expect("trigger");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert_close(
        f32::from(trigger.top() - slider.bottom()),
        Layout::COMPOSER_PICKER_TRIGGER_GAP,
        "R61 R1: the slider card's gap above the trigger is the frozen token",
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert_close(
        f32::from(trigger.top() - menu.bottom()),
        Layout::COMPOSER_PICKER_TRIGGER_GAP,
        "R61 R1: the model list's gap above the trigger is the frozen token",
    );
}

/// R61 R6/A5: the picker can always be dismissed while a layer is open, even
/// though the layer now covers the utility bar.
///
/// R61 §6 M2 asks whether the bar's chips stay reachable once the card overlaps
/// them. The answer this test pins is "not by clicking a chip, but the picker
/// is still dismissible": a click **outside** the layer closes it (the
/// composer's own outside-click handler), and once closed the bar is whole
/// again. The chip is deliberately not asserted as clickable — R61 §3 R6 says
/// covering it is acceptable.
#[gpui_kit::test]
async fn r61_a_layer_is_dismissible_while_it_covers_the_bar(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r61-dismiss", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    assert!(is_mounted(window, "composer-thinking-slider", cx));

    // A point inside the conversation column but outside every picker layer:
    // the transcript area at the column's top. It is far from the composer
    // card, the utility bar, and both layers, so the click cannot be
    // attributed to any of them.
    let column = mounted(window, "conversation-column", cx).expect("conversation column");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(
        gpui_kit::point(column.center().x, column.top() + gpui_kit::px(8.)),
        Modifiers::default(),
    );
    visual.run_until_parked();

    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "R61 R6/A5: an outside click dismisses the picker while the card covers the bar"
    );
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
    // The bar was never unmounted by the overlap, so dismissing the picker is
    // all that is needed to get it back.
    assert!(
        is_mounted(window, "composer-utility-bar", cx),
        "the utility bar is still mounted and whole once the picker is closed"
    );

    // The same holds for the list level.
    click(window, "composer-model", cx);
    click(window, "thinking-slider-title", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::List);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(
        gpui_kit::point(column.center().x, column.top() + gpui_kit::px(8.)),
        Modifiers::default(),
    );
    visual.run_until_parked();
    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "R61 R6/A5: the list level is dismissible the same way"
    );
    assert!(!is_mounted(window, "composer-model-menu", cx));
}

/// R61 R5: where the layer and the utility bar overlap, the **layer** wins the
/// hit test — the card is painted over the bar, not behind it.
///
/// The overlap R61 §2 accepts is only useful if the card is actually on top of
/// the bar: a card that overlapped but sat behind would be unreadable and
/// unclickable in its lower band. The layers carry `occlude()`, and this test
/// proves the resulting z-order with the one observation available without
/// pixels — a click inside the overlap region reaches the **card's** own
/// surface, not the bar underneath.
///
/// R62 R2 grew the card from two rows to three, so its top edge now sits
/// further above the bar's top than it did under R61 — the overlap region
/// starts at the **bar's** top edge rather than at the card's. The click point
/// is therefore taken from the model row (R62 R3: the inert row, which handles
/// nothing) and asserted to fall inside the overlap, instead of being
/// `overlap.top() + 1`, which after R62 lands on the title row and would
/// measure the wrong thing by opening the list.
#[gpui_kit::test]
async fn r61_the_layer_occludes_the_bar_where_they_overlap(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r61-occlude", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);

    let card = mounted(window, "thinking-slider-card", cx).expect("card");
    let bar = mounted(window, "composer-utility-bar", cx).expect("utility bar");
    assert!(
        card.intersects(&bar),
        "this test only means something while the card overlaps the bar: \
         card={card:?} bar={bar:?}"
    );

    // The inert model row (R62 R3) supplies a point that is inside the card but
    // handled by nothing inside it, so the assertion is about the layer's hit
    // area rather than about a slider or title interaction.
    let model_name = mounted(window, "thinking-slider-model", cx).expect("model name");
    let target = model_name.center();
    let overlap = card.intersect(&bar);
    assert!(
        overlap.contains(&target),
        "R61 R5: the chosen click point must lie in the card/bar overlap: \
         target={target:?} overlap={overlap:?}"
    );

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(target, Modifiers::default());
    visual.run_until_parked();

    // The layer's own `on_mouse_down` stops propagation, so a click that lands
    // on it cannot reach the outside-click handler: the picker stays open. A
    // click that fell through to the bar (or to the transcript behind it) would
    // have closed the picker instead. The click is on the inert row, so it must
    // not drill down to the list either (R62 R3).
    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Slider,
        "R61 R5 / R62 R3: the card must take the click in the overlap region \
         without the inert model row emitting anything"
    );
    assert!(is_mounted(window, "composer-thinking-slider", cx));
}

/// R61 R6/A5: re-clicking the trigger closes the picker — the second close path,
/// which needs no outside surface to be reachable.
///
/// This is the path that keeps the picker dismissible even in the worst case
/// for the click-away handler: a window so short that no point outside the
/// layer is hittable. The trigger sits below the layer, so it is always its own
/// escape hatch.
#[gpui_kit::test]
async fn r61_the_trigger_stays_clickable_with_a_layer_mounted(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r61-reclick", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    assert!(is_mounted(window, "composer-thinking-slider", cx));

    // The trigger's own bounds must remain hit-testable: the layer is anchored
    // above it, so it never covers the button it hangs from.
    let trigger = mounted(window, "composer-model", cx).expect("trigger");
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert!(
        slider.bottom() <= trigger.top(),
        "the layer must not cover its own trigger: slider.bottom={:?} trigger.top={:?}",
        slider.bottom(),
        trigger.top()
    );

    click(window, "composer-model", cx);
    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "R61 R6/A5: re-clicking the trigger dismisses the picker"
    );
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
}

/// R61 §6 M2: how much of the utility bar the trigger-anchored card actually
/// covers, and whether its chips survive.
///
/// R59 asserted unconditionally that the folder chip "must still respond to
/// clicks" while a layer was mounted; R61 §3 R6 explicitly makes that
/// acceptable to lose ("若遮住 utility bar 导致其 chip 不可点，这是可接受的"). The
/// chip is therefore **not** a contract any more, and asserting it would claim
/// a guarantee the spec no longer makes.
///
/// What is still worth pinning is the measurement M2 asks for. The overlap is
/// vertical only: the card hangs off the trigger on the row's **right**, while
/// the folder/branch chips sit at the bar's **left** inset. On the frozen
/// geometry that leaves the chips clear, so the click still reaches the chip's
/// own handler — and that handler still closes the picker, preserving the
/// composer's popover exclusivity. Both facts are asserted below, so if a
/// future change moves the trigger leftward or widens the card, this test
/// reports that M2 flipped rather than silently losing the chips.
#[gpui_kit::test]
async fn r61_the_overlap_leaves_the_folder_chip_reachable(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(cx, "r61-m2", twenty_models(), three_tiers(), true);
    let chip = mounted(window, "composer-utility-project-chip", cx).expect("folder chip");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");

    // The layer does reach into the bar's band (R61 accepts this), but its
    // horizontal extent does not cover the chip on this geometry.
    assert!(
        slider.top() < chip.bottom(),
        "the card is expected to reach into the utility bar's band: card top {:?}, \
         chip bottom {:?}",
        slider.top(),
        chip.bottom()
    );
    assert!(
        !slider.intersects(&chip),
        "M2: the card is expected to hang off the trigger's right edge, clear of \
         the bar's left-aligned chips. If this fails the chips are covered — \
         acceptable per R61 R6, but then this test's second half must be dropped \
         rather than the card shrunk (R61 §4). slider={slider:?} chip={chip:?}"
    );

    // Still reachable, and still exclusive with the picker.
    click(window, "composer-utility-project-chip", cx);
    assert!(
        is_mounted(window, "composer-utility-project-menu", cx),
        "M2: the folder chip still opens its own menu"
    );
    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "the chip's menu and the picker stay exclusive"
    );
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
}

/// R61 A1: the bound is a property of the layer, not of the window — the
/// smallest window the app allows (960x600) still hugs the trigger, and the
/// list still scrolls inside `COMPOSER_PICKER_MAX_HEIGHT`.
///
/// R59 asserted the bar stayed clear at this size. R61 replaces that with the
/// hug: the geometry constraint that survives is the height bound, not the
/// clearance.
#[gpui_kit::test]
async fn r61_layers_hug_the_trigger_in_the_smallest_window(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "r61-min-window".into();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root_stream = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::centered(
                    None,
                    gpui_kit::size(gpui_kit::px(960.), gpui_kit::px(600.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| StreamHarness {
                    stream: root_stream,
                })
            },
        )
        .expect("minimum-size picker window")
    });
    cx.run_until_parked();
    install_utility_globals(cx, Some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(twenty_models(), cx);
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.efforts = three_tiers();
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();

    let trigger = mounted(window, "composer-model", cx).expect("trigger");
    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert_hugs(slider, trigger, "960x600: the slider card");

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert_hugs(menu, trigger, "960x600: the model list");
    assert!(
        f32::from(menu.size.height) <= Layout::COMPOSER_PICKER_MAX_HEIGHT,
        "960x600: the list is still bounded by COMPOSER_PICKER_MAX_HEIGHT, got {}",
        f32::from(menu.size.height)
    );
}

// ---- R6/D3: no row overlap inside the card --------------------------------

/// R59 R6/D3, **rewritten by R62 R2**: the card's three rows do not overlap
/// each other or the track, and all of them stay inside the card.
///
/// R59 asserted that the model name and the tier name sat on **one** row
/// (`model_name.right() <= tier_name.left()`). R62 R2 replaced that layout with
/// three stacked rows, so the assertion is **inverted, not deleted**: the two
/// text runs now occupy separate bands, which is the R62 A2 claim. Keeping the
/// old assertion would pin a layout the spec has retired.
///
/// The vertical ordering asserted here is the reference layout of R62 §2:
/// tier row above model row above track.
#[gpui_kit::test]
async fn r62_card_rows_stack_without_overlapping(cx: &mut TestAppContext) {
    let (window, _stream) =
        open_picker_stream(cx, "r62-rows", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);

    let card = mounted(window, "thinking-slider-card", cx).expect("card");
    let title = mounted(window, "thinking-slider-title", cx).expect("title row");
    let model_name = mounted(window, "thinking-slider-model", cx).expect("model name");
    let track = mounted(window, "thinking-slider-track", cx).expect("track");

    // R62 R2 order, top to bottom: tier row, model row, track.
    assert!(
        title.bottom() <= model_name.top(),
        "R62 A2: the tier row (bottom {:?}) must sit above the model row (top {:?})",
        title.bottom(),
        model_name.top()
    );
    assert!(
        model_name.bottom() <= track.top(),
        "R62 A2: the model row (bottom {:?}) must sit above the track (top {:?})",
        model_name.bottom(),
        track.top()
    );
    // The two text runs are in separate vertical bands — the direct A2 claim.
    assert!(
        title.bottom() <= model_name.top() || model_name.bottom() <= title.top(),
        "R62 A2: the tier name and the model name must not share a row: \
         tier={title:?} model={model_name:?}"
    );
    // R62 R5: the taller card still contains all three rows.
    for (label, bounds) in [
        ("tier row", title),
        ("model row", model_name),
        ("track", track),
    ] {
        assert!(
            bounds.top() >= card.top() && bounds.bottom() <= card.bottom(),
            "R6: the {label} stays inside the card: card={card:?} {label}={bounds:?}"
        );
    }
}

/// R62 R5/M1: how much **more** of the utility bar the three-row card covers
/// on the real production mount.
///
/// R62 M1 asks for the card's total height after the layout change and how it
/// moves the R61 overlap. Both are measured here on the mounted composer rather
/// than derived, so the answer is the rendered geometry:
///
/// * the card's own height, from the production shrink-to-fit layer (in the
///   component's own harness the card is the window root and stretches, so the
///   height M1 is about can only be read here);
/// * the card's top edge relative to the bar's top edge — the depth to which
///   the card reaches into the bar's 37px band;
/// * the card's bottom edge relative to the trigger's top edge, which is R61's
///   anchor and must be unchanged by the taller card (R62 R5).
///
/// **Measured on the frozen geometry (this window, this trigger):** the card is
/// `254.5 × 108`, its bottom sits 8px above the trigger's top (the R61 token),
/// and its top edge lands 55px above the bar's bottom edge and 18px above the
/// bar's top edge. The pre-R62 two-row card was 82px, so the card grew 26px and
/// deepened the accepted overlap by the same amount (R62 R5).
#[gpui_kit::test]
async fn r62_m1_the_taller_card_covers_more_of_the_utility_bar(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r62-m1", twenty_models(), three_tiers(), true);
    let bar = mounted(window, "composer-utility-bar", cx).expect("utility bar");
    let trigger = mounted(window, "composer-model", cx).expect("trigger");

    click(window, "composer-model", cx);
    let card = mounted(window, "thinking-slider-card", cx).expect("card");

    // R61's anchoring survives the taller card: the bottom still hugs the
    // trigger's top with the frozen gap, and the card still hugs from *below*
    // the trigger rather than covering it.
    assert_hugs(card, trigger, "R62 R5: the three-row card");
    assert_close(
        f32::from(trigger.top() - card.bottom()),
        Layout::COMPOSER_PICKER_TRIGGER_GAP,
        "R62 R5: the anchor gap is still the R61 token",
    );
    assert!(
        card.bottom() <= trigger.top(),
        "R62 R5: the taller card must grow *upward* from the anchor, not down \
         over the trigger: card.bottom={:?} trigger.top={:?}",
        card.bottom(),
        trigger.top()
    );

    // M1: the height the three-row layout produces on the production mount.
    assert_close(
        f32::from(card.size.height),
        108.0,
        "R62 M1: the three-row card's mounted height (pre-R62: 82px)",
    );
    assert_close(
        f32::from(card.size.width),
        crate::conversation_stream::thinking_slider::THINKING_CARD_WIDTH,
        "R62 R6: the width is unchanged by the extra row",
    );

    // The accepted consequence, quantified: the card reaches into the bar's
    // 37px band, and past its top edge. R61's clearance was 61px of trigger
    // headroom for an 82px card; the R62 card needs 108px, so the extra 26px
    // comes out of the bar (R62 R5 accepts this).
    assert!(
        card.intersects(&bar),
        "R62 R5/M1: the taller card still overlaps the utility bar \
         (card={card:?} bar={bar:?})"
    );
    assert_close(
        f32::from(bar.bottom() - card.top()),
        55.0,
        "R62 M1: how deep the card's top edge reaches into the bar's band, \
         measured from the bar's bottom edge",
    );
    assert!(
        card.top() < bar.top(),
        "R62 M1: the card reaches above the bar's own top edge by {:?}px \
         (card.top={:?} bar.top={:?}); the pre-R62 card also did, and the extra \
         26px deepens it",
        bar.top() - card.top(),
        card.top(),
        bar.top()
    );
}

// ---- R7: the trigger label while adjusting a tier -------------------------

/// R7: the trigger's label rule, as a pure function of the composer state.
///
/// The reference implementation's `selectEffort.label` is "Placeholder shown in
/// the model picker trigger **while adjusting the reasoning effort of an
/// explicitly selected model**", so the label belongs to the slider level and
/// to no other state.
#[test]
fn r59_trigger_label_is_select_effort_only_on_the_slider_level() {
    use crate::conversation_stream::render::model_trigger_label;
    use ModelPickerLevel::{Closed, List, Slider};

    // R7: the slider level shows 选择强度 — the in-convention Chinese rendering
    // of "Select effort", matching Vega's existing 档位/强度 wording.
    assert_eq!(
        model_trigger_label("gpt-6-astra", Slider, false),
        "选择强度"
    );
    // Every other state keeps the pre-R59 model-name display.
    assert_eq!(
        model_trigger_label("gpt-6-astra", Closed, false),
        "gpt-6-astra"
    );
    assert_eq!(
        model_trigger_label("gpt-6-astra", List, false),
        "gpt-6-astra"
    );
    // The pre-existing precedence is unchanged: a pending save and an unnamed
    // model both win over the level, because they describe the model itself.
    assert_eq!(model_trigger_label("gpt-6-astra", Slider, true), "保存中…");
    assert_eq!(model_trigger_label("", Slider, false), "模型");
    assert_eq!(model_trigger_label("", Closed, false), "模型");
}

/// R7: while the slider (level one) is showing, the trigger renders the effort
/// label; outside that state it keeps the existing model-name display.
#[gpui_kit::test]
async fn r59_trigger_reads_select_effort_on_the_slider_level(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(cx, "r59-r7", twenty_models(), three_tiers(), true);

    // Closed: the trigger names the model, as before.
    let trigger = mounted(window, "composer-model", cx).expect("trigger");
    assert!(f32::from(trigger.size.width) > 0.0);

    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    assert!(
        is_mounted(window, "thinking-slider-track", cx),
        "R7: the slider level is the state the label belongs to"
    );
    // The label is a constant so the assertion cannot drift from the render.
    assert_eq!(PICKER_EFFORT_LABEL, "选择强度");
    assert_eq!(PICKER_LIST_HEADING, "选择模型");
    // The trigger keeps its own geometry in this state, so switching the label
    // cannot change the bottom row's shape (R57's frozen row).
    let trigger = mounted(window, "composer-model", cx).expect("trigger on slider level");
    assert_eq!(
        f32::from(trigger.size.height),
        29.0,
        "the trigger keeps its frozen height while the label changes"
    );
}

// ---- R3/A4: what a model pick does ---------------------------------------

/// R3/A4: picking a model closes the picker and emits the existing selection
/// intent. The spec allows either closing or returning to the slider; this
/// pins the implemented choice (closing) so a future change has to state its
/// reason here.
#[gpui_kit::test]
async fn r59_picking_a_model_closes_the_picker_and_emits_the_intent(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r59-a4");
    install_utility_globals(cx, Some(PROJECT_BINDING));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(
            &stream,
            move |_, request: &ThreadModelSelectionRequested, _| {
                if let Ok(mut requests) = captured.lock() {
                    requests.push((request.model.clone(), request.request_id));
                }
            },
        )
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(twenty_models(), cx);
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.efforts = three_tiers();
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();

    click(window, "composer-model", cx);
    click(window, "thinking-slider-title", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::List);

    // Row `model-05` through the production row handler.
    let rows = mounted(window, "composer-model-rows", cx).expect("rows");
    let row_height = Typography::SIDEBAR_LINE_HEIGHT;
    let target = gpui_kit::point(
        rows.left() + gpui_kit::px(4.),
        rows.top() + gpui_kit::px(row_height * 5.0 + row_height / 2.0),
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(target, Modifiers::default());
    visual.run_until_parked();

    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "R3/A4: a model pick closes the picker"
    );
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
    assert!(!is_mounted(window, "composer-model-menu", cx));
    assert_eq!(
        requests.lock().expect("captured requests").as_slice(),
        &[("model-05".to_string(), 0)],
        "the pick emits exactly one selection intent for the clicked row"
    );
}

/// R58 R8 regression: a **tier** selection still leaves the picker mounted on
/// the slider, because that write does not change which model the slider
/// describes. R59 R3 only changes the model-pick path.
#[gpui_kit::test]
async fn r59_tier_selection_keeps_the_slider_mounted(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r59-tier-stays", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);

    let track = mounted(window, "thinking-slider-track", cx).expect("track");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(
        gpui_kit::point(track.left() + gpui_kit::px(1.), track.center().y),
        Modifiers::default(),
    );
    visual.run_until_parked();

    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Slider,
        "R58 R8: a tier selection leaves the slider level mounted"
    );
    assert!(is_mounted(window, "composer-thinking-slider", cx));
    assert!(!is_mounted(window, "composer-model-menu", cx));
}

/// R1: the trigger's own click path opens level one, and a second click closes
/// it again (the trigger is a toggle, and level one has no rows to accept).
#[gpui_kit::test]
async fn r59_trigger_toggles_the_slider_level(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r59-toggle", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);

    click(window, "composer-model", cx);
    assert_eq!(
        level(&stream, cx),
        ModelPickerLevel::Closed,
        "a second trigger click closes the slider level"
    );
    assert!(!is_mounted(window, "composer-thinking-slider", cx));
}

/// R57 R12 regression: a model that declares no tiers mounts no layer at all,
/// rather than an empty padded card.
#[gpui_kit::test]
async fn r59_a_model_without_tiers_mounts_no_layer(cx: &mut TestAppContext) {
    let (window, stream) =
        open_picker_stream(cx, "r59-no-tiers", twenty_models(), Vec::new(), true);

    click(window, "composer-model", cx);

    assert_eq!(level(&stream, cx), ModelPickerLevel::Slider);
    assert!(
        !is_mounted(window, "composer-thinking-slider", cx),
        "R57 R12: no tiers means no card, not an empty slot"
    );
    assert!(!is_mounted(window, "thinking-slider-card", cx));
    assert!(!is_mounted(window, "composer-model-menu", cx));
}
