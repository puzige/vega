# Sidebar task titles: final-width ellipsis

## Scope and acceptance

Fix task titles rendered by `ThreadsBlock::render_row_at_height` in Pinned, Projects, Recents and archive projections. No global GPUI patch, dependency change, stored-title mutation or action-layout change.

- Use the final laid-out title slot, after leading insets, pin marker, gaps and the fixed trailing action slot have been allocated.
- Shape using the inherited font, weight and size. Display the entire title if it fits; otherwise display a shaped prefix with `…` whose complete shaped width fits the slot.
- Use shaped glyph cluster boundaries for prefix candidates, retaining valid UTF-8. Treat line breaks as spaces only in the single-line presentation.
- If the slot cannot fit even an ellipsis, paint nothing rather than half a glyph.
- Preserve the complete row accessibility label and expose the complete title on its title container. Keep click, double-click rename, drag, actions, focus and status behavior unchanged.
- Preserve existing typography, colors, vertical line metrics and fixed action reservation; no additional visible copy.

## Evidence and trigger

The affected installation reports v0.1.396. That tag and the current baseline have the same title subtree: a flexible title div with `min_w_0().truncate()` inside an overflow-clipped row; the fixed action rail is a sibling, not a text-covering overlay.

The pinned dependency is gpui-kit 0.6.1 / gpui-pre 0.3.4. In `elements/text.rs`, `TextLayout::layout` computes `truncate_width` from a measurement callback's known/available width and stores shaped lines. `TextLayout::prepaint` only records final bounds; it does not recompute truncation for them. Painting consumes the stored lines inside the ancestor content mask. Thus a measured line wider than the final slot is hard-clipped rather than ellipsized. Its fallback truncator also decides by summed single-character layout widths rather than checking the complete final shaped candidate. These are static code findings, not a runtime trace establishing which discrepancy occurred in the supplied screenshot.

## Minimal implementation

1. Add a private `threads_block/title.rs` presentation helper using a GPUI canvas that receives final bounds during prepaint.
2. Shape the full single-line title with the inherited `TextStyle`; compare its advance width with the final slot.
3. For overflow, search shaped cluster-prefix candidates, shaping the prefix plus ellipsis and retaining only a candidate that fits. Shape the ellipsis first to handle degenerate widths.
4. Paint the chosen line with the existing inherited line height, vertically centered in the row. Retain the existing title element ID and full accessible name.
5. Replace only the title's raw text child and measurement-time `truncate` dependency with this helper. No other row render or engine behavior changes.

## Verification

Local validation is limited to rustfmt and diff/static checks. No Vega execution, build or tests locally or over SSH. Final-head required GitHub Actions and independent read-only review remain merge prerequisites. No temporary Rust tests are introduced for this small presentation-only change; required CI must still report its actual discovered/executed counts.

User manual acceptance after release: the reported mixed CJK/Latin title, short and exact-fit titles, long Latin titles, emoji/combining text, unread bold, pin marker, running status, hover/focused/open actions, sidebars at 240/304/365 logical pixels, narrow/wide resizing and light/dark themes. Check both full-title accessibility and click/rename/menu behavior. Remote installation is not modified as part of source validation.
