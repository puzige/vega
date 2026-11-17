# ArtifactCard and ArtifactGroup NSH migration

## Authority and scope

Owned leaf: `artifact_card.rs`, `artifact_card/`, this document. Baseline
`bc7d8ae0d91649a8ed0fb48679eb43d6efd6ce4d`; original leaf SHA-256
`124aa7a985096807cb9279c3c54ea3d7c0914e4a2e46f45a5f9af14b678fb996`.
The existing #907 full migration authorizes implementation. No Git mutation,
build/test execution, application operation, shared files, dependencies or IO.
Parent owns coordinated compilation, regression execution and final CI.

## Acceptance matrix (before implementation)

| Case | Required outcome |
| --- | --- |
| Compact/isolated card | NSH document icon, directory/name, bounded geometry, hover/focus Open-in trigger, six-target menu; original disabled reasons and opening state |
| Legacy virtual rows | Separate stable per-row surfaces; exact label/action/code/error row identity and ROW_HEIGHT, original bounded preview lines and empty handling |
| Group | NSH header/review/files/toggle/errors, original total/visible labels, original height/row_count and dedup/capture_order subscriptions |
| Preview | Only explicit eligible activation emits original bounded request; read-only preview retains original preview eligibility; mount/reload emits no IO |
| External open | Exact displayed thread/project/card/current file, fixed six-target allowlist, !reading_preview, !opening; handoff outcome is not claimed as completed |
| Clear | Original cancellation event and inline reset, never history deletion; existing ArtifactClear/ArtifactActivate bindings retained and guarded |
| Errors/empty | Original translated error mapping; group errors without files remain visible, empty group is zero-height; no new status claims |
| Stale/recycle | Reject old Surface/mount, projection revision, metadata/file, menu, opening, reading mode, group order/review availability; hidden/unmounted leaf cannot execute queued actions |
| Keyboard/AX | Real native host FocusHandle.tab_stop, displayed enabled order, reverse descendant entry, no held Enter/Space, external Tab boundaries, Escape menu close, mouse selection consistency, NSH focus ring and native AX active descendant |
| Narrow/theme/i18n | Existing tokens, label truncation, per-row/menu bounds and language/theme updates; no old GPUI product pixels or arbitrary path/URI |
| UI recovery | Surface reload is UI-only; no automatic preview/open/review/clear replay; controller lifetime independent of presentation |

## Independent-review corrections (acceptance before correction)

- P1: A compact card remains exactly 36 logical pixels high and a one-file
  group retains its normal height. Their six-target menu must be a separate
  NSH AppView with its own sufficient viewport, hosted in the original
  window-level anchored/deferred route. An NSH overlay inside the card AppView
  does not satisfy this acceptance. The native shell is paint-free layout,
  event/focus/lifecycle hosting only. Menu identity, viewport geometry, mount
  lease and anchor visibility must fence queued actions; closing, recycling,
  hiding or replacing the menu invalidates them. All six targets, including
  the bottom target, need reachable measured bounds without enlarging rows.
- P2: Pinned NSH AX Click falls back to Press unless an explicit `activate`
  action is declared. Every NSH button must declare that action and route it
  separately from pointer Press. AX activation requires the current focused
  selection and current displayed measurement, current viewport/clip revision
  and all existing business authority checks. Missing, stale, clipped,
  unselected or replaced-menu evidence rejects AX Click while ordinary pointer
  activation remains available under its existing checks. Native AX proxies
  use the same guarded policy; accessibility labels/roles remain meaningful.
- Preserve all nine existing development tests/assertions and add bounded
  behavioral regressions for menu viewport/normal-row geometry and guarded AX
  activation (including pointer independence). Runtime acceptance remains
  unverified until the parent can execute it. The parent's attempted compile
  was permission-denied before dispatch, not a compiler or test result; this
  writer must not try an alternative build route.

### Trigger-geometry follow-up acceptance

The original P1 viewport and P2 AX-route findings remain statically closed;
this follow-up corrects only the trigger rectangle shared by menu visibility
and outside-click exclusion. Use the current snapshot/mount and host layout,
not the unrelated 8px toolbar inset. NSH compact rows have 12px horizontal
padding, a 1px top border and a fixed 24px trigger centered in the remaining
35px content height. At width W the compact rectangle is
`[W-36, 6.5, 24, 24]`; group body side borders inset it one additional pixel
and its y coordinate includes the current file's row top. Both decisions must
use that same rectangle. A pointer in the leftmost 4px must not dismiss the
menu before Press; clipping only the trailing padding must not hide the menu,
but clipping the actual trigger must. Recycled/unmounted snapshots must not
exclude outside clicks. Preserve all twelve existing development tests and
assertions; add bounded geometry/visibility/pointer regression coverage, with
no build/test/app/git mutation or shared-file operation by this writer.

## Implementation plan

1. Preserve original public signatures/events and service/controller bodies. Replace only visible renderers/helpers and add leaf presentation state/revision fences.
2. Build Send+Sync snapshots from displayed nonsecret metadata, native-owned route/controller state and opaque projection identities. Keep Entity/subscriptions/mount leases on native thread.
3. Render group content in one NSH Surface; isolated compact and each legacy row use independent presentations. Resolve actions back to current native-owned card entities after full snapshot comparison.
4. Use a paint-free native keyboard/AX host, shared button traversal policy, NSH selected rings and a separate window-deferred NSH menu Surface. Constrain legacy controls with measured horizontal reveal if needed; no delayed native focus.
5. Add focused development tests of production eligibility/authority and selection policy. Scoped rustfmt and static checks only; report exact hashes, parent hooks and unrun native/compile coverage. Archive/remove development Rust tests before merge.

## Evidence and limits

Pinned registry inspected: nsh-core/platform-gpui 0.4.0. Ordinary View focusable
is not a native keyboard stop (`collect_stops` includes only Input/Native).
ElementRef.measure reports parent frame plus root page coordinates with ancestor
scroll applied; scroll_to takes absolute positive content offsets. NSH overlays
support anchor refs, outside press, cancel and safe-area placement, but cannot
escape the AppView viewport; compact/group menus therefore use a separate
window-deferred NSH Surface. GPUI is only
the Surface, lifecycle, keyboard/focus and AX host; all product pixels are NSH.

## Resumed implementation handoff

This resumes the preserved implementation, not a replacement. Before editing,
all three provided interrupted hashes matched the working files:

| Preserved file | SHA-256 |
| --- | --- |
| `artifact_card.rs` | `99aaf21e855b9d572a2c0600680860239388fabe3810c7a99f57e51d0068e39f` |
| `artifact_card/host.rs` | `a7d0ede080b4650fbe2e2b1af2599ec3e15284003327ede3bcf879ad88df45ec` |
| `artifact_card/nsh_view.rs` | `91b998bc56807fb1afdb490a05908139f5905efa4a019843b553d3254b9d03bd` |

Parent supplied the completed fetch/fast-forward prerequisite. Read-only
`git rev-parse HEAD` confirmed `bc7d8ae0d91649a8ed0fb48679eb43d6efd6ce4d`.
No dependency, shared source, navigation, index, commit, build, application or
subagent operation was performed by this writer. Applicable root AGENTS,
execution guide, design guidelines and this acceptance matrix were read;
there were no nested AGENTS files under `crates/` or `docs/`.

### Changes

- Kept public events, controller signatures, preview/open/clear behavior,
  preview row splitting, error mapping, group dedup/capture order and
  subscriptions. Presentation changes add revision invalidation rather than
  replacing the controller or its event ingress.
- Retained one NSH Surface for a group and independent compact/legacy-row
  presentations. Document/file labels, preview lines, action rows, menus,
  errors, group controls and focus styling are NSH. The native host adds no
  product drawing; it owns lifecycle, focus, keyboard and AX routing only.
- Corrected pinned NSH API mismatches: `.horizontal()`, `.border_t(...)`,
  borrowed SVG markup, `.tint_color(...)`, available ChevronDown rotated for
  collapse, cloned command sender and owned menu-state closure capture.
- Kept Entity handles and controller access on the native side. Queued actions
  compare Surface/mount/revision, route/message, complete safe file projection,
  preview/error/opening/reading/menu state, capture order, visible count,
  expansion, review availability and language/appearance before dispatch.
- Added mounted-lease and measured host-visibility checks; native geometry
  changes and horizontal scroll invalidate displayed activation evidence.
  Reveal acknowledgments carry the native geometry revision, so an older
  acknowledgment cannot authorize a newer layout.
- Enabled/disabled the real host FocusHandle and NSH AppView descendant handle,
  retained shared enabled-order traversal, added descendant/reverse entry and
  held-key suppression, preserved mouse-selected action through native focus
  transfer, and stopped outside menu closure from reclaiming external focus.
- Added measured horizontal reveal for legacy action rows using positive
  absolute NSH content offsets, followed by remeasurement and native visibility
  acknowledgment. Keyboard and native AX activation require current measured
  display evidence, selection identity, focus and geometry revision.
- Observed Surface failures on the native side. Failed presentation is stopped
  and replaced by a fresh NSH retry-only presentation with fresh authority;
  explicit Retry replaces only that presentation. No preview/open/review/clear
  is replayed and controller state is retained. This uses existing Surface and
  AppView APIs, not a nonexistent shared `Surface::reload` hook.
- Removed prohibited comments and an obsolete import in the owned root file.
- Completed the previously missing `tests` module with nine bounded development
  regressions: read-only eligibility, unavailable/opening policy, compact-menu
  versus legacy allowlist, recovery/authority fences, focused selection,
  empty/error-only groups, horizontal offsets, ordered traversal and preview
  row identity. No test-only dependency was added.

### Previous stopped-write Rust version (superseded below)

| Path relative to repository | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/artifact_card.rs` | `99329678b41e1c1168439aa9432af7fdcd125ea1ba40a89e1fde9759c34021db` |
| `crates/vega_ui/src/artifact_card/host.rs` | `db13c2dfe9f28263e9a8b18b992df8779ffda12b4f749f18fe91adaf5fd4fe43` |
| `crates/vega_ui/src/artifact_card/nsh_view.rs` | `2c556085a3f87e5834b6bdd87713eeef9ea7d268a7c7fb7067ba3844fad71c14` |
| `crates/vega_ui/src/artifact_card/tests.rs` | `65c16b5ee89d45d27eb0343e5036be5f870f601d3f8b879b98c2afd3c600d089` |

The fifth changed path is this document. Its final hash is reported separately
in the READY response to avoid a self-referential document hash.

### Executed checks

- Registry-source inspection against nsh-core/platform-gpui 0.4.0 and the
  existing KitFast/gpui-kit 0.7.3 pins; no upgrade or fork.
- Pinned `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, with
  `--edition 2024 --config skip_children=true`, applied only to the four Rust
  files above, then the same scoped `--check`: exit 0.
- Pinned Rust identity queried without compilation:
  `rustc 1.98.0 (88d9e12ae 2026-08-18)`.
- Production redline scan over root/host/NSH sources: no comments,
  `.unwrap()`/`.expect()`, filesystem/process/path APIs, or the known invalid
  NSH APIs remained.
- `git diff --check -- crates/vega_ui/src/artifact_card.rs`: exit 0.
  Final direct-file whitespace scan also covers the untracked leaf files.
- Nine `#[test]` cases present. Discovery/execution/pass/fail counts are not
  obtained: tests were not run. No Cargo invocation was made; actual parent
  Cargo/fork identity remains for the parent report.

### Parent integration and verification hooks

No new shared-file hook is required. Existing integration remains:
`ArtifactCard::render`, `ArtifactCard::render_row`, `ArtifactGroup::render`,
metadata/preview/open/error ingress, the three Artifact events, and
`OpenWorkspaceDiffRequested`. Existing shared `nsh::Surface`, `ProjectionId`,
`OrderedCommands`, keyboard policy, theme and safe icon-source helpers are
consumed without modification. Parent separately owns the navigation error.

1. Compile the complete coordinated source, then run Clippy and the nine
   `artifact_card::tests` cases in `vega_ui`. This writer did not run Cargo,
   compile, lint or tests and does not claim they pass.
2. Native GPUI regressions must verify real Tab/Shift-Tab entry through both
   host and AppView descendant, disabled descendant skipping, held Enter/Space,
   mouse selection, menu Escape/outside focus, and external boundary exits.
3. Native lifecycle tests must queue then hide/unmount/recycle a row, change
   file/thread/project/metadata/reading/opening/menu/order/review state, and
   confirm no stale business event. The pure regressions cannot construct
   cross-crate opaque file/card identities and do not replace this coverage.
4. Exercise a narrow legacy row and an anchored menu after vertical scroll,
   resize, theme/language changes and delayed measurement. Require actual
   reveal and current AX active descendant before keyboard/AX activation.
   Retry attempts are bounded to four per selection/observed scroll offset;
   native layout/scroll scheduling and subpixel geometry need runtime evidence.
5. Inject a Surface failure, verify fresh NSH recovery and explicit UI-only
   Retry, and count zero preview/open/review/clear replays. Verify controller
   subscriptions and outstanding business state survive presentation recovery.
   Recovery itself requires NSH to remain capable of rendering the minimal
   retry-only document; repeated engine-level failure is not proven recoverable
   by static inspection.
6. Compare compact/group/legacy visuals in narrow and normal layouts, light and
   dark themes, available/unavailable/opening/read-only states and i18n. Native
   app operations and screenshots were not authorized for this writer.
7. Archive exact development test source/patch and actual results, then remove
   `artifact_card/tests.rs` and its root `#[cfg(test)] mod tests` wiring before
   merge. Bind independent review and required CI to the final coordinated
   head after test cleanup; neither is claimed by this handoff.

## Independent-review correction handoff

### Implemented corrections

- P1: Removed the in-AppView NSH overlay. A menu gets a separate NSH Surface
  with a 240×226 logical-pixel AppView viewport: six 36-pixel NSH rows, 8 pixels
  of vertical padding and two 1-pixel borders. Native code provides only the
  original anchored/local, snap-to-window, priority-2 deferred route and a
  paint-free event/measurement shell. Compact height stays 36; one-file group
  height stays 98. All menu pixels and labels remain NSH.
- The menu shares the controller snapshot and native host focus/traversal but
  has separate mount lease, opaque identity and native geometry/revision.
  Measurement, reveal and activation choose the menu viewport only for its
  open-target controls. Close/replacement/remount invalidates old identity;
  a clipped anchor closes the menu. Outside-click exclusion for the trigger
  avoids close-on-down/reopen-on-up; Escape and external focus retain the
  existing native policy. Failure in either Surface enters the existing
  independent NSH retry-only presentation and stops both old Surfaces, without
  replaying business work.
- P2: Every NSH button declares explicit accessibility `activate` and handles
  `on_accessibility_action` by sending `Request::Accessible`, distinct from
  pointer `Request::Act`. The pinned platform's `action_or_press` checks this
  declaration first, preventing native AX Click from falling through to Press.
  Native AX proxy, NSH AX request and keyboard activation now share the
  production `measured_activation` guard. It checks current focused selection,
  displayed measurement, matching selection identity, target, geometry revision
  and complete visibility inside both the AppView viewport and outer clip;
  dispatch still applies all snapshot/business/lifetime checks. Pointer Press
  keeps its existing eligibility path and does not acquire an AX selection
  precondition. Roles, labels, hints and selected/disabled state remain exposed.
- Kept all nine existing regressions/assertions and appended three development
  regressions: six menu rows including the bottom row fit the independent
  viewport without normal-row expansion; AX rejects missing/stale/clipped/
  unselected/unfocused evidence while pointer policy remains independent;
  closed/replaced menu identity invalidates queued evidence. No test dependency
  or shared helper was added. These are policy/geometry regressions, not native
  AppView paint/hit-test/AccessKit integration tests; opaque conversation IDs
  remain inaccessible from this leaf and are not fabricated.

### Previous stopped-write Rust version (superseded by trigger follow-up)

| Path relative to repository | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/artifact_card.rs` (unchanged this correction) | `99329678b41e1c1168439aa9432af7fdcd125ea1ba40a89e1fde9759c34021db` |
| `crates/vega_ui/src/artifact_card/host.rs` | `a87a5c3579102fc0182a6a5aa73fa235c55d4ea3fce70c3b98bea4843ddadfd8` |
| `crates/vega_ui/src/artifact_card/nsh_view.rs` | `a299391f307f178e840ab247277d5442b9501e97102d3692ffa6615c61f30254` |
| `crates/vega_ui/src/artifact_card/tests.rs` | `6d83f549dd623ae734b6d743cfbfebb4d077cb37b11203e92bb667fc9884b3e2` |

This correction changes only host.rs, nsh_view.rs, tests.rs and this document.
The document hash is supplied in READY, not embedded in itself.

### Actual static checks and remaining gates

- Scoped pinned Rust 1.98 `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`
  applied to the three edited Rust files; `--edition 2024 --config
  skip_children=true --check` on all four owned Rust files: exit 0.
- Production redline scan over root/host/NSH: no comments, unwrap/expect,
  filesystem/process/path APIs, old overlay or known invalid pinned APIs.
- `git diff --check -- crates/vega_ui/src/artifact_card.rs
  docs/907/nsh-artifact.md`: exit 0. Direct whitespace/newline checks cover all
  five files, including untracked leaf files. Twelve development tests are
  present; discovery/execution/pass/fail counts are not obtained.
- Pinned source inspection: NSH 0.4.0 `access.rs` explicit-action precedence,
  `builder.rs` accessibility-action API, `render.rs` viewport/hitbox clipping;
  gpui-fast 0.1.9 `anchored.rs`, `deferred.rs` and `window.rs` deferred layout,
  window placement and paint-mask contract, via existing KitFast 0.7.3 facade.
  No dependency changes or forks.
- No build, test, Clippy, app, git mutation or subagent operation was run. The
  parent reports its integrated compile failed permission approval before
  dispatch; this is not a compilation result. No alternate build was attempted.
  The earlier base-94 test result does not validate these twelve current tests.
- No new parent/shared hook is required. Parent must compile/lint coordinated
  source and execute the twelve current `artifact_card::tests` when permitted.
  Then verify isolated compact and one-file group bottom-target pointer and
  keyboard/AX reachability; window-bottom snap; scroll/clip/hide/unmount/recycle;
  trigger toggle/outside click/Escape/external Tab/focus; queued AX Click on a
  nonselected, stale, hidden, clipped or replaced-menu control; and menu failure
  recovery with zero business replay. Native hitboxes, actual menu geometry,
  focus traversal, AccessKit route and visuals remain unverified here.
- Archive development test source/results and remove its file/module wiring
  before final merge under the existing policy. Independent review and final
  required CI remain parent gates; this handoff claims neither.

READY means stopped-write leaf corrections ready for parent integration and
independent review, not compiled, runtime-accepted, CI-passed or merge-ready.

## Trigger-geometry follow-up handoff

The original P1 viewport and P2 AX bypass remain statically closed and were not
reworked. Only host.rs, nsh_view.rs, tests.rs and this document changed in this
follow-up; artifact_card.rs remains byte-identical.

- Replaced both host trigger reconstructions with one `menu_trigger()` path.
  It uses the live mount lease, shown snapshot/menu index, latest host bounds
  and clip. The outside-click callback also validates its captured snapshot
  against the shown snapshot before exempting the trigger.
- The NSH layout and rectangle calculation share the existing 12px row padding
  explicitly. The trigger remains 24px and is nonshrinking; compact row height
  remains 36px. Group calculation includes its 1px body side border, current
  file row offset and the row's 1px top border/vertical centering. The unrelated
  toolbar inset remains only for snapping the popup to the window margin.
- At width 320 and host origin (100,200), the compact trigger spans x=384..408,
  y=206.5..230.5; group file 0 spans x=383..407, y=267.5..291.5;
  group file 2 spans x=383..407, y=355.5..379.5. These values follow the unchanged
  NSH border-box layout, not a reconstructed toolbar hitbox.
- Preserved all twelve prior tests/assertions and added one bounded regression
  exercising the production geometry/visibility function and point containment:
  leftmost-edge preservation, false-right-edge rejection, top/bottom rejection,
  clip exactly at the actual trigger edge versus clipping one pixel inside,
  left clipping, group row offsets, live resize and missing bounds/clip. This
  tests host decisions, not native capture-to-NSH Press event dispatch.

### Exact current Rust artifacts

| Path relative to repository | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/artifact_card.rs` (unchanged) | `99329678b41e1c1168439aa9432af7fdcd125ea1ba40a89e1fde9759c34021db` |
| `crates/vega_ui/src/artifact_card/host.rs` | `eab7541db836b673fd60f6b24aedd80b7622df16cfab541d0003b60303ce6587` |
| `crates/vega_ui/src/artifact_card/nsh_view.rs` | `ae090716202104dbb3ba251d7e2fba23f2e092fff84dcf93591ff373e3f50512` |
| `crates/vega_ui/src/artifact_card/tests.rs` | `22e160d0982a539ef476c34152b8da77035b957aafff4026cac5b8e50704ea49` |

The final document SHA-256 is supplied in READY to avoid a self-referential hash.

### Actual follow-up checks and unrun gates

- `rustfmt +1.98.0 --edition 2024 --config skip_children=true` applied only to
  host.rs, nsh_view.rs and tests.rs. Same tool/options with `--check` on all
  four owned Rust files: exit 0. Tool version is `rustfmt 1.9.0-stable
  (88d9e12ae1 2026-08-18)`.
- Direct whitespace/newline checks on all five scoped files passed. Production
  comment/unwrap/expect/filesystem/process redline scan passed. The first scan
  mistakenly classified `*error = code` as a comment; inspected that evidence
  and corrected the scanner, without modifying production code for the scan.
- Pinned NSH 0.4.0 source inspection confirmed `.px(...)` generated style API,
  cross-axis center alignment, top-border setter, border-box sizing and the
  public asynchronous `ElementRef.measure()` route. This narrow fix uses the
  permitted exact layout-derived fallback rather than adding a new asynchronous
  trigger-measurement protocol. Physical-pixel rounding and native event timing
  still require runtime verification; geometry is expressed in logical points.
- Thirteen development test declarations are present; none were compiled,
  discovered or executed here. No build/test/Clippy, app operation, git command,
  shared-file write or subagent operation was performed in this follow-up.
  Parent compilation remains permission-blocked before dispatch; no alternate
  build route was attempted and no prior test result validates this version.
- No parent/shared hook is needed. Once permitted, parent should compile/lint
  and execute the thirteen current leaf tests, then check compact and group
  leftmost 4px trigger toggling without capture dismissal, right-padding-only
  clipping versus actual-trigger clipping, scroll/resize/recycle invalidation,
  and fractional/native-pixel geometry. Existing independent review, runtime
  acceptance, development-test archival/removal and final CI gates remain.

READY: writes stopped; not compiled or runtime-accepted.

## Parent independent-review checkpoint

Nonwriter review of the exact Rust hashes above closed the viewport P1 and AX-bypass P2 after checking the separate window-level menu Surface and pinned explicit-action precedence (declared NSH activate action replaces Press rather than falling through). A subsequent narrow review closed the trigger-geometry P2: shared 12px row padding, nonshrinking 24px trigger, group side/top borders and vertical alignment now agree with the layout-derived native fence, and both visibility/outside-click use that same current snapshot/mount/host-bounds path. No remaining concrete finding was established in the reviewed difference; unchanged conclusions were not reopened.

At that checkpoint the thirteen Artifact and four navigation regressions were UNRUN, and normal compilation had been refused before dispatch (`permission decision failed closed`). That was not a compiler failure or completed migration.

## User-authorized retry: execution restored and local regressions run

Normal command execution is restored without changing permissions/configuration or using an alternate route. Standard check exposed actual API errors: mouse enter/leave callbacks need `&MouseEvent`, horizontal scroll needs `&ScrollEvent`; opacity literals were made explicitly f32. A strict collapsible-match lint was corrected by moving the unchanged `menu_open` predicate into the CloseMenu match guard. No acceptance/assertion was weakened; all thirteen Artifact tests are unchanged (SHA-256 `22e160d0982a539ef476c34152b8da77035b957aafff4026cac5b8e50704ea49`). Final corrected `host.rs` SHA-256 `e47b3876d3c76c10b4ccbce54632d500ca7ddc4a16cb2abec99754232170a735`; `nsh_view.rs` `0f04bc2eae9393cfd63ab219c2e4702dcd3217fa35944ee893c2d547aca992e4`; artifact_card.rs remains unchanged from the previous handoff.

On the stopped coordinated dirty version based on `bc7d8ae0d91649a8ed0fb48679eb43d6efd6ce4d`, parent actually passed `cargo check -p vega -p vega_ui --locked --offline -j 2`, workspace fmt/diff and strict all-target Clippy `-D warnings`. Nextest discovery111, executed111, passed111, skipped0; run `74a739c9-cb15-4040-82a0-646607972288`, execution0.167s, includes all thirteen Artifact and four navigation regressions. Python groups5/15/40 pass. Tools rustc1.98.0, Cargo1.101.0/huacnlee0.102.0-9, Nextest0.9.151; CI pin0.9.146 unchanged. Actual local logs `/tmp/vega-907-resume-{check,clippy,tests,release,computer,python}.log` and discovery JSON `/tmp/vega-907-resume-test-list.json`.

Correction-only independent review completed with no concrete finding. In-memory reversal of just the API/lint corrections reproduced the earlier reviewed host/view hashes, and final hashes stayed stable. No current development commit/cloud CI yet. Native geometry, pointer timing, focus/AT and three-desktop acceptance remain separate and unrun; no app/build package/install/merge/release. Archive/remove all development Rust tests before final merge. Parallel palette/branch changes begun after this stopped version are not validated by these results.
