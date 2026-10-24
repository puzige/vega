# Repeated code-selection input verification

This follow-up records the user's failed ordinary-reply acceptance after [PR #585](https://github.com/puzige/vega/pull/585) and the integration of [PR #511](https://github.com/puzige/vega/pull/511). It does not change either PR's historical CI result or claim that the current native application has recovered. Contract: [code-selection-repeat-drag.md](code-selection-repeat-drag.md), v1.

## Investigation

- User feedback on 2026-10-25: the horizontal scrollbar works, but held text selection towards the right edge does not automatically scroll. User confirmed ordinary reply code blocks. #584 restored to In progress and read back; [progress record](https://github.com/puzige/vega/issues/584#issuecomment-6386901804).
- Remote fetched and new task branch based on `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`, the squash merge of PR #511. Release v0.1.226 targets that commit. Read-only bundle inspection reports v0.1.226; this alone does not establish the exact loaded binary revision.
- Pinned macOS adapter starts the 16ms repeated held-event source before dispatching the original drag. The Vega move handler drops and re-arms its own 16ms task for every event. Continuous held input can starve progress. Existing #511 tests did not inject the native periodic source while checking progress. Product/source evidence, not local native reproduction.
- A dedicated implementation agent independently traced this order and ruled out an incorrect native Left-button conversion as the explanation. Root read the same pinned adapter and handler. All original selection/window/focus/content ownership checks remain required.

## Current validation

Development implementation and focused cloud regressions succeeded as recorded below. Local work is restricted to static source inspection, formatting, diff and offline metadata; no local Vega execution, compilation, build, Clippy or tests. The final cleaned head still needs its own independent review and required CI; its immutable SHA and result will be recorded on the PR.

Initial development head `86e766f621af4ecce5f5836d6731f0346e4ec007` was pushed to [PR #593](https://github.com/puzige/vega/pull/593), starting [Actions 40114672905](https://github.com/puzige/vega/actions/runs/40114672905). Independent review confirmed the production change and continuous-input observation, and identified a fixture geometry error: `right - 4px` remains inside the final glyph at maximum scroll, while the copy assertion correctly expects the full source suffix. The corrective head `aaeb2f6f75536ce6d371dddc27dc3cbe9d9c6b27` keeps the inside-edge progress coverage, then moves the held endpoint past the last glyph and asserts its logical content key before the unchanged exact copy expectation. It received independent complete-head static GO and started [Actions 40114957627](https://github.com/puzige/vega/actions/runs/40114957627); this run's actual outcome remains to be recorded.

Readback of the first run: overall cancelled by the geometry push, but [Nextest job 120933385507](https://github.com/puzige/vega/actions/runs/40114672905/job/120933385507) had already failed test compilation. Raw logs report `E0689` at `count.get().is_multiple_of(2)`: the development fixture's `Cell::new(0)` needs an explicit `usize`. Clippy's raw log reports the same compiler error before cancellation; fmt completed and the three Python suites executed 5 / 15 / 40 tests successfully. Rust discovery did not complete and no Rust behavior execution result was obtained. The glyph-coordinate issue therefore was found statically, not observed as a failed behavior assertion. Correct only the counter type, retaining production behavior, event timing and all assertions; do not rerun identical failing code.

The second run completed with failure: [Clippy 120934252098](https://github.com/puzige/vega/actions/runs/40114957627/job/120934252098) and [Nextest 120934252323](https://github.com/puzige/vega/actions/runs/40114957627/job/120934252323) both report the same `E0689` before Rust discovery/execution; Python 5 / 15 / 40 executed successfully. This head had corrected glyph input before the first run's compiler error was read. The explicit-type correction was based on that first raw error, not a blind rerun. Corrected full head `90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9` received independent static GO and passed [Actions 40115241945](https://github.com/puzige/vega/actions/runs/40115241945).

The new stop/restart event checks observe fully flushed frames. They do not inject button release into the precise interval between offset update and pending cursor refresh. This change preserves the existing epoch/ownership mechanism unchanged; that is checked statically against the baseline and is not described as a new pending-race event test.

## Successful development verification and preserved source

Full head: `90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9`; original base: `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`.

- [Nextest job 120935070817](https://github.com/puzige/vega/actions/runs/40115241945/job/120935070817): **4 discovered / 4 executed / 4 passed / 0 failed / 0 skipped**. Raw footer: `4 tests run: 4 passed, 0 skipped`. Three existing Python suites executed and passed 5 / 15 / 40 tests. Raw Nextest log SHA256: `097b0a223be40471d8dc6bc48ad056411bcfba5f1de7c52edc53df3ae9eb4be1`.
- [Clippy job 120935070959](https://github.com/puzige/vega/actions/runs/40115241945/job/120935070959): `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` succeeded. [Required job 120935867631](https://github.com/puzige/vega/actions/runs/40115241945/job/120935867631) succeeded.
- [Original four-test source](https://github.com/puzige/vega/blob/90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9/crates/vega_ui/src/conversation_stream/code_drag_repeat_tests.rs), SHA256 `14c04f14ade8dc26e6d41a123bb7d9d0538eaec2e7cb2bb6a078ddc57360b0dc`. GitHub API returned blob `5a1b97ea0925864e4da72f597a31db49233dfd41`; decoded bytes match the hash.
- [Complete original patch](https://github.com/puzige/vega/commit/90b3b3f2cd3cd21b59c6c30287c7f7e9945276f9.patch) includes all fixture, observation and development dependency support. This successful commit remains an ancestor of cleanup.
- [PR evidence comment 6387254042](https://github.com/puzige/vega/pull/593#issuecomment-6387254042) fixes the full SHA, original source, hash, run/job and actual counts. It was read back and verified before cleanup authorization.

Root independently executed `cargo fmt --all -- --check`, `git diff --check`, and `cargo metadata --no-deps --offline --locked --format-version 1`, all exit 0. A standard-library TOML comparison confirmed all 914 original package identities unchanged with ten development support identities added. These were static checks, not local compilation or behavior execution.

## Upstream integration and final cleanup

After development success, fetch found master `888d6e1f4ef81d4bc2024ebfbb9965ff89b4f1f9` (#594). Its only production changes are conversation paragraph gap 15.5 to 14 and Markdown list-item gap 6 to 0; it adds the corresponding spacing document. It changes neither code typography/padding, these two-code-block fixtures, code selection/viewport, Cargo dependencies nor CI. Merge `b936560f4665e5535955d8f50a387c6158380c9f` preserves the successful source commit as an ancestor. The targeted interaction evidence is reused for the unchanged behavior; final static review and complete required CI must cover the integrated head.

Cleanup removes only the new four tests and their exclusive observation/construction modules and GPUI development support. See [cleanup report](selection-repeat-cleanup.md). Final CI must report `no Rust tests` (0 discovered / 0 executed), separately from the four-test development success above. Exact final SHA/run/result will be fixed on PR #593 after that immutable head completes; no empty-suite behavior-success claim.

Native physical-device behavior remains user manual acceptance after a normal user-controlled update. No application launch, installation, restart, force-quit, database or credential mutation is part of this work.
