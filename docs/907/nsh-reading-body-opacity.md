# Production reading body opacity integration

## Scope and acceptance

This application-only increment consumes the existing fixed-version reading opacity API. It changes no backend, dependency, shared root, renderer or shaping. The ordinary message entry in render_rows.rs is parent-owned. Existing body::render now consumes MessageCopy.stream_fade directly, so ordinary body paint is enabled without changing that file. This writer additionally supplies render_model for content-revision caching; the parent must replace the existing Content::from_model plus render pair with render_model(model, copy, *colors, window, cx). Thinking is connected in the owned thinking_body module.

- Build displayed PartKey to actual Fragment byte-range bindings only when Content is built from a model. Validate every consumed displayed scalar against the exact fragment text in structural model order, including list markers/checkboxes, physical code lines and table cells. These are not Markdown-source offsets. No substring search may substitute an equal string from another part.
- On paint, obtain a fresh current ReadingSnapshot under the existing owner/root/mount/projection checks. Verify the complete mounted fragment sequence and exact tag text, then intersect the captured FadeFrame UTF-8 ranges and convert only scalar-safe endpoints to native UTF-16. Atomically replace sorted nonoverlapping ranges through the current snapshot's publication fence.
- Per-frame updates do not sync the engine, rebuild the document, change attributed text, selection, projection or geometry identities, or fade containers/copy/selection highlights. Cluster-origin behavior remains the backend's existing style attribution.
- Preserve StreamFade's 700 ms normal, 150 ms finish, 24 ms queue spacing, 800 ms queue cap and 2500 ms age cap. Reduced motion/inactive/hidden or invalid ownership clears current publication with its guarded fence, and clears active animation state rather than replaying old fades on reveal. Initial unacknowledged mounting clears only publication, not the timeline, while waiting for native layout acknowledgement. Publication is removed on projection changes and unmount. Selection changes obtain a new exact snapshot/fence, never relaxed matches.
- New tests cover exact displayed mapping through styled multibyte fragments, repeated parts, list/checkbox/table/code boundaries, mismatch rejection, stale frame rejection and UTF-16 scalar boundaries. Parent owns Cargo and mounted/native regression execution. Static formatting is not runtime validation.

## Source anchor boundary

The existing AppView reading interface exposes current snapshots, retained geometry and opacity, but no synchronous conditional ancestor-scroll actuator. ElementRef scroll_to remains an asynchronous unconditional native command. An app ticket or an extra query cannot close the gap between geometry validation and command execution after a newer native user scroll. Therefore production source-caret restoration remains BLOCKED pending parent approval/implementation of the exact conditional seam described in nsh-reading-source-anchor.md. This increment does not alter the existing row anchor, follow policy or stale checks and does not claim full source-anchor delivery.

## Implementation handoff and validation

Production files: selection_menu/body.rs, new body_opacity.rs, stream_fade.rs and thinking_body.rs. Development tests: new body_opacity_tests.rs (five cases) and one added stream_fade/tests.rs hidden/reveal case. Existing fade timing/range, body selection/source and preview tests remain intact. No vendor, Core/Kit/Fast, manifest, dependency, registry, shared root or runner files were written.

The mapping is retained in Content. It walks native structured blocks against the model's logical lines; code offsets are physical displayed-line offsets within the exact code fragment, table ranges retain row/column PartKey, marker trailing layout space has no fabricated ink and checkboxes consume their own part. Nested list marker ownership follows the model's outer override and does not publish duplicate ranges. Paint uses a fresh complete matching snapshot and native test-id tags, rejects invalid scalar boundaries/alpha/overlap, captures the current fence, and stores only the latest successful fence. Projection change, invalid ownership, empty/disabled frame, failed mapping and unmount remove the current owned publication. The unmount callback is deferred outside App borrowing and checks an opaque mount token so an old callback cannot clear a newer mount.

Parent small wiring in render_rows.rs selectable_markdown_content:

```rust
let body = super::selection_menu::body::render_model(model, copy, *colors, window, cx);
```

This replaces its current two Content::from_model/render lines. Without that parent wiring, existing paint is enabled but the old caller still constructs Content on each render; the cache/no-rebuild acceptance is not complete. Thinking already rebuilds Content only on source_revision changes. Per-frame paint itself never calls Surface.publish, document construction or engine updates.

Executed by this writer: scoped official rustfmt 1.98.0 toolchain, edition 2024, skip_children=true; final check status is recorded in the stopped handoff. No Cargo/rustc/build/test/native/Git/network execution. New tests are UNRUN; previous parent-reported 287 app / 33 host / 12 real-font results predate this delta. Parent must compile the final integrated bytes, execute the five mapping cases, hidden lifecycle case and existing StreamFade/selection/geometry/copy/preview regressions, then the mounted body suite without removing its budget. Source-anchor tests remain development-only and not production acceptance.

Still unverified: runtime paint ordering and scene alpha; selected single/cross text staying current throughout normal/finish frames; no geometry invalidation from publication; repeated selection changes recapturing fences; offscreen/unmount/rebind/stop cleanup; inactive/reduced-motion no residual half-opacity; real emoji/ligature/combining/bidi/stroke/inline-background/shadow appearance; parent render_model wiring and three-platform native acceptance. Indivisible clusters follow existing NSH style-origin alpha, not independent character opacity. The separate conditional-scroll backend approval remains the concrete source-anchor blocker.
