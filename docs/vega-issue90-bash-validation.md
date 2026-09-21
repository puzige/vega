# Issue #90 — bash 参数校验反馈与工具卡

关联 [#90](https://github.com/puzige/vega/issues/90) 与 [#85](https://github.com/puzige/vega/issues/85)。本规格仅处理 `bash` 参数误用与校验失败的展示。#90 评论已明确否决 `command` 别名；`bash` 仍只接受必填 `cmd` 与逻辑可选的 `timeout_ms`，不得执行含 `command` 的输入。2026-09-21 的 [Issue #85 严格参数规格](vega-issue85-tool-schema-adherence.md) 仅覆写 provider schema 表达与 `timeout_ms: null` 的默认语义，不放宽本规格的别名、额外字段、权限前拒绝或安全卡契约。

## 行为契约

1. 工具定义须明确展示可直接复制的参数示例 `{"cmd":"rg ..."}`，并说明字段名只能是 `cmd`，`command` 不受支持。#90 交付时的 schema 为 `required: ["cmd"]`；后续 #85 将 provider schema 冻结为 `required: ["cmd", "timeout_ms"]` 且 `timeout_ms` nullable，`additionalProperties: false` 保持不变。
2. 无效 `bash` 输入在权限请求和 spawn 前拒绝。返回模型的错误须使用固定、内容安全的词汇，说明 `cmd` 必填且必须是非空字符串、`command` 不受支持；不得回显原始 JSON、命令、路径或 provider 正文。既有已持久化的旧错误文本仍须能够安全恢复。
3. 参数校验拒绝是无 proposal、无权限请求的终态。Live UI 与重启后的历史卡应显示 `bash`、拒绝状态和“参数无效 / 请使用 cmd”之类的可纠正摘要；不得显示“工具结果损坏”或原始输入。只有验证失败的未知/篡改状态才显示“工具结果损坏”。
4. 只有与真实 `bash` 校验拒绝严格匹配的 call id、状态、validation approval、固定错误词汇和空执行元数据可进入这张安全卡。其他无 proposal 的终态仍 fail closed。成功 `bash`、write/edit 无效输入、reused terminal、历史恢复的现有安全行为不得回退。
5. 记录的真实问题会话有 70 次 `bash` 因 `command` 字段被拒绝（其中 15 次还同时传 `cmd`）。这用于缺陷背景，不构成接受别名的依据。

## 验收矩阵（先于实现）

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A | 正常调用不回退 | owned temp repo；允许 `bash` | 模型调用 `{"cmd":"pwd"}` 并通过原审批链 | 执行一次，成功卡显示 exit 0 | E2E-REAL | controller/UI 结果与 repo 终态 | 待测 |
| B | 常见字段误用 | owned temp repo | 模型调用 `{"command":"pwd"}` | 零审批、零 spawn；模型得到固定可纠正反馈；UI 显示参数无效 | E2E-REAL + UI | 事件、卡片与进程证据 | 待测 |
| C | 歧义与非法类型 | 同 B | 同时传 `cmd`/`command`，或空值、非字符串、未知字段 | 全部拒绝，零执行；原始值不出现在反馈或卡片 | UNIT + E2E-REAL | parser/事件断言 | 待测 |
| D | 历史兼容 | 有旧版 `invalid_input` 记录 | 重启并打开同一会话 | 显示安全参数错误卡；不展示原始输入 | 集成/UI | hydration 断言与真实 UI | 待测 |
| E | 篡改与回归 | 伪造无 proposal 终态；已有 write/edit 结果 | 加载/回放 | 伪造状态仍显示“工具结果损坏”；write/edit/正常 bash 卡无回退 | UNIT/集成 | 定向测试 | 待测 |

## 2026-09-21 验收结果

| ID | 结果 | 证据与边界 |
| --- | --- | --- |
| A | 通过 | 既有生产 Bash 权限/执行测试与本次全 workspace 回归通过；未单独留原生成功卡截图。 |
| B | 通过 | 隔离数据根、真实模型、独立构建的 Vega 原生窗口里，双字段输入在执行前被拒绝，模型收到固定 `cmd` 提示，实时卡显示“参数无效”。 |
| C | 通过 | 严格 parser、原子拒绝和内容安全审计的定向测试通过；原生双字段调用也被拒绝。 |
| D | 通过 | 同一真实调用在原生应用重启后恢复为安全错误卡；旧版记录与篡改分支另有测试。 |
| E | 通过 | 篡改状态保持 fail closed，write/edit 与正常 Bash 回归在全 workspace 测试中通过。 |

冻结源码为 `64efd22`；定向测试 8 通过，全 workspace 1,648 通过、9 忽略，格式、Clippy、构建均通过。原生实时与重启截图、构建哈希及原始日志留存在仓库外的 `issue90/manifest.md` 私有证据目录。规格偏离：无。共享 target 曾混用旧 UI 产物，原生验收因此改用独立构建目录重新构建并复验；旧产物不作为验收证据。

## 实现边界与顺序

- 先加失败测试，确认当前错误卡与反馈不满足 B/D；然后修改工具描述、固定反馈、严格事件投影及历史恢复投影。
- 不增加依赖、不改权限门禁、不改数据库 schema。#90 本身不修改 bash 解析接受集；后续 #85 仅让 `timeout_ms: null` 与缺省采用同一默认值，其他拒绝集合保持不变。
- 校验失败路径的安全投影必须不保留原始命令；不得以宽松解析换取 UI 可见性。
- 验收运行仓库门禁、真实生产 controller/UI 路径，并把证据保存在任务 worktree 之外。通过审查后按项目流程集成与回写。
