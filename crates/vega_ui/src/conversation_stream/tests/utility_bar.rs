//! R49 composer utility bar: one state, one production test.
//!
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame's debug bounds — no test-only seam decides any branch. The
//! project rows come from the same `vega_store::projects` query the sidebar's
//! project block uses.

use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

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

#[gpui_kit::test]
async fn r49_new_task_page_renders_the_utility_bar_at_its_frozen_height(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-utility-bar");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let bar = bounds(window, "composer-utility-bar", cx);
    assert_close(
        f32::from(bar.size.height),
        Layout::COMPOSER_UTILITY_BAR_HEIGHT,
        "utility bar height",
    );
}

#[gpui_kit::test]
async fn r49_session_page_with_a_message_renders_no_utility_bar(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r49-session-page");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_some(),
        "the new-task page must start with the utility bar"
    );
    // The production commit path for a sent message: the durable start landed,
    // so the local echo enters the stream.
    stream.update(cx, |stream, cx| {
        stream.composer_submit_pending = true;
        stream.accept_composer_submission("first message", cx);
    });
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_none(),
        "a conversation with messages must not render the utility bar"
    );
}

#[gpui_kit::test]
async fn r49_utility_bar_keeps_the_frozen_inset_and_chip_ladder(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-utility-geometry");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let card = bounds(window, "composer-shell", cx);
    let bar = bounds(window, "composer-utility-bar", cx);
    assert_close(
        f32::from(bar.left() - card.left()),
        Layout::COMPOSER_UTILITY_BAR_INSET,
        "utility bar left inset from the card",
    );
    assert_close(
        f32::from(card.right() - bar.right()),
        Layout::COMPOSER_UTILITY_BAR_INSET,
        "utility bar right inset from the card",
    );
    assert_close(
        f32::from(bar.center().x),
        f32::from(card.center().x),
        "utility bar shares the card's axis",
    );
    assert!(
        f32::from(bar.bottom() - card.top()).abs() <= 1.0,
        "utility bar bottom must meet the card top with zero overlap"
    );

    let folder = bounds(window, "composer-utility-project-chip", cx);
    let branch = bounds(window, "composer-utility-branch-chip", cx);
    assert_close(
        f32::from(folder.left() - bar.left()),
        Layout::COMPOSER_UTILITY_CHIP_INSET,
        "first chip inset from the bar's left edge",
    );
    assert_close(
        f32::from(branch.left() - folder.right()),
        Layout::COMPOSER_UTILITY_CHIP_GAP,
        "chip gap",
    );
}

#[gpui_kit::test]
async fn r49_folder_chip_selects_another_project_from_the_shared_rows(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-project-chip");
    let ids = install_utility_globals(
        cx,
        &[("/tmp/r49-alpha", "alpha"), ("/tmp/r49-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    click(window, "composer-utility-project-chip", cx);
    let menu = bounds(window, "composer-utility-project-menu", cx);
    assert!(
        f32::from(menu.size.height) > 0.0,
        "the project menu must render real rows"
    );
    let row_selector: &'static str =
        Box::leak(format!("composer-utility-project-row-{}", ids[1]).into_boxed_str());
    click(window, row_selector, cx);
    let selected = cx.update(|cx| cx.global::<crate::sidebar::SelectedProject>().0.clone());
    assert_eq!(
        selected.as_deref(),
        Some(ids[1].as_str()),
        "choosing a row must rewrite the shared SelectedProject global"
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-project-menu")
            .is_none(),
        "the menu must close after a choice"
    );
}

#[gpui_kit::test]
async fn r49_utility_bar_is_absent_without_a_project_context(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-no-project");
    install_utility_globals(cx, &[], None);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_none(),
        "a route without a project context renders no utility bar"
    );
}
