# 长任务工具上下文的有界投影

## 授权、证据与范围

2026-10-18 用户要求完善上下文管理并直接实现，不能靠硬限制轮数结束长任务。脱敏样本中的调研输入由约4,900 tokens增至219,000 tokens，包含大量压缩脚本和源码读取。现有投影主要压缩大bash结果，read/grep/glob仍可能占据很大上下文；只有行数截断不能控制单行大文本。

本卡负责确定性的模型侧工具结果预览与精确重复证据压缩，以及先定位再范围读取的工作契约。保留工具执行、UI/存储审计、读取权限和编辑前读取；不修改用户文件/配置/会话，不删旧历史，不承诺真实模型速度提升百分比。分支 `feat/453-tool-context-projection`，基线为包含#443/#445/#341的origin/master。

## 验收矩阵（先于实现）

自动项待Actions，真实模型效果待用户。

| 前置与操作 | 可观察结果 | 层级 |
|---|---|---|
| 小型正常read/grep/glob/bash结果 | 原输出语义及call/result不变，不产生多余预览说明 | provider + loop |
| 大型单行、多个长行及UTF-8文本 | 模型预览最多8KiB正文头尾加最多1KiB固定结构元数据；UTF-8合法，明确有省略及恢复方法 | 投影单元与请求断言 |
| 同工具、同规范参数、相同完整结果重复 | 后续结果保留独立call/result，精确引用当前上下文仍保留的前次证据；不跳过执行 | 真实loop |
| 参数不同、结果中间部分变化、状态/退出码/截断变化 | 不按相同证据省略；不能仅比较头尾或新call ID | 正/负配对用例 |
| 首结果已被压缩移除，或冷历史/切换模型 | 不留下悬空引用；新的第一份按普通预览保留 | production compaction/history |
| 重复结果是失败、拒绝、取消、截断或元数据未知 | 保守不去重；保留明确错误/状态，不假装成功 | provider/runtime |
| read/grep/glob 结果超过已有4000行保留范围，或省略的中部改变 | 真实 runtime 截断标志为 true，不将不完整结果作为精确去重证据；现有审计正文与行截断算法不变 | readonly 真实执行 + loop |
| 写入、MCP、CUA、图片、AskUserQuestion、Goal结果 | 不套通用去重/预览，不破坏结构化或安全业务协议 | runtime/provider |
| 连续追加请求、再次投影 | 已发送前缀稳定；二次投影幂等；reasoning和布局不变 | Chat/Responses请求断言 |
| 带#452/#411指导的结果 | 指导独立保留一次，不纳入工具正文相同判定，不在截断时丢失 | 集成 |
| 停止继续、失败重试、重开、真实结果篡改 | 合法历史压缩正常；篡改仍触发严格source_changed | conversation production-root |
| 500轮各有新结果 | 仍可持续执行；不会复用过期数据或因工具量停机 | loop |

## 冻结行为

1. 主要改变 `project_model_messages` 的模型侧投影；权威 `ChatMessage.content`、持久化 tool output、状态、退出码及完整输出路径保持。一个必要的完整性修正：`agent/tools_exec.rs::execute_readonly` 已经调用 `truncate_output_lines`，却仅复制更内层的 `output.truncated`，导致超过4000行而省略中部的结果仍可能标 false。本卡保留原正文和行截断算法，但使新结果的 `truncated` 同时反映工具本身截断与该既有行截断（布尔 OR）。旧历史不迁移、不重新推断完整性，未知元数据仍保守不去重。大bash结果若早已按工具契约有界，不能假称恢复已被工具本身丢弃的字节。
2. 为内建只读read/grep/glob和bash的普通文本结果生成UTF-8头尾预览，正文上限8192 bytes，头4096尾4096，边界向内取合法字符边界；固定元数据额外不超过1024 bytes。只在确实超过预算时加省略说明。保留已存在的完整输出路径/截断标志；无完整产物时提示通过更窄的read offset/limit或带过滤的命令重新获取，不能杜撰文件路径。bash命令不能由runtime擅自重放。
3. 去重保守限于本次仍可确定完整且成功的内建read/grep/glob结果，至少1024 bytes才值得省略；bash、错误、未知/截断、可变副作用和外部工具不去重。使用完整脱敏结果摘要、规范化参数、工具名及真实状态证据；头尾相同而中部不同必须分开。
4. 扫描顺序只能引用较早且在同一上下文仍保留的首份结果，每条原tool消息仍占一条结果。引用使用结构化转义的call ID与长度，不携带可执行文本或新授权。不要用一条摘要替掉一整组tool calls，也不要把历史工具恢复成可执行调用。
5. 仅保留有界索引（最多128个候选键，超过则停止为该次扫描增加候选，仍完整预览后续结果）。去重判断不依赖后续消息，保证追加后旧前缀不变化；同一投影的二次调用保持幂等。首份因压缩/冷重建不在时重新保留新的首份，未知元数据默认不去重。
6. 运行时 guidance 是独立固定元数据，先确定结果预览/引用，再附加一次提示；原工具输出不能伪装成本地guidance。改变metadata不得掩盖历史正文/参数/顺序/状态差异。估算、usage anchor、出站请求及warm replay必须使用同一投影语义。
7. 基础提示与工具描述补充先定位文件/符号、范围读取、大结果过滤、合并独立查询与保存阶段结论的契约。不自动改变权限、已有用户明确请求范围、工具顺序，不为省tokens取消合法研究或强制Final。范围内小结果不添加解释噪声。

## 实现位置与顺序

1. `vega_runtime/src/provider.rs` 中工具身份/结果私有metadata与确定性出站投影；必要时拆私有 `tool_context` 模块。所有构造、clone、相等、内存核算与重复投影一起审查。
2. `agent/loop_.rs` 仅在新结果形成处附真实状态/截断等保守元数据；不改变执行或权威结果。冷历史未知数据不猜测；必要的历史已知状态取已有类型化字段。
   `agent/tools_exec.rs::execute_readonly` 同步修正既有行截断的标志传递，使用实际截断决定，不从正文标记字符串猜测；本卡不改变已有行数上限。
3. `agent/context_accounting.rs`、context estimate、warm replay、`vega_conversation/src/agent/compaction.rs` 核对同一投影及严格源比较；剥离仅限新增类型化投影metadata，不放宽正文/工具对校验。
4. `vega/src/agent_context.rs` 与 `agent/tools_exec.rs` 的工具描述完善读取策略。新测试进入provider/runtime和conversation现有测试层。

与#452/#411的共同文件串行集成；不为减少冲突复制guidance实现。测试必须通过实际模型请求和Store/history恢复路径断言审计未变，不能只测hash或文案。

## 验证与回滚

本地仅 `git diff --check`、格式、静态走查。Actions运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`；保持原错误/安全/压缩断言。通过最新head后合并In review，保留任务资源供用户手测。

用户手测大型文件调研、同文件修改后重读、停止继续与历史压缩；应能取回需要的具体范围、不中途丢任务。回滚代码无schema迁移；不修改生产数据来通过验收。用户本轮授权Ready后Codex直接组织专职agent实施。
