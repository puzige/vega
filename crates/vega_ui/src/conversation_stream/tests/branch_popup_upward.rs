//! Composer-mounted branch popup geometry and interaction regressions.
//!
//! These cases render the production `ConversationStream` inside the real
//! Composer utility bar. They deliberately avoid an isolated selector mount:
//! the placement bug only exists when the selector is mounted by the Composer
//! route.

use super::menu_lists::branch_snapshot;
use super::r64_popup_deferred::{PROJECT_BINDING, bounds, install_utility_globals};
use super::*;
use gpui_kit::{VisualTestContext, px, size};
use vega_conversation::types::{BranchSnapshot, GitWorkspaceErrorCode};

fn resize(window: WindowHandle<StreamHarness>, width: f32, height: f32, cx: &mut TestAppContext) {
    window
        .update(cx, |_, window, _| {
            window.resize(size(px(width), px(height)));
        })
        .expect("resize Composer test window");
    cx.run_until_parked();
}

fn branch_selector(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    stream.read_with(cx, |stream, _| stream.branch_selector())
}

fn open_branch_in_window(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    snapshot: BranchSnapshot,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    let selector = branch_selector(stream, cx);
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("branch-selector-popup")
            .is_some(),
        "the Composer-mounted branch popup must render after opening"
    );
    selector
}

fn assert_popup_above_trigger(
    window: WindowHandle<StreamHarness>,
    cx: &mut TestAppContext,
    label: &str,
) {
    let trigger = bounds(window, "composer-utility-branch-chip", cx);
    let popup = bounds(window, "branch-selector-popup", cx);
    let viewport = window
        .update(cx, |_, window, _| window.viewport_size())
        .expect("Composer viewport size");
    let popup_bottom = f32::from(popup.bottom());
    let trigger_top = f32::from(trigger.top());
    assert!(
        popup_bottom <= trigger_top + 1.0,
        "{label}: popup must finish above its Composer trigger; popup={popup:?} trigger={trigger:?}"
    );
    assert!(
        f32::from(popup.left()) >= -1.0
            && f32::from(popup.right()) <= f32::from(viewport.width) + 1.0
            && f32::from(popup.top()) >= -1.0
            && f32::from(popup.bottom()) <= f32::from(viewport.height) + 1.0,
        "{label}: popup must stay inside the viewport; popup={popup:?} viewport={viewport:?}"
    );

    // The Composer fixtures leave enough vertical room at both requested
    // sizes, so the anchored layer is not window-constrained. The frozen gap
    // is therefore the exact 4px contract from the selector spec.
    if f32::from(popup.top()) >= 7.0 {
        let gap = trigger_top - popup_bottom;
        assert!(
            (gap - 4.0).abs() <= 1.0,
            "{label}: popup/trigger gap must remain 4px when unconstrained; got {gap}px"
        );
    }
}

#[gpui_kit::test]
async fn branch_popup_upward_uses_production_composer_bounds_at_normal_size(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "branch-upward-normal");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    resize(window, 1403.0, 860.0, cx);
    open_branch_in_window(window, &stream, branch_snapshot(&["main", "feature"]), cx);

    assert_popup_above_trigger(window, cx, "1403x860");
}

#[gpui_kit::test]
async fn branch_popup_upward_keeps_empty_and_error_states_above_at_minimum_size(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "branch-upward-minimum");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    resize(window, 960.0, 600.0, cx);
    let selector = open_branch_in_window(
        window,
        &stream,
        BranchSnapshot {
            generation: 1,
            branches: Vec::new(),
        },
        cx,
    );
    assert_popup_above_trigger(window, cx, "960x600 empty");
    assert!(
        bounds(window, "branch-selector-popup", cx).size.height > px(0.0),
        "empty state must keep a visible bounded popup"
    );

    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
        assert!(selector.request_open(cx));
        selector.apply_error(GitWorkspaceErrorCode::GitFailed, cx);
    });
    cx.run_until_parked();
    assert_popup_above_trigger(window, cx, "960x600 error");
    assert!(
        bounds(window, "branch-selector-search", cx).size.height > px(0.0),
        "error state must preserve the real search row"
    );
}

#[gpui_kit::test]
async fn branch_popup_upward_caps_long_lists_inside_the_composer_viewport(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "branch-upward-long-list");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    resize(window, 960.0, 600.0, cx);

    let labels = std::iter::once("main".to_string())
        .chain((0..48).map(|index| format!("feature-{index:02}")))
        .collect::<Vec<_>>();
    let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
    open_branch_in_window(window, &stream, branch_snapshot(&refs), cx);

    assert_popup_above_trigger(window, cx, "960x600 long list");
    let popup = bounds(window, "branch-selector-popup", cx);
    assert_eq!(
        popup.size.height,
        px(240.0),
        "long branch lists must retain the existing 240px menu bound"
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("branch-row-0")
            .is_some(),
        "the bounded long-list popup must still mount its first visible row"
    );
}

#[gpui_kit::test]
async fn branch_popup_upward_preserves_trigger_escape_and_outside_dismissal(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "branch-upward-dismissal");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    resize(window, 1403.0, 860.0, cx);
    let selector =
        open_branch_in_window(window, &stream, branch_snapshot(&["main", "feature"]), cx);

    // The trigger is no longer covered by a downward popup, so its normal
    // production click path remains a real toggle.
    let trigger = bounds(window, "composer-utility-branch-chip", cx);
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(trigger.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));

    // Reopen through the same trigger and send Escape through the selector's
    // production key context rather than mutating its model directly.
    let trigger = bounds(window, "composer-utility-branch-chip", cx);
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(trigger.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    // `BranchSelector::toggle` focuses its own handle on the real open path;
    // Escape therefore exercises the Composer chip's production focus chain.
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));

    // The popup's capture-phase outside handler still owns the same close
    // path after the placement change.
    let trigger = bounds(window, "composer-utility-branch-chip", cx);
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(trigger.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let popup = bounds(window, "branch-selector-popup", cx);
    let outside = gpui_kit::point(px(4.0), px(4.0));
    assert!(
        !popup.contains(&outside),
        "outside point must miss the popup"
    );
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(outside, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
}
