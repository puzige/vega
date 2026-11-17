# Composer and history attachments — NSH presentation

## Acceptance before implementation

Base: `4148291cc5b377362d0ac64aa88b285cc9e06532`. Ownership is limited to `conversation_stream/attachments.rs`, its new `attachments/**` presentation modules, and this document. Call signatures and all other files remain unchanged.

1. NSH paints the composer grid, Remove controls and hover/press/focus states, pending import text, import error text, and the history image-group layout. Existing `image_preview::ImagePreview::thumbnail` is already NSH and may be embedded using the pinned native-component API. No visible legacy GPUI controls are retained in this leaf.
2. Preserve the `ImagePreview` structure/constructor, observation and ownership methods, import/paste/drop/picker/remove/cancel controllers byte-for-byte. Original validated bytes, limits, permissions, generation cancellation, no-NUL and safe-file guards are not changed. Presentation delegates accepted removal only to the existing `remove_image` method.
3. Removal carries the displayed attachment ID, import generation, preview entity identity, thread, synchronous `run_notice_epoch`, and presentation identity. Reception rechecks the exact current attachment, active route, Settings closed, non-preview state, permissions, current owner, active window, live mount and nonempty current clipped geometry. Hide/reopen must not revive an old command. Keyboard and native AX use the same guarded action path.
4. Real native focus handles participate in Tab/Shift-Tab; Enter/Space reject repeats, modifiers and character-preference events. NSH draws focus, not a hidden legacy control. No NSH global shortcut registration or claimed marked-text guarantee is introduced.
5. History remains right-aligned with 72px thumbnails, 8px wrapping gaps and the existing 24px message top inset; composer keeps its wrapping gaps and genuine pending/error lifetime. Image entities/bytes, chronological ordering, preview event routing and parent callers remain unchanged.
6. Surface recovery, if required, replaces presentation only after explicit user action and never replays an attachment import, removal or preview command. Failure and unverified native behavior must remain explicit.
7. At Remove business dispatch, synchronously read the current GroupHost: its current mount must be pointer-identical to the control's parent mount and its Surface must exist with no current error. A failed group must reject an already queued Remove even while its asynchronous error watcher has not yet revoked `GroupMount.active`. Keep all existing control-Surface, source/image/epoch, mount and geometry guards. Regressions exercise the production group decision with a real failure watch token before watcher revocation, and reject a replaced group mount; no source scans or mirrored decision logic.

## Verification and integration plan

Only read-only inspection, scoped formatting/diff/static checks and source hashing are authorized here. No builds, tests, network, Git mutation, app launch or delegation. The reported baseline 175 passing tests are not evidence for this delta. Parent owns integrated compilation, meaningful production-render callback/guard regressions, independent review, final-head CI and native acceptance.

Native acceptance includes zero/one/many attachments, narrow wrapping, image-only history, mixed text/image spacing, pending/error, remove during import, stale queued remove after generation/route/Settings/preview/hide/unmount changes, pointer/keyboard/AX, clipping/scroll and foreground changes, light/dark themes, English/Chinese, explicit recovery without replay, and image-preview routing. The existing platform marked-text pre-dispatch limitation remains unaccepted by this leaf.

## Stopped implementation

- Both unchanged renderer signatures now return a RenderOnce group that obtains Window. A keyed, paint-free measured element hosts the group Surface; NSH owns wrapping/right alignment, image slots, status text and removal pixels. Native components embed the same already-NSH thumbnail entities, not old visible controls. No core/mod/render caller edits or dependency changes were made.
- Remove reception checks displayed ID + generation + preview entity + immutable byte allocation identity/length, thread, synchronous stream epoch, current route/Settings/preview/permissions/owner, active window, Surface identity/error, active parent mount, per-control mount, current clipped bounds, viewport, foreground height and window size. Stopping a group explicitly revokes its old mount even while a previous frame retains a reference. Late retired-Surface errors are identity-checked.
- Real native focus handles and native AX Click use the same guarded path. NSH paints focus/hover/press. Group updates are partitioned so unchanged geometry publications do not rebuild native thumbnail nodes; removal callbacks retain displayed identity rather than borrowing new authority. Original controllers and image preview events remain unchanged.
- Separate group recovery is explicit and presentation-only. No import/remove/open action is replayed. Native input interception for its paint-free host is handled explicitly; its true focus/AX/pointer behavior still needs acceptance.

## P2 group-failure dispatch correction

- Fresh reads retained the parent's two `gpui_kit::IntoElement` derive fixes, `0.0_f32` literal correction and synthetic `nsh_core::ROOT_ID` ancestor traversal stop. This correction writes only `attachments/presentation/removal.rs`, its `tests.rs`, and this document. All other owned Rust file hashes remain unchanged from this correction's starting reads; original pixel coordinate/size expectations are untouched.
- `RemoveHost::activate` now reads its current GroupHost synchronously and calls the production `accepts_group_mount` decision with that host's current lease and Surface error receiver. It requires pointer-identical active parent/current mounts and a present, error-free Surface. Existing own-Surface, source/image/epoch, route/permission, control mount and geometry checks remain intact. There is no retry or business-command replay.
- Two additional development regressions are **UNRUN**: a production-rendered Remove is queued, a real watch failure is published while the mount remains active and the error watcher has an unconsumed update, then the production group decision rejects dispatch; a second test rejects replaced, missing-Surface, inactive and released mounts, with healthy matching-mount positive controls. They exercise the actual group decision, not the complete GPUI controller/window path or native input delivery.
- Current correction checks: HEAD `4148291cc5b377362d0ac64aa88b285cc9e06532`; `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; `rustfmt --edition 2024 --config skip_children=true --check` passed for all six owned Rust files after fixing formatter-reported assertion layout. Scoped `git diff --check` passed. Formatting is not compilation or test execution.
- Parent-reported prior integration: 193 full-run tests, 192 passing and one ancestor-frame harness failure; after the synthetic-root fix, the single attachment geometry test passed with 192 filtered. These reports precede this correction, were not rerun here and are not evidence for its two new tests. The older 175-test baseline is not delta evidence either.
- No builds, Cargo/rustc, tests, network, Git mutation, app launch, delegation or shared-file writes were performed for this correction. Independent review and final integrated validation remain pending.

## Initial implementation checks (historical, not rerun evidence)

- Source HEAD inspected before implementation and again after Rust stop: `4148291cc5b377362d0ac64aa88b285cc9e06532`. Concurrent changes outside this leaf were observed and left untouched. No fetch/update was performed because the user prohibited network/Git mutation and fixed the source HEAD.
- Formatter: `rustfmt 1.8.0-stable (1159e78c47 2025-09-14)`. Scoped `rustfmt --edition 2024 --config skip_children=true --check` passed for all six Rust artifacts below. Scoped `git diff --check -- crates/vega_ui/src/conversation_stream/attachments.rs` passed. Formatting parses syntax; it is not a type check.
- A read-only Python source comparison against the exact base confirmed the complete prefix before `render_attachments` is byte-for-byte unchanged except the added presentation module registration. Thus the struct, constructor, naming, observation/ownership, paste/drop/path picker/import/remove/cancel implementations and imports are preserved. Static inventory found no NUL/trailing whitespace in owned artifacts and no added production comments, unsafe, unwrap or expect. Entry-point inventory confirmed the old icon-button/status-notice GPUI render calls are gone. These are source checks, not behavioral proofs.
- Four development Rust tests were added but **not run**: production Remove mount/callback authority; identity replacement rejection; production history native-slot geometry at 180px; production composer pending/error/remove-slot mount without business action. Geometry expectations remain 72px tiles, 8px gaps, 24px top inset, right edges at 180px; the harness sums parent-relative frames. No baseline result is claimed for this delta.
- No Cargo/rustc, build, Clippy, test command, network, Git mutation, app or delegation was run. Cargo/rustc versions were not obtained. Native and cross-platform acceptance, independent review and final-head CI were not obtained.

## Parent integration needs and remaining gaps

Current parent validation: all six attachment regressions passed within full Nextest `cb12b46b-ba67-4897-87bf-1d1c399dbd4e` (203/203, 0 skipped); workspace strict Clippy/fmt/diff checks passed. Independent incremental review closed the synchronous parent-Surface-fault P2 on the recorded removal hashes, with no new finding. The reviewer explicitly distinguished the production callback/parent-mount predicate tests from an unexecuted full `RemoveHost::activate → remove_image` runtime test. See [batch validation](nsh-header-batch-validation.md). Writer-local UNRUN and pending statements above describe the earlier handoff, not current parent execution. Native integration, final committed-head cloud gates and Rust-test cleanup remain pending.

1. Compile/type-check the integrated final head using the parent's authorized toolchain. Run all six attachment development tests (including the two UNRUN group-dispatch regressions) and required regressions; investigate failures at their responsible layer without changing the acceptance geometry or authority guards to match output. The existing 175-test baseline does not cover these new tests.
2. Independently review native component registry timing, measured-height feedback and virtual-list/narrow-resize behavior, current-frame mount/clipping revocation, focus traversal into/out of embedded thumbnail and Remove hosts, native AX and explicit recovery. Live controller tests for stale queued removal across Settings/route/epoch/generation/owner/geometry changes are still needed; the added renderer/identity tests do not prove these host integrations.
3. Exercise the native acceptance matrix above. Source inspection cannot establish visible pixels, focus delivery, AX delivery or image-preview behavior. The pinned marked-text pre-dispatch limitation is retained, not solved here. Three-platform runtime acceptance is pending.
4. Before master, preserve the six development tests/patch and results then remove both `tests.rs` files and their two `#[cfg(test)] mod tests` declarations under the repository's Rust-test delivery policy; run the required final-head gates after cleanup. No leaf commit or merge exists. This handoff is an implemented, statically checked leaf, not a full #907 migration or production acceptance claim.

## Stopped Rust SHA-256

Paths are relative to `crates/vega_ui/src/conversation_stream/`.

| Artifact | SHA-256 |
| --- | --- |
| `attachments.rs` | `a73b8af8278cd4ea35d5db1987045eaa9505655b174541f50cfb754b62b9c437` |
| `attachments/presentation.rs` | `8a4e1a81c49a57fa7c7a572223b2d0b284bb6a2b80f6fb511489604761df2214` |
| `attachments/presentation/pixels.rs` | `148eecf0d4b86dd52593bfba4feecd8c31fd1b32c63b3776b9823b921f74a2a6` |
| `attachments/presentation/removal.rs` | `4efacf2cdf522eab4bb77a5d7e3b8ed3ee95618d753754f1aea21fe8917f4ed7` |
| `attachments/presentation/pixels/tests.rs` | `5e896760c110f28e8db01a2b1ee7eb4ef251ba982a11dbcbb7d314a62660b5bc` |
| `attachments/presentation/removal/tests.rs` | `d76a82e1f90671f88cb0bac15fa0b7607592b70f55044cab7cca5366412f322b` |
