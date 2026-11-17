# Current native text observation — development validation repair

## Scope and acceptance

This lane repairs development observation only; it is neither independent review nor a production capability change. The parent-provided dirty baseline is `0b5ea426abfd8d4358489e916273d89474fb190f`; the parent reports origin/master fetched at `2a29adf9` (#922) and owns subsequent integration. No Git, Cargo, dependency, fixture or production behavior changes are authorized here.

The mounted fixture must not require a new `TextLayout` event for every projection. Native layouts may be reused. Under the existing `vega-host-regressions` gate only, add `AppView::integration_text_observation() -> Option<Vec<IntegrationTextObservation>>`, reading the exact current surface subtree instead of event history. Each owned result contains the current nonselectable `DrawnText` string, native `TextLine` records, node id, parent-relative native frame and text-local content bounds, all in native points. It does not return input/editor values.

Only these paths may be written: `vendor/nsh-platform-gpui/src/app/integration_support.rs`, new sibling `text_integration_support.rs`, the existing feature-gated export statement in `vendor/nsh-platform-gpui/src/lib.rs`, and this document. No new feature, dependency, ordinary code comment or lint suppression.

## Authority and readiness contract

- Start from this `AppView`'s exact `Surface::root`, not a caller-supplied id or historical event. Require an existing engine control through the existing synchronous `Surface::control` callback, without invoking any control command or engine channel.
- Reject an active mutable mounter borrow using `try_borrow`, and hold the shared guard through traversal. Require a GPUI text system through `try_borrow` and an unchanged surface generation on return.
- Traverse current children in native bottom-to-top depth-first order. Every visited node must have an available shared `try_state`, be attached, equal the actual pointer-identity view returned by this surface's registry for its id, and have the expected actual parent. The root must have no parent. Reject duplicate ids. This checks root membership and the entire traversed parent chain, excluding detached, foreign, deleted and old replaced views.
- Prune `style.hidden`, `concealed` and zero/nonpositive-opacity subtrees, including inherited exclusion. Nonfinite opacity or bounds fail closed. Frames must have finite origins, dimensions and endpoints, with nonnegative dimensions; reported text frames/content must additionally have positive dimensions.
- Downcast each eligible current `DrawnText.layout` to `GpuiLayout` first, then copy lines with the actual `GpuiTextEngine` implementation of `TextEngine::lines`. This creates only a local reader over the existing text system; it does not shape, lay out, dispatch, flush, drain, repaint, install callbacks or mutate mounted state. Reject an absent/wrong backend layout, empty line set, nonfinite metrics, negative line width, nonpositive line height, or unordered/out-of-text UTF-16 ranges. Preserve actual line records: this backend's `TextLine.text` is currently empty; do not fabricate it from the full string or confuse it with a new event.
- `None` means unavailable/not ready/invalid under these checks. `Some([])` means no eligible nonselectable text in the successfully checked visible subtree, not that the requested fixture content exists. The fixture must match the expected current text and assert real layout metrics, not merely accept `Some`.

Existing lifecycle API limitations remain explicit. `Surface::stop` removes the control handle and clears mounted children, so stopped snapshots fail. A fresh load exposes only the newly mounted current subtree; snapshots cannot promise that a pending projection has arrived. The surface retains no queryable error/fatal-status flag, and a fatal VM error can leave its control handle present. The parent fixture must retain its existing `on_app_error` rejection and scheduling/deadline checks; this accessor cannot independently certify absence of errors. Registry, parent and child APIs use ordinary shared borrows rather than fallible borrows; the mounter guard excludes normal in-progress mount mutation, while unsupported reentrant direct mutation outside that path is not claimed safe.

Results are instantaneous owned observations, not live leases or tokens. The public node id is diagnostic and can be reused; it makes no cross-snapshot ABA/identity claim. There is no retained authority requiring an `is_current` method. Reacquire the snapshot for every fixture decision. Native frame/content bounds are not window-coordinate geometry, clipped viewport bounds, painted pixels or accessibility evidence. Transforms, clipping, scroll offsets, occlusion, backface state, window presentation and actual painting are not certified.

## Parent verification handoff

No synthetic tree tests are added: they would not establish the mounted fixture's real lifecycle. Parent owns compilation/strict linting and the existing mounted runner. Required development coverage includes expected current strings and native nonempty line metrics after projection without requiring a fresh event; changed/replaced text; missing/deleted nodes; same-id replacement without historical content; hidden/concealed ancestor exclusion; busy view/mounter state where reachable; stop and subsequent reload; and existing error/deadline rejection. Preserve the original fixture cases, assertions and timing budgets. This lane does not modify that fixture.

## Delivery status and static evidence

**READY — implementation complete; lane writes stopped after final readback/hashing. Not independent review.**

The API is implemented with the fields and checks above. Only the four authorized paths were written; `lib.rs` changed only its existing feature-gated export statement. No synthetic tests or fixture changes were added. The parent-provided HEAD/baseline is reported rather than re-queried in this lane; no Git command was run against the target worktree. No fetch/rebase, Cargo/rustc, build, test, Clippy or CI command was run. Parent owns compile/tests, fixture repair, independent review and integration; none is claimed passed here.

Executed static checks against final Rust bytes:

- `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- `rustfmt --edition 2024 --check --config skip_children=true vendor/nsh-platform-gpui/src/app/text_integration_support.rs vendor/nsh-platform-gpui/src/app/integration_support.rs`: initial result 1 for one wrapping difference; final result 0 after correction. `lib.rs` was not reformatted, preserving parent-owned content outside its single export statement.
- Python 3.15.2 scoped source-policy/wiring assertions: exit 0; no ordinary comments, new lint suppression, unwrap/expect in the new file; existing feature gate/export and sibling wiring present. These are static checks, not behavioral regression evidence.
- Full new Rust/support/document readback, focused `lib.rs` export readback, and SHA-256 hashing. No authority is inferred from a successful format/source-policy check.

Final Rust SHA-256 values:

| Path | SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/app/integration_support.rs` | `ad96f234d393ec4e283bc44bab60335b018033ac1efbf492efb18dd3e54cc4e7` |
| `vendor/nsh-platform-gpui/src/app/text_integration_support.rs` | `d9a90d8d5a2a9558113288d37afecb320b9e353560d4bffeecdf5c1688f3ec4c` |
| `vendor/nsh-platform-gpui/src/lib.rs` | `a509bd210b55681f08df36848e5f16f2396d9298f46b11571bb778298d0a58fc` |

The final document hash is supplied in the delivery packet rather than embedded recursively. Remaining validation is the parent-owned mounted coverage above, strict compilation/linting, and final integration checks. No spec deviation; no production capability or behavior changed.
