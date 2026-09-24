# Issue #174 — stable Composer during provider readiness

Status: user-authorized implementation contract, 2026-09-24.

This revision supersedes the visible pending/preparing indication in [R9](vega-r9-workspace-panels.md). The R9 provider-readiness, cancellation and durable-echo contracts remain in force.

## Scope

The current production render condition inserts a separate `正在准备请求…` row below the Composer while `composer_submit_pending` is true. That row adds vertical space during submission and removes it after the durable echo or a preflight rejection. Remove the row and its conditional layout contribution.

Keep `composer_submit_pending` as the single-flight submit guard and preserve the current preflight worker, draft retention, send/run state, durable echo, and error handling. Do not reserve an empty row or replace it with another waiting label. Keep genuine controller errors, warnings, active-run status, and the existing send/stop controls under their current conditions. An actual error may add its existing feedback row.

No provider, credential, persistence, message, keyboard, or theme behavior changes are in scope.

## Acceptance matrix

| ID | Scenario | Required result | Evidence |
|---|---|---|---|
| P1 | Composer enters and remains in its pending state while provider readiness is unresolved | No preparation label or extra row appears; the Composer's height does not change because of the pending transition | Production-render regression compares idle with the retained pending state; existing provider-readiness tests cover the controller path |
| P2 | Provider readiness succeeds and the durable echo arrives | Exactly one submission follows the existing path; the draft and echo handshake behave as before | Existing or focused production-path test |
| P3 | Provider preflight rejects quickly | The draft remains available, pending state clears, and the actionable preflight error remains visible | Existing or focused production-path test |
| P4 | Duplicate send while pending | No second submission is emitted; existing busy/single-flight behavior remains | Focused Composer regression |
| P5 | A later controller failure or active run is shown | Existing error, warning, run-status, and send/stop projections remain intact | Focused render/state regression |

For layout evidence, compare the production Composer bounds before and during a pending-only transition with no error or warning. The comparison must include the full Composer wrapper, not just the input field. Do not hide or suppress a genuine error to satisfy the stable-height assertion.

## Implementation plan

1. Add the render regression first, using the existing production Composer test harness and real pending-state transition.
2. Remove only the conditional preparation row and any now-unused selector or render-only state dependency.
3. Run the focused `vega_ui` regressions for pending send, error projection, and Composer layout; record exact commands and results in the delivery report.
4. Check the diff for unrelated behavior and spec drift before opening the PR.

## Verification

The new layout regression failed against the original render row as expected:

```text
pending-only transition changed Composer wrapper height from 131px to 158.5px
exit code: 100
```

Focused passing runs after the change:

| Command | Bounded output | Exit |
|---|---|---:|
| `cargo nextest run -p vega_ui issue174_pending_preflight_keeps_composer_wrapper_height_and_stop_projection` | `PASS [0.030s]`; 1 passed, 458 skipped | 0 |
| `cargo nextest run -p vega_ui composer_echo_waits_for_durable_acceptance` | `PASS [0.016s]`; 1 passed, 458 skipped | 0 |
| `cargo nextest run -p vega_ui credential_failure_keeps_draft_and_renders_recovery_error` | `PASS [0.014s]`; 1 passed, 458 skipped | 0 |
| `cargo nextest run -p vega_ui issue66_enter_respects_the_submit_guard` | `PASS [0.080s]`; 1 passed, 458 skipped | 0 |
| `cargo nextest run -p vega_ui issue73_enabled_mcp_failure_is_visible_without_remote_content` | `PASS [0.070s]`; 1 passed, 458 skipped | 0 |
| `cargo nextest run -p vega a7_repaired_readiness_submits_retained_draft_once` | `PASS [0.346s]`; 1 passed, 200 skipped | 0 |
| `cargo nextest run -p vega a7_first_submit_missing_credential_rejects_before_materialization` | `PASS [0.209s]`; 1 passed, 200 skipped | 0 |
| `cargo nextest run -p vega a7_repeated_rejected_submit_creates_no_rows_or_run` | `PASS [0.255s]`; 1 passed, 200 skipped | 0 |
| `cargo nextest run -p vega r11_composer_preparation_stop_preserves_draft_and_prevents_late_start` | `PASS [0.372s]`; 1 passed, 200 skipped | 0 |
| `cargo fmt --all -- --check` | no output | 0 |
| `git diff --check` | no output | 0 |

Provider-path tests use owned fixtures and a mocked provider boundary. No workspace-wide tests or lint checks were run locally; the PR cloud check remains authoritative. Real-provider and native UI acceptance remain with the user.

Spec deviation: none.
