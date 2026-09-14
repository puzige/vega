//! R62 §7: the bottom row's permission chip opens a three-row picker.
//!
//! Contract source: `docs/vega-r62-slider-card-three-rows.md` §7 (R7/R8/R9)
//! and §9 (A9/A10/A11). Every case mounts the real [`ConversationStream`] in a
//! window and drives the production click paths — no test-only seam decides
//! any branch.
//!
//! The defect these tests pin: R57 P2b made `⚠ 确认` static text on the
//! reasoning that the reference implementation's `Full access` label was not
//! an affordance. The user's screenshot proved otherwise, and this suite
//! exists so the decision cannot silently flip back.

use super::*;
use crate::conversation_stream::render::{PERMISSION_PICKER_LEARN_MORE, PERMISSION_PICKER_TITLE};
use gpui_kit::{Modifiers, VisualTestContext};

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
) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
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

/// A9: clicking `composer-permission-status` opens the picker, the three rows
/// are present in the reference implementation's order, and the current mode
/// carries the checkmark.
#[gpui_kit::test]
async fn r62_permission_chip_opens_the_three_row_picker(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r62-permission-picker");

    // The chip is inert until clicked: no picker in the first frame.
    assert!(
        !mounted(window, "composer-permission-picker", cx),
        "the picker must not be mounted before the chip is clicked"
    );

    click(window, "composer-permission-status", cx);
    assert!(
        mounted(window, "composer-permission-picker", cx),
        "clicking the permission chip must open the picker"
    );

    // All three rows, in `PERMISSION_ORDER` order.
    let readonly = bounds(window, "composer-permission-option-readonly", cx);
    let confirm = bounds(window, "composer-permission-option-confirm", cx);
    let auto = bounds(window, "composer-permission-option-auto", cx);
    assert!(
        f32::from(readonly.top()) < f32::from(confirm.top())
            && f32::from(confirm.top()) < f32::from(auto.top()),
        "the rows must render top-to-bottom as 只读 / 确认 / 自动"
    );

    // The fixture thread is `confirm`, so exactly that row is marked — and the
    // marker is the checkmark element, not merely a colour.
    assert!(
        mounted(window, "composer-permission-option-confirm-check", cx),
        "the current permission mode must carry the checkmark"
    );
    assert!(
        !mounted(window, "composer-permission-option-readonly-check", cx),
        "an unselected row must not carry a checkmark"
    );
    assert!(
        !mounted(window, "composer-permission-option-auto-check", cx),
        "an unselected row must not carry a checkmark"
    );

    // The title row is present with both its parts.
    assert!(mounted(window, "composer-permission-picker-title", cx));
    assert!(!PERMISSION_PICKER_TITLE.is_empty());
    assert!(!PERMISSION_PICKER_LEARN_MORE.is_empty());

    // The chip keeps the frozen R57 slot: the picker is an absolutely
    // positioned layer inside the chip's wrapper, so the bottom row's geometry
    // is unchanged while it is open.
    let add = bounds(window, "composer-add", cx);
    let chip = bounds(window, "composer-permission-status", cx);
    let model = bounds(window, "composer-model", cx);
    assert!(f32::from(add.right()) <= f32::from(chip.left()));
    assert!(f32::from(chip.right()) <= f32::from(model.left()));
    assert!(
        f32::from(chip.size.height) < Layout::COMPOSER_MIN_HEIGHT,
        "the chip itself must stay one line tall"
    );

    // The chip is a toggle: a second click closes the layer it opened.
    click(window, "composer-permission-status", cx);
    assert!(
        !mounted(window, "composer-permission-picker", cx),
        "clicking the chip again must close the picker"
    );
}

/// A9/A10: choosing a row emits exactly the existing settings request, closes
/// the picker, and the chip's own label follows the authoritative thread once
/// the app applies it.
#[gpui_kit::test]
async fn r62_picker_selection_requests_the_exact_mode_and_follows_the_ack(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "r62-permission-select");
    click(window, "composer-permission-status", cx);
    click(window, "composer-permission-option-auto", cx);

    assert_eq!(
        events.lock().expect("settings events").as_slice(),
        &[ThreadSettingsRequested {
            thread_id: "r62-permission-select".into(),
            mode: None,
            permission_mode: Some(PermissionMode::Auto),
        }],
        "a picker choice must emit one permission-only settings request"
    );
    assert!(
        !mounted(window, "composer-permission-picker", cx),
        "the picker must close on selection"
    );

    // The durable acknowledgement projects the new value; the chip label and
    // the picker's marker both read it.
    stream.update(cx, |stream, cx| {
        let mut persisted = stream.thread.clone();
        persisted.permission_mode = PermissionMode::Auto;
        stream.apply_thread(persisted, cx);
    });
    cx.run_until_parked();
    click(window, "composer-permission-status", cx);
    assert!(
        mounted(window, "composer-permission-option-auto-check", cx),
        "the picker must mark the acknowledged mode"
    );
    assert!(
        !mounted(window, "composer-permission-option-confirm-check", cx),
        "the previous mode must lose its checkmark"
    );
}

/// R57 P1 regression + R62 R9/A11: the `+` menu keeps its permission group,
/// and the two entries agree — a change made in one is visible in the other
/// because both read the one authoritative `thread.permission_mode`.
#[gpui_kit::test]
async fn r62_plus_menu_permission_group_agrees_with_the_picker(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r62-permission-entries");

    // The `+` menu still carries all three rows.
    click_composer_add(window, cx);
    for selector in [
        "composer-action-permission-readonly",
        "composer-action-permission-confirm",
        "composer-action-permission-auto",
    ] {
        assert!(mounted(window, selector, cx), "{selector} must be visible");
    }
    assert!(mounted(
        window,
        "composer-action-permission-confirm-check",
        cx
    ));

    // Opening the picker closes the `+` menu: the two entries are never
    // mounted at once.
    click(window, "composer-permission-status", cx);
    assert!(
        !mounted(window, "composer-action-permission-auto", cx),
        "opening the picker must close the `+` menu"
    );
    assert!(mounted(window, "composer-permission-picker", cx));

    // Selecting through the **picker** and applying the ack updates the `+`
    // menu's marker too — one authoritative value, two renderers.
    click(window, "composer-permission-option-readonly", cx);
    stream.update(cx, |stream, cx| {
        let mut persisted = stream.thread.clone();
        persisted.permission_mode = PermissionMode::ReadOnly;
        stream.apply_thread(persisted, cx);
    });
    cx.run_until_parked();
    click_composer_add(window, cx);
    assert!(
        mounted(window, "composer-action-permission-readonly-check", cx),
        "the `+` menu must follow a mode chosen in the picker"
    );
    assert!(
        !mounted(window, "composer-action-permission-confirm-check", cx),
        "the previous mode must lose its `+`-menu checkmark"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thread_permission_mode()),
        PermissionMode::ReadOnly
    );
}

/// The picker participates in the composer's popover exclusivity both ways,
/// and a click outside closes it (R59's rule, extended by R62).
#[gpui_kit::test]
async fn r62_permission_picker_is_exclusive_and_closes_on_outside_click(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r62-permission-exclusive");

    // Opening the model picker closes the permission picker.
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(vec!["mock".into()], cx);
    });
    cx.run_until_parked();
    click(window, "composer-permission-status", cx);
    assert!(mounted(window, "composer-permission-picker", cx));
    click(window, "composer-model", cx);
    assert!(
        !mounted(window, "composer-permission-picker", cx),
        "opening the model picker must close the permission picker"
    );

    // And the reverse.
    click(window, "composer-permission-status", cx);
    assert!(mounted(window, "composer-permission-picker", cx));
    assert!(
        !stream.read_with(cx, |stream, _| stream.model_picker_level.is_open()),
        "opening the permission picker must close the model picker"
    );

    // A click on the conversation surface closes it without a request.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let chip = visual
        .debug_bounds("composer-permission-status")
        .expect("permission chip");
    visual.simulate_click(
        gpui_kit::point(chip.center().x, chip.center().y - px(220.)),
        Modifiers::default(),
    );
    visual.run_until_parked();
    assert!(
        !mounted(window, "composer-permission-picker", cx),
        "an outside click must close the permission picker"
    );
}
