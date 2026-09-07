# R14-S sidebar and folder delivery

Root integration update (2026-09-06): final application `0d7056a` passed 988/0/0 workspace gates and native acceptance. Root-owned checks listed below are historical executor handoff boundaries; final results and remaining limits are in [R14 acceptance](vega-r14-acceptance.md).

## Freeze

- verified_at_utc: 2026-09-06T07:35:43Z; local: 2026-09-06 15:35:43 +08:00
- branch: `codex/r14-sidebar-fix`; base git_head: `4c81f1f54ce64212a8fd6e6bb6702858bddbf840`
- tracked_diff_sha256 before this report: `3c29818aa7e3e9008a1322fc7ccfff15f774520fbbae2851b7db7c616c29869f`
- task_contract: `docs/vega-r14-sidebar-folder-fix.md`
- os_arch: Darwin arm64; rustc: 1.98.0; cargo: 1.98.0; git: 2.55.0

## Changes

The shared navigation titlebar now contains the sidebar button in both layouts. Its pointer events stop titlebar dragging and propagation; Tab, Enter, Space and Cmd+B share the root action. Explicit reveal overrides automatic narrow-window collapse. Existing collapse preferences remain the persistence boundary.

Folder registration runs on the background executor. The conversation service atomically reuses or creates the canonical folder registration, touches its recent timestamp, selects Projects/ByProject and clears precisely its collapse marker. Manual groups, ordering and unrelated collapse preferences remain intact. The completion checks database ownership, mutation epoch and route identity before selecting the folder. A replaced database cannot consume the new owner's mutation counter. The refreshed exact project header issues one autoscroll request, including for empty folders. Registration errors are surfaced in the organization projection with a retry instruction.

## Results

| Requirement | Evidence class | Exact command | Result | Bounded footer |
|---|---|---|---|---|
| Production folder registration, duplicate path, empty group, pointer-created task ownership, failed registration, database reopen, late owner completion; existing sidebar regressions | E2E-REAL | `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega_ui sidebar:: -- --nocapture` | PASS | `22 passed; 0 failed; 0 ignored; 0 measured; 137 filtered out; finished in 2.36s` |
| Reveal atomicity, stale revision rejection, unknown project rollback and unrelated metadata preservation | E2E-REAL | `cargo test -p vega_conversation r14_reveal -- --nocapture` | PASS | `1 passed; 0 failed; 0 ignored; 0 measured; 288 filtered out; finished in 0.03s` |
| Card compilation/lint | BUILD | `cargo clippy -p vega_ui -p vega_conversation -p vega --all-targets -- -D warnings` | PASS | `Finished dev profile ... in 3.33s` |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS | empty output |

Raw logs: `/private/tmp/vega-r14-s-sidebar-tests-final.log`, `/private/tmp/vega-r14-s-service-test.log`, `/private/tmp/vega-r14-s-clippy-final.log`.

Earlier failures retained: `/private/tmp/vega-r14-s-check.log` (incorrect `.ok()` on unit-returning AsyncApp update, corrected); `/private/tmp/vega-r14-s-test-build.log` (shared-target cross-worktree ProviderConfig metadata contamination, moved to isolated target); `/private/tmp/vega-r14-s-sidebar-tests2.log` and `/private/tmp/vega-r14-s-clippy.log` (GPUI callback must precede Stateful `.id()`, corrected). No assertions were removed.

## Residuals

- NOT RUN here: native folder picker cancellation, real root close/reopen, 960×600 and 1280×750 light/dark pointer/keyboard/titlebar acceptance. Root agent owns native acceptance and final assembled-workspace gates.
- E2E folder registration invokes the actual production `ProjectsBlock::register_path` used by the picker, through a mounted Sidebar; the picker itself is not simulated. Task creation is a real pointer click. Reopen evidence is owned database reopen, not native application restart.
- INTEGRATION DEPENDENCY: follow-up routes sidebar preference edits through R14-D `config::update`, whose shared lock covers latest read through atomic save. The assembled-workspace gate must validate this follow-up after R14-D is included; this base does not expose that API.
- No new dependency, schema migration, provider/network, cost UI or Keychain changes. Specification deviation: none.

## Native-discovered focus follow-up

- verified_at_utc: 2026-09-06T07:42:45Z; integration base: `5882034`; tracked diff SHA-256 before this appendix: `822e4e4af2303e36794c6fc2234a84455e98e75de30dfca0b251597b34525831`.
- Root's native pointer-reveal followed by Space exposed missing mouse focus assignment. The actual `VegaWindow` regression reproduced `Space after pointer reveal must collapse` before the fix; the preserved failure log is `/private/tmp/vega-r14-s-focus-before.log`.
- Mouse down now focuses the persistent navigation toggle handle, retained across expanded/collapsed layouts. Cmd+B does not take focus from the composer.
- E2E-REAL exact command: `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega window::navigation::tests -- --nocapture`. Raw log `/private/tmp/vega-r14-s-root-navigation-final.log`; footer: `8 passed; 0 failed; 0 ignored; 0 measured; 71 filtered out; finished in 0.51s`.
- The new actual-root test uses pointer reveal, Space collapse, Enter reveal, then editor-focused Cmd+B; checks durable draft text and retained editor focus. Individual green log: `/private/tmp/vega-r14-s-focus-after.log`.
- `cargo clippy -p vega -p vega_ui --all-targets -- -D warnings`: PASS, `Finished dev profile ... in 5.90s`; raw log `/private/tmp/vega-r14-s-focus-clippy.log`. This run includes the assembled R14-D config authority API.
- Native keyboard re-verification of this final patch remains root-owned.

## R14-B automatic current HEAD

- Contract: `docs/vega-r14-current-branch.md`; verified_at_utc: 2026-09-06T07:56:06Z; specification/base commit: `95f897a`; staged implementation diff SHA-256 before this appendix: `62c754725b212150ac3ac3b09acf54d9ff51f6c4feed86a3d33f6682a1a2be6e`.
- The composer automatically resolves its registered project path off the UI thread and uses the existing bounded `ProjectBranchService`. The read-only current-HEAD cache is separate from the selector's branch-switch snapshot and does not mint switch authority, open the popup, select a default branch, or execute Git checkout.
- Active-task polling works with the sidebar hidden, refreshes every two seconds, rejects mismatched database/project/path/generation, and invalidates pending read results around a branch switch. Render reads only cached state. Ordinary/linked worktree branch names follow actual HEAD; detached displays `detached`; non-Git hides the trigger; failures display `分支暂不可用`; initial resolution displays `读取分支…`. Unborn symbolic HEAD displays its truthful branch name, matching the unchanged R12 sidebar contract.

| Evidence | Exact command | Result / raw bounded footer |
|---|---|---|
| E2E-REAL actual VegaWindow, normal repository, hidden-sidebar external branch update, linked worktree, detached state, non-Git trigger suppression, task return; no selector click and unchanged real HEAD after UI observation | `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega r14_current_head -- --nocapture` | `1 passed; 0 failed; 0 ignored; 0 measured; 79 filtered out; finished in 4.42s`; `/private/tmp/vega-r14-b-root-final.log` |
| E2E-REAL asynchronous path resolution with replaced database owner and unborn HEAD; stale original request cannot populate current target, both HEAD files unchanged | `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega_ui r14_head_resolution -- --nocapture` | `1 passed; 0 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.07s`; `/private/tmp/vega-r14-b-owner-test.log` |
| Existing switch/controller safety regressions | `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega branch -- --nocapture` | `8 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.66s`; `/private/tmp/vega-r14-b-branch-tests.log` |
| Production root palette folder registration canonical-path regression | `XDG_CONFIG_HOME=/private/tmp/vega-r14-s-owned/config XDG_DATA_HOME=/private/tmp/vega-r14-s-owned/data cargo test -p vega production_root_palette_escape -- --nocapture` | `1 passed; 0 failed; 0 ignored; 0 measured; 79 filtered out; finished in 0.72s`; `/private/tmp/vega-r14-b-palette-test.log` |
| Scoped strict lint | `cargo clippy -p vega -p vega_ui --all-targets -- -D warnings` | PASS, `Finished dev profile ... in 1.77s`; `/private/tmp/vega-r14-b-clippy-final.log` |

The palette test correction follows the explicitly approved R14-S canonical registration behavior: only its lookup path now uses `canonicalize`; identity/count/cancel/draft assertions remain. Root's initial full-workspace failure is preserved at `/private/tmp/vega-r14-final-workspace-tests.log`, SHA-256 `fcec08094b2dcf3c489e58f7568dbed2ff15e86bcd008494516ee56bd97f7986` (982 passed / 1 failed). It is not represented as a passing gate. The initial R14-B root-test compilation failure (mutable/static debug-selector test API requirements) is retained at `/private/tmp/vega-r14-b-root-test.log`.

Remaining root-owned checks: native final current-branch display and final assembled-workspace gate after this patch. No provider, branch-switch permissions, runner timeout, dependency or migration changes.
