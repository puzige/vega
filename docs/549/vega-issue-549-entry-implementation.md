# #549 first verification slice — implementation handoff

The initial handoff below is retained as historical evidence, including its original hashes and unexecuted-test status. Its present-tense version/PR statements describe that first freeze only. The [repair followup](#repair-followup--current-freeze) records the subsequent immutable source, actual cloud failure, review findings, updated baseline and current uncommitted delta; it does not retroactively validate the initial source.

## Version and authority

- Stage: shared overlay helper + E01 file menu + E11 command card only. This is not whole-card completion; E02–E10/E12–E13 and O01–O11 remain in the parent contract.
- Contract: [overlay-entry spec](vega-issue-549-overlay-entry.md), already present at HEAD `2abbc608a9b7f8bd53e776936fd5416fd5f5de1a`; production base `572a3c6f1b50ea34c2214245dcd18ee3a70bb737`.
- Branch: `feat/549-overlay-entry`. HEAD remains unchanged. All implementation files are uncommitted/unstaged; there is no implementation commit, PR, run, or merge to cite.
- Single implementation writer; no delegation. Parent supplied completed fetch/update and takeover authorization. No fetch/rebase/index write/commit/push/Issue/PR/CI action was performed by this writer.
- Source freeze verified at `2026-10-25T03:48:09Z`. No spec changes or product-acceptance deviations.

## Implementation

- `Motion::OVERLAY_ENTER_MS = 120`, Kit default easing and no delay.
- `OverlayEntry` is clonable owner-held metadata: explicit open/close edges, revision, last actual surface-paint alpha, executor-clock close time, pending seed, settlement and an owner-lifetime activation observer. No registry, Presence, child/focus ownership, exit animation, close timer, or replacement engine.
- Same-owner reopen at elapsed <=120ms reuses last paint; expired/unpainted/invalidated/new owners start at zero. Same-frame close/open is recorded independently of render's final boolean. Ordinary refreshes do not seed again. Mount-local Kit state can be reconstructed from retained owner paint.
- The generic surface delegates identity, layout, prepaint, paint and accessibility to the original interactive element. It applies opacity immediately before delegating real paint and records alpha only afterward. Gating/bypass precedes seed; interruptions discard stale origins and surviving open surfaces settle at one.
- The existing `decorative_value` interface remains intact. Its narrow internal seeded operation consumes the activation interruption once for seed+retarget; the seed cannot swallow the interruption.
- E01 uses the original menu inside priority-2 deferred drawing, with unchanged positioning/content/input routes. File-query, close, accept, retry and invalidation edges update metadata; loading/results share one opening. Index failure snaps the whole surface. Reading-preview suspension is recorded at its accepted owner edge.
- E11 stores metadata in persistent `AppPalette`, passing a handle to each new `CommandPalette`. Only the original inner card fades; backdrop remains immediate at its existing opacity. `PaletteClosed` records intent before render removes the view. The existing cancel/fence/focus cleanup body is shared by the production close method. Route mismatch invalidates continuity; search generation is not animation identity. Already-open refocus remains unchanged.
- No changes to anchors, render_rows, rename, dependencies' versions, lockfile, registry/vendor, workflows, other entries or other worktrees.

## Development-only verification definitions — NOT executed

Nine new GPUI tests are defined, not compiled, discovered or executed:

| Boundary | Defined checks |
|---|---|
| Shared helper (5 tests) | Full frame draw/clear; seed zero/intermediate/one; actual inherited child quad alpha; deferred geometry; closed/finished callback drain; reopen with/without closed draw at 0/120/121ms; elapsed-but-unpainted origin; owner replacement/invalidation; failure/hidden/Reduce Motion snap and no replay; activation round trip with an independent root-paint counter asserting no inactive frame |
| Real E01 host (2 tests) | Real ConversationStream and file menu quads; loading-to-results continuity; acceptance at zero alpha and immediate removal/focus; accepted cancel and emitted cancellation; stale generation rejection; same-frame close/open; failure snap; invalidation reset |
| Real E11 card + AppPalette (2 tests) | Real CommandPalette, persistent AppPalette metadata and production close/intent/route helpers in an IO-free host; view recreation with/without closed draw; unchanged backdrop alpha; already-open entity/refocus; cancellation and prior-focus restoration; failure snap and route continuity invalidation |

Temporary support consists only of the three new test modules, their `#[cfg(test)] mod tests` declarations, one `#[cfg(test)]` debug-selector block in overlay prepaint, and dev-only `gpui_kit` test-support feature entries in the two crate manifests. The debug bounds observe real layout, not invented alpha. The child probe delegates production paint; quads do not prove glyph/SVG raster pixels.

Cloud prerequisites and residuals:

1. Parent must fix an accessible source commit/patch, open the draft PR and obtain Actions compilation/discovery/execution and independent review of that exact content. The existing no-feature-argument PR workflow is unchanged; dev dependency features supply test-support. Counts of discovered/executed/passed tests are **not obtained**.
2. Sequential same-key seed/retarget, full host setup, paint observations and strict no-inactive-frame recovery still require first cloud execution. The activation test asserts the root paint count before/after the round trip; if the harness draws, diagnose the observation/activation route rather than relaxing that assertion. No #550 gap is accepted here.
3. Tests deliberately do not instantiate full VegaWindow with updater/storage/background workers. E11 uses the real card/AppPalette and extracted production cleanup/route helpers, not a full app search/navigation integration test. Original full-app generation/search routing needs independent static review and later integration/manual verification.
4. Elapsed time without paint is covered; a separate forced sampled-but-skipped-child-paint experiment is not defined. No result claims that experiment passed. Complete native trigger down/up, backdrop/input dispatch, AX/VoiceOver, OS Reduce Motion, glyph/SVG raster and long-stream behavior remain unverified/manual; no performance claim.
5. Before master: preserve accessible development sources/results, remove all Rust tests and exclusive support above, independently inspect the final test-free head and obtain complete required CI with Rust 0 discovered/0 executed (`no Rust tests`, not behavioral PASS). Parent continues remaining entries separately.

## Local static evidence

Only low-CPU formatting/diff/source/manifest checks were executed. No Vega execution, compilation/check/Clippy/build/test/package/profile/CUA/install/restart occurred.

- `rustfmt --edition 2024 --config skip_children=true <12 changed Rust paths>`: exit 0; direct formatter only, no Cargo invocation. Local formatter reports `rustfmt 1.9.0`.
- `rustfmt --check --edition 2024 --config skip_children=true <same 12 paths>`: final exit 0. An earlier check found only two unformatted final test edits; formatting was applied and the final check passed.
- `git diff --check`: final exit 0.
- Inline Python static check using `tomllib`, source diff and explicit scope allowlist: exit 0; exactly 14 authorized source/manifest paths, two dev-only test-support entries, nine defined/non-ignored tests, no added code comments or production unwrap/expect.
- `git diff --cached --exit-code`: exit 0; index unchanged. HEAD and branch were read back as above.
- SHA-256 of `git diff --binary --no-ext-diff` (tracked changes only): `69cd20ed50c0dac819d67eb3ad430059b04f903a72b3ccb75ade6114604a57f5`.
- SHA-256 of sorted `shasum -a 256` lines for the following 14 source/manifest paths (including new untracked sources, excluding this report): `ff65d917851835f58c36b9e5c1286727ffeceb206228e92f63e8384c58d6a333`.

## Frozen file identities (SHA-256)

```text
38217def8e01b51a8c48a081ca492e5221723ad278c5a7f8d918ba12a4ef56cb  crates/vega/Cargo.toml
dfca6b22f3c3acbb1c8792e126519c42beb2dc0be9f02077506f3e04f18b4b0b  crates/vega/src/app_palette.rs
bffc75ebd28d7e145ea64d8f32ce11801effdeb7dd50357aa56b8065c7fe42e2  crates/vega/src/app_palette/tests.rs
a3a864b6c9501cdc98666b3408d5db476ed7a394a2f570df0b74b2f8fc20b730  crates/vega_theme/src/lib.rs
3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5  crates/vega_ui/Cargo.toml
3caa908067a7bf1b04565b44936267692b5a1021b4f4f5fd0c921a7abc0ac726  crates/vega_ui/src/command_palette.rs
4861c2907c6d40844b094b76fe1fa6ce440fadde3879d8723d8128b441077c41  crates/vega_ui/src/conversation_stream/core.rs
d5b21b3afcfcd86f90dfdb21781879af6a6e6b12fb053a857cb3631f5a3422ee  crates/vega_ui/src/conversation_stream/file_reference.rs
fa8896cad66d9e1298f27b2dc9f4b25863ca16c40acb792b9dce4cd90d97c662  crates/vega_ui/src/conversation_stream/file_reference/tests.rs
d5547496491d31ca4c14cb99218e8b68366d0cd7dd5f1a0627d97861123c4bf7  crates/vega_ui/src/conversation_stream/render.rs
bdb9335e005bce8cea2acf2e1f72d34f5b8cf580effef740cb10c98a604e5a5b  crates/vega_ui/src/interaction.rs
491b315a5700e3044f41bb352f3e2dbba7b44f73ece18fee6593246143d7f109  crates/vega_ui/src/interaction/decorative.rs
9029828ecb7b8fa4d2515067c4314388611f2afe5ded931910a5b35b4ba67510  crates/vega_ui/src/interaction/overlay.rs
e23e50814a10a77e5457eeefc5a19b506a95d5e173726e7e6c265a526bd06f69  crates/vega_ui/src/interaction/overlay/tests.rs
```

READY means this first-stage implementation is ready for parent inspection, not reviewed, cloud-green, merged or user-accepted. Writer stops all scope writes at handoff. Next: parent fixes this content, obtains independent review and Actions, addresses evidence-based failures, then coordinates the remaining entries.

## Repair followup — current freeze

### Immutable baseline and failure evidence

- Previous frozen implementation: `9115ff4c034aee99e08fbc903c8b9d709b71f2ba`, Draft [PR #612](https://github.com/puzige/vega/pull/612). This is the source of the failed cloud attempt, not a successful behavior run.
- Parent-inspected original [Actions run 40160001377](https://github.com/puzige/vega/actions/runs/40160001377): failure. Clippy job `121069369407` and Nextest job `121069369177` both failed at test compilation with `recursion limit reached while expanding #[test]`, `interaction/overlay/tests.rs:120`. Parent-supplied raw log SHA-256 identities are respectively `cb200a56aadeea0d83e8df8148cf2666ceb0ef7348b8ec36733934671a4b7b8d` and `4d708332184b0f53085e6a72f30a4d5e8f3e08db1be9d2c31dabb7adda95a287`. This writer did not query Actions or rerun either job. Discovery/execution counts were not obtained; nine definitions are not nine executed tests.
- Independent review of that exact source was NOT READY with two P2 findings: retained loaded E01 menus lacked application visibility suspension; E11 task/preview success did not record accepted close time. These are product wiring omissions; the macro failure is a separate test-harness import fault. No acceptance was reduced.
- Parent fetched/updated and normal-merged master `429c6a819fc5a30f8280afd2f6ed015d4682bf41` at `a8e1ef3c7e17027bbfc64ffe8a4d072e3effd1ea`, then recorded repair authorization/evidence in spec commit `265036a90d4f127d921975ea56e76201efd63a41`. Current branch remains `feat/549-overlay-entry`; clean starting HEAD and unchanged current HEAD are that full `265036a9` SHA.
- Current authority is the parent-owned overlay-entry spec §5 at this HEAD, read before edits. Only E01/E11 and their shared first-stage support are repaired; no remaining entry is implemented. The #601 artifact groups and #518 Native fork production code are retained. The fixture supplies `None` for both new Thread provenance fields without altering upstream Thread production behavior.
- One writer, no delegation. No fetch/merge/rebase/index mutation/commit/push/remote writes/Issue/PR/CI action by this writer. No new comments, dependencies, manifest features, versions, lockfile, registry/vendor, workflows or recursion-limit changes. Current changes are unstaged and uncommitted, not a new commit.

### Repair delta

1. Test import responsibility: all three affected development modules now explicitly import Kit types and their own parent symbols, with no facade, prelude or parent wildcard. Pinned Kit `lib.rs:84–93` warns about importing the test macro; pinned `gpui-pre-macros-0.3.4/src/test.rs:186/279` generates a bare `#[test]`. Those source facts were read locally; the repair leaves every existing `#[gpui_kit::test]` attribute and test body intact. There is no recursion_limit or attribute-semantics workaround.
2. E01 visibility responsibility: `ConversationStream::suspend_decorative_overlays` only suspends its existing file-entry metadata. It does not close the business menu, invalidate its index, cancel workers, move focus, notify or modify artifact/permission behavior. Existing window observers call a narrow route-hidden helper on Settings/thread changes regardless of whether a file-index worker exists. Settings rendering also suspends the retained stream independently of whether `settings_view` was already cached. Both cached/new stream replacement paths suspend the departing entity, including same-thread/different-entity replacement. The existing Codex preflight replacement is the only additional direct `stream_view` assignment outside render; it also suspends the previous entity before replacement. Existing cancellation, capacity guards, hydration, artifact and permission code remain unchanged.
3. E11 close-intent responsibility: task acceptance success and file-preview success now call the existing `request_close(cx)` at their original success branches. Preview payload assignment, route/generation fences, activation handling and notify/refresh ordering are otherwise unchanged. The only remaining `close_requested = true` assignment is in `request_close`; the false cleanup reset remains. The close timestamp is therefore captured at accepted intent, before a deferred render or reopen can rebase it.

### Development regressions — definitions only

Static comparison against current baseline HEAD found all **nine original test bodies and their attributes unchanged** after whitespace normalization. Two targeted definitions bring the source total to **11** (5 helper, 3 E01, 3 E11), with no dropped, ignored, weakened or renamed original assertions:

- `loaded_file_menu_retained_across_page_and_thread_unmount_resumes_opaque`: a retained-host window owns two real ConversationStream entities and conditionally removes the original entity from the element tree. It loads a real file-menu result, paints zero then intermediate alpha, uses the public production suspension method at the host hide edge, completes two hidden frames (no surface debug bounds), waits 200ms, and restores the same loaded entity. Both Settings-like no-child and other-thread-child cases assert opaque restore, unchanged menu/snapshot/index generation/focus, and callback drain. This is an actual child unmount/restore, not a helper `visible=false` draw.
- `accepted_async_close_expires_before_reopen_without_an_intervening_paint`: the existing real CommandPalette/AppPalette host paints zero then intermediate alpha. An asynchronous entity callback invokes the same production `request_close` helper now used by both accepted success branches. A one-pixel absolute paint counter asserts no intervening root paint through callback delivery and 121ms elapsed time. The open_palette-like reopen then recreates the view, cancels the old token, preserves focus behavior and must paint zero before reaching one after 120ms. It observes the accepted helper, not merely the final boolean, and does not use a forced closed draw.

The E01 fixture is shared by the original and added cases and supplies the two upstream provenance `None` values. Existing temporary manifest test-support wiring, original strict no-inactive-paint assertions, production layout/deferred adapter and all helper implementation are unchanged. The E11 observation counter has an absolute one-pixel surface and does not change card geometry.

### Current static evidence and limitations

Only low-CPU source/diff/hash inspection and direct rustfmt ran. No Vega code execution, compilation, cargo check, Clippy, build, test, CUA, package, profile, install or restart occurred.

- `rustfmt --edition 2024 --config skip_children=true` on the nine changed Rust files, then `rustfmt --check --edition 2024 --config skip_children=true` on those exact paths: exit **0** each; formatter `rustfmt 1.9.0`. The latest source formatting/check completed at `2026-10-25T04:35:00Z`.
- `git diff --check`: exit **0**.
- Inline Python static comparison/scope checks: exit **0**. It compares nine original test attributes/bodies after whitespace normalization, counts 11 definitions, rejects wildcard test imports/ignore/recursion_limit/added comments/production unwrap or expect, checks authorized changed paths, unchanged HEAD/branch and no untracked files, and verifies that all three accepted palette close producers use `request_close` with only one direct true assignment left.
- `git diff --cached --exit-code`: exit **0**, unchanged index. Read-only HEAD/branch/status read-back confirms the above baseline and unstaged changes.
- The exact source-only binary diff relative to this HEAD (`git diff --binary --no-ext-diff -- crates`) SHA-256 is `0f70fd6764fa60b0c586372b7ec992b0df4d3bd0e99be7d5672195446f64b5f0`. The sorted source-file SHA-256 manifest below, including its newline-terminated lines, hashes to `bd95357e9710e9e4d37db12e04b510b36f89bdb366b1c6ee07b2294624f9b652`. Full diff/report hashes are delivered separately to avoid a self-referential report hash.

```text
adba603d070d1d77c91d20062385b8849365ea996084af3273aa63999843a025  crates/vega/src/app_palette.rs
ceba3abc8c2d57e2c1e4a21f6a21137779f99963a4ad0b90c461e4aa94b2d252  crates/vega/src/app_palette/tests.rs
68b73c46f38e81d9adff5468b90db62c2c80e37051d6b8c4f6d67eb70e157a25  crates/vega/src/window/agent.rs
519c69cb5ded5753901232b825e1f206245c26f10b0941813bb0122737c64695  crates/vega/src/window/mod.rs
d327643bf735faeeea0cc8d57a33dc809c623c88e24b93231635d3ce5d18c5f9  crates/vega/src/window/navigation.rs
45da7cd716d79684e902f7ba0a2bbab20d096ec352d554c72645a2b2c71acba4  crates/vega/src/window/render.rs
b90b2b8289bfc338c78c7acbaba1a31a0566d24b2e6dd7aa33f8a4e5adbe6cdf  crates/vega_ui/src/conversation_stream/core.rs
de4a55a8040a6f6c3d5be3de6faad2ac8010e408e1a838b3d9f8e6d09d7ec6ee  crates/vega_ui/src/conversation_stream/file_reference/tests.rs
e43fd8a765792dff50078fa4c907ff3967185701d0e80acec9fb324dcebbd61e  crates/vega_ui/src/interaction/overlay/tests.rs
```

Cloud compilation/discovery/execution of this repair is **pending**, not passed; counts and new run/job identities are **not obtained**. Parent must commit this exact delta, obtain independent delta review and Actions on PR #612, and diagnose any remaining harness or product failure without weakening the preserved assertions. The E01 regression uses the real retained menu and production lifecycle seam, not a full VegaWindow/storage/worker instance; root Settings/thread observer and cached replacement integration still need independent inspection and later full-app/manual coverage. The E11 regression exercises the shared accepted helper asynchronously, not IO task/preview workers. Existing no-inactive-frame recovery, sequential keyed seeding and paint observations remain cloud prerequisites; no gap is inherited from #550. Separate sampled-but-unpainted-child instrumentation, full native input/AX/OS/glyph/SVG/long-stream acceptance remain unverified as recorded above.

READY is limited to this repair delta being available for parent inspection. No spec edits or acceptance deviations. All scope writes stop after final static/read-back identity verification; parent owns commit/review/Actions and subsequent entries. Development Rust tests and their exclusive support still must be preserved as accessible source/results and removed before final master delivery with the required final-head empty-test CI gate.
