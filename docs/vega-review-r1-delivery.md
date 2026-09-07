# R1 当前任务模型选择交付记录

## 联合树复验更新（2026-09-05）

修正 Settings 事件顺序后的 4 个模型选择测试已 fresh 通过，最终全量中也全部通过。原生模型切换、持久化、配置不变和重启保持均已验证。最终全量为 816 passed / 13 failed / 1 ignored，R0 与 Artifact 时序失败阻止总门禁放行。详见 [联合交付记录](vega-review-current-status.md)。下方保留独立 R1 交接时的历史快照，其中 NOT RUN / pending 已由本节及联合记录更新，不代表当前状态。

## Freeze（独立 R1 历史快照）

- verified_at_utc: 2026-09-05 05:01 UTC
- verified_at_local: 2026-09-05 13:01 +0800
- branch: `feat/review-r1-current-model`
- base/current ref: base `c888054`; implementation parent `1b4fd25`; the amended implementation HEAD is returned to the coordinating agent outside this repository document.
- tracked_diff_sha256: `2f7c7969711d056ba3912cffa45a9758b66efd35a099f808a3306038033a271c` (implementation diff against `1b4fd25`, docs excluded, after the Settings event-order correction)
- task_contract: R1 / A2-14 / S8-T47; current task model must persist to `threads.model`, gate conflicting actions during save, and be used by the next real request.
- os_arch: macOS arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- git: `git version 2.39.5 (Apple Git-154)`

## Scope delivered

The implementation keeps the model choice as an intent until the app handler has persisted and reread the authoritative `Thread`. The successful acknowledgement updates the stream projection and the next request reads the durable model. A failed or lost save retains the old authority and exposes a bounded error.

The independent R1 review found two defects. First, the model selector treated the generic trusted-action busy state as a model-save owner, so the app handler could silently return while leaving a pending selection forever. Second, a Settings-deferred full `Thread` snapshot could overwrite a newer sidebar rename when Settings closed. v0.5 now gives model saves an exact request owner separate from generic trusted busy; both UI and app boundaries explicitly reject invalid requests, and rejection only cleans the request's own pending state. The model ack and deferred refresh own only the model field and merge it into the newest entity projection.

The save path checks the trusted-action lease result, request/thread/project identity, and the current route before changing global or stream projections. Stale and duplicate callbacks only clean their own owner. Route changes clear the old stream owner; returning to the route reapplies durable authority to the current entity. A pending model save blocks submit, approved-plan start, review-plan approval, trusted branch/commit actions, and mode/permission changes in both app handlers and UI projections. The Settings regression deliberately waits for the worker to finish and release its lease before performing the durable rename, so the old full-snapshot behavior would fail the title assertion.

The configured model catalog is read through a strict read-only, path-parameterized config entry point on a worker. Settings close invalidates the catalog generation, retries empty/failing loads, and rejects old generation results. Catalog options and pricing are projected from the current Ready authority. The shared selection request payload lives in `vega_conversation::types`; no new dependency, DDL, runtime/provider protocol, Git behavior, or thinking behavior was added.

The real acceptance test uses an owned temporary config and SQLite database, the production `VegaWindow` handlers, and a `MockProvider` request recorder. It covers successful persistence and request routing, Settings catalog invalidation, pending action gates, active and stale-route protection, failure without false success or provider request, strict read-only config reads, recreated controller state, and close/reopen durable authority. The two new v0.5 cases cover generic busy ownership and Settings rename preservation; the latter's event-order correction is committed but awaits the coordinating agent's joint-tree rerun.

## Results

| requirement | evidence class | exact command | result | bounded footer/hash |
| --- | --- | --- | --- | --- |
| Focused R1 model-selection E2E before final Settings event-order correction | E2E-REAL | `cargo test -p vega --locked model_selection -- --nocapture` | PASS (historical) | 4/4 passed; `40-model-selection-r1-v05.log`; the Settings case renamed before worker completion, so this is not final evidence for the corrected ordering |
| Focused R1 model-selection E2E after Settings event-order correction | E2E-REAL | `cargo test -p vega --locked model_selection -- --nocapture` | NOT RUN — joint-tree review pending | The committed test now waits for deferred refresh and lease release before rename; no post-correction 4/4 result is claimed here |
| Existing `vega_ui` test target | UNIT/INTEGRATION | `cargo test --workspace --locked --no-fail-fast` target output | PASS for target in prior run | 115/115; `31-workspace-test-no-fail-fast.log`; the enclosing workspace run retained its separate failures |
| Strict read-only config behavior | E2E-REAL | included in the historical focused command above | PASS (historical) | `model_selection_config_read_is_strictly_read_only` passed in `40-model-selection-r1-v05.log` |
| Lint before final test-order-only edit | UNIT/INTEGRATION | `cargo clippy --all-targets --locked -- -D warnings` | PASS (historical) | zero clippy errors; existing `block` future-incompatibility notice only; `41-clippy-all-targets-after-review.log` |
| Workspace build | INTEGRATION | `cargo build --workspace --locked` | PASS | `Finished dev profile`; `34-workspace-build.log` |
| Formatting before final test-order-only edit | UNIT/INTEGRATION | `cargo fmt --all -- --check` | PASS (historical) | no output; `39-cargo-fmt-check-after-review.log`; post-correction Cargo checks are deferred to the joint tree |
| Diff hygiene | UNIT/INTEGRATION | `git diff --check` | PASS | no output; final check is retained externally |
| Full workspace tests after v0.5 correction | INTEGRATION | `cargo test --workspace --locked --no-fail-fast` | NOT RUN — joint-tree review pending | Do not rewrite the prior baseline; the coordinating agent will run the full tree after independent review |
| Previous full workspace baseline, retained unchanged | INTEGRATION | `cargo test --workspace --locked --no-fail-fast` | ACCEPTED RESIDUAL | 813 passed / 14 failed / 1 ignored; 12 Git branch failures match the known R0 baseline; two artifact failures occurred under full-suite load; `31-workspace-test-no-fail-fast.log` |
| Artifact open-in regression | FAULT-INJECTION | `cargo test -p vega_conversation --lib artifact::tests::preview_open::open_in_uses_six_exact_raw_argv_forms -- --nocapture` | PASS in isolation | 1 passed; the full suite had an additional `process_control_failed` result; `32-artifact-open-isolated.log` |
| Artifact preflight regression | FAULT-INJECTION | `cargo test -p vega_conversation --lib artifact::tests::preview_open::open_in_preflight_is_zero_attempt_and_failures_are_one_attempt -- --nocapture` | PASS in isolation | 1 passed; the full suite result was load-sensitive `TimedOut` vs expected `GitFailed`; `33-artifact-preflight-isolated.log` |

The retained full-suite footer reported three failed targets: `vega --bin vega`, `vega_conversation --lib`, and `vega_conversation --test s6_acceptance`. The twelve Git failures are the existing Apple Git `check-attr --source` capability residual documented by R0. The preflight artifact result is the known baseline load-sensitive failure and passed when isolated. The open-in raw-argv test produced an additional full-suite `process_control_failed` result and passed when isolated; this remains an environment/full-suite residual and is not folded into the R1 pass claim. No post-correction full-suite number is recorded until the joint tree rerun.

## Residuals

- **[BLOCKED] R0** — Git-source branch acceptance remains blocked by the baseline Apple Git executable lacking the required `check-attr --source` capability. R1 does not modify Git, assertions, timeout policy, or provider behavior.
- **PENDING** — The corrected focused E2E and full workspace suite await the coordinating agent's joint-tree rerun. The retained 813/14/1 footer above is the prior baseline and is not a post-correction result.
- **ACCEPTED LIMIT** — The retained full workspace baseline is not all-green because of the twelve known Git failures and the two artifact failures described above. The two artifact cases were independently rerun with their exact test names and passed in isolation; no source workaround was added.
- **ACCEPTED LIMIT** — The MockProvider proves the real app/controller request model and recorder boundary. It does not claim validation against a live provider, keychain, or network, which is outside R1.
- **NOT RUN** — No release, push, merge, PR, tag, or UI redesign action was performed. The UI redesign remains a separate task.

## Changed files

- `crates/vega_conversation/src/types/thread.rs`, `threads.rs`: shared selection request and authoritative model persistence/readback.
- `crates/vega_store/src/threads.rs`, `config.rs`: durable model update and strict read-only config path.
- `crates/vega/src/window/{mod,session,render,agent,pricing,branch,commit}.rs`, `trusted_action.rs`: worker, catalog generation, exact owner/ack handling, model-only route refresh, pricing projection, and action gates.
- `crates/vega_ui/src/conversation_stream/{core,mod,render,composer,content}.rs`: pending/error projection, durable model initialization, submit/mode/permission guards, and separation of model ownership from generic trusted busy.
- `crates/vega/src/tests/model_selection.rs`, `crates/vega/src/tests.rs`: production-handler E2E coverage and registration, including the corrected Settings event ordering.
- `docs/vega-review-r1-model-selection.md`, `docs/vega-s8-tasks.md`: frozen R1 contract and task linkage; the v0.5 SDD is included unchanged in the local delivery commit.
