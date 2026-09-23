# Issue #151 交付证据 · 默认展开最新活动（thinking / 工具调用）

关联 [Issue #151](https://github.com/puzige/vega/issues/151) 与
[冻结规格](vega-issue-151-latest-activity-expanded.md)。

本文是规格 §验收 引用的交付记录：测试先行矩阵、实现摘要、门禁结果、原生 E2E
证据要求与剩余限制。

## 根因与实现摘要

原行为：每个 `ThinkingBlock` / `ToolCard` / `ToolActivityGroup` 都在 `new()` 里把
`expanded` 初始化为 `false`，因此一次运行里所有活动单元都保持收起，用户看不到模型
当下在想什么、在跑什么。需求是「默认展开最后一个，出现新内容时自动合起来」。

实现（纯 UI 内存态，不持久化、不新增依赖/schema）：

- `crates/vega_ui/src/conversation_stream/thinking.rs`：新增
  `ThinkingBlock::set_expanded`；`append_thinking` 在创建新块前调用
  `collapse_current_activity`，随后把新块置为展开（R151-1/R151-2）。
- `crates/vega_ui/src/tool_activity_group.rs`：新增
  `ToolActivityGroup::set_expanded`。
- `crates/vega_ui/src/tool_card.rs`：新增 `ToolCard::set_expanded` 与
  `#[cfg(test)]` 访问器 `is_expanded` / `compact_visible_text`（展开后
  `visible_text()` 会含 detail 行，测试需要独立的紧凑文案断言）。
- `crates/vega_ui/src/conversation_stream/content.rs`：新增
  `collapse_current_activity`（`rposition` 找最后一个活动单元并收起 + 只重测量该
  item）；`append_live_tool` 的三个分支（单卡→分组升级、加入既有分组、边界新建）
  按 R151-3 处理；`TextDelta` 分支在分段首条非空 delta 时收起当前单元
  （R151-4）。`append_hydrated_tool` 与 `merge_tool_entries_around` **未改动**，
  保持 #70 语义与水合默认折叠（R151-6）。

## 变更文件

| 文件 | 说明 |
|---|---|
| `crates/vega_ui/src/conversation_stream/content.rs` | 状态机：`collapse_current_activity` + `append_live_tool` + `TextDelta` 分段收束 |
| `crates/vega_ui/src/conversation_stream/thinking.rs` | `set_expanded`；新建块默认展开并先收起前驱 |
| `crates/vega_ui/src/tool_activity_group.rs` | `set_expanded` |
| `crates/vega_ui/src/tool_card.rs` | `set_expanded` + 测试访问器 |
| `crates/vega_ui/src/conversation_stream/tests/issue151_latest_activity.rs` | 新增 7 条 R151 生产流测试 |
| `crates/vega_ui/src/conversation_stream/tests/issue70_tool_activity.rs` | 9 条被 #151 取代的 #70 断言按新契约更新（未削弱：仍断言同一安全/边界/生命周期事实） |
| `crates/vega_ui/src/conversation_stream/tests/mod.rs` | 注册 `issue151_latest_activity` |
| `crates/vega/src/tests/composer_actions.rs` | `i61_*` 两条 app 级测试按新契约更新：`i61_newest_live_thinking_block_is_expanded_by_default`（R151-1）与既有 `i61_provider_reasoning_*` 的显式意图注释 |
| `docs/vega-issue-151-latest-activity-expanded.md` | 冻结规格 |
| `docs/vega-issue-151-delivery.md` | 本文 |

## 测试先行矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| R151-1 | 新建活动单元默认展开 | 新线程无活动 | 发 `MessageStarted` + 首条 `ThinkingDelta` | `activity_expansion == [true]` | mounted production stream | `issue151_current_unit_opens_and_the_superseded_unit_steps_down` | PASS |
| R151-2 | 任意时刻至多一个自动展开 | 已有展开的 thinking | 追加 `ToolCallProposed` | `[false, true]`，展开数 ≤1 | mounted production stream | 同上 | PASS |
| R151-3 | 单卡→分组升级：分组展开、子调用紧凑 | 单卡展开 | 追加第二个相邻调用 | `[false, true]`，分组展开，无子调用自带 detail | mounted production stream | 同上 / `issue151_group_join_...` | PASS |
| R151-3 | 加入既有分组保持展开 | 分组展开 | 追加第三个调用 | 分组仍展开，`row_count == 4`，子调用状态不变 | mounted production stream | `issue151_group_join_keeps_the_group_expanded_with_compact_children` | PASS |
| R151-4 | 正文使当前单元收起（每分段一次） | thinking 展开 | 发首条 `TextDelta` | `[false]` | mounted production stream | `issue151_text_segment_steps_down_once_per_segment` | PASS |
| R151-4 | 同分段后续 delta 不重复扫描 | 用户手动展开 thinking | 再发同分段 `TextDelta` | 仍 `[true]`（不被打断） | mounted production stream | 同上 | PASS |
| R151-2 | 手动展开的更早单元不受影响 | thinking 收起、工具行展开 | 点击 `thinking-toggle`，再追加新调用 | `[true, true]`（旧单元保持，最新单元展开） | mounted production stream | `issue151_manual_expansion_of_an_older_unit_survives_newer_content` | PASS |
| R151-5 | 手动收起不被重新展开 | thinking 展开 | 点击收起后再发同单元 delta | 仍 `[false]`，`thinking-content` 不挂载 | mounted production stream | `issue151_user_collapsed_unit_is_never_re_expanded` | PASS |
| R151-6 | 水合/重开保持折叠 | 历史页含相邻工具 | 实时对照 vs `apply_history_page` | 实时分组展开；水合/重开分组收起、`row_count == 1` | mounted production stream | `issue151_hydration_and_reopen_stay_collapsed` | PASS |
| R151-7 | 展开只重测量自身 item | 正文 + 工具行 | 收起工具行 detail | 上方 `assistant-message` 的 top/left/height 不变（≤0.2px） | mounted production stream | `issue151_expansion_remeasures_only_its_own_item` | PASS |
| R70 回归 | 既有 #70 安全/边界/生命周期不变 | — | 全套 #70 断言 | 12 passed / 0 failed | unit | `issue70_tool_activity` | PASS |
| R151-1 | 真实 app 中最新思考块默认展开 | 真实 worker，仅一轮推理后结束 | 提交并 pump 到该轮结束 | `thinking-block` 与 `thinking-content` 均挂载（无需点击） | app E2E（真实 root + mock provider 边界） | `i61_newest_live_thinking_block_is_expanded_by_default` | PASS |
| R61 回归 | 思考边界/上限/水合不变 | — | 全套 #61 断言 + app 级 `i61_*` | 全绿 | unit + app | `thinking` / `composer_actions` | PASS |

### 测试判别力（先失败证明）

- 把新建单元的 `set_expanded(true)` 改为 `false`（去掉 R151-1 默认展开）：
  `issue151` 套件 **6 failed / 1 passed**（如
  `R151-1: the first activity unit ... opens by default`，left `[false]` right `[true]`）。
- 把 `collapse_current_activity` 体改为 no-op（去掉 R151-2/R151-4 收起）：
  `issue151` 套件 **3 failed / 4 passed**（如 `R151-2: the thinking block steps down`
  left `[true, true]` right `[false, true]`）。
- app 级：把 `thinking.rs` 新建块的 `set_expanded(true)` 改为 `false`，
  `i61_newest_live_thinking_block_is_expanded_by_default` **FAILED**（
  `#151 R151-1: the newest live thinking block is expanded by default`）；
  恢复后 PASS。
- 恢复实现后两轮均回到 **7 passed / 0 failed**。

说明新增测试确实由本实现驱动，不是与实现同构的恒真断言。

## 门禁与结果

分支 `feat/issue-151-latest-expanded`，先基于 `da4cc48` 实现，随后 rebase 到
`origin/master` @ `c926c76`（PR #158）。独立 worktree + 默认 Cargo `target/`。

| 门禁 | 命令 | 退出码 | 结果 | 原始日志 |
|---|---|---|---|---|
| 格式 | `cargo fmt --all -- --check` | 0 | 空输出 | `fmt.log` |
| Clippy | `RUSTC_WRAPPER= cargo clippy --workspace --all-targets -- -D warnings` | 0 | 无警告 | `clippy.log` |
| 工作区测试 | `RUSTC_WRAPPER= cargo test --workspace` | 0 | 1828 passed / 0 failed / 10 ignored（rebase 后） | `workspace-tests.log` |
| Doc 测试 | `RUSTC_WRAPPER= cargo test --workspace --doc` | 0 | 全绿 | `doc-tests.log` |
| 无 UI 直连 SQLite | `rg -n 'rusqlite' crates/vega_ui/src/` | 1（无匹配） | 空 | 规格 R151-8 |

`#151` 定向：`cargo test -p vega_ui --lib issue151` → 7 passed / 0 failed。

### 云端门禁（PR [#158](https://github.com/puzige/vega/pull/158)，base `master` @ `c926c76`）

| Job | 结论 | 耗时 |
|---|---|---|
| Quality (fmt, clippy, doc-tests) | pass | 4m06s |
| Build test archive | pass | 2m34s |
| Test shard 1/4 | pass | 4m25s |
| Test shard 2/4 | pass | 3m29s |
| Test shard 3/4 | pass | 4m16s |
| Test shard 4/4 | pass | 2m47s |
| **check (fmt, clippy, test)**（汇总门禁） | **pass** | 4s |

### 环境首次失败（保留）

`cargo clippy --workspace --all-targets -- -D warnings` 首次以 exit 101 失败，原因是
环境 `sccache` 指向已删除的临时目录：

```text
sccache: error: Failed to create temp dir
sccache: caused by: No such file or directory (os error 2)
  at path "/private/tmp/.vega-bash-.../sccacheFYBh3H"
error: could not compile `tokio` (lib)
```

同一命令加一次性 `RUSTC_WRAPPER=` 后 exit 0。未修改任何用户或仓库配置。

## 原生 E2E 验收要求（BLOCKED · 未完成）

**阻碍**：本卡验收时需要独占真实应用；检查时用户日常应用
`~/Documents/Vega/Vega.app`（PID 35339）**正在运行且有活跃任务**
（`ps` 观测 ~33% CPU、16 分钟累计 4 分钟 CPU 时间）。仓库约定
（`AGENTS.md`「安装须独占，先检查正在运行的任务；未经确认空闲不得强退」）
禁止为本次验收强退用户正在使用的进程。因此本卡**不替换**已安装应用、不执行
原生截图，按 skill §4「无法真实运行或保存证据时，记录未验收与阻碍，不关闭卡片」
处理。

需要在真实 `~/Documents/Vega/Vega.app`（`ai.vega`）中，用真实 provider 跑一次
多轮「思考 → 工具 → 正文」的会话，观察并保存：

1. 运行中出现新 thinking 块 / 工具行 / 分组时，**最新**一个默认展开（可见推理
   正文或安全 detail）。
2. 出现更新内容（下一个活动单元或正文）时，上一个自动收起。
3. 重开该线程后，所有活动单元为收起态。
4. Light / Dark 各一张截图，含构建 commit 与时间。

截图与 manifest 存于持久目录 `~/Documents/Vega/evidence/issue-151-latest-activity/`
（位于待删除 worktree 之外）。

## 剩余限制

- BLOCKED：原生 E2E 未执行（用户日常应用正被占用，见上）；真实 provider、原生
  窗口像素与持久化重启行为尚无本卡证据。合并/安装/清理与 Issue 回写须待该证据
  补齐后完成。
- LIMIT：本地 GPUI 测试证明生产 `ConversationStream` + 渲染路径行为，不能证明真实
  provider、原生窗口像素或持久化重启。
- LIMIT：现有依赖 `block 0.1.6` 有 Cargo future-incompatibility 提示；本卡未修改
  依赖，测试通过。
- 规格偏离：无。无 schema 迁移；回滚为撤销本卡代码改动（删除
  `collapse_current_activity` 调用点与 `set_expanded` 使用即可回到全折叠）。
