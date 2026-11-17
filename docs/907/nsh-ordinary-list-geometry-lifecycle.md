# Ordinary controls: current List geometry lifecycle

## Observed failure and scope

The final-callback snapshot passes both original focused failures and all 357 UI unit cases. The ordinary mounted runner passes the timestamp case, then fails while constructing the actual production ConversationStream: the migrated message footer queries `ListState::viewport_bounds()` during child prepaint while Fast owns the List state's mutable borrow. Original evidence is retained in `target/nsh-closure-final-callback-mounted-backtrace.stderr`.

The ordinary controls' own `Mounted::prepaint` has the same concrete API misuse: both `viewport_bounds()` and `bounds_for_item()` borrow the state while the parent List prepaints children. This must be corrected before interpreting a subsequent ordinary mounted result. The footer correction has a separate writer and scope.

## Acceptance before implementation

- Keep the actual production ConversationStream, original List, row and ordinary submission controller in all mounted cases; do not replace them with a fixture-only list or relax readiness/assertions.
- Prepaint must revoke the previous mount geometry and preserve the actual element bounds and content mask without querying ListState.
- Capture the exact completed current List viewport and row bounds during paint, after the parent's mutable prepaint borrow has ended. Intersect the current element bounds, preserved prepaint content mask and foreground-adjusted reading viewport; publish no geometry for an absent row or empty intersection.
- Preserve all immutable displayed snapshot, source/attempt/facts, geometry equality, row equality, active window, focus and live mount checks at command validation. Missing geometry is rejection, not a fallback to old geometry.
- Preserve width-driven presentation invalidation and height convergence. Notify the host only for actual width or geometry changes; unchanged frames must converge.
- Re-run all four ordinary mounted cases, the 357-case UI regression and affected reading/scroll regressions. Native AX dispatch and three-platform acceptance remain separate, unverified requirements.

## Minimal implementation plan

Use the element's typed prepaint state to carry the preceding geometry, actual element bounds and current content mask to paint. Retain the existing geometry structure and action validation unchanged. Perform the List viewport/row queries in paint, then notify on a genuine geometry delta before forwarding child paint. No Fast changes, dependencies, synthetic authority, relaxed assertions, retry loop or new product copy is needed.

Fixed-version API evidence: `gpui-fast 0.1.9/src/elements/list.rs:1619` holds the mutable borrow through child prepaint and completes current layout state at lines 1660–1662. Its paint implementation at lines 1665–1713 does not hold that mutable borrow while invoking row element paint. This timing correction is an implementation defect repair, not an acceptance change.
