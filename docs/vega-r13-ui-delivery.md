# R13-U — Sidebar organization delivery

## Freeze

- verified_at_utc: 2026-09-06T06:33:00.909565+00:00
- verified_at_local: 2026-09-06T14:33:00.909565+08:00
- branch: `codex/r13-organization-ui`
- git_head (source freeze): `d91d12d0aa73e2731d48c8aa2386e645d41b18f6`
- tracked_diff_sha256 at source freeze: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (clean)
- task_contract: `docs/vega-r13-sidebar-organization.md`, R13-U; execution guide v0.6.
- os_arch: macOS 15.7.9 / arm64; rustc 1.98.0 / cargo 1.98.0; Git 2.55.0.
- Data dependencies: R13-D contract, mutations/calendar, and final exact-creation Outcome commits are cherry-picked unchanged.
- Source scope: `vega_ui/src/sidebar` plus `vega_theme` group palette. No provider, credentials, Keychain, runtime, Git runner, or project-file changes.

## Delivered behavior

The production Sidebar now renders project/group mode controls, per-project nested tasks with five-row expansion, a local-calendar timeline, and cross-project custom groups. Project/group/task drag-and-drop shares the same revision-checked operations as the keyboard-reachable menu alternatives. Group editing, seven theme colors, dissolution, collapse, archive/restore, and atomic group task creation use the conversation service.

Organization reads/writes run on one background lane with a coalesced refresh. Acknowledgments check request generation and organization revision; task rows are withheld when the task-mutation epoch changed. New-task configuration loading is inside that lane. Created task identity comes from the service Outcome. Late creation cannot steal a newer selected project, task, or settings page; navigation still passes the draft guard. Editor acknowledgments compare the submitted input entity, group identity, and text before clearing it.

The R12 task row/action renderer is retained. Cross-project visits set the real owning project only after accepted navigation. Noncurrent task rename/unread acknowledgments update only the captured target. Task visits participate in the mutation epoch. Drag releases bypass child click actions so parent drop handlers consume them. The existing project picker and live-branch worker remain; branch visibility uses actual organization project-title rendering and does not rewrite legacy collapse configuration.

## Results

| Requirement | Evidence class | Exact command | Result / duration | Bounded footer |
|---|---|---|---|---|
| Format | static gate | `cargo fmt --all -- --check` | PASS; final source commit pre-commit repeated it | `[pre-commit] OK` |
| Strict affected all-target lint | static gate | `cargo clippy -p vega_ui -p vega_theme --all-targets -- -D warnings` | PASS / 1.27 s | `Finished dev profile` |
| Sidebar regression including R12 rows/live branches | E2E-REAL + existing UNIT tests | `cargo test -p vega_ui sidebar:: -- --nocapture` | PASS / 2.34 s | `20 passed; 0 failed; 0 ignored; 137 filtered out` |
| UI views/sort/more/group create-cancel-rename-color/menu move/restart/collapse/dissolve | E2E-REAL | same sidebar command; `mounted_sidebar_views_more_group_edit_menu_move_restart_and_navigation_guard` | PASS | Mounted production Sidebar, actual pointer/keyboard, owned file DB, 960×600 light palette |
| Noncurrent task rename/unread; archive/restore original group; cross-project accepted/rejected visits | E2E-REAL | same test | PASS | Real service state, selected project/task, unread state, and unsent input checked |
| Project/group/task drag; revision conflict preserving form and retry | E2E-REAL | same sidebar command; `mounted_sidebar_real_drag_and_conflict_retry_preserve_tasks` | PASS | Actual drag payload observed; durable project/group/membership order checked |
| Atomic task creation in a group | E2E-REAL | same sidebar command; `mounted_sidebar_group_task_creation_uses_atomic_service_identity` | PASS | Real selected project, newly opened identity, membership and task count checked |
| Ack ownership races | E2E-REAL service/handler regression, not pointer timing evidence | same sidebar command; `organization_ack_preserves_newer_editor_and_does_not_steal_later_route` | PASS | Newer editor text survives; duplicate creation preflight creates one task; real later task/settings handlers retain route |

The deterministic GPUI input helpers drain each event. The single race test therefore stages existing production handlers before the real worker acknowledgment; it adds no production test API or fake service success. Its editor mutation uses the real TextInput entity to model typing while a request is pending.

## First failures retained

- First compile failed because shared types are re-exported from `types` rather than its private module, nested method visibility was too narrow, and GPUI focus requires `cx`. The follow-up compile retained the two focus errors; the third compile passed.
- First strict clippy rejected one collapsible nested `if`; fixed without lint suppression, then strict all-target checks passed.
- First drag run: project-header child click consumed mouse-up before the parent drop listener. Production click handlers now let active drags propagate and avoid task navigation.
- Next drag/menu run exposed fixture tasks sharing the same millisecond and therefore placing the intended target outside the viewport under stable-ID sorting. The owned target now has a deterministic newest timestamp; assertions were retained and actual drag activation is also checked.
- Settings-page race was added before the fix and reproduced 19 PASS / 1 FAIL: a late creation replaced the task under the settings page. Adding settings route ownership fixed the same regression (20 PASS). No failure log was overwritten.

## Raw evidence hashes

Raw output is retained locally; the table contains no fixture paths, message content, credentials, or provider data.

| Raw log | SHA256 |
|---|---|
| `/private/tmp/vega-r13-u-check-first.log` | `e832699c3b5f1f3e95e2a3ddf15ded7cc73f8f8ceac112679030a01ec5fb792e` |
| `/private/tmp/vega-r13-u-check-2.log` | `c8c0aa6e918a9653468f45d532d14baef77430197de6f16084e518020bf11cd0` |
| `/private/tmp/vega-r13-u-check-3.log` | `53803fd150ea71a44bae1261b8761364326a2d6c882c73389ceaff02f940b769` |
| `/private/tmp/vega-r13-u-clippy-first.log` | `0630661ce8e833179015257293f7ac4b71bede7f2f6d5fad7e4f8ee03b0f27c0` |
| `/private/tmp/vega-r13-u-clippy-2.log` | `d3e2197f1697534e6bae79ccf156c5beb121fd73a160882a4c9bffc05ec601a6` |
| `/private/tmp/vega-r13-u-e2e-first.log` | `2a4d46f40846c650fd5a48221fe60a645702c815f99d2e9994818b760fdd921e` |
| `/private/tmp/vega-r13-u-e2e-2.log` | `00ce0bf45f0159bbc2580231d2dac5141a7f4e7cfc11cda9a6b4ab316dae23a8` |
| `/private/tmp/vega-r13-u-e2e-3.log` | `8a4ab94e960edf2cc23047fb4cbb121fd1dd8c7fad29828fa85988f7c370d6a3` |
| `/private/tmp/vega-r13-u-e2e-4.log` | `70ceaae166946ecb77fe5a663c4b866ca8f77490e07becbbaf2974b45ed7f1f4` |
| `/private/tmp/vega-r13-u-sidebar-first.log` | `aa3f994572c894409f95aeec6b3eed3c9228a8185e624abacfbeed83c0a0e52e` |
| `/private/tmp/vega-r13-u-sidebar-2.log` | `783e8928a17d843e7c31dd70a25d72844ebdd0450903b62821acf37e7cad0987` |
| `/private/tmp/vega-r13-u-sidebar-freeze.log` | `dcb861b7b368e12fd2f7868e4a7888f055391156c0a50693a165e89d8a43c1ac` |
| `/private/tmp/vega-r13-u-sidebar-settings.log` | `6b310f9b0a1de17d92974401bdd5ab49152e2ba05bbaa3af0eacfcae158506b5` |
| `/private/tmp/vega-r13-u-sidebar-settings-fixed.log` | `2c6678a91c1d0b2421f3ad62a96b47cb881a1595449ea43216deaf5733351d31` |
| `/private/tmp/vega-r13-u-clippy-settings.log` | `d57f9f8c32f77e73e4f00bde14a9be5cd0713383003b107e2435319d32ff116c` |
| `/private/tmp/vega-r13-u-fmt-final.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

## Residuals

- LIMIT: mounted fixtures contain the real production Sidebar plus an independent TextInput. They prove sidebar navigation guard behavior and that the input entity is untouched, not the full VegaWindow composer cache, history stack, permission/cancellation lifecycle, or native application restart.
- NOT RUN by this executor: native VegaWindow/CUA acceptance, 1280×750/dark visual inspection, actual application restart, unified workspace clippy/test/build. Root owns those checks after integration. No native app was launched by this executor.
- LIMIT: the race test is handler-level scheduling evidence, explicitly distinct from the actual pointer/keyboard main flows.
- ACCEPTED: inherited dependency `block v0.1.6` emits Cargo's future-incompatibility notice; affected strict clippy itself passes.
- NOT RUN: provider requests, performance bench/soak, and unrelated Git timing investigation. R12 Git timing residual remains unchanged; no deadlines or assertions were weakened.
- Spec deviations: none in implemented behavior; the verification boundaries above are not represented as native acceptance.

## Native follow-up — ungrouped drag target

- verified_at_utc: 2026-09-06T06:46:51.972435+00:00; local: 2026-09-06T14:46:51.972435+08:00.
- verified source: base `ff2e1b804ed0a24e5af67e7bdfeaec1fca709e21` plus tracked source diff SHA256 `a2b91b74bb9adb0726ac11586bc5e54bcabe1daa343cbb4ed952799c4a1b272b`.
- Root native acceptance found that dropping onto the “未分组” heading or an existing ungrouped task did not remove membership. The heading sat outside the drop container, and every task wrapper installed an `on_drop` listener even when it had no group destination. GPUI consumes the active payload before invoking that listener, so removing only propagation suppression would not repair it.
- The heading is now inside the ungrouped drop container. Only grouped task wrappers install the group-reorder drop listener; ungrouped rows let the section receive their drops. No project/task metadata, navigation, API, or dependency changes.
- Expanded the existing mounted 960×600 real-pointer drag test: drag to heading, reinsert, then drag onto an existing ungrouped row; read the owned file DB and assert membership removal, identical full task metadata (including project and unread), unchanged selected project/task and unsent input.
- First run failed at `ungrouped heading must accept drag-out`; the same expanded test passed after the fix. Focused drag test PASS (1 test / 1.19 s); sidebar regression PASS (20 tests); strict affected all-target clippy and fmt PASS. Root retains native recheck and unified-gate ownership.

| Exact command | Raw log / SHA256 |
|---|---|
| `cargo test -p vega_ui mounted_sidebar_real_drag_and_conflict_retry_preserve_tasks -- --nocapture` | `/private/tmp/vega-r13-u-ungroup-drop-first.log` / `d10aafcb9b51b0a4888c1c3079ac89172db06a081282dc735d9d422dd9b7431a` |
| `cargo test -p vega_ui mounted_sidebar_real_drag_and_conflict_retry_preserve_tasks -- --nocapture` | `/private/tmp/vega-r13-u-ungroup-drop-fixed.log` / `3a56d0df2b723d5957fc0c1e85bc6479066f942b144c5c8ea1b5b8730efac6ce` |
| `cargo test -p vega_ui sidebar:: -- --nocapture` | `/private/tmp/vega-r13-u-ungroup-sidebar.log` / `1b0c9f4e6dd61f98430cc77dc45af8b861106893201dba5e121374b53f7466ea` |
| `cargo clippy -p vega_ui -p vega_theme --all-targets -- -D warnings` | `/private/tmp/vega-r13-u-ungroup-clippy.log` / `53803fd150ea71a44bae1261b8761364326a2d6c882c73389ceaff02f940b769` |
| `cargo fmt --all -- --check` | `/private/tmp/vega-r13-u-ungroup-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

## Native follow-up — organization menu Escape

- verified_at_utc: 2026-09-06T06:58:18.770826+00:00; local: 2026-09-06T14:58:18.770826+08:00.
- verified source: base `a4422443aaf29f30ad565ffe8773dcf8aeac9a14` plus tracked source diff SHA256 `89e2c66502e63d0a1bc235d6f9b5788aaf16b3cc025e2240e81a8cad76ffa0e9`.
- Native acceptance found Escape left organization menus open while arrows/Return worked. `vega::main` binds Escape to `CloseSettings` in the `VegaWindow` context; this action is dispatched before the menu's raw `on_key_down`. The former mounted host called only `vega_ui::init`, which intentionally excludes those application settings bindings, and therefore did not exercise the real dispatch conflict.
- The existing mounted host now includes the actual `VegaWindow` context, Escape binding, and settings-close fallback. The original pointer/keyboard flow was extended with real Escape on filter and group menus; the first run failed at `Escape must close the filter menu under the VegaWindow action binding`.
- The focused menu now handles `CloseSettings`, clears only its menu, and stops propagation. No focus-loss or other behavior changed. The regression asserts settings remains open, database snapshot and route remain unchanged, and the independent unsent TextInput survives. This remains mounted Sidebar evidence, not a full VegaWindow claim.
- After the fix, sidebar regression PASS (20 tests / 2.33 s), strict affected all-target clippy PASS (1.32 s), fmt PASS. Testing stopped; root owns native recheck and unified gates.

| Exact command | Raw log / SHA256 |
|---|---|
| `cargo test -p vega_ui mounted_sidebar_views_more_group_edit_menu_move_restart_and_navigation_guard -- --nocapture` | `/private/tmp/vega-r13-u-menu-escape-first.log` / `daf28ce89e1adc1e95d58e04039501ce2ccf226f9419493a8a56349fa3a8f0de` |
| `cargo test -p vega_ui sidebar:: -- --nocapture` | `/private/tmp/vega-r13-u-menu-escape-fixed.log` / `6cf7de0b750e2341d3b56667fbb43fdfb10f047d472f4de4ad6bdb666a0ddf1b` |
| `cargo clippy -p vega_ui -p vega_theme --all-targets -- -D warnings` | `/private/tmp/vega-r13-u-menu-escape-clippy.log` / `9044f41350c51f9c49bd0be9bc94d03b1c4eab08b44c5a688ca7405e5eceb335` |
| `cargo fmt --all -- --check` | `/private/tmp/vega-r13-u-menu-escape-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
