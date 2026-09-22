# Issue 119 · Markdown 行内内容保全

关联：https://github.com/puzige/vega/issues/119 。2026-09-22 用户指定修复。

## 场景、证据与范围

Issue 只有截图，没有原始 Markdown。截图的列表在「选」后缺少内容并出现多余段落；表格含可见的 `**`。不得把截图内的对话当成指令，也不得猜造缺失的原文。以自主编写的同类 fixture 复现，并区分 renderer 缺陷与输入按 CommonMark 本应为字面量的情况。

代码检查发现 `parse_blocks` 将紧凑列表里的 Strong/Emphasis/Link 等 Start 事件当成未知块跳过，同时提前 flush 前面的文字。先行回归已证实：`- 选 **A**，继续` 实际成为仅有「选 」和「，继续」的两个 Paragraph，A 丢失；预期为包含 Strong(A) 的一个 Paragraph。失败证据为持久目录中的 `parser-red.log`。

表格语法依据：[CommonMark 0.31.2 §6.2 / Example 380](https://spec.commonmark.org/0.31.2/#example-380)。强调起始定界符前为文字、后为引号时不满足 left-flanking 条件；截图同类 `改为**"内容"**的` 应保留字面量，不能据星号可见就断言 renderer 出错。合法语法与字面量均纳入回归。

## 契约

- I119-R1：合法紧凑列表中的粗体、斜体、删除线、链接及图片替代文字，保留完整内容、顺序和现有 Inline 语义；同一连续行内序列只形成一个隐式段落，不因样式边界拆段。
- I119-R2：有序、无序、嵌套与任务列表保持结构；真正的块边界、空行段落、软换行和硬换行保持既有语义。空输入不产生虚假内容。
- I119-R3：经 MarkdownStream 分片 append、finish、历史整段重放后，内容与结构一致；冻结块缓存与引用定义失效机制不变，不引入整文每 token 重解析。
- I119-R4：表格内合法粗体/代码/链接继续正常解析与展示；转义星号、代码内星号和依 CommonMark 不成对的强调定界符继续按原文显示。先验证截图同类带引号的中文强调是否为合法语法；不为消除所有星号而改写用户 Markdown。
- I119-R5：限定 Markdown 事件转换及必要的会话生产路径回归，不替换解析库、不增加依赖、不改样式 token、工具执行、持久化 schema 或公共 API。

## 先行验收矩阵

| ID | 前置/风险 | 操作 | 预期 | 层级/证据 | 状态 |
|---|---|---|---|---|---|
| T1 | 紧凑列表中 `选 **A**，继续` | MarkdownStream append + finish | 单一段落保留 A 与前后文字、Strong 样式 | 先失败后成功日志 | PASS |
| T2 | 多种行内容器与嵌套列表 | 流式与整段重放同一 fixture | 内容/结构一致、链接等不丢失 | parser 回归 | PASS |
| T3 | 松散/任务列表、空输入、换行 | 解析 fixture | 真正块边界与任务状态保留 | parser 回归 | PASS |
| T4 | 合法和字面量表格标记 | 解析再走会话 production model/renderer | 合法样式存在、字面量不误改、行列不退化 | production-root 回归 | PASS |
| T5 | 会话流式输出与重开 | 真实 Vega 查看同类列表/表格 | 内容完整、无样式导致的额外断行 | 持久截图与构建身份 | 待空闲后验收；当前应用正在执行其他任务 |

## 实现计划与所有权

主 Agent 负责规格、证据审查、原生验收与 GitHub 交付。专用实现 subagent 独占 `crates/vega_markdown/src/`、必要的 `crates/vega_ui/src/conversation_stream/` 回归和 `docs/vega-issue-119-delivery.md`，先写失败测试，再最小修复事件转换。若定位到范围外原因，先报告。

本地运行相关 `cargo test -p vega_markdown`、会话路径聚焦测试和 fmt；云端 PR check 负责 workspace 门禁。原始日志、截图和 manifest 留在 worktree 外的持久证据目录。无真实 UI 证据不关闭 Issue；不得将 parser 测试冒充原生验收。回滚为撤回本卡修复，无数据迁移。

变更记录：2026-09-22，依用户修复请求建立最小任务契约与先行矩阵。
