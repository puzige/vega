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
//! ## What these tests cannot pin
//!
//! The paint **order** itself (A2). The GPUI test platform has no headless
//! renderer, so `debug_bounds` is the only geometry the harness exposes and no
//! test can observe which quad was drawn last. A2's evidence is the native
//! pixel scan of R64 §5 A3/A4, not anything in this file — see the R64 report.

use super::*;
use gpui_kit::{Bounds, Pixels, VisualTestContext};

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
