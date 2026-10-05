# Issue #64 — 上下文占用指示交付记录

## 需求与范围

- 用户场景：在 Composer 模型选择器旁只读查看当前会话输入上下文估算。
- 数据来源：复用当前 `ConversationStream` 的输入估算；容量只取唯一匹配的启用 provider/model 对中显式保存的模型输入上限。
- 容量未知时保留中性圆环并说明未配置；没有估算时隐藏。
- Composer 的分支入口依据 #191 规格保留，本卡不改它的位置、可见性、交互和 BranchSelector 行为。
- 不新增 provider 请求、数据库字段、供应商容量发现或自动压缩行为；不存取 prompt 正文。

## 验收矩阵

| ID | 风险/需求 | 前置状态 | 操作 | 预期可观察结果 | 测试层级 | 状态 |
|---|---|---|---|---|---|---|
| C64-01 | 已知用量与容量 | 135000 估算、精确 provider/model 策略显式保存 258000 输入上限 | 渲染并悬停 | 圆环可见；同一提示显示估算、52%、135k / 258k 和模型设置输入上限来源 | 纯函数 + GPUI | PASS |
| C64-02 | 容量未知 | 有估算、无有效配置上限 | 悬停或聚焦 | 显示估算及“容量未配置”；无百分比或猜测分母 | 纯函数 + GPUI | PASS |
| C64-03 | 估算未知 | 无输入估算 | 渲染 | 不挂载指示器，不显示 0 或旧会话数据 | GPUI | PASS |
| C64-04 | 超过设置上限 | 估算大于正上限 | 计算并渲染 | 提示保留真实超限百分比；圆环填充封顶 100% | 纯函数 | PASS |
| C64-05 | 整数边界 | 零、最大整数、极小正上限 | 计算与格式化 | 不除零、不溢出、不产生非有限数；输出稳定 | 纯函数 | PASS |
| C64-06 | 可访问性与焦点 | 指示器可见 | Tab 聚焦并移开鼠标 | Tab 可达；hover 与 focus 展示同一提示，辅助文本包含完整数字及来源 | GPUI | PASS |
| C64-07 | 主题和尺寸 | Light / Dark | 分别渲染 | 圆环和提示使用主题 token；控件不增加 Composer 高度 | GPUI | PASS |
| C64-08 | 更新与隔离 | 会话 A 有估算 | 切换模型或更新投影 | 仅显示当前 owner 的值；清除估算后不残留 | GPUI + 投影回归 | PASS |
| C64-09 | 运行行为 | 会话可发送 | 查看、悬停、聚焦 | 只读交互无 provider 请求，不改模型、发送状态或自动压缩 | GPUI | PASS |

## 实现计划

1. 在 `vega_ui` 新增纯显示模型，覆盖容量有效性、四舍五入百分比、超限圆环封顶和紧凑数字格式。
2. 从当前上下文控制器状态派生模型，不引入共享跨 crate 类型或新依赖。
3. 在模型选择器左侧挂载 16px 圆环与唯一的多行 tooltip；用主题 token，复用 hover / focus 状态，并设置完整 aria 文本。
4. 保持当前模型 selector popup 锚点、Composer 行高及 #191 分支入口逻辑不变。
5. 执行本卡 Nextest、fmt、diff-check、`vega_ui` Clippy。真实桌面 hover、Tab、Light/Dark 检查留待用户安装该版本后完成。

## 验证与交付

- 基线：`origin/master` `2f38c5b`（v0.1.19，包含 #157、#146、#191）。
- 分支：`feat/64-context-usage`；任务专属 worktree。
- 功能测试：`cargo nextest run -p vega_ui issue64_context_usage_` — rebase 后 PASS 11/11（504 tests skipped）。
- 格式：`cargo fmt --all -- --check` — PASS。
- Clippy：`cargo clippy -p vega_ui --all-targets -- -D warnings` — PASS。
- Diff：`git diff --check` — PASS。
- 窄布局：360px GPUI 测试覆盖 hover 和键盘 focus；触发器与提示框无间距，测试以 2px 步进穿过原间隙区域并确认提示始终挂载，移入提示后保持可见，移开后关闭。两条路径都确认提示在视口内，且不与模型选择、发送、Composer 分支入口相交。
- Spec 偏离：无。
- 原始交付记录中的桌面检查当时为 NOT RUN；后续键盘焦点修复已另有 v0.1.22 原生复验记录。当前容量来源修复仍需在合并候选上做真实桌面验收，见下方 2026-10-05 follow-up。

## Follow-up：真实桌面发现与键盘焦点修复

- 真实桌面复验版本：Vega v0.1.22。鼠标点击圆环能显示当前估算提示，但从 Composer 输入框使用 Tab / Shift+Tab 时，圆环不在可见焦点序列中，无法通过键盘打开提示。已把 #64 从 In review 退回 In progress，并记录于 Issue #64 与 v0.1.22 回归卡 #220。
- 根因：`move_composer_focus` 的自定义焦点列表包含输入框、加号、模型选择和发送/停止，漏掉了已渲染的 context usage focus handle。
- 修复：当上下文估算有效、圆环实际渲染时，将其焦点句柄插入加号与模型选择之间；没有估算、圆环隐藏时不加入不可见焦点目标。
- 回归先行：新增 GPUI 测试从实际 Composer 输入焦点按 Tab 两次到达圆环，移开鼠标后确认完整 tooltip 和无障碍 label，再验证 Tab 到模型选择及 Shift+Tab 返回圆环。修复前该测试因焦点未落到圆环而失败（exit 100）；修复后 `cargo nextest run -p vega_ui issue64_context_usage_tab_from_composer_input_reaches_indicator_and_tooltip` 通过（1/1，exit 0）。
- 定向回归：`cargo nextest run -p vega_ui issue64_context_usage_` 通过（12/12，504 skipped，exit 0）；`cargo fmt --all -- --check` 与 `git diff --check` 均 exit 0。未运行 workspace 全量测试。
- 集成状态：follow-up PR [#225](https://github.com/puzige/vega/pull/225) 已开放，代码提交 `8627edc` 的云端 Clippy、Nextest workspace 与 required check 均通过；PR 保持开放且未合并，v0.1.22 实机缺陷仍存在。合并并安装新版本后还需 Computer Use 确认 Tab/Shift+Tab、tooltip 与无估算时的焦点顺序。

## S21 production-root Tab discrepancy investigation

The S21 report says Tab from the Composer input reaches the attachment button, but another Tab does not focus the visible context ring. The existing `vega_ui` regression mounts `ConversationStream` in `StreamHarness`; this follow-up checks the same sequence through the real `VegaWindow` root and an owned fixture, without launching the app or sending provider requests.

| ID | Risk / setup | Action | Expected observation | Test layer | Status |
|---|---|---|---|---|---|
| C64-P1 | Production `VegaWindow`; current-thread context projection has an estimate and no provider request | Focus the Composer text input, press Tab once | The attachment button paints its focused surface and the tooltip stays closed | GPUI production-root regression | PASS |
| C64-P2 | Continue from the attachment button with the same visible ring | Move the pointer away, press Tab once | The context ring owns focus and its tooltip remains visible | GPUI production-root regression | PASS |
| C64-P3 | Continue from the focused context ring | Press Shift+Tab | Focus returns to the attachment button and the tooltip closes after pointer exit | GPUI production-root regression | PASS |
| C64-P4 | Production Composer with no context estimate | Traverse the same focus sequence | The ring is absent and no tooltip is shown | GPUI production-root regression | PASS |

Verification: `cargo nextest run -p vega issue64_production_root_tab_reaches_context_ring_after_attachment` passed (1 passed, 211 skipped). The test uses an owned app fixture with `MockProvider`, moves the pointer away before keyboard traversal, confirms the first Tab paints focus on the attachment button, confirms the next Tab keeps the ring tooltip visible, checks reverse focus and hidden-ring behavior, and asserts zero provider requests. A temporary removal of the context-ring handle from `move_composer_focus` made this test fail at the expected ring-tooltip assertion (0 passed, 1 failed); restoring the existing implementation returned it to PASS. `cargo fmt --all` completed successfully.

No production change was made: current `origin/master` already contains the #225 focus-list repair, and this production-root regression confirms that code handles the reported sequence. The S21 native report and source behavior remain in tension; this test does not replace a native run against a hash-verified build. Do not change context settings, persist synthetic data, install an app, or issue a provider request during this follow-up.

## 2026-10-05 follow-up: saved model capacity missing from Composer

Native acceptance found that Settings → Providers contains an explicitly saved context input policy for the selected model, while the Composer ring for a conversation using that model still says “容量未配置”. No setting was changed, no provider request was made, and no provider identity, endpoint, or local capacity value is included in this record.

The original C64-01 path used legacy per-thread `ContextSettings.context_limit`; it did not prove that the model-owned value corrected by #76 reached the Composer. The #64 contract is amended to read capacity from the exact currently unique enabled provider/model pair. Only an explicitly saved numeric input limit is a ring denominator. An absent policy, unknown input limit, or ambiguous provider stays unconfigured; the ring does not use legacy per-thread capacity or the runtime's unedited assumed default. This is a presentation rule only: it does not change #76 runtime behavior, where an absent policy row can still resolve to the editable assumed budget. Output reserve does not contribute to the displayed denominator.

| ID | Risk / setup | Action | Expected observation | Test layer | Status |
|---|---|---|---|---|---|
| C64-R1 | Exact selected provider/model has a saved numeric input limit | Read Composer context projection and hover ring | Estimate and percentage use that exact input limit | Projection 1/1; UI 13/13; production-root 1/1 | PASS (layered) |
| C64-R2 | Exact policy missing or input limit explicitly unknown | Read projection while a legacy per-thread limit exists | Ring says “容量未配置”; no percentage or legacy fallback | Projection 1/1; UI 13/13 | PASS |
| C64-R3 | Same model ID exists under another provider, or there is no unique enabled provider | Read current projection | Other provider's policy is not used | Provider resolver 1/1; projection 1/1 | PASS |
| C64-R4 | A projection read finishes after the conversation or model changes | Complete stale read after route/model switch | Stale policy is rejected and cannot replace current capacity | Stale-model UI regression; production route round-trip 1/1 | PASS |
| C64-R5 | Hover/focus with explicit saved input limit, then switch to an unknown model | Inspect tooltip and Composer state | Tooltip names the value as an estimate against the model setting; old value clears | UI 13/13; production-root tooltip 1/1 | PASS |

定向验证命令与结果：

- `cargo nextest run -p vega_conversation issue64_context_projection_uses_only_exact_saved_model_input_capacity` — PASS 1/1（543 skipped）。覆盖精确 provider/model、同 model ID 的另一 provider、无 provider、unknown 输入上限及 legacy per-thread 上限不作为分母。
- `cargo nextest run -p vega_ui issue64_context_usage_` — PASS 13/13（528 skipped）。覆盖已知/未知容量显示、旧模型延迟投影被拒绝及 tooltip 文案/状态。
- `cargo nextest run -p vega provider_model_resolution_is_exact_and_unique` — PASS 1/1（216 skipped）；`cargo nextest run -p vega i76_context_metadata_failure_retries_after_real_settings_route_roundtrip` — PASS 1/1（216 skipped）。覆盖唯一 provider 解析与设置路由往返后的投影刷新。
- `cargo nextest run -p vega i76_context_settings_real_inputs_persist_and_reopen` — PASS 1/1（216 skipped）。从 Settings UI 保存模型策略、关闭设置并刷新 production-root 投影，确认圆环及 percentage/capacity tooltip 节点出现，且无 provider 请求；fixture 同时保留不同的 legacy per-thread 上限。
- `cargo fmt --all -- --check` 与 `git diff --check` — PASS。未运行 workspace 全量测试。

覆盖边界：`ConversationStream::context_usage_source()` 在 `vega_ui` crate 内为 `pub(crate)`，`vega` crate 的 production-root 集成测试不能直接读取其分母；没有为测试扩大公开 API。分母隔离由 conversation projection 用例断言，UI 用例断言已知输入上限显示与 unknown 状态无百分比，production-root 用例覆盖 Settings 保存后投影到 Composer tooltip 的链路。真实桌面验收仍待此修复的合并候选，届时检查已知/未知容量、hover/focus、会话/模型切换和主题。

Implementation: resolve the provider through the existing unique-enabled-provider rule on the bounded worker; read the exact policy without touching credentials or the network; pass only the optional saved input limit through the conversation projection; preserve the existing load sequence and thread/model owner checks. Regressions were added before production changes.

## 2026-10-06 follow-up: Settings route leaves Context tooltip visible

Native v0.1.52 acceptance found that an open Context tooltip can remain visible after Settings → Back to app → clicking the empty Composer while the pointer is outside both tooltip and ring. Reentering and leaving the ring clears it. The minimal reproduction did not require a theme or size change. Native AX exposes no internal focus ownership, so that observation does not establish which GPUI handle was focused. The amended [C64-N1–N7 contract](vega-issue-64-context-usage.md#2026-10-06-修订settings-往返后的提示生命周期) governs this repair.

Settings removes the rendered stream while retaining its entity. GPUI's element hover state is lost on an unmounted frame; when remounted outside the pointer, its new false state sends no false transition. The stream's two entity hover flags retained the previous true value and reopened the tooltip independently of focus. The fix adds three lines in the existing `SettingsOpen(true)` observer to clear the two Context hover flags and notify. It preserves the live focus handles, estimate, model input capacity, model/provider owner, draft, runtime state, and persistent data.

The regressions use actual GPUI MouseMove events to establish trigger/popover hover. Four tests mount the production VegaWindow with an owned standalone thread, local config/database, saved model policy and message, and a MockProvider with zero requests. No real Git/shell process, Provider/MCP request, application launch, dependency, public API, migration, or user data/config mutation was introduced. Settings Back uses the production CloseSettings bindings. The actual Sidebar click tests passed before the fix; the separate `SettingsOpen` route-entry seam tests failed after returning while actual Composer focus was asserted. The seam covers the production route lifecycle, and does not prove the Settings opening action binding. Those two evidence classes remain distinct.

### Freeze and first business result

- Fresh fetch/rebase completed before repository edits; task branch `codex/64-context-settings-tooltip` stayed on the reviewed baseline.
- Spec and test sources preceded production edits. The entire 825-file tracked candidate, configuration and toolchain were archived privately before each run. Both runs used Rust/Cargo 1.98.0, Nextest 0.9.146, Darwin arm64, the worktree's default isolated target, default profile, and `retries=0`.
- First run UTC/local: 2026-10-05 20:30:24–20:31:46 UTC / 2026-10-06 04:30:24–04:31:46 Asia/Shanghai.
- Command: `cargo nextest run --offline --locked -p vega -p vega_ui -E 'test(issue64_context_settings_) | test(issue64_context_usage_settings_open_)'`.
- Run ID: `9be7948b-b283-410e-98d2-4edeaccba028`; exit 100. Raw footer: `Summary [0.336s] 5 tests run: 2 passed, 3 failed, 771 skipped`. Total command time including the new target build: 82.517 seconds.
- The two Sidebar click cases passed. Both route seam cases failed at `Settings return must not revive a stale context hover while Composer owns focus`; the observer case failed at `Settings must invalidate indicator hover`. These were business assertion failures; compilation and fixture setup succeeded. No retries or assertion changes were used.
- Raw log SHA-256: `b9076cf34baa79b405aa7a85ccce5621966f78bb81d3d911cb7ccc5fc7488c83`; first source archive SHA-256: `bb34b43c4fdc124288fc00142f67fb27aa174fb7834373b887e9e7eb1cb5d089`.

### Repair result

- The only source difference between the first failure candidate and the passing candidate was the three-line production observer repair. All test sources and configs stayed identical; neither run changed a frozen source file while executing.
- Passing run UTC/local: 2026-10-05 20:32:23–20:32:34 UTC / 2026-10-06 04:32:23–04:32:34 Asia/Shanghai.
- Command: `cargo nextest run --offline --locked -p vega -p vega_ui -E 'test(issue64_context_settings_) | test(issue64_context_usage_)'`.
- Run ID: `b49a0a25-ac9f-482d-a347-fb7c8884c8be`; exit 0. Raw footer: `Summary [0.355s] 18 tests run: 18 passed, 758 skipped`. Total command time: 10.266 seconds.
- Scope: four production-root Settings cases, one observer case and all thirteen existing Context usage tests. This covers C64-N1–N7, including valid forward/reverse ring keyboard focus, fresh hover and pointer transfer, known/unknown capacity, estimate/model invalidation, narrow tooltip geometry, full owned message equality and model policy equality, and zero Provider requests. Ordinary navigation visits may update timestamps; those columns are excluded from the unchanged-data claim.
- Raw log SHA-256: `86117b8eb7f7447432dc518ed45d264d9b2a40142306f493867dfe5026ffedfd`; passing source archive SHA-256: `87767b332c9c3467c7541af9f34d9f7ac6ec96a44add09f42fc1c24954d76671`.
- Frozen production-root test source SHA-256: `7b5ddfec25b5967f79edd094822bd2cc29b5a442b99789ffbaa18935374a7509`; frozen UI Context test source SHA-256: `0026895716881b60e1ebcf04009dd131f66438275b1bb711508eb33e213151c9`.
- Existing dependency future-incompatibility notice for `block 0.1.6` remains visible in both raw logs; it did not fail either compilation. No dependency was changed.

### Residuals and recovery

- NOT RUN: local workspace gate and repaired native app. Full fmt/Clippy/Nextest remains the cloud PR gate; the main agent owns PR/merge and hash-verified native repeat. This report does not claim cloud or native PASS.
- The first two Sidebar click cases being green before the repair limit the inference about exact native event order. The route seam's red→green proves the cached-hover lifecycle defect, while the supplied native failure remains separate evidence.
- Spec deviation: none. Rollback is reverting this task's commit; no user data, installation, config, schema, or public API needs restoration.

## 2026-10-06 integration update after S18 merge

PR [#276](https://github.com/puzige/vega/pull/276) originally tested `761ad45`. The main agent retained the original cloud evidence: attempt 1 was cancelled before a hosted runner was acquired, with zero steps; attempt 2 completed with Clippy and the required check successful, and Nextest run `241ebf2b-a386-4a12-b730-aef510deb9b9` reported 2017 passed / 5 skipped under the default profile. S18 PR [#274](https://github.com/puzige/vega/pull/274) then merged into master, making #276 behind the strict required-check base. The original cloud result is historical evidence for its exact original HEAD, not a gate pass for the rebased candidate.

- Fresh fetch and rebase used actual `origin/master` `4fc330ac`; rebased tested HEAD was `7f006e8d`. The worktree was clean when frozen. The entire 827-file tracked source, HEAD/tree, configuration and toolchain were archived before the first new-baseline run.
- The Context production file, both Context test files, test module declaration, Cargo.lock, Nextest config and toolchain file were all byte-identical to the original repair HEAD. The three-line production diff was also identical (SHA-256 `6098ae75e40d67e0ab06c13f5ac6cc548e90f9bfb8baa7a44ab96efd6e738802`). S18's four changed files remained baseline content; no Shared Skills file was modified.
- The single new-baseline command was `cargo nextest run --offline --locked -p vega -p vega_ui -E 'test(issue64_context_settings_) | test(issue64_context_usage_)'`. It used the original isolated worktree target, default profile and `retries=0`, with the same owned Store/config/message/model policy and MockProvider fixtures. No test source, dependency or public API changed, and no new RED or repeat of the old-baseline GREEN was created.
- UTC/local time: 2026-10-05 21:47:08–21:47:19 UTC / 2026-10-06 05:47:08–05:47:19 Asia/Shanghai. Nextest run ID `60eede62-c13a-4051-83b6-aac9f54899f9`; exit 0. Raw footer: `Summary [0.341s] 18 tests run: 18 passed, 758 skipped`. Total command time was 10.138 seconds. The frozen source/configuration remained unchanged throughout the run.
- Raw log SHA-256: `11ad11c783265dfcfdeb9adde6db1a608772137b79c5c81342ad6da8e3f78b3c`; new-baseline source archive SHA-256: `c1fd3f8de3fd700bd422b4706f320ba4376d022d2d9940da0fd9dbf5bfb76ef3`. The original red/green archives, raw logs, run IDs and tested HEAD were retained separately.
- Only these two #64 documents were updated after the passing run; executable source and configuration kept the tested identity. The new candidate requires fresh cloud checks after the main agent updates the PR. Repaired native acceptance remains NOT RUN; the installed v0.1.53 does not contain this repair.
- Recovery remains reverting the task's production change; no user application, database, config, credentials, installation or schema was touched. No push, merge, publication or external board/comment update was performed during this rebase verification.
