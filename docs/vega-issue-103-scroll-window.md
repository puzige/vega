# Issue #103 — bounded tool and thinking disclosures

Source: https://github.com/puzige/vega/issues/103
Status: frozen for implementation, 2026-09-22.

## Problem and scope

Expanded tool detail, adjacent tool groups and thinking currently grow without
a viewport limit. The outer conversation already uses a variable-height list;
this change bounds individual expanded entries, not the entire timeline.
The user supplied screenshot establishes a compact output surface; exact
dimensions below are Vega design choices, not measurements of another app.

## Contract

- Keep each existing disclosure header outside its scrolling body, so it can
  always be collapsed. Preserve collapsed defaults, order and status summaries.
- Tool detail and thinking bodies use `Layout::DISCLOSURE_CONTENT_MAX_HEIGHT`
  (240 logical px). Expanded tool-group children use
  `Layout::TOOL_GROUP_MAX_HEIGHT` (320 logical px). Short bodies retain natural
  height. Viewports must not flex-shrink their children to fit; overflow remains
  reachable with wheel/trackpad scrolling and clipped to the body bounds.
- 240 = 60 × the 4px layout unit; 320 = 80 × that unit. These are maximum body
  heights, excluding the existing header and external margins. Group children
  may contain bounded detail bodies; their scroll identities must be separate.
- While the inner viewport can move, scrolling it must not move the outer
  conversation. At its boundary use the existing GPUI scroll chaining policy.
  Scrolling outside a disclosure continues to operate the outer conversation.
- Preserve a mounted body's reading offset during streaming/status updates and
  collapse/reopen. Clamp offsets if content shrinks. Do not introduce automatic
  follow-to-bottom behavior or jump away from the user's reading position.
- Preserve current theme tokens, tool safe projections, truncation notices,
  approval controls and all existing content/memory limits. This is UI-only:
  no persistence migration, provider/runtime changes or new dependencies.
- Thinking stays memory-only and unavailable after restart by existing design;
  restored tool history must render the same bounded detail and group views.
- This contract refines Issue #70 progressive disclosure and thinking geometry;
  all previous chronology and security contracts remain authoritative.

## Acceptance matrix (before implementation)

| ID | Scenario and operation | Observable expected result | Layer / initial status |
|---|---|---|---|
| A1 | Expand long standalone shell output; wheel within it | body ≤240px; later lines reachable; outer offset unchanged until boundary | production GPUI + native / pending |
| A2 | Expand long thinking; scroll then append reasoning | body ≤240px; reading offset retained; header available | production GPUI + native / pending |
| A3 | Expand many adjacent calls and a nested long shell detail | group ≤320px; nested detail ≤240px; final child reachable; independent scrolling | production GPUI + native / pending |
| A4 | Short/empty body, error and truncation states | no unnecessary blank height; safe status and truncation copy retained | production GPUI / pending |
| A5 | Collapse/reopen and rerender each body | collapse works; reopened reading position retained; other bodies unaffected | production GPUI / pending |
| A6 | Hydrate existing tool history / restart UI | same bounded geometry and chronological order; thinking remains transient | existing hydration regression + native / pending |
| A7 | Light/Dark, 1403×860, 1200×760, 960×600; 1229/1230 width | readable unclipped headers; body and outer scroll still usable | native / pending |

Failure evidence must be captured before the geometry fix. Native evidence must
use the actual built app and be retained outside the disposable worktree;
GPUI tests do not substitute for native screenshot/interaction acceptance.

## Plan and ownership

1. Main agent freezes this spec and design-token intent, coordinates and reviews.
2. Dedicated implementation agent owns `tool_card.rs`, `tool_activity_group.rs`,
   `conversation_stream/thinking.rs`, focused tests and `vega_theme` tokens.
   First demonstrate the unbounded geometry, then implement bounded scroll views.
3. Run formatting and focused production GPUI/tool/thinking regressions, retaining
   original failures and command output. Cloud PR checks remain the merge gate.
4. Main agent reviews diff, verifies real app behavior when exclusive access is
   available, records delivery evidence and submits the PR. Unverified required
   acceptance remains explicitly pending; do not close the issue prematurely.

Rollback: revert this UI-only change; stored conversation data is unaffected.
