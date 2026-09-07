# R7 Provider / models UI delivery record

## Source and freeze

- Worktree: `/Users/puzige/Workspace/worktrees/vega-provider-models-ui`
- Branch: `codex/vega-provider-models-ui`
- Implementation HEAD at handoff: `4f3f36ef5bf4dbabfcd1ac2168a4c2ffb3fe884f`
- Base: `325ac60`
- The implementation is three commits: `4db5c00`, `c77c98b`, and `4f3f36e`.
- At the handoff, `git status --short --branch` was clean before this delivery
  document was added. This document is a delivery-only diff and does not alter
  the implementation commit.

The implementation covers the Provider form's newline-separated model IDs,
bounded validation, same-name update semantics, empty-key preservation, owned
config/key seams, real focus and keyboard paths, `SettingsSaved` emission, and
the application catalog reload regression. The existing pricing gate and
custom-pricing route remain in place. The current thread model, R1
acknowledgement, production timeouts, Diff cadence, timeline/DDL, dependencies,
and performance criteria are outside this card.

## Validation commands and evidence

Commands were run from the worktree with the shared target explicitly selected:

```sh
env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo test -p vega_ui provider_ -- --nocapture
env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo test -p vega tests::model_selection::model_selection_app_handler_persists_and_runs_exact_model -- --exact --nocapture
set -o pipefail; env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo test -p vega_ui settings::tests -- --nocapture 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-ui-settings-suite.log
set -o pipefail; env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo test -p vega tests::model_selection -- --nocapture 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-model-selection-suite.log
set -o pipefail; cargo fmt --all && cargo fmt --all -- --check && git diff --check && env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo clippy -p vega_ui --all-targets -- -D warnings 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-ui-clippy-after-alias.log
set -o pipefail; env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo clippy -p vega --all-targets -- -D warnings 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-clippy.log
set -o pipefail; env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo check -p vega_ui -p vega --all-targets 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-ui-vega-check.log
set -o pipefail; env CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target cargo build -p vega 2>&1 | tee /private/tmp/vega-r7-validation-20260905/vega-build.log
set -o pipefail; cargo fmt --all -- --check 2>&1 | tee /private/tmp/vega-r7-validation-20260905/final-fmt-check.log; git diff --check
```

Observed results:

| Check | Result | Raw evidence |
|---|---:|---|
| Provider UI focused tests | 6 passed, 0 failed | The terminal result was not tee'd; see the provenance note below. |
| Full `vega_ui` Settings tests | 13 passed, 0 failed | [`vega-ui-settings-suite.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-settings-suite.log) |
| `vega` model-selection suite | 4 passed, 0 failed; the library test binary also reported 0 selected tests | [`vega-model-selection-suite.log`](/private/tmp/vega-r7-validation-20260905/vega-model-selection-suite.log) |
| `vega_ui` clippy, all targets, `-D warnings` | passed | [`vega-ui-clippy-after-alias.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-clippy-after-alias.log) |
| `vega` clippy, all targets, `-D warnings` | passed | [`vega-clippy.log`](/private/tmp/vega-r7-validation-20260905/vega-clippy.log) |
| `vega_ui` + `vega` check, all targets | passed | [`vega-ui-vega-check.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-vega-check.log) |
| `vega` build | passed | [`vega-build.log`](/private/tmp/vega-r7-validation-20260905/vega-build.log) |
| Final formatting and diff check | passed | [`final-fmt-check.log`](/private/tmp/vega-r7-validation-20260905/final-fmt-check.log) |

The Rust toolchain emitted the existing future-incompatibility notice for
`block v0.1.6`; no R7 warning remained after the fixes below.

## First failures and repair record

The first UI test compilation attempt failed before running tests. The task
transcript identified two `App::subscribe` closures with the wrong arity and
several `.focus_handle` calls without the `Focusable` trait in scope. The
closures were changed to the app API's three-argument form and `Focusable` was
imported. The corrected focused run reported 6/6.

The first UI clippy run is preserved in
[`vega-ui-clippy.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-clippy.log).
It rejected the two test-only fake backend closure types with
`clippy::type_complexity`; the fix introduced test-only type aliases. The
follow-up clippy log is listed above.

An earlier app test used a short filter together with `--exact`, so its test
binary selected 0 tests. The command was corrected to the full
`tests::model_selection::model_selection_app_handler_persists_and_runs_exact_model`
path. That exact app regression then passed 1/1, and it is also included in the
captured 4/4 model-selection suite.

## Evidence boundary

The initial compile failure, the first 0-test invocation, and the standalone
6/6 focused output were not piped to files. An attempted search of the task's
readable local transcript/log locations found no recoverable raw transcript or
source tool identifier. They are therefore recorded here as transcript-level
results, not represented as reconstructed raw logs. The explicit provenance is
stored at `/private/tmp/vega-r7-validation-20260905/evidence-provenance.md`.

All captured tests use owned temporary config/store state and fake seams where
needed. No user config, profile, Keychain, real provider, or remote API was
read. No retry, benchmark, soak run, production timeout, or performance
criterion was introduced.

## Native acceptance correction

The follow-up native pass at 960×628 found that the bare multi-line
`TextInput` used by Settings had no independent frame and that a second line
could overlap the helper copy below it. Commit `9cb9d09` adds the
Settings-only 2–4 row viewport frame, keeps the full draft and caret-follow
tail for five or more lines, and records the correction in SDD v0.2. The
Composer and commit-message callers retain their default 1–8 row behavior.

The layout test and follow-up checks are captured in the same validation
directory:

- [`provider-models-layout-final.log`](/private/tmp/vega-r7-validation-20260905/provider-models-layout-final.log): 1/1, including the 2-row minimum, four-row growth, five-line tail, and frame/helper separation assertions.
- [`vega-ui-settings-suite-layout-fix.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-settings-suite-layout-fix.log): 14/14.
- [`vega-ui-text-input-suite-layout-fix.log`](/private/tmp/vega-r7-validation-20260905/vega-ui-text-input-suite-layout-fix.log): 2/2, preserving the default multi-line behavior.
- [`vega-r7-layout-clippy-after-fix.log`](/private/tmp/vega-r7-validation-20260905/vega-r7-layout-clippy-after-fix.log), [`vega-r7-layout-check.log`](/private/tmp/vega-r7-validation-20260905/vega-r7-layout-check.log), and [`vega-r7-layout-build.log`](/private/tmp/vega-r7-validation-20260905/vega-r7-layout-build.log): all passed.

The layout correction's first compilation failure is preserved in
[`provider-models-layout-first.log`](/private/tmp/vega-r7-validation-20260905/provider-models-layout-first.log), and its first clippy failure is preserved in
[`vega-r7-layout-clippy.log`](/private/tmp/vega-r7-validation-20260905/vega-r7-layout-clippy.log). Both were fixed without changing provider parsing, key handling, save ordering, events, or model catalog behavior.

## Root native acceptance

Root integrated the source as `a82321057ecb09985fdfdc4de00c2af65e3bca2c` and
completed the scoped CUA pass in a 960×628 Settings window. The observed
contract passed:

- two entered model lines (`vendor/model-1.0` and `model-flash`) were fully
  visible inside the framed Models field, with the helper copy below and no
  overlap;
- five lines grew to the four-row viewport, kept the caret-facing final line
  visible, and retained the full draft;
- Home brought the retained first line back into view;
- Select All/Delete cleared the draft before closing Settings, with no Provider
  save, key input/read, or real model request.

The native evidence is recorded in
[`provenance.json`](/Users/puzige/Workspace/vega-review-20260905/native-review-r7-layout/provenance.json)
and
[`native-findings.md`](/Users/puzige/Workspace/vega-review-20260905/native-review-r7-layout/native-findings.md).
The provenance records a clean source tree, source binary SHA-256
`ee08b66245d7acd2ab933fb7269ca0df0dcfb3c9395ca83df7c69e78c1191af3`, signed
binary SHA-256
`0aca3facf9e946cc2bc325ae1f1bc81d51b3e6290dca90be84da185aaaf15168`, and the
build log
`/private/tmp/vega-main-review-20260905/r7-layout-integration-build.log`.
No screenshot was written to disk. The initial shortcut attempt did not open
Settings, so this pass records mouse-entry and does not claim shortcut
acceptance.
