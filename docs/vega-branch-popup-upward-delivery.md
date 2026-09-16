# Composer branch popup upward — delivery

## Freeze

- verified_at_utc: 2026-09-16T07:50:39Z (implementation/package evidence)
- verified_at_local: 2026-09-16T15:50:39+0800 (implementation/package evidence)
- git_head: `feat/branch-popup-upward`, based on the R69 home lazy-draft Composer integration
- scoped_content_sha256: `137ee4fdcb74cd1b844a2823d56d2ba0d096a302f788f25c4eadbd54477fef6c` (implementation, focused tests and spec; delivery record excluded to avoid self-reference)
- task_contract: `docs/vega-branch-popup-upward.md`
- os_arch: macOS arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- git: `git version 2.55.0`
- dependencies: no new dependencies

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
| --- | --- | --- | --- | --- | --- |
| Composer popup placement, empty/error/long-list bounds, trigger/Esc/outside dismissal | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui conversation_stream::tests::branch_popup_upward -- --nocapture` | PASS: 4 passed, 0 failed | 0.72s | `vega-branch-popup-upward-focused.log`; SHA256 `983b3cc51780cd3827aa0e641d561f102040931c953310c709f0ae87370d1149` |
| R68 dismissal/search/padding regression | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui conversation_stream::tests::r68_popup_dismiss -- --nocapture` | PASS: 10 passed, 0 failed | 0.29s | `vega-branch-popup-upward-r68.log`; SHA256 `582b128a42db52216d6e4fbb23beaff2236ada6725b3c22fd2b60a564929910d` |
| Branch controller projection/keyboard authorization regression | E2E-REAL | `scripts/cargo-lock.sh test -p vega branch_selector_real_projection_keyboard_first_wins_and_visible_range -- --nocapture` | PASS: 1 passed, 0 failed | 0.31s | `vega-branch-popup-upward-controller.log`; SHA256 `facc21a2792c5a7ad1455f267f7c97875dd3c0ce41cf8254db114d2d8f8dac6c` |
| Formatting | gate | `cargo fmt --all -- --check` | PASS | <2s | `vega-branch-popup-upward-fmt.log`; SHA256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Strict lint | gate | `scripts/cargo-lock.sh clippy --all-targets -- -D warnings` | PASS: zero warnings | 4.61s | `vega-branch-popup-upward-clippy.log`; SHA256 `5db4283574bb3700a34347709e2ec614fed782751d020c39e9ede0662e560034` |
| Workspace tests and doctests | gate | `scripts/cargo-lock.sh test --workspace` | PASS: all result lines 0 failed; expected load-sensitive tests remain ignored | ~76s | `vega-branch-popup-upward-workspace.log`; SHA256 `6e6b1e73495ddb4ee85458190ed46a9f7c9284612c355b8861c3628c29ba1467` |
| R69-based package candidate | packaging | `scripts/cargo-lock.sh xtask package` | PASS: release build 28.4s, signed bundle and Info.plist validation passed | 28.4s release build | `vega-branch-popup-upward-package.log`; SHA256 `33635a5055079751eac76a4fa10aa0309d9dec97729c3f2a66dcf0085a136cd7`; `dist/Vega-macos-arm64.zip` SHA256 `e60002c54d652d15eb408f626eeaea2721cd299f99c3a3ea6eb413c9173afd3f` |

The first R68 focused run failed 1/10 because `exposed_point` still assumed the old constrained downward popup overlapped the trigger. The scoped helper was updated for the intentional upward placement while retaining its constrained-layout fallback; the rerun above is the preserved final result. No production dismissal behavior was changed.

## Residuals

- ACCEPTED: the shared `GitBranch` icon is an independently authored, three-node, 16px-neutral outline SVG using the existing icon viewBox/stroke conventions; it is used by both the Composer trigger and branch rows.
- ACCEPTED: the Composer render no longer overwrites the selector's default upward anchor; non-Composer callers retain the explicit placement API.
- ACCEPTED: the R69 draft route's branch controller remains disabled as specified by R69; this task does not expand into draft-route controller behavior.
- Native light-mode acceptance: parent launched the final package, clicked the branch chip and observed the popup wholly above the trigger, with the new three-node icon. Clicking outside dismissed it. Repeated after installing and launching the installed bundle. Dark-mode native pixels and native minimum-window pixels remain NOT RUN (minimum-window layout is covered by the production tests above).
- LIMIT: the existing `block v0.1.6` future-incompatibility advisory remains outside this change.

## Final package and installation

The earlier packaging row precedes the final SVG refinement; it is not the installed artifact. Parent repackaged implementation commit `889dc28` using `scripts/cargo-lock.sh xtask package`, successfully verified the signature, and verified the installed executable matches the candidate SHA-256 `5a43212249dc16052b0168b00de3351ae4ea53f485b86ec756ca9bc9957347d5`. The previous app bundle was backed up; user data was untouched. This task branch retains the R69 base and is not merged to master or pushed.
