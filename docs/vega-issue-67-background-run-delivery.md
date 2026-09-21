# Issue #67 — background conversation navigation delivery

## Freeze

- Verified at: `2026-09-21T08:07:35Z` / `2026-09-21T16:07:35+08:00`
- Branch/base: `feat/67-background-conversation-run` on `origin/master`
  `19a1416`; frozen contract commit `7c8b56b`
- Task contract: [`vega-issue-67-background-run.md`](vega-issue-67-background-run.md)
- Code/test patch SHA-256 before this delivery record:
  `645a9be0b459f2d444f9e12289ded2fb851d955578732d40299a1750cd677dc5`
- Toolchain: macOS arm64; `rustc 1.98.0`; `cargo 1.98.0`; Git `2.55.0`

## Changed files

- `crates/vega/src/app_agent.rs`: exposes an exact-thread lookup of the one
  active stream. It does not add a second active run or a stream collection.
- `crates/vega/src/window/render.rs`: removes route-driven agent cancellation,
  skips permission cleanup only for the exact active stream, remounts that
  entity when its originating task returns, and leaves Settings' existing
  stream-level hidden-permission fail-closed observer intact without
  cancelling the run token.
- `crates/vega/src/tests/agent.rs`: adds one mounted production-root regression
  with a file-backed store, real `VegaWindow`/`ConversationStream`/controller/
  worker, and `MockProvider` only at the provider boundary. It covers new-task
  navigation, a durable-task round trip with exact entity reuse, Settings,
  background terminal isolation, non-destructive single-flight refusal, and
  production-window teardown cancellation.
- `docs/vega-r12-task-navigation.md`: the conflicting draft-only navigation
  sentence was superseded by Issue #67 in the preceding frozen-spec commit.
- `docs/vega-issue-67-background-run-delivery.md`: this evidence record.

No file outside the frozen primary scope changed. There is no schema,
dependency, provider protocol, public conversation API, execution-policy,
permission auto-approval, or general stream-cache change.

## Test-first evidence

The regression was added and run before either production file changed.

| Evidence | Exact command | Exit | Result |
| --- | --- | ---: | --- |
| Baseline red | `./scripts/cargo-lock.sh --wait test -p vega issue67_production_routes_keep_one_background_run_and_origin_stream -- --nocapture` | 101 | 0 passed, 1 failed; assertion: `opening the real new-task route must not cancel the origin run` |
| Final production green | same command | 0 | 1 passed, 0 failed, 183 filtered; includes independent window-teardown cancellation fixture |

The bounded baseline log was retained at `/tmp/vega-issue67-red.log` with
SHA-256 `5b024842eede8953e844161ae23775a9ecd61549e7f6e27aa7588f13e4584036`.
The final green log was retained at `/tmp/vega-issue67-green-final.log` with
SHA-256 `bc986d6096b6c4df51c18e182209089c42baf380775c8e8947e60077ac51fe31`.
The earlier compile-only invocation selected zero tests because `--exact` was
used without the module-qualified test name; it is not counted as evidence.

## Results

All Cargo commands use the repository-wide shared-target lock. Per the human
decision on 2026-09-21, this task uses affected-feature verification rather
than a full-workspace test run.

| Requirement | Evidence class | Exact command | Result |
| --- | --- | --- | --- |
| I67-01–I67-06 production-root regression | `E2E-REAL` | `./scripts/cargo-lock.sh --wait test -p vega issue67_production_routes_keep_one_background_run_and_origin_stream -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 183 filtered; final test includes independent window teardown |
| Existing agent/permission/cancellation regressions | production controller | `./scripts/cargo-lock.sh --wait test -p vega tests::agent:: -- --nocapture` | PASS (exit 0): 14 passed, 0 failed, 170 filtered |
| Existing explicit Stop regression | production root | `./scripts/cargo-lock.sh --wait test -p vega r11_composer_preparation_stop_preserves_draft_and_prevents_late_start -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 183 filtered |
| Existing route/Settings regression | production root | `./scripts/cargo-lock.sh --wait test -p vega navigation_real_root_palette_mouse_shortcuts_and_settings_preserve_drafts -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 183 filtered |
| Settings hidden-permission fail-close | `vega_ui` production entity | `./scripts/cargo-lock.sh --wait test -p vega_ui settings_hidden_and_terminal_paths_fail_closed_without_rendering -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 409 filtered |
| Non-active stream permission cleanup | `vega_ui` production entity | `./scripts/cargo-lock.sh --wait test -p vega_ui thread_switch_timeout_contract_removes_prompt_before_view_replacement -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 409 filtered |
| Formatting | static | `cargo fmt --all -- --check` | PASS (exit 0) |
| Patch whitespace | static | `git diff --check` | PASS (exit 0) |
| Strict Vega lint | static/build | `./scripts/cargo-lock.sh --wait clippy -p vega --all-targets -- -D warnings` | PASS (exit 0); the initial run found one `collapsible_if`, which was fixed before this final pass |
| Signed release candidate | packaging | `./scripts/cargo-lock.sh --wait xtask package` | PASS (exit 0); `dist/Vega.app` structure and signature verified by the packager |

## Residuals

- Native interactive acceptance is intentionally handed to the user after the
  merged candidate replaces the local installation. The mounted production-root
  test is behavior evidence, not a real-network claim; the Issue remains open
  pending that manual result.
- A `MockProvider` replaces only provider/network transport. The real file
  store, route handlers, controller, worker, permission queue, and stream
  entities remain in the exercised path.
- Full-workspace tests were intentionally not run under the human-directed
  affected-feature policy; unrelated `vega_mcp` full-suite resource contention
  observed in another worktree is not represented as Issue #67 evidence.
- No known implementation deviation from the frozen Issue #67 contract.
