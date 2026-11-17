# Exact retained reading geometry package

## Parent integration status

The original writer-only status below is historical ownership evidence, not the present integrated state. The parent connected every-invalidation, resize/scale-before-observers, and changed render-prepaint origin hooks. The integrated backend received independent source review with no findings; later scroll-decoration repaint received its own focused review. Actual movement and layout retain geometry revocation. Only paint-only scroll decoration redraw preserves a stationary geometry lease; no same-numeric-rectangle reacquisition exemption exists.

The latest executed local batch passed 258 application tests, 30 backend host tests, and six main-thread real-font geometry cases, with zero failures/ignored. The sixth case drives the actual shared scrollbar fade state machine and rejects scroll/return ABA. These are development process-local results, not native paint, production ancestor compatibility, full BodyState preview lifecycle, or macOS/Windows/Linux acceptance. See [integrated validation](nsh-reading-editing-integrated-validation.md) for the exact command, current repair identities, test-harness diagnostics, and review limits. Original hashes below describe the initial handoff only.

## Original writer status and ownership

Implemented inside the approved same-version NSH 0.4.0 reading patch. This package changes only `reading.rs`, `text.rs`, `app.rs`, new `reading/geometry.rs`, new `reading/geometry_host_tests.rs`, and this document. No shaping, renderer, clipboard, editing, source-fade, wheel, manifests, app body, shared exports, or test-wrapper changes were made.

Source is ready for parent integration and independent review, not runtime acceptance. The current tree deliberately cannot issue geometry leases until the parent connects the invalidation hook below. No Cargo command, compilation, tests, native app, Git, network, or CI was run by this writer. Earlier 252 application / 30 host passes predate this package and are not its validation.

## Public API implemented

`nsh_platform_gpui::reading::ReadingGeometry` is opaque and `Clone`; fields are private, it retains exact `TextLayout.0` Arcs and `ReadingSnapshot` identities, and owns only a weak Surface reference.

```rust
AppView::reading_geometry(&self, snapshot: &ReadingSnapshot) -> Option<ReadingGeometry>
AppView::reading_geometry_is_current(&self, geometry: &ReadingGeometry) -> bool
ReadingGeometry::range_rects(&self, text_index: usize, range: Range<u32>) -> Option<Vec<[f32; 4]>>
ReadingGeometry::caret_rect(&self, text_index: usize, position: nsh_core::TextPosition) -> Option<[f32; 4]>
ReadingGeometry::hit_test(&self, text_index: usize, point: [f32; 2]) -> Option<nsh_core::TextPosition>
```

Rectangles are `[x, y, width, height]`, in Surface-local logical points, independent of the physical pixel ratio. Text indexes refer to the captured snapshot ordering; offsets are UTF-16 scalar boundaries. Inverted/out-of-bounds ranges, split surrogates, invalid text indexes, unsupported geometry and stale leases return `None`. An empty or fully clipped valid range returns `Some(Vec::new())`.

Range queries call the retained `GpuiLayout::selection_rects` and intersect with Surface viewport, every clipping ancestor, and the text view's own content-drawing bounds. Hit queries require a finite point inside that effective rectangular clip, inverse-map to layout coordinates and call `GpuiLayout::hit_test`; this is nearest-caret hit testing, not proof of glyph ink or link ownership. The returned `TextPosition` preserves upstream/downstream affinity. Caret queries call `GpuiLayout::caret_rect` and intentionally remain **unclipped**, including off-viewport positions, for source-anchor pixel offsets. A caret rectangle must not be used directly as a clipped interactive region.

Host conversion to window coordinates adds the separately validated current Surface host origin; no geometry result authorizes copy, navigation or mutation. Recheck app root/owner/source authority independently. An async completion must recheck this lease as well as app authority. Reacquire after invalidation; no old lease becomes current again when coordinates return to their earlier values through a hooked mutation.

## Required parent hooks outside this writer's scope

Original evidence: `Surface::invalidate` at the initial `surface.rs:517–535` coalesces only a boolean and `drawing()` clears it. Therefore `invalidated() == false` plus equal numeric geometry cannot detect scroll-and-return, reparent-and-return or layout-and-return after repaint. `Surface::resized` changes scale/root frame before calling invalidate; callbacks run before the final invalidate. `render::SurfaceElement::prepaint` at the initial `render.rs:750–759` writes `surface.origin` without invalidating if only origin changes.

This package provides:

```rust
pub(crate) fn crate::reading::geometry_invalidated(surface: &Surface)
```

The function stores a fresh retained `Rc<()>` identity token in a UI-thread registry keyed to the actual weak Surface, on **every** invocation, without an integer wraparound or boolean ABA assumption. Dead Surface entries are pruned on invalidation. Capture requires an existing token; there is no numeric-only fallback. No patch to Surface/render was made here.

Parent must wire all of these before activation:

1. Call `crate::reading::geometry_invalidated(self)` as the first operation in `Surface::invalidate`, before `invalid.replace(true)`, including when already invalid. This covers the existing view changed/moved, scroll, mount and animation redraw paths.
2. In `Surface::resized`, compare incoming width/height/scale to the existing root frame/scale **before** `scale.replace` or root/environment mutation. When any differs, revoke using the same helper before mutation and before resize observers. Do not revoke on unchanged size every prepaint; that would unnecessarily defeat stable leases.
3. In `render::SurfaceElement::prepaint`, compare incoming host origin with `surface.origin.get()`, revoke via the helper before changing the origin when it differs, then store it. Unchanged origin must not revoke every prepaint. This is required for origin-away-and-return ABA, not just current numeric mismatch.

Surface generation/batch and `ReadingSnapshot::matches` remain unchanged and are additionally checked. Each query checks the live owner, invalidation token, `invalidated()`, origin/scale/root frame, actual mounted view objects and ancestor chain, finite frame/shift/scroll origin/content dimensions/mirroring, effective transform, clip, visibility/opacity, exact attributed content Arc, exact layout Arc and content inset. `window_hidden` fails closed. Scope does not promise detection of arbitrary direct private-state mutation that bypasses both existing changed/moved/invalidate paths and the required resize/origin hooks.

**Until the first parent invalidation hook has executed, `AppView::reading_geometry` returns `None`.** Do not treat the missing hook as a reason to weaken the capture guard or tests. Numeric stamps detect immediate mismatch, while the token hook is specifically what prevents stale leases reviving after repaint/ABA. Stop is additionally fenced by existing generation and mount/snapshot retirement; dropped Surface is rejected by weak upgrade.

## Supported and rejected geometry

Supported: real retained GpuiLayout; single- and cross-text snapshots; proportional wrapping, ligature/bidi/combining/emoji geometry as represented by that layout; nested rectangular clipping; text content inset; scrolling/content inset; mirrored scroll placement and sticky shift through the existing `child_origin`; finite positive **uniform** axis-aligned scaling and translation, including a supported root transform. Composition follows actual renderer parent/local multiplication, not `visible_bounds`' rotated AABB.

Rejected rather than approximated: rotation/skew/perspective or any matrix outside positive uniform 2D scale+translation; reflection/zero/non-uniform scale; rounded clipping at any clipping view, even when a particular target appears away from its corners; non-finite frame/transform/scroll/content/point/output; hidden/concealed/transparent ancestors; foreign or detached/recycled mounts; foreign layout type; stale layout/content/selection/batch/generation/host state. The whole lease is conservatively rejected if any snapshot text has unsupported geometry, and any geometry-invalidating Surface mutation revokes the whole lease, including an unrelated generic invalidation. The parent-added narrow scroll-decoration `repaint()` does not invoke that mutation hook; real displacement and relayout still do.

Uniform scale is an intentional conservative subset: `DrawCx::transform_scale` uses the larger axis for glyph size, so supporting arbitrary non-uniform rectangle transforms would not establish exact glyph-rendering parity. No renderer changes or axis-width estimates were introduced. Rounded and rotated `visible_bounds` AABBs are not used.

## Development regression sources for parent export and execution

New module: `reading/geometry_host_tests.rs`, compiled only with existing `vega-host-regressions`. Parent owns the `lib.rs::host_regressions` re-exports and test wrappers. Preserve the existing 25+5 host regression functions unchanged. These five additional functions each accept an `Arc<gpui::TextSystem>` supplied by the host:

- `geometry_uses_shaped_wrap_affinity_bidi_and_utf16`
- `geometry_clips_ranges_and_hits_but_preserves_unclipped_carets`
- `geometry_revokes_scroll_resize_origin_scale_and_repaint_aba`
- `geometry_rejects_layout_content_mount_selection_and_surface_retirement`
- `geometry_accepts_translation_uniform_scale_and_rejects_unsupported_clips`

The fixture uses the actual `GpuiTextEngine::prepare/measure/layout`, not handbuilt boxes, and asserts installed font names and nonempty real layout lines. It requires proportional W/i advances, actual soft wrapping with two affinity carets, correct RTL direction/hit, split-surrogate rejection, clipped mounted single/cross-text geometry, unclipped scroll-anchor carets, real layout range/caret/hit delegation, layout replacement without batch change, content replacement, selection changes, invalidation/repaint ABA, resize/scale ABA, explicit origin fence, detach/remount, stop and rejected transforms/clips/non-finite input. It does not load the missing gallery font or start its own native platform. Supply a real font-backed host TextSystem on the appropriate platform thread; a mock text system or zero-font environment is a concrete validation blocker, not a passing skip. Origin fence coverage invokes the proposed helper explicitly; actual render-prepaint hook integration still needs parent inspection and native/integration evidence.

These are source-only development regressions, **not run** here. They must be exported/wrapped and exercised by the single parent build owner after the hooks land. They do not claim native paint, keyboard/AX, preview lifecycle, or application source-anchor acceptance. Follow existing development-test removal policy before master.

## Static validation and source identities

Scoped rustfmt tool: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, edition 2024, `skip_children=true`. Formatting and final checks are scoped to the five owned Rust paths; this writer ran no compiler/toolchain query beyond rustfmt and no runtime tests. Static source inspection confirms the three existing GpuiLayout methods only gained crate visibility and AppView only gained the two accessors; no production selection identity logic changed. Parent must perform compilation, all existing regressions, new real-font regressions and independent review on the integrated bytes.

| Owned Rust file | SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/reading.rs` | `e59b5d300bf3a64c4c5708d80bf3b96812bc6c30174f9f91798b65a60339343e` |
| `vendor/nsh-platform-gpui/src/text.rs` | `b4fd1bb159aac5dc431b387ae11ae3593f02d8b44028d2f8ac1e6bf87764c335` |
| `vendor/nsh-platform-gpui/src/app.rs` | `247620592e2f1df08b9c083f791706dbfcc80eec8c3c01246d9df8d367b7f200` |
| `vendor/nsh-platform-gpui/src/reading/geometry.rs` | `7c172b676c454f402c2da77a043a36e39619a4b1fa7760855f9939b5eeaade44` |
| `vendor/nsh-platform-gpui/src/reading/geometry_host_tests.rs` | `300756d4967805c8dd558a6eb8d3f6da81f204c7c5f256e957bf316bc5123ba0` |

The document's own final SHA is supplied separately in the stopped-writer handoff to avoid a recursive self-hash.
