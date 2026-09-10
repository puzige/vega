# Vega R24 Sidebar footer edge-to-edge correction — implementation contract

**Status:** implementation contract
**Frozen:** 2026-09-10 (Asia/Shanghai)
**Scope:** the persistent Settings footer at the bottom of the main Sidebar.

## 1. Correction to R23

R23 fixed the Settings control to 32px but interpreted “fill” as filling the
Sidebar's already-inset content column. Because the Sidebar parent still owns
12px horizontal and bottom padding, the visible hover surface remained inset
and looked effectively unchanged. The user's follow-up rejects that
interpretation.

This contract supersedes R23 only for the footer's outer bounds and shape.
R23's 32px height, existing route behavior, and automated width coverage stay
authoritative.

## 2. Visual contract

- The Settings footer is an edge-to-edge rail row: its left and right bounds
  equal the Sidebar bounds, and its bottom equals the Sidebar bottom.
- Its interactive and hover surface is exactly 32px high. It is a footer strip,
  not an inset capsule, so it has no independent outer corner radius.
- Brand, New Task, Search, project/session rows, and the scroll region keep the
  existing 12px Sidebar content inset. Only the persistent footer breaks out
  of that content wrapper.
- Keep 12px breathing room between the scrollable content region and the footer
  without restoring side or bottom margins around the footer.
- Existing Vega hover/pressed/focus tokens, Light/Dark colors, centered label,
  and accessibility behavior remain authoritative. No color literal or new
  geometry number may be introduced.

## 3. Structure

- The Sidebar root owns the full rail bounds and clipping.
- A dedicated inner content column owns the existing 12px top/horizontal/bottom
  padding and the current vertical gaps for every non-footer child.
- A painted Settings surface is a direct sibling of that inner column and fills
  the full root width. Its interactive button fills the surface. Do not rely on
  negative margins or a hard-coded width.

## 4. Acceptance

1. The production-mounted GPUI test measures
   `sidebar-settings-surface.left == sidebar.left`,
   `sidebar-settings-surface.right == sidebar.right`, and
   `sidebar-settings-surface.bottom == sidebar.bottom`, each within ±1px.
2. The painted surface is 32px high, `sidebar-settings` fills all four of its
   bounds, and `sidebar-new-task` remains inset by 12px on both sides.
3. Those assertions pass at 240px, 304px, and 365px Sidebar widths.
4. Clicking the footer still opens General Settings; `Cmd+,`, tooltip, focus,
   Sidebar resize/collapse, and scroll behavior are unchanged.
5. Formatting, strict Clippy, all workspace tests, package verification, and
   real macOS Light/Dark hover inspection pass.

## 5. Non-goals

- No edge-to-edge treatment for ordinary Sidebar list rows.
- No redesign of the Settings page or shared Button component.
- No new icon, separator, animation, setting, or persistence field.
