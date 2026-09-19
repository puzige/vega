# Vega Issue #76 delivery

The backend section below records its independent handoff. The final
cross-layer verification and native acceptance follow at the end of this
document; the earlier "remaining gates" paragraph is historical, not the
current delivery status.

## Scope and handoff

- Backend slice: `vega_runtime`, `vega_store`, `vega_conversation`, shared
  conversation contracts and tests only.
- Worktree: `/Users/puzige/Workspace/worktrees/vega-issue76-context-compaction`
- Branch: `feat/issue76-context-compaction`
- No commit, push, install, credential change, UI/app edit, or GitHub mutation
  was performed by this slice. Root owns the remaining cross-layer/native gates.
- Raw evidence directory:
  `/Users/puzige/Workspace/vega-evidence/issue-76-2026-09-19/`

## Implemented contract

- Runtime context budgets validate positive output reserve, preserve an
  unconfigured send path, estimate the actual wire request once (system,
  tool schemas, images and run-memory reasoning included), set reserved
  `max_tokens`, enforce trigger/target/absolute-cap arithmetic, and invoke an
  automatic compaction hook once per live source revision.
- Store migrations `0009_context_compaction.sql` and
  `0010_context_compaction_status.sql` are additive. Source capture uses one
  bounded SQLite snapshot, ordered message/tool/image projections,
  `text_offset_bytes`, SHA-256 fingerprints and redacted Debug output.
  Checkpoint installation uses source/model/predecessor CAS, complete terminal
  coverage, append-only rows and a cancellation guard after writer-lock
  acquisition, before insert and before commit.
- Conversation history no longer applies the old recent-50 cut. It rebuilds
  complete chronological text/tool groups, preserves raw rows/images and
  dedup authority, applies the latest checkpoint plus uncovered tail, and
  rejects malformed or unpaired projections. Summary requests are tool-free,
  explicitly untrusted-data scoped, bounded to 60 seconds/output/input,
  usage-aware on success and failure, and never install empty/truncated/
  cancelled/failed results.
- Manual service APIs expose exact-model projection/settings and an accounted
  manual entry point. Ordinary summary failure/cancellation returns typed
  terminal events plus any received Usage; preparation/storage failures remain
  typed service errors. Operation attempts use parseable ULIDs rather than a
  process-local counter. Unknown summary accounting is sticky at thread scope
  across model changes and reopen.
- Strict `result_large_err` findings are resolved with a boxed
  `ContextCompactionFailure` error while retaining the original error and
  Usage.

## Final verification

All cargo commands below used `scripts/cargo-lock.sh`.

| Gate | Exact command | Result | Evidence |
| --- | --- | --- | --- |
| Store full library | `scripts/cargo-lock.sh test -p vega_store --lib` | 114 passed, 0 failed, 0 ignored | `store-regression-final.log` |
| Runtime full library | `scripts/cargo-lock.sh test -p vega_runtime --lib` | 115 passed, 0 failed, 4 ignored | `runtime-regression-final.log` |
| Conversation full library | `scripts/cargo-lock.sh test -p vega_conversation --lib` | 344 passed, 0 failed, 3 ignored | `conversation-regression-final.log` |
| Runtime Issue-76 focus | `scripts/cargo-lock.sh test -p vega_runtime --lib issue76_ -- --nocapture` | 11 passed, 0 failed | `runtime-issue76-focused-restored.log` |
| Conversation Issue-76 focus | `scripts/cargo-lock.sh test -p vega_conversation --lib issue76_ -- --nocapture` | 13 passed, 0 failed | `conversation-issue76-focused-restored.log` |
| Store compaction focus | `scripts/cargo-lock.sh test -p vega_store --lib context_compaction::tests:: -- --nocapture` | 11 passed, 0 failed | `store-issue76-focused-restored.log` |
| Paired projection guard | `scripts/cargo-lock.sh test -p vega_conversation --lib complete_projection_rejects_orphan_duplicate_and_missing_results -- --nocapture` | 1 passed, 0 failed | `paired-group-regression-after-strengthen.log` |
| Backend strict lint | `scripts/cargo-lock.sh clippy -p vega_runtime -p vega_store -p vega_conversation --all-targets -- -D warnings` | PASS | `backend-clippy-final-r3.log` |
| Formatting | `cargo fmt --all -- --check` | PASS | terminal output; `git diff --check` PASS |

The four new store/conversation mutation regressions were restored before the
final gates:

| Mutation | Temporary production change | Required test | Evidence of expected failure |
| --- | --- | --- | --- |
| Disable trigger | `should_compact = false` | `issue76_auto_compaction_triggers_after_tool_result_without_reexecution` | `mutation-disable-trigger-fail.log` |
| Remove paired-group role/ID protection | skip the result role and `tool_call_id` comparison | `complete_projection_rejects_orphan_duplicate_and_missing_results` (includes interleaved User counterexample) | `mutation-remove-paired-group-role-id-fail.log` |
| Allow stale checkpoint | skip source version/fingerprint fence | `stale_same_seq_checkpoint_is_rejected_and_raw_rows_remain` | `mutation-allow-stale-checkpoint-fail.log` |
| Bypass failure rollback | treat summary collector failure as a successful summary | `issue76_summary_usage_received_before_failure_is_persisted_without_checkpoint` | `mutation-bypass-failure-rollback-fail.log` |

The paired-group test was strengthened with an assistant call followed by an
interleaved non-tool message and then its result; normal code rejects it, and
the single role/ID mutation fails the assertion. An earlier broader mutation
attempt (`mutation-remove-paired-group-fail-r4.log`) is retained as historical
evidence but is not counted as the final mutation check.

## First failures retained

- `initial-runtime-tests.log` retains the initial `E0277` compile failure in
  the estimator draft; it was not counted as behavioral evidence.
- `manual-failure-first-r2.log` retains the two pre-fix failures where manual
  summary Usage/typed terminal events were discarded; `manual-failure-fixed-r1.log`
  records the corrected behavior.
- `store-context-foundation.log` retains the first cascade-test failure;
  later store foundation logs and the final full run pass.
- `backend-review-clippy-baseline-r1.log` retains the six original strict
  `result_large_err` findings; `backend-clippy-final-r3.log` is the corrected
  package-scoped strict gate.
- `conversation-regression-review-r1.log` and `conversation-regression-main-r3.log`
  retain earlier schema/list and load-sensitive image failures. The final
  serial conversation run passed the image case and all 344 active tests.
- `conversation-cancel-install-wait-r1.log` records the initially flawed
  unpolled-future race fixture; `conversation-cancel-install-wait-r3.log`
  records the corrected independent-lock-thread fixture (1 passed), and the
  store-level writer-wait guard is also in the final 114-test run.

## Remaining gates / not run by this slice

- Full workspace tests/lint, app/controller integration, installed native GPUI
  acceptance, keyboard/focus/scroll/pixel checks, and the configured real
  provider/UI journey are **NOT RUN by this backend delivery**. Root owns those
  gates and must not infer them from the package results above.
- The four runtime tests intentionally ignored by the full runtime suite remain
  load-sensitive existing tests; their final inventory is explicitly retained
  as `4 ignored`, not silently converted to pass.
- No real model/network billing claim is made. Mock/local-provider coverage
  verifies request ordering, usage persistence, fences, cancellation and
  reload semantics only.

## Final cross-layer acceptance (root, 2026-09-19)

The final installed app executable SHA-256 is
`dd56bbaaf694309d9f2b626b36e3a38bec9e7a2125f882d51676029f1877b467`.
It matches the packaged feature-worktree binary. Strict code signing and
Info.plist checks passed. The persistent local evidence bundle is
`vega-evidence/issue-76-2026-09-19` outside the worktree; its native
observations, screenshots with SHA-256, mutation logs, and test logs are not
copied into this public repository.

| Acceptance | Final evidence and scope |
| --- | --- |
| C01 | Native unconfigured `deepseek-v4.1-flash` session sent normally and received a real reply; full-history projection is covered by production tests. |
| C02 | Native UI rejected zero/equal-reserve values, persisted `7700/1024` with auto enabled across restart, and showed `未配置` after switching the same owned conversation to `hy4`; exact-model store row remained scoped to DeepSeek. |
| C03 | Runtime estimator/budget tests cover rounding, reserve, system/tool/reasoning/image contributions, absolute cap, and unknown budget. |
| C04–C06 | Conversation/controller production tests cover bounded manual summary, >50-message early constraint, paired tool chronology, auto trigger in a live tool loop, checkpoint/tail reload and no re-execution. Native real-provider auto and manual checkpoint paths both succeeded. |
| C07–C11 | Fault-injection and store/controller tests cover timeout/failure/truncation/cancel, stale source/model/route ownership, crash/reopen and CAS, impossible budgets, usage accounting and hostile source data. Four specified mutation checks each failed a named regression before restoration. |
| C12 | Real-provider native journey: automatic compaction (checkpoint 1), then manual compaction after one more complete turn (checkpoint 2), followed by real answer and app restart/continued answer. Original transcript remained browseable. The repeated `ORCHID-76` marker makes the native answer a continuation check, not an isolated earliest-turn proof; C05 provides the isolated deterministic >50-message coverage. |
| C13 | Native light/dark appearance and outside-click behavior passed. Final installed build closed the popup with one Esc both from trigger focus and from the limit input; the original native failure was repaired and host tests passed. Wide/narrow viewport is covered by production GPUI tests. |

Final commands in the feature worktree, using default test concurrency:

| Command | Result |
| --- | --- |
| `./scripts/cargo-lock.sh test --workspace --no-fail-fast` | exit 0; 1392 passed, 0 failed, 9 ignored; `workspace-final.log` |
| `./scripts/cargo-lock.sh fmt --all -- --check` | exit 0 |
| `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | exit 0; `clippy-final.log` |
| `./scripts/cargo-lock.sh xtask package` | exit 0; `package-final.log` |
| `git diff --check` | exit 0 |

The nine ordinary-suite ignored cases are four runtime load-sensitive tests,
three conversation tests, and two restart-repair tests; the relevant ignored
groups were also run separately during review and passed. Earlier failing and
interrupted full runs are retained in the local evidence bundle and were not
counted as the final gate.

An unrelated pre-existing model-selector defect remains in [Issue #68](https://github.com/puzige/vega/issues/68): after selecting a model with no thinking tiers (`hy4`), its thinking-level entry renders no menu, so the owned test conversation could not switch back through that UI. This does not change the observed #76 exact-model settings isolation. No user runtime database or configuration was manually edited for acceptance, and no unrelated user conversation was touched.

Remote publishing is intentionally separate: local `master` contains unrelated
unpublished history, so this card must not push that history merely to publish
#76. The implementation is to be squash-integrated into local `master` after
this review; the Issue/Project closure record must state local versus remote
delivery honestly.
