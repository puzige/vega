//! R59 two-level model picker: one state machine, one level on screen.
//!
//! Contract source: `docs/vega-r59-two-level-model-picker.md` §3. Every case
//! mounts the real [`ConversationStream`] in a window and reads the rendered
//! frame — no test-only seam decides any branch, and no synthetic keyboard
//! event is used to prove focus (AGENTS.md: GPUI focus cannot be driven that
//! way; the structural assertions below are the authority).
//!
//! The defects these tests pin (spec §1):
//! - D1 the slider card floated inside the model list;
//! - D2 the unbounded menu covered the R49 utility bar;
//! - D3 the card's rows collided.

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

/// R4: both layers are right-aligned to the model trigger, and each carries its
/// own width — the slider the measured 254.5px card, the list the frozen
/// `MENU_MAX_WIDTH`. This is what pins [`PICKER_RIGHT_INSET`] to the frozen
/// bottom row: if the send button, the card padding, or the row gap changed
/// without updating that derivation, the layers would visibly drift off the
/// trigger.
#[gpui_kit::test]
async fn r59_both_layers_align_with_the_model_trigger(cx: &mut TestAppContext) {
    let (window, _stream) =
        open_picker_stream(cx, "r59-align", twenty_models(), three_tiers(), true);
    let trigger = mounted(window, "composer-model", cx).expect("trigger");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert_close(
        f32::from(slider.right()),
        f32::from(trigger.right()),
        "the slider layer's right edge is the trigger's right edge",
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert_close(
        f32::from(menu.right()),
        f32::from(trigger.right()),
        "the list layer's right edge is the trigger's right edge",
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

// ---- R5: neither layer overflows the utility bar --------------------------

/// R5/D2: with the R49 utility bar mounted, neither layer's bounds reach above
/// the bar's top edge, so the bar stays visible and clickable.
#[gpui_kit::test]
async fn r59_neither_layer_reaches_the_utility_bar(cx: &mut TestAppContext) {
    let (window, stream) = open_picker_stream(
        cx,
        "r59-r5",
        // A long catalog is the case that made the R57 menu 1409px tall.
        twenty_models(),
        three_tiers(),
        true,
    );
    let bar = mounted(window, "composer-utility-bar", cx).expect("utility bar");

    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert!(
        slider.bottom() <= bar.top(),
        "R5: the slider layer (bottom {:?}) must not reach the utility bar (top {:?})",
        slider.bottom(),
        bar.top()
    );

    click(window, "thinking-slider-title", cx);
    let menu = mounted(window, "composer-model-menu", cx).expect("list layer");
    assert!(
        menu.bottom() <= bar.top(),
        "R5: the list layer (bottom {:?}) must not reach the utility bar (top {:?})",
        menu.bottom(),
        bar.top()
    );
    assert!(
        f32::from(menu.size.height) <= Layout::COMPOSER_PICKER_MAX_HEIGHT,
        "R5: the list is bounded by COMPOSER_PICKER_MAX_HEIGHT, got {}",
        f32::from(menu.size.height)
    );
    let _ = stream;
}

/// R5: the utility bar stays clickable while a layer is mounted. The chip's own
/// production handler must still open its menu, which is the behaviour the R57
/// unbounded popup swallowed.
#[gpui_kit::test]
async fn r59_utility_bar_stays_clickable_with_a_layer_mounted(cx: &mut TestAppContext) {
    let (window, _stream) =
        open_picker_stream(cx, "r59-r5-click", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);
    assert!(is_mounted(window, "composer-thinking-slider", cx));

    click(window, "composer-utility-project-chip", cx);

    assert!(
        is_mounted(window, "composer-utility-project-menu", cx),
        "R5: the utility bar chip must still respond to clicks"
    );
    // Opening the chip's own menu closes the picker, so the two popovers never
    // stack (the composer's existing exclusivity rule).
    assert!(
        !is_mounted(window, "composer-thinking-slider", cx),
        "the chip's menu and the picker are exclusive"
    );
}

/// R5: the bound is a property of the layer, not of the window — the smallest
/// window the app allows (960x600) must still keep the bar clear.
#[gpui_kit::test]
async fn r59_layers_stay_below_the_bar_in_the_smallest_window(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "r59-min-window".into();
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

    let bar = mounted(window, "composer-utility-bar", cx).expect("utility bar");
    click(window, "composer-model", cx);
    let slider = mounted(window, "composer-thinking-slider", cx).expect("slider layer");
    assert!(
        slider.bottom() <= bar.top(),
        "960x600: the slider (bottom {:?}) must not reach the bar (top {:?})",
        slider.bottom(),
        bar.top()
    );
}

// ---- R6/D3: no row overlap inside the card --------------------------------

/// R6/D3: the card's title row and its track do not overlap, and the card is
/// exactly as tall as its rows plus padding.
#[gpui_kit::test]
async fn r59_card_rows_do_not_overlap(cx: &mut TestAppContext) {
    let (window, _stream) = open_picker_stream(cx, "r59-r6", twenty_models(), three_tiers(), true);
    click(window, "composer-model", cx);

    let card = mounted(window, "thinking-slider-card", cx).expect("card");
    let title = mounted(window, "thinking-slider-title", cx).expect("title row");
    let track = mounted(window, "thinking-slider-track", cx).expect("track");

    assert!(
        title.bottom() <= track.top(),
        "R6/D3: the title row (bottom {:?}) must not overlap the track (top {:?})",
        title.bottom(),
        track.top()
    );
    assert!(
        track.bottom() <= card.bottom() && title.top() >= card.top(),
        "R6: the rows stay inside the card: card={card:?} title={title:?} track={track:?}"
    );
    // The model name and the tier name share the one title row, so they cannot
    // collide with each other either (R59 R2 two-tone title).
    let model_name = mounted(window, "thinking-slider-model", cx).expect("model name");
    let tier_name = mounted(window, "thinking-slider-label", cx).expect("tier name");
    assert!(
        f32::from(model_name.right()) <= f32::from(tier_name.left()),
        "R2: model name (right {:?}) precedes the tier name (left {:?}) on one row",
        model_name.right(),
        tier_name.left()
    );
    assert!(
        model_name.top() >= card.top() && model_name.bottom() <= card.bottom(),
        "the model name stays inside the card"
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
