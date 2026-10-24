# Repeated code-selection input verification

This follow-up records the user's failed ordinary-reply acceptance after [PR #585](https://github.com/puzige/vega/pull/585) and the integration of [PR #511](https://github.com/puzige/vega/pull/511). It does not change either PR's historical CI result or claim that the current native application has recovered. Contract: [code-selection-repeat-drag.md](code-selection-repeat-drag.md), v1.

## Investigation

- User feedback on 2026-10-25: the horizontal scrollbar works, but held text selection towards the right edge does not automatically scroll. User confirmed ordinary reply code blocks. #584 restored to In progress and read back; [progress record](https://github.com/puzige/vega/issues/584#issuecomment-6386901804).
- Remote fetched and new task branch based on `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`, the squash merge of PR #511. Release v0.1.226 targets that commit. Read-only bundle inspection reports v0.1.226; this alone does not establish the exact loaded binary revision.
- Pinned macOS adapter starts the 16ms repeated held-event source before dispatching the original drag. The Vega move handler drops and re-arms its own 16ms task for every event. Continuous held input can starve progress. Existing #511 tests did not inject the native periodic source while checking progress. Product/source evidence, not local native reproduction.
- A dedicated implementation agent independently traced this order and ruled out an incorrect native Left-button conversion as the explanation. Root read the same pinned adapter and handler. All original selection/window/focus/content ownership checks remain required.

## Current validation

Implementation and focused cloud regression results are pending. Local work is restricted to static source inspection, formatting, diff and offline metadata; no local Vega execution, compilation, build, Clippy or tests. Actual commands, precise development and final heads, raw failures (if any), source/hash evidence, cleanup and final required CI will be recorded here or on the PR for its immutable final head.

Initial development head `86e766f621af4ecce5f5836d6731f0346e4ec007` was pushed to [PR #593](https://github.com/puzige/vega/pull/593), starting [Actions 40114672905](https://github.com/puzige/vega/actions/runs/40114672905). Independent review confirmed the production change and continuous-input observation, and identified a fixture geometry error: `right - 4px` remains inside the final glyph at maximum scroll, while the copy assertion correctly expects the full source suffix. The corrective head `aaeb2f6f75536ce6d371dddc27dc3cbe9d9c6b27` keeps the inside-edge progress coverage, then moves the held endpoint past the last glyph and asserts its logical content key before the unchanged exact copy expectation. It received independent complete-head static GO and started [Actions 40114957627](https://github.com/puzige/vega/actions/runs/40114957627); this run's actual outcome remains to be recorded.

Readback of the first run: overall cancelled by the geometry push, but [Nextest job 120933385507](https://github.com/puzige/vega/actions/runs/40114672905/job/120933385507) had already failed test compilation. Raw logs report `E0689` at `count.get().is_multiple_of(2)`: the development fixture's `Cell::new(0)` needs an explicit `usize`. Clippy's raw log reports the same compiler error before cancellation; fmt completed and the three Python suites executed 5 / 15 / 40 tests successfully. Rust discovery did not complete and no Rust behavior execution result was obtained. The glyph-coordinate issue therefore was found statically, not observed as a failed behavior assertion. Correct only the counter type, retaining production behavior, event timing and all assertions; do not rerun identical failing code.

The new stop/restart event checks observe fully flushed frames. They do not inject button release into the precise interval between offset update and pending cursor refresh. This change preserves the existing epoch/ownership mechanism unchanged; that is checked statically against the baseline and is not described as a new pending-race event test.

Native physical-device behavior remains user manual acceptance after a normal user-controlled update. No application launch, installation, restart, force-quit, database or credential mutation is part of this work.
