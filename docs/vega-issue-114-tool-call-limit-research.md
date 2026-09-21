# Issue #114 — tool call limit 参数调研

Status: 调研记录，2026-09-21。本文只回答「本地 Codex 是否存在同类 tool call 数量限制及可配参数」，
不改变任何实现，也不构成对 `TOOL_CALL_LIMIT` 的修改承诺。结论中的建议需先转成 spec 与任务卡再实现。

## 结论（TL;DR）

1. 截图里的 `Tool call limit (100) reached` 是 **Vega 自己硬编码**的行为，不是 Codex 的行为。
2. 本地 Codex（`codex-cli 0.155.0-alpha.9.2`）**没有 tool call 次数上限**，也**没有对应可配参数**；
   不存在 `tool_call_limit` / `max_tool_calls` / `max_steps` / `max_turns` 这类配置项。
3. Codex 的收敛走的是 token / 上下文 / 并发维度（如 `tool_output_token_limit`、
   `model_auto_compact_token_limit`、`agents.max_threads`），不是数调用次数。
4. 截图上半句 `已拒绝运行：参数无效·cmd 需为非空字符串…` 同样来自 Vega 的严格 JSON 校验，
   Codex 的 shell 工具没有这层「只允许 `cmd`」的字段白名单。

## 截图两个现象的归属

Issue 图片里是两个独立现象，容易一起被误认为 Codex 行为：

| 截图现象 | 实际来源 |
|---|---|
| `已拒绝运行：参数无效·cmd 需为非空字符串…` | Vega `vega_runtime` 的 bash 入参校验 |
| `Tool call limit (100) reached; stopping…` | Vega `vega_runtime` 的硬编码上限 |

## Vega 侧证据

硬编码常量，无任何 config / env / 入参读取路径：

- `crates/vega_runtime/src/agent/mod.rs:41`

  ```rust
  /// Maximum number of tool calls executed by one task.
  pub const TOOL_CALL_LIMIT: usize = 100;
  ```

- `crates/vega_runtime/src/agent/loop_.rs:1624`（达到上限即停止，不执行后续调用）

  ```rust
  if tool_call_count >= TOOL_CALL_LIMIT {
      let notice = format!(
          "Tool call limit ({TOOL_CALL_LIMIT}) reached; stopping without executing additional tools."
      );
  ```

- 终止原因沿 `RuntimeFinishReason::ToolLimit` → `ConversationStopReason::ToolLimit` 上报
  （`crates/vega_runtime/src/agent/mod.rs:429`、`crates/vega_conversation/src/types/events.rs:11`）。
- 既有测试固定了该行为：`crates/vega_runtime/src/agent/tests/loop_tools.rs:638`
  `stops_after_one_hundred_tool_calls_with_visible_notice`。

全仓搜索 `tool_call_limit` / `max_tool_calls` / `toolCallLimit` **零命中**，说明当前只能改代码，不能配置。

### 截图上半句同样来自 Vega

- `crates/vega_runtime/src/agent/mod.rs:52`

  ```rust
  pub const BASH_INVALID_INPUT_OUTPUT: &str = "Tool error: invalid bash input. Use {\"cmd\":\"rg ...\"}. cmd must be a non-empty string; timeout_ms, if provided, must be a positive integer. Other fields, including command, are unsupported.";
  ```

即：bash 工具只接受 `cmd`（必填非空字符串）+ `timeout_ms`（可选正整数），其余字段一律拒绝。

## Codex 侧证据

对象：`/Applications/ChatGPT.app/Contents/Resources/codex`，`codex-cli 0.155.0-alpha.9.2`。

### 1. 二进制静态检索

| 检索项 | 结果 |
|---|---|
| `Tool call limit` | 0 命中 |
| `tool_call_limit` / `max_tool_calls` / `max_steps` / `max_turns` | 0 命中 |
| `MAX_*TURN*` / `MAX_*TOOL*` 常量 | 无 |
| `CODEX_*TOOL*MAX*` 类环境变量 | 无 |
| 桌面端 `app.asar` 搜 `maxToolCalls` 等 | 无 |

### 2. 配置项实测（`--strict-config` 会拒绝未知字段）

```text
[tool_call_limit]        -> unknown configuration field
[max_tool_call_count]    -> unknown configuration field
[max_tool_calls_per_turn]-> unknown configuration field
[tools.max_tool_calls]   -> unknown configuration field
[max_steps]              -> unknown configuration field
[max_turns]              -> unknown configuration field
```

对照：以下限制类配置 **被接受**，但都不是「调用次数」维度：

```text
tool_output_token_limit=2000        ACCEPTED   # 单次工具输出 token 上限
model_auto_compact_token_limit=100  ACCEPTED   # 上下文自动压缩阈值
agents.max_threads=4                ACCEPTED   # 子代理并发
agents.max_depth=2                  ACCEPTED   # 子代理深度
```

### 3. 真实会话反证（最关键）

本地一个真实 Codex 会话 rollout 里，**单轮（1 个 `task_started` / 1 个 `task_complete` / 1 个 `turn_id`）
执行了 132 次 `function_call`**，全程未被截断，也没有任何 limit 提示。

```text
task_started   : 1
task_complete  : 1
turn_id (uniq) : 1
function_call  : 132
```

若存在 100 次上限，该轮必然在第 100 次被拦下。实测未发生，故可判定不存在该上限。

> 注意：早先在本机历史 rollout 中搜到 18 个文件含 `Tool call limit` 字样，那是 **Vega 源码/文档文本被读进
> Codex 上下文**造成的命中，不是 Codex 自己抛出的错误。二者需区分。

### 4. Codex 实际存在的限制维度

- `tool_output_token_limit`、`model_auto_compact_token_limit`：token / 上下文预算
- `agents.max_threads`、`agents.max_depth`、`max_concurrent_threads_per_session`：子代理并发与深度
- `job_max_runtime_seconds`：运行时长
- rollout / goal token budget：token 预算
- `total_tool_call_count` / `tool_call_count`：仅是**遥测埋点字段**，不是闸门

## 对照表

| 维度 | Vega | 本地 Codex |
|---|---|---|
| tool call 次数上限 | 硬编码 100（`TOOL_CALL_LIMIT`） | 无 |
| 次数上限是否可配 | 否（改代码） | 不适用（无此概念） |
| 达到后的行为 | 硬停，追加 notice，`ToolLimit` 终止 | 不适用 |
| 主要收敛手段 | 次数上限 + 上下文预算 | token / 上下文 / 并发预算 |
| bash 入参校验 | 严格白名单：仅 `cmd` + `timeout_ms` | `exec_command`（`argv`/`workdir`/`yield_time_ms` 等），无同款白名单 |

## 建议（待裁决，非本次实现）

1. 若目标是对齐 Codex：`TOOL_CALL_LIMIT` 应改为**可配置**，默认值提高或取消，
   收敛优先依赖上下文 / token 预算而非调用次数。
2. 若必须保留护栏：保留硬停语义但改为可配（并在 UI 明确提示），而不是常量。
3. bash 入参的严格校验属于 Vega 的有意安全契约，是否放宽需单独评估，
   不应与「tool call 次数限制」混为一谈。

以上任一项都需先补 spec / 任务卡，再按 `AGENTS.md` 的卡内流程实现。

## 复现命令

```bash
# Codex 版本
/Applications/ChatGPT.app/Contents/Resources/codex --version

# 静态检索：确认无次数上限字样（0 命中）
strings -a /Applications/ChatGPT.app/Contents/Resources/codex \
  | grep -c -iE 'tool_call_limit|max_tool_calls|max_steps|max_turns'

# 配置项实测：应报 unknown configuration field
/Applications/ChatGPT.app/Contents/Resources/codex exec --strict-config \
  -c max_tool_calls=50 "echo hi"

# Vega 侧：常量定义与停止点
rg -n 'TOOL_CALL_LIMIT' crates/
```
