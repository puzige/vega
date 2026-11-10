# Notice/list geometry lifecycle repair

## Scope and baseline

Implementation lane for #907, not independent review. Baseline HEAD is `0b5ea426abfd8d4358489e916273d89474fb190f` in the parent-owned dirty integration worktree. The user reports the remote was just fetched, branch remote equals HEAD, and master `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5` is already integrated. This lane performs no Git mutations or Cargo execution. Only `run_notice/presentation.rs`, `computer_use/presentation/mounted.rs`, `attachments/presentation/removal.rs`, and this new document may change. Ordinary submissions and message footer are other writers' scope.

## Diagnosis established before implementation

The retained production backtrace `target/nsh-closure-final-callback-mounted-backtrace.stderr` reports the mutable ListState borrow conflict through `ListState::viewport_bounds` from message-footer prepaint. The same unsafe timing exists in the three scoped migration leaves, plus run-notice initial height estimation during child rendering/layout.

Inspected `gpui-fast 0.1.9` source: `src/elements/list.rs:1619` holds a mutable ListState borrow across item rendering/layout/prepaint, and only writes completed `last_layout_bounds` at line 1660. Child prepaint must not call either `viewport_bounds` or `bounds_for_item`. List paint at lines 1665–1713 invokes children without that long-lived borrow. `src/fast/layers/lists.rs::paint_row` invokes child paint without a ListState borrow (including direct, nested, swapped and retained-row paths; retained rows can skip painting).

`selection_menu::mounted::list_bounds` obtains the actual current scoped ReadingList bounds without reading ListState, but exposes no row lookup. Attachment `Source::current` checks epoch, route, settings, preview/revocation and attachment identity only; it does not query ListState.

## Acceptance and intended minimal repair

1. Prepaint revokes each local geometry lease, retains the live mount in element state, and carries actual child bounds and the prepaint content mask in typed PrepaintState. It does not read ListState. A child not subsequently painted cannot regain geometry authority from that prepaint.
2. Paint captures the completed current ListState viewport, and run notice also captures the actual completed row. Visibility uses the saved real bounds/mask and existing foreground subtraction/window clipping rules. No cached viewport or synthesized row is used as action authority.
3. Run-notice initial height keeps the existing measured-height preference and unchanged `estimated_height` algorithm. Before measurement, use the matching current ReadingList scoped width. If no current scope exists, use the actual current window width only as a provisional height estimate, never as viewport/action authority; never fall back to ListState during layout. The existing natural NSH layout measurement remains authoritative.
4. Attachment removal continues to require a live active parent mount, current source, exact parent visibility and exact observed geometry. Publish only changed geometry; do not introduce unconditional invalidation loops. Other leaves do not add presentation notifications when geometry is not part of their render snapshot.
5. All activation checks remain intact: exact route/source/projection, live mount, selection, surface health, current viewport/foreground, run-notice row fence, CUA measurement/consumption, and attachment parent/image identity. No business retries, action replay, visible copy, dependency, vendor or shared infrastructure changes.

## Focused parent-owned verification

After all writers stop, compile the integrated migration and run the existing run-notice, CUA, attachment-removal and ordinary/footer regressions. A useful additional mounted harness should place the real leaves under an actual ReadingList/List, exercise the first frame before NSH measurement and a second frame after measurement, then resize/scroll and change foreground occlusion. It should verify no RefCell panic, exact current viewport and row authority, stable natural height after measurement, unchanged-geometry convergence, and rejection after source/route/epoch replacement, parent removal or a revoked/nonpainted mount. Retained-list-layer behavior should be included rather than assuming paint always runs after prepaint. A callback-only renderer harness without the actual list borrow is insufficient for this defect.

No Rust test/support files are added in this restricted lane. Parent owns compilation/regression, independent review, final-head CI and native acceptance. Native focus/AX, compositor pixels, live provider/CUA operations and macOS/Windows/Linux acceptance are not established by this repair.

## Validation record

Implemented the three scoped lifecycle changes. Production diff against baseline HEAD is 80 insertions / 66 deletions across three files; SHA-256 of `git diff --` in run-notice, CUA, removal argument order is `799e82fb823214f35409c53e53a1bdb04a31156908716adae0408e47965727b6`.

Executed on the final production bytes:

- `rustfmt --edition 2024 --config skip_children=true` on only the three authorized Rust files, then the same command with `--check`: passed. Actual formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Scoped `git diff --check`: passed.
- Read-only Python static checks: terminal newlines and no trailing whitespace passed; added Rust lines have no comments or lint/ignore suppression. Byte comparisons against HEAD confirmed the run-notice action/source/selection implementation before Render is unchanged, and attachment action/source implementation before Mounted plus NSH rendering after Mounted are unchanged. CUA changes are confined to its Mounted implementation; its action implementation is outside the write scope and untouched.
- Inspected final diffs: all three prepaint paths revoke geometry and contain no ListState read; run-notice Render has no ListState read; paint captures actual completed viewport and run-notice row. Existing geometry equality guard still gates attachment snapshot publication; no unconditional notify was introduced.

Final production SHA-256:

| File | SHA-256 |
| --- | --- |
| `run_notice/presentation.rs` | `ff2e5b6b27c00b207ac63effb086b81e1bbb6c1d42b49f3489631f58b8efd2fd` |
| `computer_use/presentation/mounted.rs` | `9fa769e46a4885cabd70766dde8266398712fd9036b59a0389c83031064e47c6` |
| `attachments/presentation/removal.rs` | `171b98db67931c6dcc21db9dcbbe1c5b3e7e5c31c223ebf329142c34a34a0938` |

Residuals: when no current ReadingList scope exists, the initial run-notice height estimate uses real window width and may temporarily underestimate wrapping in a narrower host until the existing NSH layout measurement arrives. Attachment snapshot publication now follows completed paint geometry; queued callbacks with an older geometry remain rejected by the unchanged exact-geometry check until reconciliation. A prepainted row whose paint is skipped remains revoked (fail closed); the parent should check retained-row interactivity in the actual integrated harness. These timing behaviors have only static evidence in this lane.

No Cargo, compiler, Clippy, regression, CI, app launch, native focus/AX or three-platform acceptance result is claimed. Compilation and runtime verification are explicitly centralized with the parent after writers stop. This implementation lane is not the required independent review.
