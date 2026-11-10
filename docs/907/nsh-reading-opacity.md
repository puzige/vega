# Fixed-version reading opacity backend

## Scope and acceptance (written before implementation)

This increment changes only nsh-platform-gpui 0.4.0 reading/paint state and its existing AppView/Surface/text seams. It does not connect application body/source anchors, modify Core/Kit/Fast, add dependencies, change shaping, or paint legacy text. The development base is user-reported HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, dirty. Git, network, builds and native execution are forbidden for this writer; compilation and runtime acceptance belong to the parent.

1. A publication fence is captured from an exact current ReadingSnapshot and the Surface's current publication token. Full replacement validates the fence, exact snapshot, UTF-16 scalar boundaries, text indices, lexicographically ordered nonoverlapping nonempty ranges, and finite opacity in [0, 1] before changing anything. Rejection leaves the old publication intact. Uncovered text paints normally. Public snapshot fields do not bypass private mounted-view/content identity checks.
2. Successful replacement rotates the opaque publication token and returns a new fence. Old replacement/clear requests cannot overwrite or remove a later publication. Guarded clear checks exact Surface and publication token, rotates the token and releases the publication; it may clean up its own publication even if its snapshot has since expired. Surface stop retires publication authority before observer callbacks. Surface invalidation retires paint leases, including mount/content/selection/layout/ancestor ABA; installation and clearing use the independent paint-only `repaint_reading` queue and do not themselves invalidate geometry. Existing `repaint` retains its dirty semantics, including the already-reviewed preview/scrollbar paths.
3. One Surface holds at most one current area's publication, not a global registry or publication history. Drawn text holds only a weak publication reference; the publication fence holds a weak Surface reference. Replacement releases old ranges/content. Every paint pass revalidates the current exact snapshot and actual mounted content before applying opacity. Expired publication paints original native content normally, never stale cached content. Selection changes require recapturing a snapshot and fence; ReadingSnapshot::matches remains unchanged.
4. No text/style props, engine update, text/layout Arc, selection endpoint/revision, geometry lease or layout identity changes on opacity publication/clear. No reading::changed or selection listener publication is emitted. Container backgrounds, attachment children, copy/AX text and selection highlights are not faded. Hidden/clipped native text retains existing culling. Reduced motion, hidden animation timing and finish policy remain application-owned; empty replacement/guarded clear restores normal paint without changing content.
5. Native glyphs use their existing Glyph.style UTF-16 cluster origin; boxes use their existing style origin. Indivisible ligature/combining/ZWJ clusters crossing segments follow that origin, matching existing NSH style attribution and fixed Fast 0.1.9's glyph-origin decoration convention. No glyph splitting, reshaping, max-alpha policy, x-coordinate text reconstruction or independent-character promise is introduced. Bidi retains native visual order. Ellipsis follows its existing style origin; attachment child rendering is unaffected.
6. Opacity reaches actual native glyphs including color emoji, text shadows and content-shadow silhouettes, underlines/strikethroughs (all existing stroke styles), and inline text backgrounds. Selection highlight and surrounding view decoration remain unchanged. Native rounded inline background geometry must remain intact: opacity partitions clip the original joined rounded shape, rather than inventing rounded boxes at fade boundaries. Emoji uses only the public default Interactivity::paint opacity wrapper with no element/global ID, hitbox, focus handle, listeners, prepaint or layout call. No interaction/dispatch target is added.

## Verification requirements

Development host functions must exercise actual mounted snapshots, atomic range rejection (surrogates, ordering, overlap, NaN/infinity/out-of-bounds), full replacement, stale fence/clear ABA, selection/content/remount/stop expiry, bounded replacement ownership and weak lifetime. A real-font TextSystem route must inspect actual GpuiLayout glyph/box cluster attribution, bidi/emoji/combining/ligature layouts, and unchanged layout/geometry/snapshot identity across opacity-only changes. Production paint planning must expose the same opacity used by glyph/background/decoration/shadow paths to development observation; synthetic metrics alone are not shaping acceptance.

Parent must wire development host exports without this writer editing lib.rs/manifests, run the scoped official toolchain regressions, and retain exact command/results/source identities. Native paint/pixel fidelity, input/focus/dispatch neutrality of the emoji wrapper, actual platform emoji fallback, reduced-motion/finish integration and three-platform acceptance remain separate runtime checks. Static inspection or unrun host functions are not passed tests.

## Initial source identity

SHA-256 before this increment:

- reading.rs: `e59b5d300bf3a64c4c5708d80bf3b96812bc6c30174f9f91798b65a60339343e`
- app.rs: `247620592e2f1df08b9c083f791706dbfcc80eec8c3c01246d9df8d367b7f200`
- surface.rs: `eda7db67ae67647d1c4e06856a2f73905eaa483040c924b99c70b60977ee43bb`
- text.rs: `b4fd1bb159aac5dc431b387ae11ae3593f02d8b44028d2f8ac1e6bf87764c335`

## Implemented interface and parent wiring

Public types are available under the existing public `reading` module:

- `ReadingOpacityRange { text: usize, range: Range<u32>, opacity: f32 }`.
- `ReadingOpacityFence`: cloneable opaque snapshot/publication capability; no public identity fields.
- `AppView::reading_opacity_fence(&ReadingSnapshot) -> Option<ReadingOpacityFence>` captures an exact native snapshot, not the caller's mutable public fields.
- `AppView::replace_reading_opacity(&ReadingOpacityFence, Vec<ReadingOpacityRange>) -> Option<ReadingOpacityFence>` replaces all ranges and returns the only new publication token. Empty ranges publish normal opacity. Carry the returned fence into the next frame; reuse of the old fence rejects.
- `AppView::clear_reading_opacity(&ReadingOpacityFence) -> bool` clears only the currently owned token. A fence captured before a newer publication cannot clear it. After structural invalidation/stop it rejects; draw already falls back to normal native opacity.

App body is deliberately not connected. Obtain a current snapshot after the same native mount/content/projection has been validated; convert retained source anchors to native UTF-16, validate application root/window authority, capture fence and publish. Reacquire after selection/content/mount changes. Finish/reduced-motion/hidden-state scheduling remains in the existing app StreamFade owner, not this backend. Do not rebuild attributed text to publish opacity.

Surface owns one `OpacityState` strong publication; DrawnText stores a weak publication lease and native text index. Successful replacement releases the previous publication. Every draw/shadow/silhouette checks actual current snapshot, Surface object, token and publication object before using ranges. `geometry_invalidated` now also retires opacity, covering its existing origin-change hook without editing render.rs. Stop clears opacity before the reading observer can run. Original `repaint` dirty behavior is retained; a separate reading paint queue avoids poisoning `ReadingGeometry::current_for` and participates in flush/frame scheduling. No selection observer is called by opacity operations.

Actual glyphs (ordinary and emoji), text-shadow copies and non-emoji content-shadow silhouettes take alpha from `Glyph.style`. Background and decoration partitions use `CBox.style`; original joined background shape/radius and original full decoration stroke/phase are retained underneath nonoverlapping clips. Selection highlight stays outside these partitions. Existing native content-shadow exclusion of emoji silhouettes is unchanged, not replaced by a new emoji shadow implementation.

### Development host exports required from parent

This writer did not edit lib.rs or manifests. Under the existing `vega-host-regressions` host_regressions module, parent can re-export these functions from `crate::reading::opacity_host_tests`:

1. `opacity_rejects_invalid_ranges_atomically_and_full_replaces`
2. `opacity_keeps_snapshot_layout_and_geometry_exact`
3. `opacity_uses_native_cluster_origins_and_original_background_shape`
4. `opacity_retires_selection_content_mount_stop_and_surface_aba`
5. `opacity_cross_text_publication_rejects_forged_snapshot_and_old_area_clear`
6. `opacity_publication_lifetime_is_bounded_and_does_not_retain_surface`

Each accepts `Arc<gpui::TextSystem>`. Use the existing main-thread harness=false geometry route with `gpui_kit::platform::current_platform(true)` and its real font-backed TextSystem; do not silently substitute uniform/mock metrics. The emoji case requires an actual emoji glyph in that fixture and fails rather than skipping when unavailable. Development observation calls production `for_draw`, background planning and cluster band helpers; it is not a GPU scene/pixel or actual Interactivity dispatch test. These functions and their exclusive observation helpers must be archived/removed before master under repository policy.

## Parent integration and actual validation

The parent exported all six functions and retained the original six geometry regressions in the main-thread real-font harness. Official Cargo/rustc 1.98.0 compiled the integrated bytes. The first unified attempt passed 279 application, 32 host and **12 real-font geometry/opacity cases** before the separate mounted-body runner panicked. After later paste/list fixes, the next attempt passed **284 application, 33 host and the same 12 real-font cases**; the mounted-body runner timed out with no result. Neither attempt is an overall green gate. Raw outputs are `target/nsh-integrated-opacity-test.*` and `target/nsh-integrated-fixture-test.*`; exact commands and responsible-layer diagnostics are in [integrated validation](nsh-reading-editing-integrated-validation.md).

The non-implementer independently reviewed the frozen six Rust identities below (`01M6WM385AEQFH0ABFNMGM1S64`), checked beginning/end hashes, and found no findings. That review ran no tests. It traced snapshot/fence ABA, bounded/weak storage, actual cluster-origin planning, preserved rounded/stroke shape, selection-highlight isolation and the fixed Fast emoji opacity path. This does not prove GPU pixels or runtime interaction neutrality. App per-source mapping/scheduling is still not connected; finish/reduced-motion/hidden behavior remains an application integration requirement.

## Original writer static validation and unrun gates

Executed only scoped rustfmt from the preinstalled official 1.98.0 toolchain, with `--edition 2024 --config skip_children=true`, on the six owned Rust files; final `--check` exited 0. Actual formatter reports `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. New production opacity.rs contains no comments, unwrap/expect or added lint allow; new AppView methods add no missing-docs allow. Existing file comments/attributes were not globally rewritten.

No Cargo/rustc invocation, build, test, native execution, Git, network, dependency/registry modification or delegation occurred. Cargo/rustc version and runtime results for this delta are not independently obtained; parent-reported earlier tests predate this implementation and are not its validation. Parent still needs production compile/lints, six new host cases plus existing selection/geometry/preview/editing regressions and independent review. Native pixel/scene evidence must cover color emoji alpha, partial rounded inline backgrounds, all decoration styles (especially wavy/dotted boundaries), shadows, bidi/ligature/combining/ZWJ boundaries, selection highlight immunity and no added hitbox/focus/dispatch. Three-platform acceptance and application finish/reduced-motion integration remain unverified.

Risk notes: full exact snapshot validation is intentionally performed for each text paint/shadow pass and opacity lookup scans the current text's bounded ranges; large documents may need performance measurement, not weakened identity checks. Background/decoration alpha boundaries clip the original shape and may expose device-pixel antialiasing seams requiring native observation. Public default Interactivity::paint was statically traced through no-ID/no-hitbox/default-empty-listener paths, but its runtime neutrality remains a native gate. No new renderer or unapproved package capability was required statically; compilation/runtime compatibility is not yet established.

## Frozen implementation identity

All paths below are relative to `vendor/nsh-platform-gpui/src/`; SHA-256 after scoped formatting:

- reading.rs: `c24de0f41d0e3b4c9f1a227e9006ffc96b3c3bcc36cac72e0b968f8092a9fac9`
- app.rs: `96131c1008e8111e573f6c9291ec17963da459cc671057bca8bc63ebdd65f780`
- surface.rs: `54a978bbf362e44a7b55140e3e0362ad196b8cef0a7ec7f636ce5b79247ecb0a`
- text.rs: `91d6ea7c2737175abb4eefcc8d4c75641479b47e897f9f3dc6898b68bceea784`
- reading/opacity.rs: `998ecb160625e391fe9312d72b978e3ac18a86919a089a229e2b3136b5d16e82`
- reading/opacity_host_tests.rs: `c26d64cc6a687a582f77971a190dde5f532dd3e49536fa5ca07b90f69099369e`
