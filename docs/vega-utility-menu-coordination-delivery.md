# Utility menu coordination delivery

Status: verified; ready for parent review/native acceptance. This report belongs to the bounded C1–C3
follow-up in `docs/vega-utility-menu-coordination.md`; the contract document
is main-owned and remains unmodified.

Scope is limited to the Composer-mounted branch selector, ConversationStream
menu coordination, and production UI tests. The preceding chip geometry,
colors, capture/outside dismissal, popup width, and row layout remain frozen.

## Contract coverage

- C1: initial/reopened branch menus paint only the current row's selected
  surface/check; automatic logical Enter candidates remain available without
  becoming visual keyboard focus. Arrow navigation still paints and activates
  its intentional target.
- C2: project and branch popups are mutually exclusive at their production
  trigger/event boundaries in both directions. Closing a branch uses
  `request_close`, preserving `BranchSelectorClosed` cleanup ownership.
- C3: real mounted alternating trigger sequences, repeated toggles, outside
  close, Escape, light/dark painted state, and keyboard activation are covered.

## Freeze

- verified_at_utc: `2026-09-16T16:50:04Z`
- verified_at_local: `2026-09-17T00:50:04+0800` (Asia/Shanghai)
- branch: `feat/branch-popup-upward`
- git_head_before_delivery_commit: `80a2c5146e4f064478b0fab671e92078ac7bfc02`
- git_tree_before_delivery_commit: `6588b1b7ee53b6a01e7184e3c4ae5072019b1ab3`
- implementation_diff_sha256 (scoped production/test files, excluding this report and the main-owned spec): `1cdd3e686bd40022602b8d16c540515e3a87687bccef9bafe2672b717e3639d5`
- task_contract: `docs/vega-utility-menu-coordination.md` C1–C3; main-owned
  spec/design files are not staged by this executor
- os_arch: `Darwin 24.6.0 arm64`
- rustc: `1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `2.55.0`

## Results

Fresh focused, mutation, formatting, workspace-test, and all-targets clippy
commands are recorded here with exact named failures and bounded raw-log
paths.

Initial focused compile attempt failed before running tests because the new
test subscription used the four-argument `Context::subscribe` callback shape
instead of the three-argument `TestAppContext::subscribe` shape. The callback
was corrected; the rerun passed:

```text
./scripts/cargo-lock.sh test -p vega_ui utility_menu_coordination -- --nocapture
  4 passed, 0 failed, 0 ignored, 327 filtered out
```

The focused regression reruns also pass:

```text
./scripts/cargo-lock.sh test -p vega_ui r68_ -- --nocapture
  10 passed, 0 failed, 0 ignored, 321 filtered out
./scripts/cargo-lock.sh test -p vega_ui branch_popup_upward -- --nocapture
  4 passed, 0 failed, 0 ignored, 327 filtered out
./scripts/cargo-lock.sh test -p vega_ui r62_branch_ -- --nocapture
  3 passed, 0 failed, 0 ignored, 328 filtered out
```

The full gates pass on the current tree:

```text
./scripts/cargo-lock.sh fmt --all -- --check
  passed
./scripts/cargo-lock.sh test --workspace
  1217 passed, 0 failed, 9 ignored; 5 doctests passed
./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings
  passed; no warnings
```

Raw logs:

- workspace: `/private/tmp/vega-utility-menu-coordination-workspace.log`
- clippy: `/private/tmp/vega-utility-menu-coordination-clippy.log`
- final format check: `/private/tmp/vega-utility-menu-coordination-fmt-check-final.log`
- focused coordination: `/private/tmp/vega-utility-menu-coordination-focused-final.log`
- focused R68 dismissal: `/private/tmp/vega-utility-menu-coordination-r68-final.log`
- focused branch regression: `/private/tmp/vega-utility-menu-coordination-branch-regression-final.log`
- focused branch model/filter regression: `/private/tmp/vega-utility-menu-coordination-branch-model-final.log`

## Mutation evidence

Each behavioral reversal was applied independently with `apply_patch`, run
through `cargo-lock`, and restored before the next mutation. The raw logs are
kept under `/private/tmp`:

- C1 visual/logical merge reversal (`this.model.focused()` rendered directly):
  `/private/tmp/vega-utility-menu-coordination-mutation-c1.log`; both
  `c1_light_branch_menu_paints_current_only_then_keyboard_focus` and
  `c1_dark_branch_menu_paints_current_only_then_keyboard_focus` failed at
  `automatic first switchable candidate must not paint focus initially`
  (`0 passed, 2 failed, 329 filtered out`).
- C2 project→branch reversal (removed the `BranchListRequested` sibling-close
  subscription):
  `/private/tmp/vega-utility-menu-coordination-mutation-c2.log`; both
  `c2_project_branch_project_trigger_sequence_is_mutually_exclusive` and
  `c2_branch_project_branch_sequence_keeps_one_popup_and_escape_closes`
  failed at `at most one utility popup may be mounted` (`0 passed, 2 failed,
  329 filtered out`). The temporary unused-import warning was mutation-only.
- C2 branch→project reversal (removed project-side `request_close`):
  `/private/tmp/vega-utility-menu-coordination-mutation-c2-project-direction.log`;
  both same named C2 tests failed at `at most one utility popup may be mounted`
  (`0 passed, 2 failed, 329 filtered out`).

All three production reversals were restored before the green focused runs.

## Residuals

- ACCEPTED: native macOS installation/visual acceptance remains parent-owned;
  production test and painted-quad evidence are complete.
- LIMIT: the workspace/clippy logs are retained under `/private/tmp`; the
  report records bounded command results rather than embedding full output.
