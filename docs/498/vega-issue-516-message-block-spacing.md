# #516 — Message block spacing

## Contract v1

Parent: [#498](https://github.com/puzige/vega/issues/498). Implementation card: [#516](https://github.com/puzige/vega/issues/516).
Baseline: `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`.

Implement the card's explicitly requested paragraph/list spacing values as a bounded visual adjustment. This is not a new measurement or claim of parity with another application. Historical third-party unpacking research is not an implementation input. This specific contract supersedes the older paragraph/list spacing description in design-guidelines §5 for these two shared tokens only.

## Acceptance before implementation

| Case | Operation / observable result | Verification |
|---|---|---|
| Adjacent ordinary paragraphs | Gap becomes 14 logical px, down from 15.5 | Token diff and shared-function review; actual native appearance pending user |
| Adjacent items in the same list block/quote context | Inter-item gap becomes 0 logical px, down from 6 | Token diff and `markdown_block_gap` branch review; tight/loose list appearance pending user |
| Continuation paragraph within a list | Existing 8 logical px token remains; existing quoted continuation behavior remains | Static unchanged-branch review; user sample |
| Code, table, quote and heading boundaries | Existing max-before/after computation and 16px block / 24px heading-before / 12px heading-after tokens remain | Static diff and branch review; user sample |
| Wrapped CJK/Latin paragraph with inline code | Font remains 13.5 and line box 23 logical px, wrapping/source/selection/copy unchanged | Static unchanged scope; narrow/wide native user check |
| Final reply and expanded Thinking; scroll/history navigation | Rendering, estimation and Thinking retain one shared gap function, no new divergent spacing path | Static call-chain review; native scrolling check pending user |
| Default/running/completed/error/focus/theme | No new controls, text, status or visibility behavior; light/dark and keyboard behavior retained | Two-token-only production diff; applicable native user check |

No persistence, security, error recovery or backend semantics change. No new dependency or Rust tests/test support are needed for this two-value visual adjustment. Existing required Actions remain mandatory; an empty Rust suite is `no Rust tests`, not behavioral test success.

## Minimal implementation

Only production file: `crates/vega_theme/src/lib.rs`.

| Symbol | Before | After | Difference |
|---|---:|---:|---:|
| `Layout::CONVERSATION_PARAGRAPH_GAP` | 15.5 | 14.0 | 14.0 − 15.5 = −1.5 logical px |
| `Layout::MARKDOWN_LIST_ITEM_GAP` | 6.0 | 0.0 | 0.0 − 6.0 = −6.0 logical px |

For N adjacent ordinary paragraphs, gap-only reduction is `1.5 × max(N − 1, 0)` logical px. For N adjacent items in one matching list block/quote context it is `6 × max(N − 1, 0)` logical px. These are spacing arithmetic, not measured whole-message heights. Paragraph line height stays `13.5 × (23 / 13.5) = 23` logical px; clock row height stays 23.

Verified call chain at baseline:

- `render_rows::markdown_blocks` applies `markdown_block_gap` once before a block.
- `render_rows::markdown_block_gap` returns the list-item token only for matching list block/quote context, preserves list continuation/quote rules, and otherwise uses `after.max(before)`.
- `message_anchors::estimate_markdown_height` sums that same gap function.
- `thinking_body::PreparedBody::new` uses that same gap function; no implementation change there.

Do not change typography/line height, clock height, block/heading/list-paragraph gaps, the unused `MARKDOWN_PARAGRAPH_GAP`, gap logic, markdown parsing, caches, anchors, Copy/Fork/edit buttons, assets, backend or database behavior. No code comments or new user-visible copy.

## Verification and delivery

1. Dedicated implementation Agent changes the two values only, reports an exact diff/content identity and stops writes.
2. Local static checks only: `git diff --check`, targeted `rustfmt --edition 2024 --check crates/vega_theme/src/lib.rs`, scoped symbol/call-chain and Rust-test-support diff inspection. Do not run Vega code, build, cargo check, Clippy or tests locally.
3. Non-implementer independent review of the immutable final content, including unchanged boundaries and shared render/estimation path.
4. Existing cloud PR check on final head: fmt, all-target Clippy, three existing Python check groups, Rust inventory/Nextest and aggregate required `check (fmt, clippy, test)`. Record run/job results and actual Rust discovered/executed counts. Poll at five-minute intervals; do not alter CI to pass.
5. Squash merge only after latest-head required CI success and independent approval. Keep #516 open and Project In review pending user hand test; update parent #498 without claiming the remaining children are implemented.
6. User tests synthetic mixed CJK/Latin paragraphs, wrapped text, inline code, tight/loose/nested/quoted lists, headings/code/table boundaries, a final reply and expanded Thinking; narrow/wide windows, light/dark themes, scrolling/history navigation, selection/copy and keyboard focus. Merge does not imply installed application or accepted native appearance.

Rollback: revert the two token values together through a PR if native validation fails; retain the shared gap function.

## Change log

- v1: bounded implementation contract from #516's requested values. Current AGENTS/exec-guide risk-based policy replaces the card's historical TDD wording. No new third-party resource inspection; no native validation claimed.
