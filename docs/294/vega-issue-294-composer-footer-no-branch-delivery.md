# Issue #294 — Composer footer branch removal delivery

Contract: [Issue #294](vega-issue-294-composer-footer-no-branch.md).

## Test plan and ownership

One dedicated implementation agent owns the footer mount and its now-unused
helpers. First prove the defect with a production GPUI render test using an
owned Git project, then remove the mount and preserve the existing footer
controls. Retain the old #191 selector request, switch, pending and HEAD-update
assertions through the unchanged New Task utility-bar entry. Update the #64
narrow-tooltip regression only where it explicitly required the withdrawn entry.

| Case | Operation and expected observable result | Layer | Status |
|---|---|---|---|
| F01 | Existing Native Git conversation has no footer branch; add, permission, model and send remain | Production GPUI | PASS after expected red |
| F02 | History hydration and subsequent thread projection do not restore the branch | Production GPUI | PASS |
| F03 | Existing Codex conversation has no branch; backend status and send/stop remain | Production GPUI | PASS |
| F04 | Light/Dark, ordinary/narrow layout and send/stop states retain controls without branch/placeholder | Production GPUI geometry | PASS in scoped slices |
| F05 | New Task utility selector still opens, emits exact switch requests and updates pending/HEAD; draft preserved | Production GPUI request path | PASS; controlled snapshot |
| F06 | Projectless/non-Git footer remains branch-free | Production GPUI | PASS |
| F07 | Official app matches the user's affected conversation after merge | User manual | NOT RUN |

No runtime configuration, installed app, user Git branch, persistence schema,
provider or ACP behavior is changed. Full workspace tests remain the cloud PR
gate. Automated GPUI geometry is not native screenshot or user acceptance.

## Implementation and review

The production change removes the single footer mount and both footer-only
helpers (18 deleted lines). The route-owned branch selector, utility-bar mount,
subscriptions, Git policy, input state, provider/ACP behavior and all existing
send/stop guards are unchanged. The #191 selector test retains its exact request,
switch, pending, completion and current-label assertions at the utility-bar entry;
it now also asserts preservation of the New Task draft. The #64 narrow-tooltip
test retains its viewport, hover continuity and no-overlap assertions and checks
the surviving add/permission controls instead of the withdrawn footer branch.

No code comments, dependencies or schema changes were introduced. Spec deviation:
none. Legacy #191 entry and capacity documents explicitly retain historical text
while marking only footer mounting requirements superseded by #294.

## Freeze

- verified_at_utc: `2026-10-11T02:38:11.223743+00:00`
- verified_at_local: `2026-10-11T10:38:11.223967+08:00`
- baseline git_head: `e52c23baa727388f8d573d530b02ce1b6e4a0dd6`
- branch: `feat/294-composer-footer-no-branch`
- final-test tracked_diff_sha256: `05acb89b23a6b8c1f201febc648db65efa760dbd385bfc952131ad4bbe9ea819` (captured before this evidence prose was finalized)
- os_arch: `macOS-15.8.1-arm64-arm-64bit-Mach-O`
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`; cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`; git: `git version 2.55.0`

## Results

| Requirement / phase | Evidence class | Exact command | Exit / result | Test duration | Raw log SHA-256 |
|---|---|---|---|---|---|
| F01 first red before production edit | Production GPUI | `cargo nextest run -p vega_ui issue294_existing_native_composer_has_no_branch_entry` | 100; expected FAIL, unwanted footer mount detected | 0.050s | `e5be275f3a0666805b0d87dab99833cdc0e12a976f1d3e6eaaf464f145b81073` (`red-native-footer.log`) |
| First green attempt | Test compilation | `cargo nextest run -p vega_ui -E 'test(conversation_stream::tests::utility_bar::) &#124; test(conversation_stream::tests::issue64_context_usage::issue64_context_usage_hover_and_focus_tips_fit_narrow_window_without_covering_composer_controls) &#124; test(conversation_stream::tests::utility_menu_coordination::) &#124; test(branch_selector::)'` | 101; nonexistent fixture method set_running; no tests executed | not applicable | `86e372fd068173ab9bf3c869db8d662b374544f2d0c82258876ada06032abae4` (`green-composer-neighbors.log`) |
| F01–F06 and unchanged entry/menu/selector neighbors | Production GPUI | `cargo nextest run -p vega_ui -E 'test(conversation_stream::tests::utility_bar::) &#124; test(conversation_stream::tests::issue64_context_usage::issue64_context_usage_hover_and_focus_tips_fit_narrow_window_without_covering_composer_controls) &#124; test(conversation_stream::tests::utility_menu_coordination::) &#124; test(branch_selector::)'` | 0; 29 passed / 0 failed; prior to final compact refinement | 0.219s | `ae648b7645f3c93514d6e7afe4b10e4d19cc6ff2b8e1deacedef40e14a59c584` (`green-composer-neighbors-2.log`) |
| Final F01–F05 after compact/draft refinement | Production GPUI | `cargo nextest run -p vega_ui issue294_` | 0; 3 passed / 0 failed | 0.127s | `93ffe3a0142e680e384d5527014f49954a3e0be405735f91570bf8143cef7321` (`green-final-issue294.log`) |
| Formatting | Format check | `cargo fmt --all -- --check` | 0; no output | not recorded | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (`final-fmt-check.log`) |

Bounded raw output retained from the runs:

```text
Issue #294 removes the branch entry from an existing Git conversation's Composer footer
Summary [   0.050s] 1 test run: 0 passed, 1 failed, 561 skipped
Summary [   0.219s] 29 tests run: 29 passed, 534 skipped
Summary [   0.127s] 3 tests run: 3 passed, 560 skipped
```

`git diff --check` returned 0 with no output. Its raw empty-log SHA-256 is
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (`final-diff-check.log`).

The expected first red was a render assertion failure, not a build failure. The
later compilation failure was a new test-fixture API mistake and was corrected
by using the existing internal running-state field. The first green run emitted
an unused-mut warning in the new layout test; it was removed before the final
three-card-test run. No failure was ignored, assertions were not weakened, and
nextest used the repository default profile with zero retries and default
concurrency. The reported counts are executions: 29 scoped green tests followed
by 3 affected final tests, not 32 distinct tests. No local workspace-wide suite
was run.

F04 covers Native/Codex in six paired Light/Dark, viewport and send/stop slices
at widths 1403, 1200, 960, 360, 1229 and 1230. A separate Native/Dark 360px compact
slice checks the existing no-wrap branch and 4px leading gap; ordinary slices
check 8px. Required controls have positive bounds contained in the card. F01
also verifies the actual Composer Tab path to the model and unchanged draft.
These are scoped samples, not all combinations of runtime states.

The Git project and HEAD files are owned fixtures; the selector branch snapshot
is supplied at the existing service boundary. This verifies production GPUI
render/request paths and current-head filesystem projection, not a real user
repository branch switch. Existing Git authority/safety service code and tests
were not changed.

## Residuals and handoff

- F07 native/user manual acceptance: NOT RUN. No installed application, runtime
  configuration, user Git branch or existing history was modified.
- Required cloud PR check and master integration: NOT RUN by the implementation
  agent; the main agent owns PR, CI and merge, then moves the card to In review.
- Do not close the Issue or clean its branch/worktree before user manual acceptance.
- The dependency future-incompatibility notice for block 0.1.6 remains an existing
  upstream notice; no dependency changes are in this task.
- Rollback is a revert of this task's PR. Existing utility-bar branch selection
  and service safety contracts remain in force throughout.
