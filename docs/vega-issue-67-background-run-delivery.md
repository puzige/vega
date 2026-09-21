# Issue #67 — background conversation navigation delivery

## Freeze

- Verified at: `2026-09-21T07:53:38Z` / `2026-09-21T15:53:38+08:00`
- Branch/base: `feat/67-background-conversation-run`, frozen contract commit
  `005ac00`
- Task contract: [`vega-issue-67-background-run.md`](vega-issue-67-background-run.md)
- Code/test patch SHA-256 before this delivery record:
  `411313432ec9b11ee1f10cf3fc943d29d87b12447bbbf64407d13c8f7f063d88`
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

All Cargo commands use the repository-wide shared-target lock.

| Requirement | Evidence class | Exact command | Result |
| --- | --- | --- | --- |
| I67-01–I67-06 production-root regression | `E2E-REAL` | `./scripts/cargo-lock.sh --wait test -p vega issue67_production_routes_keep_one_background_run_and_origin_stream -- --nocapture` | PASS (exit 0): 1 passed, 0 failed, 183 filtered; final test includes independent window teardown |
| Existing agent/permission/cancellation regressions | production controller | `./scripts/cargo-lock.sh --wait test -p vega tests::agent:: -- --nocapture` | NOT COMPLETED: shared-lock wait was stopped before Cargo acquired the lock (exit 130); coordinator-owned |
| Existing explicit Stop regression | production root | `./scripts/cargo-lock.sh --wait test -p vega r11_composer_preparation_stop_preserves_draft_and_prevents_late_start -- --nocapture` | NOT RUN: shared Cargo lock unavailable; coordinator-owned |
| Existing route/Settings regression | production root | `./scripts/cargo-lock.sh --wait test -p vega navigation_real_root_palette_mouse_shortcuts_and_settings_preserve_drafts -- --nocapture` | NOT RUN: shared Cargo lock unavailable; coordinator-owned |
| Formatting | static | `cargo fmt --all -- --check` | PASS (exit 0) |
| Patch whitespace | static | `git diff --check` | PASS (exit 0) |
| Strict Vega lint | static/build | `./scripts/cargo-lock.sh --wait clippy -p vega --bin vega --all-targets -- -D warnings` | NOT RUN: shared Cargo lock unavailable; coordinator-owned |

## Residuals

- Native single-instance UI acceptance is coordinator-owned and was not run by
  this implementation executor. The mounted production-root test is behavior
  evidence, not a native pixel or real-network claim.
- A `MockProvider` replaces only provider/network transport. The real file
  store, route handlers, controller, worker, permission queue, and stream
  entities remain in the exercised path.
- Full workspace tests/build are coordinator-owned. They are not inferred from
  focused package evidence.
- The agent suite, focused Stop/navigation regressions, and strict clippy gate
  remain coordinator-owned because another worktree retained the shared Cargo
  lock through the implementation executor's verification window.
- No known implementation deviation from the frozen Issue #67 contract.
