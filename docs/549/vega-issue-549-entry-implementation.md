# #549 first verification slice — implementation handoff

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
