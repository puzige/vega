# Issue #291: Native draft mode settings regression

Issue: https://github.com/puzige/vega/issues/291

## Existing contract and observed failure

[R69 §3 R5](vega-r69-home-lazy-draft-composer.md#不写库) requires draft thread settings, including mode and permission, to update memory only and enter the thread INSERT on first submit. R2 defines the initial mode as Execute; it does not restrict later draft selections to Execute. R8–R11 retain stable draft identity, submit-time materialization, failure preservation, and exactly one durable thread.

The unified native regression reproduced this failure in official Vega 0.1.62 and 0.1.63: open New Task, open `+`, choose `/ask`, and Vega reports `操作未保存，请重试` while retaining Execute. The same operation succeeds on an existing conversation. The draft branch in `persist_thread_settings` explicitly rejects a changed mode, contradicting R5. Its existing permission path already uses the required memory-only projection.

## Acceptance matrix

| ID | Operation and expected result | Layer | Initial state |
|---|---|---|---|
| DRAFT-01 | In a Native draft, click Ask → Plan → Execute and change permission. Draft, OpenedThread, and composer agree; draft ID and input remain stable; a query-only owned Store observes zero thread writes and zero provider runs. | Production GPUI/controller | Original native FAIL retained; automated RED → PASS |
| DRAFT-02 | Apply a slash mode command on a draft. The accepted mode updates all projections, only the command prefix is consumed, and no thread is materialized. | Production GPUI/controller | RED → PASS |
| DRAFT-03 | Select Ask before submit; inject unavailable owned Store, then restore it and retry. Failure preserves mode, permission, input, and ID; retry materializes one thread with the selected settings. | Production GPUI/controller and owned Store | RED → PASS |
| DRAFT-04 | Select Plan before first submit. One thread materializes as Plan. When its plan is pending, the existing durable Execute guard still rejects a menu transition and preserves Plan and permission. | Production GPUI/controller and owned Store | RED → PASS |

## Implementation plan

1. Add tests for the actual menu/slash controller paths before changing production code and retain their first failing output.
2. In the existing draft branch only, copy an optional requested mode into the draft alongside permission. Publish the same projection to OpenedThread, the window draft, and the cached stream.
3. Keep owner/busy guards, durable settings, pending-plan checks, and materialization unchanged. Add no dependencies, tables, runtime configuration edits, or native app operations.
4. Run these cases plus the affected existing R69, mode-menu, and model-owner regressions. Full workspace gates remain in PR CI.

## Verification

### Freeze

- Verified at 2026-10-10 10:19:56 UTC / 18:19:56 Asia/Shanghai.
- Baseline: `80bd674`; branch: `feat/291-draft-mode-settings`.
- Content SHA-256 for the production file, test file, and named fixture file, joined as relative-path + NUL + bytes + NUL: `cf34226f76e9b00888db39bafa6e0cdeed925b302912b11436f829ca0d85fd30`.
- macOS arm64; Rust 1.98.0; nextest 0.9.146. Tests use the production GPUI/controller and owned Store with mock provider transport and existing in-process Git replay fixtures.

### First failures and corrected results

The first command failed before business assertions because the new test names had no Git fixture entries. Four entries now reuse the existing R69 setup capture; no external Git capture or execution was added. The retained setup-failure log is `qa291-draft-mode-red-20261010.log`, SHA-256 `0c71fd2cee3184b616fc0d15feb750f33e3d4165e52f916febf758c5b9eb6163`.

The unchanged production code then failed all four business tests: mode remained Execute instead of Ask or Plan. `cargo nextest run -p vega issue291_draft_mode --no-fail-fast` exited 100, run `0900130d-77a4-4e14-b764-d5ad1c6ffeb0`; `qa291-draft-mode-red-business-20261010.log` SHA-256 `21d5b2d6fcb50f17c54b819d003a871da207591cebe2d32cf0b1275f8e9f0db2`.

After the minimal draft assignment fix, the same command exited 0, run `132ac3bc-28b9-403c-a7dc-8c1e4f851130`; `qa291-draft-mode-green-20261010.log` SHA-256 `a630eff9b9df6496e59149cba386408e39d7f1295b236686c7bf17dcfc409915`. Raw bounded result:

```text
Starting 4 tests across 2 binaries (240 tests skipped)
PASS [0.314s] issue291_draft_mode_slash_preserves_remainder_without_a_write
PASS [0.520s] issue291_draft_mode_ask_survives_failed_submit_and_retry
PASS [0.546s] issue291_draft_mode_plan_materializes_and_keeps_durable_plan_guard
PASS [0.650s] issue291_draft_mode_menu_round_trip_writes_no_row
Summary [0.651s] 4 tests run: 4 passed, 240 skipped
```

Existing related regressions:

```sh
cargo nextest run -p vega -E 'test(r69_a2_) | test(r69_a3_) | test(r69_a3b_) | test(r69_a4_) | test(r69_a5_) | test(r69_a6_) | test(r69_a10_) | test(r69_a12_) | test(r69_a13_) | test(r57_plus_menu_thread_modes_) | test(r57_plus_menu_permission_selection_) | test(r11_composer_context_and_slash_) | test(model_selection_app_handler_) | test(model_selection_generic_busy_)' --no-fail-fast
```

Exit 0, run `01099701-5348-4e0a-82df-56dc73132f79`; `qa291-draft-mode-regressions-20261010.log` SHA-256 `852af21184761675c4edda3c04a2c80a6a7f8e366be32ff1e232543260fff042`. Raw bounded result:

```text
Starting 14 tests across 2 binaries (230 tests skipped)
PASS [0.248s] model_selection_generic_busy_rejects_without_releasing_other_owner
PASS [0.404s] r69_a2_typing_on_the_home_route_writes_no_row
PASS [0.442s] r69_a12_sidebar_new_task_opens_the_draft_without_a_row
PASS [0.455s] model_selection_app_handler_persists_and_runs_exact_model
PASS [0.564s] r69_a10_draft_text_survives_leaving_and_returning
PASS [0.578s] r69_a3_first_submit_materializes_the_draft_under_its_own_id
PASS [0.586s] r57_plus_menu_permission_selection_persists_through_the_real_controller
PASS [0.615s] r69_a13_materialization_failure_preserves_the_draft
PASS [0.221s] r69_a5_draft_route_has_no_controller_error
PASS [0.311s] r69_a4_submit_does_not_rebuild_the_cached_stream
PASS [0.276s] r69_a6_draft_settings_update_memory_without_a_write
PASS [0.524s] r69_a3b_second_submit_writes_no_second_row
PASS [0.796s] r57_plus_menu_thread_modes_persist_through_the_real_controller
PASS [0.865s] r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers
Summary [0.865s] 14 tests run: 14 passed, 230 skipped
```

`cargo fmt --all -- --check`, `git diff --check`, and the added-code no-comments check exited 0. No local workspace-wide tests were run. Spec deviation: none.

### Residuals

- PR cloud gate remains pending. No merge or native installation was performed by the implementation agent.
- Post-release native retest remains NOT RUN. The official 0.1.62/0.1.63 failures remain evidence for those installed versions; the passing process-local tests are separate evidence.
