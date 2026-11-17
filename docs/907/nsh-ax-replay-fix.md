# Fixed-version NSH AX replay repair

## Scope and current boundary

The user authorized a minimal AX crash repair inside fixed NSH 0.4.0, retaining accessibility semantics and actions. The requested order is: repair AX; establish a real application screenshot → operation → rebuild → re-observation loop; only then implement a minimal development-only actual-layout overlay. **No merge or release.**

The source repair and bounded development regressions are complete. **The native crash replay and visual loop are not complete.** Desktop preflight returned nonretryable `screen_recording_permission_missing`, `execution_state: not_dispatched`; no screenshot or input was obtained. Computer operations stopped without alternate capture/input automation. No layout overlay was implemented ahead of that prerequisite.

Recovery: macOS System Settings → Privacy & Security → Screen & System Audio Recording (or Screen Recording) → allow the currently running Vega. Also allow Vega under Accessibility for operations. If macOS requests a restart, restart Vega, then send a new message so a fresh desktop preflight can run. This report does not assert that either permission has been granted.

## Source and behavior

Base: `de594c84ab176a7a7dd28df5b1774280bf620bf5`. Only production implementation delta: `vendor/nsh-platform-gpui/src/access.rs`.

The prior adapter used Fast's synthesized-leaf interface for nodes that already contained hierarchical children, then rewrote parent relationships. Retained replay restored leaves into sibling relationships while their stored descendants remained, producing a tree with repeated children. The original native failure is preserved in `nsh-native-acceptance-blockers.md` and `target/nsh-native-20261110-ax-crash-evidence/`; it is distinct from the later permission denial.

The repair builds nonpainting semantic GPUI elements with balanced nested Push/Pop recording. Full AccessKit properties are retained while children come from the actual element hierarchy. Replayable action handlers resolve the live NSH surface/view and use the existing `perform` route. Semantic invalidation and announcements remain intact. This is not a return to legacy-painted GPUI controls, and AX is not disabled.

Two independent-review findings were corrected:

- Focus matches the canonical semantic ID, not `Entry.view`: canvas drawing nodes share an action owner but must not all claim its focus handle.
- Each top-level proxy is prepainted at its true logical window origin; the root inset is zero and descendants retain relative offsets. Advertised AccessKit bounds alone do not correct GPUI focus hitboxes.

NSH core/platform 0.4.0, Kit 0.7.3 and Fast/platform 0.1.9 remain unchanged. Root Cargo.toml, Cargo.lock and the temporarily adjusted upstream text-test path are byte-identical to the base. No installed application, permission setting, user credentials or manual-test home was modified. Existing running applications were not restarted.

## Actual development regression and recoverable evidence

Development source/evidence commit: **`48ee1bd67b6723f3c60576e2e0a8995e52259657`**.

- Verified remote recovery tag: `evidence/907-ax-development-12-tests`.
- Original test source: https://github.com/puzige/vega/blob/48ee1bd67b6723f3c60576e2e0a8995e52259657/vendor/nsh-platform-gpui/src/access/replay_tests.rs
- Archive: https://github.com/puzige/vega/blob/48ee1bd67b6723f3c60576e2e0a8995e52259657/evidence/907-ax-development/development-evidence.tar.gz
- Archive SHA-256: `f080f41c7b6d5c2798c5917be5079d9920597c165b1f387f0adcfa4251cb62df`.
- Test-source SHA-256: `5035fd6ccbb98ffb08c66dedbfa885f46b603c304e0e5e8db0935e817b6a8e18`.

The archive contains actual source, temporary manifests/lock, the original fixed-upstream font fixture, all AX command stdout/stderr (including failed attempts) and the development manifest. Its local copy remains under `target/nsh-ax-validation/`. Dedicated evidence files, new tests and their module wiring were removed from the final delivery tree only after remote commit/tag readback. Original upstream tests/dev dependencies and production assertions were preserved. The original `Built` test helpers were relocated into the original tests module because production no longer needs them; upstream assertions remain unchanged.

Official Cargo `1.98.0 (797e8a9bc 2026-08-05)` and rustc `1.98.0 (88d9e12ae 2026-08-18)`; auto-update disabled, two build jobs, target directory `target/official-nsh-validation`.

```sh
GPUI_A11Y_ACTIVE=1 CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo test \
  -p nsh-platform-gpui --lib --no-default-features --features shot access:: \
  --locked --offline -j 2 --target-dir target/official-nsh-validation
```

Final development result: **12 passed, 0 failed, 0 ignored, 197 filtered out**, runtime 0.05s after 7.97s compilation; logs `target/nsh-ax-fix-tests-member8.*`. Nine existing AX unit tests and three new behavioral regressions executed:

1. The old flat builder violates the generated tree's unique-parent invariant. TestWindow has no native AccessKit consumer, so its expected failure is an explicit structural assertion, not an invented native panic.
2. Nested semantics retain hierarchy, labels, value, action declarations/registrations and focus for four frames. `renders == 1` establishes actual cached reuse rather than rerendering each frame.
3. Multiple semantic nodes sharing a canvas-style action owner claim focus only on the canonical node, also across cached reuse.

The nested case uses the production root-prepaint API at nonzero origin `(10, 20)` with nested bounds at scale 2. After retained replay and blur, a point in the old unshifted hitbox does not focus; a point in the correct nested hitbox does focus. These are actual in-process GPUI pointer/focus observations, not OS-native input or pixel evidence. Action registrations are checked; native action dispatch is **not** certified.

### Retained harness failures and dependency distinction

The vendor package is excluded from the production workspace; standalone offline resolution could not find gstreamer. Temporarily adding it as a member resolved its full upstream development graph, adding 295 identities and replacing 12 ICU-related identities. **That test graph is archived, not accepted as a production dependency change or identical-graph behavioral gate.** It was restored before production checks/builds, with exact byte comparisons against base Cargo files.

Compilation also exposed a missing upstream Pacifico font fixture and its upstream-relative path mismatch after vendoring. The fixture was downloaded from fixed upstream commit `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`, SHA-256 `5b6c0d5334a7bf77dea52b975c5a0c408878c0f7115ed5b6fb151f634b7bf701`; only the development path was temporarily corrected. Both fixture and path correction were retired after archival, not replaced by a different font or disabled test.

An initial expected-native-panic control did not panic because TestWindow lacks the consumer validator. The corrected control asserts the same unique-parent invariant on the real generated tree; no product assertion was relaxed. Another new test used the wrong WindowHandle read API; it was corrected to the actual update API before the final passing run. No ignored tests, lint suppressions, dependency upgrades in production or assertion weakening were used. The pre-existing linker `__eh_frame` over-16MB warning remains visible.

## Independent review

Nonauthor review execution `01M71V2SPPWHJK4P3VP93MSZ83` / `01M71WDG93B7MGRDM9WW7ANNQE` closed both findings and reported no additional actionable production findings in the delta and affected contracts. Reviewed access.rs Git blob: `b177a507e29c4f0e1002f9edf5c47dc161091bde`; SHA-256 `86e0ac91fceaa1681567946338842dae45510fa3bc6cc177a67dffc2822a725d`.

Review checked canonical focus, root placement, generated semantic IDs/children, property preservation, replayable actions and existing invalidation. It was read-only/static and does not certify the parent's runtime results or native acceptance. Final access.rs SHA-256 is `a86b0b861c75ce9b8e6ae235f2b2e780fb134d4ab17283e5d2c4bb6e63429024`; exact comparison confirms its only delta from reviewed bytes is removal of the four-line dedicated replay-test module wiring. Production conclusions therefore bind to unchanged implementation, not a new unreviewed repair.

## Final production checks and binary

All following checks used the restored production dependency graph and the cleaned final source:

| Command | Actual result |
| --- | --- |
| `cargo clippy --workspace --all-targets --locked --offline -j 2 --target-dir target/official-nsh-validation -- -D warnings` | exit 0, 6.42s; `target/nsh-ax-validation/final-clippy.*` |
| `cargo build -p vega --bin vega --locked --offline -j 2 --target-dir target/official-nsh-validation` | exit 0, 13.78s; `final-build.*`; existing linker warning retained |
| `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |
| Nextest list/run workspace, locked/offline, `--no-tests=pass` for run | **no Rust tests: 0 discovered / 0 executed**, 16 binaries, 0 passed / 0 skipped; run `eb21bf70-933f-49ed-9e6d-db4089932305` |
| `python3 scripts/release_tests.py` | 5/5 |
| `python3 scripts/prepare_computer_use_tests.py` | 15/15 |
| `python3 -m unittest discover -s scripts/tests` | 40/40 |
| `python3 scripts/check_i18n.py --json` | `errors: []`; no allowlist changes |

The Nextest empty set is not a behavioral test pass and does not include the separately executed vendor development tests. Latest-head required CI remains a separate post-push check, recorded in PR/Issue comments rather than a self-referential commit loop.

Final executable: `target/official-nsh-validation/debug/vega`, SHA-256 **`ed81a6b8188018d7febc5a49073104e6db8ff7e3daad35837562a64c62fafa93`**. The earlier `296668fc…` build was before test-wiring cleanup and is superseded. This is a development executable, not an installed/released package.

Manual launch, after the user safely quits only their owned test instance:

```sh
cd /Users/puzige/Workspace/worktrees/vega-907-nsh-full
VEGA_HOME="$PWD/target/nsh-native-manual-home" \
  ./target/official-nsh-validation/debug/vega
```

The existing manual-test home is retained; do not delete it or copy credentials. Real native follow-up must first activate accessibility on the matching build, confirm no abort across redraws, then obtain a screenshot, perform a verified harmless interaction, and reobserve. Visual parity and all nine native matrix rows remain unaccepted. Windows/Linux remain not run. The minimal layout overlay waits for that real visual loop; it must be development-only/default-off and use actual current layout data without logging input content or credentials.
