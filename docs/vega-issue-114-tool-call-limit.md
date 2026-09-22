# Issue #114 — Agent turn limit（对齐 Claude Code）

Status: 实现规格（spec），2026-09-22。本文把 #114 调研结论转成可实现、可验收的规格。
调研记录见 [`vega-issue-114-tool-call-limit-research.md`](vega-issue-114-tool-call-limit-research.md)（PR #115，仅记录，不含实现）。

## 1. 背景与问题

Vega 当前在运行时硬编码了一个 **tool call 次数上限**：

- `crates/vega_runtime/src/agent/mod.rs:41` `pub const TOOL_CALL_LIMIT: usize = 100;`
- `crates/vega_runtime/src/agent/loop_.rs:1678` 达到上限即硬停，追加 notice，并以
  `RuntimeFinishReason::ToolLimit` 收敛。

这个上限：

1. **不可配**（改代码才能改）；
2. **计数轴选错了**——它按「tool call 次数」计数，而一次 provider 往返（一个 agentic
   turn）里可以合法地产生多个 tool call；
3. 与对标的两个实现都不一致：
   - 本地 Codex（`codex-cli 0.155.0-alpha.9.2`）：**没有** tool call 次数上限，也没有该可配参数。
   - Claude Code 2.1.88（`~/Workspace/claude-code-2.1.88/cli.js`）：**有**一个可配的
     **agentic turn 上限**（`--max-turns <turns>` / SDK `maxTurns`），语义是
     “Maximum number of agentic turns (API round-trips) before stopping”，
     **默认不限**，达到后是**软停**并产出可观察的 `max_turns_reached`（带 `maxTurns` 数值）。

## 2. 目标与非目标

### 目标

1. 把「收敛护栏」从**按 tool call 次数硬编码**改为**按 agentic turn 次数、可配置**。
2. 默认**不限**（`0`），与 Claude Code / Codex 对齐。
3. 达到上限时**软停**：不再发起新的 provider 请求，追加可见 notice，以可观察的
   `TurnLimit` 收敛，而不是崩溃或静默截断。
4. 收敛的主要手段回到既有的 **token / 上下文预算**（#76 / #91 的 compaction 已落地）。

### 非目标

- 不改 Settings UI（本卡 config-only，与 Claude Code 的 `--max-turns`/`maxTurns` 一致；
  需要 UI 另开卡）。
- 不改 bash 入参严格校验（那是独立的安全契约，见调研文档 §建议 3）。
- 不引入「tool call 次数上限」的可配项（Claude / Codex 都无此概念）。

## 3. 术语与语义

| 术语 | 定义 |
|---|---|
| **turn** | 一次 provider 请求往返，即 `loop_.rs` 外层 `loop {` 的一次迭代。一个 turn 内可包含 0..N 个 tool call。 |
| **turn_limit** | 允许的最大 turn 数。`0` = 不限。`N>0` = 最多执行 `N` 个 turn。 |
| **soft stop** | 达到上限后不再发起新的 provider 请求；追加一条可见 notice，并以 `TurnLimit` 结束本次 run。已完成的工具结果照常持久化。 |

**计数规则**：进入外层循环每次迭代时递增 `turn_index`（从 1 开始）。当
`turn_limit > 0 && turn_index > turn_limit` 时软停。等价于「最多发起 `turn_limit` 次
provider 请求」。

**与旧行为的差异**：
- 旧：第 100 个 tool call 被拦，同一 turn 内后续 tool call 不执行。
- 新：turn 数超过 `turn_limit` 才停；单 turn 内多个 tool call 不再被次数限制拦。

## 4. 配置契约

在 `vega_store::config::AppConfig` 新增 `[agent]` 段（`crates/vega_store/src/config.rs`）：

```toml
[agent]
# Maximum number of agentic turns (provider round-trips) before a run stops.
# 0 means unlimited. Default: 0.
turn_limit = 0
```

- 类型 `u32`，`#[serde(default)]`，默认 `0`。
- `0` 表示不限；`N>0` 表示最多 `N` 个 turn。
- 旧 config（无 `[agent]`）必须可正常加载（serde default）。
- `FILE_HEADER` 注释同步更新，说明该字段。
- 校验：非法值（溢出/非整数）由 TOML 解析失败处理；负数在 `u32` 下解析失败 → `ConfigError::Parse`。不做额外 clamp（无「合法区间」概念，`0` 已是特殊值）。

## 5. 数据流（透传链路）

运行时（`vega_runtime`）不得依赖 `vega_store`，因此值由 conversation 层透传：

```
AppConfig.agent.turn_limit
  -> vega/src/app_agent.rs: run_agent_worker[_with_mcp] 读取 config_path 后取值
  -> vega_conversation::agent::PersistenceActorConfig.turn_limit  (新字段)
  -> prepare_run_with_images_and_reasoning  (pipeline.rs)
  -> vega_runtime::RuntimeToolConfig.turn_limit  (新字段，默认 0)
  -> loop_.rs 外层循环计数并软停
```

各层改动：

1. **`vega_store/src/config.rs`**：新增 `AgentPrefs { turn_limit: u32 }`（或直接在
   `AppConfig` 加 `agent: AgentConfig`），默认 `0`；更新 `FILE_HEADER`。
2. **`vega_conversation/src/agent/persistence.rs`**：`PersistenceActorConfig` 新增
   `pub turn_limit: u32`（默认 `0`），提供 `with_turn_limit(u32)` builder。
3. **`vega_conversation/src/agent/pipeline.rs`**：`prepare_run...` 读取
   `config.turn_limit`，通过 `RuntimeToolConfig::new(...).with_turn_limit(n)` 注入。
4. **`vega_runtime/src/agent/mod.rs`**：
   - `RuntimeToolConfig` 新增字段 `turn_limit: usize`（默认 `0`）。
   - 新增 builder `pub fn with_turn_limit(mut self, limit: usize) -> Self`。
   - 删除 `pub const TOOL_CALL_LIMIT: usize = 100;`（见 §6）。
   - `RuntimeFinishReason::ToolLimit` 重命名为 `TurnLimit`，doc 改为 “The run reached its configured agent turn limit.”
5. **`vega_runtime/src/lib.rs`**：从 re-export 中移除 `TOOL_CALL_LIMIT`（若删除常量）。
6. **`vega_conversation/src/types/events.rs`**：`ConversationStopReason::ToolLimit`
   重命名为 `TurnLimit`；映射 `RuntimeFinishReason::TurnLimit => ConversationStopReason::TurnLimit`。
7. **`vega/src/app_agent.rs`**：worker 读取 `config_path` 对应的 `AppConfig` 后，取
   `config.agent.turn_limit`，通过 `PersistenceActorConfig::default().with_turn_limit(..)`
   传入。注意 `#[cfg(test)]` 与 legacy caller（`config_path == None`）走默认 `0`（不限）。

> 注意：`vega/src/app_agent.rs` 已有 `let config = config_path...read_from(...)`（约 :710），
> 直接复用该读取结果，不新增磁盘读取。

## 6. 硬编码 tool call 上限的处置

对齐 Claude / Codex：**移除按 tool call 次数的硬停**。

- 删除 `TOOL_CALL_LIMIT` 常量与 `loop_.rs:1678` 的 `if tool_call_count >= TOOL_CALL_LIMIT`
  分支。
- `tool_call_count` / `executed_tool_call_count` **保留**为遥测/结果字段（与 Codex 的
  `total_tool_call_count` 同为埋点），不再作为闸门。
- 收敛依赖：可配 `turn_limit` + 既有上下文/token 预算（`ContextBudget` compaction）。

> 风险评估：移除次数上限后，极端情况下一个 run 可能跑更多轮。缓解：`turn_limit` 可配，
> 上下文预算会在超限时触发 compaction 或 `ContextRuntimeError::OverLimit`；用户可随时中断。
> 这与 Claude Code 的取舍一致（默认不限 + token/预算 + 可中断）。

## 7. 运行时行为（loop_.rs）

在外层 `loop {` 起始处（`crates/vega_runtime/src/agent/loop_.rs`，约 :898）：

```rust
let mut turn_index = 0usize; // 在外层 loop 之前初始化
loop {
    turn_index += 1;
    if tool_config.turn_limit > 0 && turn_index > tool_config.turn_limit {
        let notice = format!(
            "Agent turn limit ({}) reached; stopping without issuing another request.",
            tool_config.turn_limit
        );
        final_text.push_str(&notice);
        emit!(events, sink, RuntimeEvent::TextDelta(notice.clone()));
        messages.push(ChatMessage::new(ChatRole::Assistant, notice));
        emit!(events, sink, RuntimeEvent::Finished(RuntimeFinishReason::TurnLimit));
        return Ok(outcome(
            events, messages, final_text,
            tool_call_count, executed_tool_call_count, false, false,
        ));
    }
    // ... 既有循环体
}
```

- 软停**必须**发生在发起 provider 请求之前（计数后立即判断），保证最多 `turn_limit` 次请求。
- notice 文本稳定可断言：`"Agent turn limit (N) reached; stopping without issuing another request."`
- `turn_index` 仅在成功进入下一次外层迭代时递增；被打断/出错路径沿用既有 `Interrupted`/`Error` 收敛。

## 8. 兼容性与迁移

- **无 DB schema 变更**：`ConversationStopReason` 不落库（DB 只存 message `status`），重命名
  仅影响内存事件与 UI 投影，无需 migration。
- **配置向后兼容**：`[agent]` 缺失 → 默认 `0`（不限）。
- **默认行为变更（有意）**：默认从「100 次 tool call 硬停」变为「不限 turn」。这是本卡的核心
  目的，需在 PR 与回写中明示。
- **API 变更**：删除 `vega_runtime::TOOL_CALL_LIMIT` 导出；重命名 `ToolLimit` →
  `TurnLimit`。仓库内所有引用必须同步更新。

## 9. 验收矩阵（测试先行）

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 层级 | 状态 |
|---|---|---|---|---|---|---|
| T1 | 默认不限：不设 turn_limit 时不再有次数硬停 | `turn_limit=0` | mock provider 单轮产出 101 个 tool call 后自然结束 | 101 个 tool call 全部执行，`tool_call_count == 101`，最后事件 `Finished(End)` | runtime 单测 `default_unlimited_turn_limit_runs_all_tool_calls_in_one_turn` | 通过 |
| T2 | 有限 turn_limit 在发起第 N+1 次请求前软停 | `turn_limit=2` | mock provider 每轮都返回 tool call（永不自止） | 只发起 2 次 provider 请求；最后事件为 `Finished(TurnLimit)`；`final_text` 含 `Agent turn limit (2) reached` | runtime 单测 `configured_turn_limit_soft_stops_before_the_next_request` | 通过 |
| T3 | 单 turn 内多个 tool call 不再被次数拦（含重复 id） | `turn_limit=0` | 单轮 101 / 150 个 tool call | 全部观测、执行一次，无旧 `Tool call limit (100)` notice | runtime 单测 `repeated_call_id_counts_every_observation_and_executes_once` | 通过 |
| T4 | 配置加载：`[agent] turn_limit` 读写与默认 | 新/旧 config | `save` 后读回；无 `[agent]` 的旧文件读回 | 值一致；缺失时默认 `0` | store 单测 `agent_turn_limit_round_trips_and_defaults_zero` | 通过 |
| T5 | 透传：AppConfig → RuntimeToolConfig | config.turn_limit=7 | 走 `prepare_run...` | `prepared.request.tool_config.turn_limit == 7`；默认 `0` | conversation 单测 `issue114_configured_turn_limit_reaches_runtime_tool_config` | 通过 |
| T6 | stop reason 映射 | — | runtime `Finished(End/Length/TurnLimit)` | 映射为 `ConversationStopReason::{End,Length,TurnLimit}` | conversation 单测 `issue114_finish_reasons_map_end_length_and_turn_limit` | 通过 |
| T7 | 回归：既有 Interrupted/Error/End/Length 路径不变 | — | 既有 loop_tools / stream_persistence 测试 | 全绿 | 回归（`verify.py`） | 通过 |

**production-root 回归**：`crates/vega` 的 agent 路径（`run_agent_worker`）需覆盖
「不传 turn_limit → 不限」与「传 turn_limit → 软停」两条，验证配置确实从 `AppConfig` 生效。

**E2E**：纯后端行为，无 UI 变化。适用验收为 runtime/conversation 生产入口测试 +
`python3 scripts/verify.py` 门禁。不要求原生 UI 截图（无 UI 改动）。

## 10. 实现计划

涉及文件：

- `crates/vega_store/src/config.rs`（+ tests）
- `crates/vega_conversation/src/agent/persistence.rs`
- `crates/vega_conversation/src/agent/pipeline.rs`（+ tests）
- `crates/vega_conversation/src/types/events.rs`
- `crates/vega_runtime/src/agent/mod.rs`
- `crates/vega_runtime/src/agent/loop_.rs`
- `crates/vega_runtime/src/lib.rs`
- `crates/vega_runtime/src/agent/tests/loop_tools.rs`（改既有上限测试）
- `crates/vega/src/app_agent.rs`
- `README.md`（文档索引补一行）

执行顺序：config → 透传 → runtime → 事件映射 → 测试 → 门禁。

验证命令：`python3 scripts/verify.py`（受影响包 + 传递依赖方）。

回滚：revert 本卡 commit；默认回到旧常量行为（若需保留旧语义，可临时把 `turn_limit` 默认设为 `100`）。

## 11. 待裁决 / 记录

- 本卡默认 `turn_limit = 0`（不限），对齐 Claude Code 默认行为。
- 是否提供 Settings UI：本卡不做，另开卡。
- 是否保留「tool call 次数」作为可配护栏：不保留（Claude/Codex 均无此概念）。
