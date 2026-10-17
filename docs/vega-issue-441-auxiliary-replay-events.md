# #441 · 辅助文本请求适配推理回传事件

版本 v1 · 2026-10-17 · Codex 审查；用户明确要求直接修复并补回归，由 Codex 组织专职子 agent 实现、审查和集成。本卡保持 In progress，不发布等待 Vega 的 Ready 交接。

Issue：https://github.com/puzige/vega/issues/441

## 问题、证据和难度

用户在 0.1.146 看到新会话标题一直是首条消息截取值。只读检查中，相关会话有 `claimed` 状态和单独的标题请求用量：请求已触发，结果在消费阶段被丢弃。该观察不代表所有历史 `claimed` 都由本缺陷导致，超时等原有失败也会保留此状态。

代码基线：`475ef16dd91c42120826496db5de9c432862b664`。
任务分支：`feat/441-auto-title-events`；独立 worktree：`~/Workspace/worktrees/vega-441-auto-title-events`。

#435 引入的两个 `ProviderEvent` 在生产解析器中正常出现：

- Chat SSE 完成时，即使没有思考正文，也在 `Done` 前发出 `ChatReasoningContent(Absent)`；有字段时为 Empty / NonEmpty。
- Responses 完成时发出 `ResponseReplay`，保存输出项的回传布局，再完成请求。
- `AutomaticTitleRequest::collect` 只忽略旧的思考事件，两个新增事件进入失败兜底，已收到的标题被丢弃；#65 的一次性 claim 使重开/后续轮次不会重试。
- 同源遗漏存在于 `trusted_git::collect_draft`：正常的完成元数据被判为 `DraftFailed`，影响 AI 提交说明。

难度：**S，偏低**。生产改动局限于两个收集器，无数据库/协议迁移、依赖或 UI 改动；主要成本是覆盖生产 SSE 到收集/持久化的链路，防止只模拟旧事件而漏检。标题修复独立于主回合运行，现有主回合回传校验不调整。

## 验收矩阵（先于实现计划）

初始状态：下列新增自动用例均待 Actions；本地不得运行 Vega 或测试。使用进程内 HTTP 替身、真实 OpenAiProvider 编解码、真实业务/存储，不访问真实模型或运行 Git 子进程。

| ID | 前置/操作 | 可观察结果 | 层级/证据 |
|---|---|---|---|
| A1 | 新标题请求，Chat SSE 分别无 reasoning_content、空值、有值，正常结束 | 收集到规范化文本标题；元数据/思考不进入标题；恰好一次请求，用量不丢 | 生产 parser → title collector 集成回归 |
| A2 | Responses 正常 output_text、完成 output 布局（含代表性的 reasoning item） | ResponseReplay 不导致失败；标题只来自可见文本，用量保留，tools 为空 | 生产 parser → title collector 集成回归 |
| A3 | 新会话真实 claim → 后台标题 worker → SQLite 完成 | fallback → generated；重开仍为生成标题；产生独立用量行，标题结果不插入聊天消息，通知/排序规则保持 | 生产 launch / store 回归；复用既有 UI 通知测试 |
| A4 | 标题生成完成前手动改名；或故障后的重开/下一轮 | 手动名称优先；一次性 claim 不被重置，无后台补发 | 现有 CAS/at-most-once 回归；新事件至少接入一个持久化代表 |
| A5 | 错误、工具事件、非 End、无 Done、空白、文本溢出、取消/超时 | 保留 fallback，不影响主回合、不重试；已知用量仍保留；元数据不是完成信号 | 保留现有负例；补带新元数据但未完成的负例 |
| A6 | AI 提交说明收到 Chat / Responses 正常文本及新增元数据 | 返回原始可编辑文本，元数据不成为正文；不触发 Git mutation | 生产 parser → collect_draft 集成回归 |
| A7 | 提交说明包含新元数据，但 Done 后还有事件、Usage 后有正文、缺/重复 Done、非 End、Thinking/ToolUse、NUL、溢出、取消/超时 | 仍返回 content-free DraftFailed，不返回 partial draft；所有原有闭合语法断言保留 | 既有语法/截止/取消表加元数据边界负例 |
| A8 | 枚举全部生产 ProviderEvent 消费点 | 给出每处处理方式；两个修复点显式穷举事件类型，新增枚举项会产生编译待适配点 | 静态审查 + 云端编译 |

## 实现计划与边界

1. `crates/vega_conversation/src/automatic_titles.rs`，`AutomaticTitleRequest::collect`：将 `ChatReasoningContent` / `ResponseReplay` 作为本次单请求不需要回传的元数据跳过，与已有思考事件的处理一致。标题只由 `TextDelta` 构成；必须正常结束才产出。使用显式枚举拒绝 ToolUse、RetryScheduled、ResponseMetadata、非 End Done，避免事件层 `_` 吞掉下一次枚举扩展。
2. 同文件测试或其私有测试子模块：使用已有 `OpenAiProvider::with_test_sse_responder`，测试 Chat/Responses 的实际完成事件序列；以真实 store/launch 连接一次持久化代表。保留已有 #65 取消、上限、一次请求、格式化、用量和主回合隔离测试。复用已有 store/UI 测试，不重复建立同构矩阵。
3. `crates/vega_conversation/src/git_workspace/trusted_git/parsing.rs`，`collect_draft`：仅在 Done 前跳过两个新增元数据事件。剥离它们后，仍严格接受 `TextDelta* Usage* Done(End) EOF`。Done 后的任何事件仍失败；元数据不能代替 Done，不能重置 usage_started，不能让 Thinking/工具事件通过。事件类型也显式穷举，不增加通用框架。
4. `crates/vega_conversation/src/git_workspace/trusted_git/tests/summary_draft.rs`：新增实际 parser 完成路径及新元数据边界回归，保留原有严格语法表、request literal、deadline、取消和失败后 authority 可用断言。

步骤 3 是对 [S6 T34](vega-s6-tasks.md) 严格流契约的局部适配：新增的两个传输元数据事件在 Done 前被投影掉；原来的正文、用量、完成、EOF、安全和 mutation 条件不变。旧 Thinking/Summary/ReasoningReplay 仍按提交说明既有拒绝规则处理；本卡不扩大为思考模型提交说明支持。

继续遵守 [自动标题 #65](vega-issue-65-auto-title.md)：原始 composer 文本上限、无文件展开/图片/历史回传、40 字符标题、512 token、15 秒、8 KiB 文本、单次请求、独立取消、手动名称优先、用量记录及刷新行为不变。异常元数据由生产解析器原有校验拒绝，辅助收集器不修补协议正文，不将元数据持久化为标题/草稿。

允许范围仅上述两个收集器及其回归、本文。无新依赖、DDL、设置开关、模型特判或公开 API；不改 CPA、不改主循环/压缩/retry/credential guard，不修改用户数据或配置，不回填旧会话的 claimed 状态，不取消旧测试、不放宽原有失败断言。

## 其他消费点审计

| 位置 | 结论 |
|---|---|
| `automatic_titles.rs::collect` | 本卡修复：两种事件落失败兜底 |
| `git_workspace/trusted_git/parsing.rs::collect_draft` | 本卡修复：两种事件违反旧事件层白名单 |
| `agent/compaction.rs` 的 summary 收集 | 已明确忽略两种回传事件，继续检查文本、用量、截止和 Done |
| `vega_runtime/src/agent/loop_.rs` | 已明确接收两种事件，检查重复状态/布局，交给原有回传校验与预算逻辑 |
| `vega_runtime/src/openai/recovery.rs` | 透传正常事件；以 Done 标记完成，不会因为两种元数据失败 |
| `agent/credential_guard.rs::protected_stream` | 两种事件从非文本分支透传，不存在本卡的事件被误判失败；本结论仅涉及事件兼容性，不替代凭据安全审计 |
| `vega_runtime/src/provider_check.rs` | 连接检查消费原始 SSE 字节，不是 ProviderEvent 收集器；枚举引用属于测试 |
| `openai/sse.rs` / `openai/responses.rs` / `openai/mod.rs` | 生产/传输事件，正常元数据不能为迁就辅助收集器而删除 |
| 其余引用 | 类型定义、Debug、mock 或测试；无第三个同类辅助文本失败收集器 |

## 检查、交付与手测

本地仅 `git diff --check`、`cargo fmt --all -- --check` 和静态源码审查；不得 cargo check/clippy/test/nextest、运行 Vega 或真实外部进程 E2E。

云端 PR Check：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

预期：全部 required check 通过，新增 A1–A7 与既有自动标题/提交说明/主回合/压缩回归一同运行。未运行前不得报告通过，不能将 mock transport 说成真实 provider 验收。PR 引用 Issue 但不用自动关闭关键字。Codex 审查最新 head 及 CI 后 squash merge，卡转 In review，保留 Issue/分支/worktree 等用户手测。

用户手测：安装包含本卡修复的云端版本后，新建一个 Chat 模型会话，发送有清晰主题的首条消息，观察临时标题更新，重开仍保留；再用 Responses 路由做一条。生成前手动改名不会被覆盖。已有暂存改动的测试仓库中点击生成提交说明，只检查得到可编辑文本，不要求实际 commit。旧 claimed 会话不会自动重命名，可手动改名。

回滚：通过 PR revert 本卡集成提交；无数据迁移。回滚不会删除已生成的标题，旧缺陷可能再次出现。真实模型和 UI 集成待用户手测，不以源码分析冒充完成。
