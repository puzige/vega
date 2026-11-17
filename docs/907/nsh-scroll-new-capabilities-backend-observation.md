# Native reading scroll observation and setup

## Authorized development contract

This development-only seam unblocks the eleven genuine production-body wheel cases. It does not implement consumption, replay wheel input, alter routing, upgrade dependencies, or change acceptance. Provenance remains NSH 0.4, Kit 0.7.3, Fast 0.1.9. The existing `vega-host-regressions` gate owns the module and all support must be archived and removed before master under the Rust test delivery policy.

`AppView::integration_reading_scroll(expected: &ReadingGeometry, area: u32, node: Option<u32>, horizontal: bool) -> Option<IntegrationScrollObservation>` validates the exact current geometry and its captured reading snapshot on this Surface. Vertical selection requires the requested mounted scroll ancestor and rejects any other movable vertical ancestor on that chain; an absent requested node succeeds only when no movable vertical ancestor exists. Horizontal selection walks the real reading-area descendants and requires exactly one native horizontal scroll with positive horizontal extent. An optional horizontal node must equal that unique target. Ambiguity, foreign or stale geometry, detached/reused nodes, unsupported transforms/zoom, nonfinite values and invisible viewports fail closed.

The observation contains public `node: Option<u32>`, `viewport: gpui::Bounds<gpui::Pixels>`, `offset: gpui::Point<gpui::Pixels>`, and `maximum: gpui::Point<gpui::Pixels>`. Bounds are clipped visible mounted bounds in window logical pixels, including Surface origin; offsets and inset-inclusive maxima are actual native state, with one native content point equal to one GPUI logical pixel under the supported unit geometry. Device pixel scale is not a coordinate multiplier. A real zero-extent target is represented as no movable target (`node: None`, zero offset and maximum), with its real viewport intersected with the reading area. A requested zero-main-axis target with nonzero off-axis state is rejected rather than disguised as zero. It is not fabricated scroll state.

`AppView::integration_position_reading_scroll(expected: &ReadingGeometry, area: u32, node: Option<u32>, desired: gpui::Point<gpui::Pixels>) -> bool` repeats currentness, source and mounted target validation synchronously. It accepts only finite, in-range desired positions and an unchanged off-axis, because NSH's existing native scroll command supports one main axis per scroll node. It issues nonanimated `scroll::command(Command::ScrollTo)` with content-inset compensation, stopping target motion through the existing core interrupt/apply/report path. Even an equal-position target goes through this setup command to stop motion. No-target setup accepts only zero and does not issue a command. Application is checked against actual native state, not remembered requests; callers must reacquire geometry after any target setup command.

## Parent-owned integration

The implementation lives in `app/scroll_integration_support.rs`, declared by the existing feature-gated `app/integration_support.rs`. Re-export `integration_support::IntegrationScrollObservation` at crate root under `vega-host-regressions`; no production export is required. The parent must supply this exact feature-gated accessor in `reading/geometry.rs`:

```text
#[cfg(feature = "vega-host-regressions")]
pub(crate) fn matches_snapshot(&self, snapshot: &ReadingSnapshot) -> bool {
    self.snapshot.matches(snapshot)
}
```

The accessor is identity comparison, not a new currentness rule. The caller first uses `reading_geometry_is_current(expected)`, acquires `reading_snapshot(area)`, then compares that snapshot to the lease. No geometry internals become public.

For the body adapter, observe vertical/NoTarget with its production `vertical_node` and `horizontal = false`; observe horizontal with `node = None` and `horizontal = true`. Setup passes the observed exact node and desired positive native offset. Intersect the returned viewport with the adapter's actual host clip before choosing the wheel hit point; reject an empty intersection. Do not reconstruct offsets from wheel deltas or source geometry. Preserve the existing 2-second / 512-tick / 10-second budgets and every assertion. Parent owns route registration, Cargo/exports, runner wiring, compilation and all eleven executions, followed by consolidated independent review after implementation lanes finish.

## Validation boundary

This writer runs focused rustfmt and read-only static checks only. Compilation, Clippy, real-host regressions and all eleven production-body cases remain UNRUN in this lane. Formatting is not a typecheck or test pass. No new tests, dependency changes, application launch, Git/network operations or writes outside the authorized support/document files are permitted.

Focused `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, edition 2024, `skip_children=true`, was run on the two support files. Final-version formatting/static results and exact SHA-256 identities are recorded in the stopped handoff. The preexisting alpha method and diagnostic functions are retained. Only `#[doc = "..."]` attributes document the new public API; no new ordinary comments, lint allowances, assertions, ignored tests, consumption algorithm or physics are introduced. The required snapshot accessor and crate-root export remain parent-owned integration, not a claimed compile pass.
