# Composer vertical caret navigation

## Problem and root cause

User clarified that keyboard Up cannot move the Composer caret upward. `TextInput` has horizontal actions but no vertical actions. Composer binds Up to `PreviousMessage`; outside its history boundary this handler returns without moving the caret. The paint cache retains only visible lines, which is insufficient for moving across viewport edges.

## Acceptance

- Up/Down move the active caret to the adjacent visual line, including explicit newlines, soft wrapping, empty lines, CJK and emoji. Keep the preferred horizontal pixel position across consecutive vertical moves, clamping to shorter lines.
- Shift+Up/Down extend/reduce the selection from its existing anchor, including reversed selections. Vertical navigation never changes text.
- Movement across the viewport boundary reveals the caret using the existing caret-following viewport policy. Manual scrolling cannot restrict reachable rows.
- Preserve Composer's existing first-visual-line history recall and repeated history traversal; after editing a recalled multiline draft, non-first-line Up moves the caret rather than replacing the draft. File/slash/action menus retain their own arrow navigation.
- IME composition owns arrow keys while marked text exists. Single-line fields keep their prior arrow routing. No new UI text, dependencies, persistence changes or send behavior changes.

## Implementation plan

1. Add Up/Down and Shift variants to TextInput actions, bindings and listeners.
2. Retain a separate complete shaped-line cache from prepaint, with content revision validity, while leaving the visible-line mouse/paint cache unchanged. Store preferred x together with selection/revision so nonvertical selection or content changes naturally invalidate it.
3. Navigate using shaped glyph positions and existing UTF-8 boundary handling. Clamp a soft-wrap end to the preceding grapheme when necessary so the caret cannot resolve onto the following visual line.
4. Let Composer's existing Up handler delegate to TextInput when history recall is not allowed, without changing popup binding precedence.

## Verification and delivery

Local: rustfmt on changed Rust files, diff/whitespace checks and static call-chain review only. No local Vega execution, build or tests. Use existing required Actions fmt/clippy/test gate; no temporary Rust tests are introduced for this narrow UI routing fix. Final Rust test inventory remains empty (`no Rust tests`, not behavioral test coverage). Non-implementer independent review is required. User manually verifies multiline/wrapped/overflow navigation, Shift selection, history, popup navigation and IME after merge; CI is not native UI acceptance.

## Change record

Initial spec established before implementation for issue #625.
