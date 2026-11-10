# Generic scroll edge fade — NSH implementation

## Scope and acceptance (written before implementation)

This third-lane implementation owns `crates/vega_ui/src/scroll_edge_fade.rs`, its private `scroll_edge_fade/**` modules, this document, and only passive fade wiring in `text_input/element.rs`. Parent has already fetched and fast-forwarded this worktree. No Cargo, Git, network, native execution, delegation, review, dependency/backend/vendor edits or wheel consumed/remainder changes are authorized for this lane.

Starting source identity supplied by parent: HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`; dirty/master `34555cec9c0c4c4c1f7a528d739752f414e53edf`. Observed SHA-256 of `scroll_edge_fade.rs`: `97157970ec94cfcccde7031460c15f5cbdce6f76b8c65bedbf21fb8ceb15dc8c`.

Required outcomes:

- All generic edge-fade pixels are genuine NSH linear-gradient views, not GPUI quads or an old GPUI fade embedded in NSH. Keep `regions`, `protect`, epsilon/quarter-viewport strength, inset, gutters, bottom extension, full outer-quad gradient coordinates, radius clamping and per-gutter corner resets.
- Keep the sole original scroll handle, original element layout identity, host child layout, child events and accessibility. No new scrolling container, wheel consumption, editor semantic changes, business commands or input behavior.
- Keep top-pinned suppression at the original post-child-paint decision point. Recompute the original region masks with `top = false` when pinned, including expansion of side masks into the vacated top strip; do not freeze the unpinned side masks.
- Prepare NSH in prepaint, never call GPUI prepaint from paint. The fixed GPUI Fast `insert_hitbox`/element-offset APIs assert prepaint. Each edge is prepared independently over the full outer geometry so the top edge can be skipped and side masks recomputed after the child paints.
- Passive surfaces prepaint under an empty native content mask, have no Tab stop, and expose no NSH accessibility content. Paint under both the original region mask and ancestor clip. Do not intercept input or emit commands.
- Geometry, clip, viewport, scale and remount must invalidate stale presentation immediately. Publish only when the actual snapshot differs; do not create a generation or publish every stable frame. Stop each display engine when its retained owner is released.
- The hidden legacy text editor must use the same prepared NSH path with its existing protected metrics; its caret, selection, IME, scroll model and input handler remain unchanged.

## Verification plan and limits

Allowed current-cycle checks: focused source/API inspection, scoped rustfmt and static checks. Behavioral Rust tests may be authored if useful but cannot run here and may not enter master. Parent owns compilation, regression execution and final integration; native appearance, pointer/Tab/AX and multi-platform behavior remain unverified until those checks actually run. No test or compile result will be inferred from formatting.

## Implementation / handoff

Implementation resumed from the released writer's preserved dirty files; all five supplied SHA-256 values matched before editing. The existing prepaint/paint split and post-child pinned-region recomputation are retained, not restarted.

The remaining implementation hardening uses a paint-free native receipt component: its reactive revision prop is delivered during Surface prepaint, after the NSH commit is mounted. A prepared edge is painted only when that receipt matches its current snapshot. Snapshot identity includes the gradient, ancestor clip, window viewport and scale. Revision advances only when this identity changes, never on a stable render; pending/stale gradients remain unpainted. The receipt component returns no GPUI element, has no focus handle, and adds no visible or accessible content. This uses existing public registration/prop APIs, with no backend or dependency edits.

### Source and integration

- `scroll_edge_fade.rs` retains the original layout, event and AX delegation and original handle tracking. Its prepaint state owns `Option<PreparedScrollFade>`; child paint runs first, then guards are sampled and `regions` recomputed with `top = false` when pinned. Side gradients use unchanged full outer coordinates, so only their masks expand into the vacated top strip.
- `scroll_edge_fade/nsh.rs` owns four retained transparent NSH surfaces, one per edge, keyed to the original host element lifetime. Gradient pixels come from NSH `bg_linear`, color stops and rounded views. The only registered native component is the zero-sized receipt; it draws nothing. The empty prepaint mask suppresses native hit testing; the surface focus handle is not a Tab stop; the NSH root and receipt subtree are AX-hidden. Region and ancestor masks intersect at paint. All retained engines receive `stop()` on host release.
- `text_input/element.rs` already had the necessary resumed wiring and is unchanged in this replacement stage. It prepares the protected metrics in prepaint under the input-specific namespace and paints the same prepared result after text/selection/caret. The original input handler, selection, IME and scroll state are untouched.
- `scroll_edge_fade/tests.rs` is preserved unchanged: five development-only geometry/protection/opacity tests, not executed here. These and their `cfg(test)` wiring remain development artifacts for the parent to preserve and remove before master according to repository policy. No new tests were fabricated to imply execution.
- No parent module wiring, Cargo features, dependencies, backend/vendor changes or additional public APIs are needed beyond the existing `PreparedScrollFade` re-export. Callers must keep the prepared value produced in prepaint and pass it to `ScrollFadeMetrics::paint` in paint; calling prepaint from paint is invalid.

### Actual validation in this replacement stage

Supplied source identity remains parent-reported (no Git execution): HEAD `0b5ea426abfd8d4358489e916273d89474fb190f` plus preserved dirty content; fetched ancestor `34555cec9c0c4c4c1f7a528d739752f414e53edf`. Read `AGENTS.md`, execution guide, design guidelines, full-migration spec and this lane spec before implementation. Scoped source/API reads confirmed the fixed host's prepaint-only API, native prop delivery before paint, `Rgba::opacity` multiplication and `AppView::stop` ownership contract; those reads are static evidence, not runtime proof.

Tool actually run: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.

Commands actually run from this worktree, both exit 0:

```text
rustfmt --edition 2024 --config skip_children=true crates/vega_ui/src/scroll_edge_fade.rs crates/vega_ui/src/scroll_edge_fade/nsh.rs crates/vega_ui/src/scroll_edge_fade/tests.rs crates/vega_ui/src/text_input/element.rs
rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/scroll_edge_fade.rs crates/vega_ui/src/scroll_edge_fade/nsh.rs crates/vega_ui/src/scroll_edge_fade/tests.rs crates/vega_ui/src/text_input/element.rs
```

Static call-site inspection found no GPUI quad/gradient drawing, wheel/pointer handlers, `unwrap`/`expect` or code comments in the fade implementation. NSH root preparation is only in prepaint; publishing is guarded by snapshot inequality. No Cargo/rustc, Git, network, native app or review command was run. Cargo/rustc versions, type-check, Clippy, test execution, required CI and native appearance/input/Tab/AX on all three platforms are **not obtained**, not passed. Parent reported its prior Cargo request denied by permission decision failed closed; this lane did not retry or use an alternate path.

Parent integration checks still required when execution is authorized: compile the final combined source (including both generic and text-input callers), execute the retained development regressions, verify initial mount and rapid scroll/resize/scale/clip changes never paint a stale snapshot, verify receipt wake-up reaches a current frame without a stable publish loop, check pinned header side-mask expansion, gutter/radius/extension geometry, pointer pass-through/Tab/AX, and stop/remount behavior. Independent review is deliberately deferred to the user's centralized four-lane review phase.

### Final source SHA-256

| File | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/scroll_edge_fade.rs` | `65afc6b852a14b4dd7c8023d53ee163d9ef3b6bd37d44ccb05e3131555fd0c9c` |
| `crates/vega_ui/src/scroll_edge_fade/nsh.rs` | `9c8414ec3654a04fcf6b87f4003538dd2b1496af4532cab8cf975a7cd26d07ce` |
| `crates/vega_ui/src/scroll_edge_fade/tests.rs` | `436da4a57307a8cf9201d8a2a361437d1e1e9906f7203806a4fb02c29696e3f5` |
| `crates/vega_ui/src/text_input/element.rs` | `59467997101b00d8cf49ca405b0295101983dc9ec2f1ae2430eb2135c2057a24` |

The document's own hash is reported separately at handoff to avoid a self-referential hash. Replacement-stage content mutations are limited to this document and `scroll_edge_fade/nsh.rs`; the other three source hashes still exactly match the takeover inputs. Implementation is handed back for parent integration, not claimed compiled, reviewed, merged or natively accepted.
