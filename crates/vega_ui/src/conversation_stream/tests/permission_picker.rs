//! R62 §7 and I58-R7: the bottom row's permission chip opens a four-row picker.
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
//!
//! R63 adds the chip's own glyph to the same contract: the chip must show the
//! mode's icon (hand / shield / warning), not a constant warning triangle.
//! That case lives at the bottom of this file.

use super::*;
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

/// The picker row's selector for one mode. A test-side fixture: the
/// production selector is built in `render.rs` from `mode.as_str()`, which is
/// not `&'static str`.
fn permission_row_selector(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "composer-permission-option-readonly",
        PermissionMode::Confirm => "composer-permission-option-confirm",
        PermissionMode::Auto => "composer-permission-option-auto",
        PermissionMode::FullAccess => "composer-permission-option-full_access",
    }
}
