# Issue #85 — 严格工具参数生成

关联 [#85](https://github.com/puzige/vega/issues/85) 与已交付的
[#90](https://github.com/puzige/vega/issues/90)。#90 只把无效 `bash` 输入改成安全、可纠正的拒绝，
没有降低模型生成无效参数的频率。本规格处理 provider 请求侧的 schema 约束；既有执行前校验、权限门禁和
工具结果投影继续作为不可绕过的第二道边界。

## 已确认事实

- #90 上线后的本机真实会话在 2026-09-21 11:22–15:19 之间产生 418 次有终态的
  `bash` 调用，其中 47 次因参数形状无效被拒绝，拒绝率为 11.2%。
- #90 之前的 258 条同类拒绝全部是合法 JSON object：191 条只有 `command`，36 条为
  `command + timeout_ms`，31 条同时含 `cmd + command`。因此主要问题不是 SSE/JSON 拼接损坏，
  而是 Chat Completions 当前仍使用 best-effort function calling，模型会违背已经下发的 schema。
- 当前 CPA OpenAI-compatible endpoint 已用不含用户内容的最小请求验证：接受
  `function.strict: true`，并按严格 schema 返回 `cmd`/`timeout_ms`。
- OpenAI function-calling 规范建议启用 strict mode；严格 schema 的每个 object 必须
  `additionalProperties: false`，且 `properties` 中的字段全部列入 `required`。逻辑可选字段以
  `null` 联合类型表达。
- 用户已在 #90 明确否决 `command` 别名。该决定保持不变。

## 行为契约

1. Vega 自有 built-in tools 在 OpenAI Chat Completions wire 上必须显式发送
   `function.strict: true`。Vega 自有 `load_skill` / `read_skill_resource` 若 schema 满足同一严格契约，
   同样启用。外部 MCP schema 不得被 Vega 擅自改写或宣称 strict；其 wire 保持 best-effort。
2. 所有标记 strict 的 schema 必须符合 provider 契约：每层 object 都关闭额外字段，并把全部 property
   列为 required。`read.offset`、`read.limit`、`grep.path`、`bash.timeout_ms` 等逻辑可选字段使用
   `type: [原类型, "null"]`；显式 `null` 的运行时语义必须与字段缺省完全相同。
3. `bash` 仍只执行非空 `cmd`；`command`、`cmd + command`、空值、错误类型和未知字段仍在权限请求与
   spawn 前拒绝。不得通过别名、静默重写、自动执行或降低 `additionalProperties` 约束来换取成功率。
4. strict 标志属于冻结的工具定义身份。请求构造、上下文预算/指纹、压缩前后的工具 authority 必须看到
   同一个值，不能发生“预算/缓存认为是旧定义，wire 却启用 strict”的漂移。
5. provider 若拒绝 strict schema，按既有 provider 错误路径真实失败；不得自动降级为 non-strict 后继续
   执行。错误中继续执行现有凭据脱敏规则。
6. 不修改权限模式、Bash 沙箱、danger 检查、数据库 schema、历史工具记录或 #90 的安全错误卡。

## 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A | strict wire | built-in Execute/Ask/Plan 工具定义 | 构造真实 Chat Completions 请求 | 每个实际注册的 Vega built-in function 都带 `strict: true`，schema 满足 strict 子集 | UNIT + HTTP fixture | 精确 wire 断言 | 待测 |
| B | 可选字段兼容 | read/grep/bash 的逻辑可选参数 | 分别传字段缺省与显式 `null` | 两者语义一致；Bash 使用既有默认 timeout；无权限/执行绕过 | UNIT + E2E-REAL | parser 与 production controller 断言 | 待测 |
| C | 非法 Bash 不放宽 | `command`、双字段、空/错类型/额外字段 | 提交工具调用 | 继续在审批/spawn 前拒绝；模型收到 #90 固定反馈；零命令执行 | E2E-REAL | controller、事件、repo 终态 | 待测 |
| D | MCP 不被改写 | 外部 MCP 提供合法但非 strict-compatible schema | 冻结 registry 并构造请求 | schema 字节语义不变，function 不宣称 strict；调用路由不回退 | INTEGRATION | registry/wire 断言 | 待测 |
| E | 定义身份一致 | 同名/schema、不同 strict 值 | 计算上下文指纹并复用/压缩 | strict 变化导致定义身份变化；预算看到实际 wire 字段 | UNIT | fingerprint/accounting 断言 | 待测 |
| F | 真实失败率回归 | 隔离项目、最终候选 Vega、真实已配置模型 | 让 agent 连续完成至少 20 个安全、只读或 `printf` Bash 步骤 | 所有实际生成的 Bash 参数均通过严格解析；无 `invalid bash input` / “参数无效”卡；任务收敛 | LIVE PROVIDER + native UI | 调用计数、只读 DB 汇总、截图与 manifest | 待测 |
| G | 历史与正常卡回归 | 既有 #90 历史拒绝和正常 Bash | 重启并打开会话；执行正常 Bash | 旧拒绝仍安全恢复；正常调用仍显示 exit/duration；无“工具结果损坏”回退 | UI + integration | 定向测试与真实窗口 | 待测 |

## 实现边界与顺序

1. 先增加失败测试，证明当前 Chat Completions wire 没有 `strict`，并证明 strict-compatible optional
   schema/显式 `null` 尚未完整工作。
2. 在共享 `ToolDefinition` 上表达 Vega 冻结的 strict 意图；更新所有构造点和定义身份计算。不要在
   OpenAI serializer 里按工具名猜测，也不要对 MCP schema做启发式改写。
3. 将 Vega 自有 schema 改为 strict-compatible；只为“缺省等价”的字段接纳 `null`。执行器仍对最终 JSON
   做独立严格校验。
4. 更新 Chat Completions wire 与本地 HTTP fixture；对 MCP 保留 non-strict wire。
5. 运行定向测试、仓库门禁和真实 provider/native E2E。真实证据放在待删除 worktree 之外，禁止记录凭据、
   原始用户提示、命令正文或绝对项目路径。

## 非目标

- 不接受或迁移 `command`；不隐藏/删除工具失败历史；不把 validation failure 当成功。
- 不引入 provider 自动探测、配置开关、Responses API 迁移或通用 JSON Schema 重写器。
- 不改变外部 MCP 的 schema 所有权，也不承诺第三方模型在 non-strict MCP 调用上的参数正确率。
