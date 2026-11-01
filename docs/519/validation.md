# Native Fork UI validation

## Scope and evidence boundaries

Issue #519 / PR #803, Native project top-level chat Fork only. No ACP/Codex Fork, user-message editing, installation, application restart or user-data mutation. Local validation is formatting, diff and static inspection only; no local Vega execution, compilation or tests. CI and merge are not user acceptance.

## User acceptance follow-up: pending feedback, destination focus and message menu

The original UI did not expose the Fork worker's pending lifetime or explicitly request destination Composer focus. The handler already attempted navigation, so the user's real-system failure to see it open is not established by historical CI; this follow-up explicitly refreshes the destination and schedules focus only after its stream loads. Source route/generation/stream guards remain authoritative; navigation refusal after a committed copy is reported as created-but-not-opened rather than storage failure.

This revision adds source-message pending feedback, shared button/context-menu activation and one-shot destination focus. The right-click menu is attached to the actual Assistant body copy object, not the distinct Final clipboard projection. Pending is cleared before stale completion rejection, busy menus revalidate at activation, and Reduce Motion/inactive windows retain a static indicator and label. Leaving the destination or entering Settings clears pending focus.

Validation choice: focused static lifecycle/API/identity inspection plus exact-head Actions fmt/Clippy/Nextest and existing Python checks. No new Rust development tests or test-only support; the historical suite below is not restored or claimed as a current-cycle test run. This UI-only delta leaves Store copy, archive/history adapters, admission and lease ownership unchanged. Native paint, animation, real menu interaction and focus require user manual acceptance, not source-string tests. Local format/diff checks pass; current-cycle independent review and cloud results pending at this revision's creation. Exact versions/results are recorded in the follow-up PR and #519.

## Development regressions

Immutable source head: `e717bf67fbe066315e64d5a4625a3875f88eac24`.

- [History projection tests](https://github.com/puzige/vega/blob/e717bf67fbe066315e64d5a4625a3875f88eac24/crates/vega_conversation/tests/display_history.rs): five real Store regressions for mixed archived/current delegation identity, inert archive details/status/approval, pagination/location/order/cursors, repeated privacy projection without stored-content mutation, ordinary-reader parity and admission-fenced restart recovery with current permission authority.
- [Window handler tests](https://github.com/puzige/vega/blob/e717bf67fbe066315e64d5a4625a3875f88eac24/crates/vega/src/window/native_fork_tests.rs): eight real handler/file-Store regressions for duplicate requests, existing source admission, atomic storage failure/retry, away-and-back generation, replaced-stream ownership and stream/controller/global running rejection.
- [Lease tests](https://github.com/puzige/vega/blob/e717bf67fbe066315e64d5a4625a3875f88eac24/crates/vega/src/trusted_action_fork_tests.rs): three regressions for shared single flight, stale release and real Store failure/retry/active-source refusal.

[Actions 41311683578](https://github.com/puzige/vega/actions/runs/41311683578), exact head above:

| Job | ID | Result |
| --- | --- | --- |
| Clippy (includes fmt) | 124863204290 | success |
| Nextest | 124863204467 | success |
| check (fmt, clippy, test) | 124864152824 | success |

Raw Nextest result: **16 discovered, 16 executed, 16 passed, 0 failed, 0 skipped**. Existing Python suites executed 5, 15 and 40 tests successfully. Tests isolate providers/desktop execution; they do not prove visual/keyboard behavior, actual privacy UI switching or all OS-thread interleavings.

## Earlier failures and responsible-layer corrections

No failing regression was removed or weakened to obtain passing results. Product implementation and safety checks were unchanged by these fixture corrections.

| Exact head | Actions | Actual outcome and correction |
| --- | --- | --- |
| `917bed42450bd88ec2da712e40faee4b74ba1bd4` | 41308278097 | Test compilation failed: glob import shadowed built-in `test` with GPUI macro, causing recursion. Tests did not execute. Replaced glob with explicit imports. |
| `2f950be4e5376e8f3d54d0641af42a8652badf8f` | 41309296005 | 16 executed, 7 passed, 9 failed, 0 skipped; window fixture configuration rejected symlink ancestors, restart fixture had nonexistent project path. Clippy found two unused items. Canonicalized owned temporary root, retained a real project directory and removed unused items. |
| `06dab6f320412f66c647cabdffb30418cb02b054` | 41310014772 | Clippy success; 16 executed, 15 passed, 1 failed, 0 skipped. Restart fixture omitted persisted run permission fence, so resumable-tail reader correctly returned None. Added real Store fence record before recovery; retained recovery assertions. |
| `d4b5e83eaeb0db5eca7d7a56340713d2acb85327` | 41310903287 | Clippy success; 16 executed, 15 passed, 1 failed, 0 skipped. Added fence rejected fixture's short thread ID; permission contract requires ULID. Changed fixture thread identity and every corresponding relationship/query/identity expectation to a valid fixed ULID. |

## Review and integration

Existing independent implementation review found the mixed archive/new delegation projection defect at `ebfc22476eb23437e0adaef0589d73a2d4c2cc01`; `6de5f789eeea1f4967d2a08e64a5eeda168a3a7e` resolved it by selecting only typed archive-backed calls for inert display decoding, leaving the headless historical API unchanged. Independent delta recheck ended without new actionable findings.

Non-writer review of `917bed42450bd88ec2da712e40faee4b74ba1bd4` covered new regressions and master integration; subsequent reviews covered explicit imports and fixture/master changes through `d4b5e83eaeb0db5eca7d7a56340713d2acb85327`. No actionable static findings remained; later actual CI fixture failures above supersede any static assumptions. Unchanged conclusions are reused. Final cleanup/ULID delta review is separate and pending at this document's creation.

Integrated master `51553d4c51361f944ba2035f587dc186e5052581` without conflicts. Fork remains a direct Copy-following action with individual hover/focus handling and unchanged footer geometry.

## Final delivery gate

After the successful development run and verified remote source access, remove all three Rust development test files, both module declarations and exclusive GPUI test-support/tempfile dev dependencies. Keep production behavior, existing Python tests, Cargo.lock and generic CI unchanged. Local `cargo fmt --all -- --check`, `git diff --check` and focused Rust test-marker/dev-support inventory pass after cleanup.

The final no-Rust-tests head must obtain its own complete required CI, including **no Rust tests (0 discovered, 0 executed)**; the development result cannot substitute for this gate. Exact final head/run/results are recorded in PR #803 and Issue #519 after completion. Only after independent delta/cleanup review and that exact-head gate may the PR be merged. Keep Issue open and Project In review after merge until user manual acceptance; no installation or user-data cleanup is performed.
