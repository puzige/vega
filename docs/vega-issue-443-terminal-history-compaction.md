# 停止或失败后的历史回合参与自动压缩

- Issue：[#443](https://github.com/puzige/vega/issues/443)
- 版本：v1，2026-10-17；基线为包含 #442 的 master。
- 本卡用户明确要求 Codex 建卡后直接修复；Codex 主会话冻结方案、审查和集成，专职子 agent 实现。
- 优先级 P1；规模 S。生产修改局部，主要工作是生产路径回归。

## 问题、根因与证据边界

Vega 0.1.146 现场在停止上一轮后继续任务，20 分 23 秒后终止。本轮 18 次主模型请求均 HTTP 200，随后自动压缩预检查在 72 毫秒内以 `summary_source_changed` 失败；没有摘要模型请求。旧助手回合为 `interrupted`，57 个工具调用均有终态和结果，工具轮次标记完整。以上来自只读诊断，不公开对话正文、工作区内容或运行标识。

`pipeline::history_from_context_source` 排除当前助手与 streaming 回合，允许恢复 interrupted/failed 的已落盘历史。`ConversationCompactionHook::compact_impl` 把旧工具回合交给 `normalize_completed_live_groups`，后者却要求旧助手 `status == "done"`，把完整的 interrupted/failed 历史也拒绝为 SourceChanged。#409 处理的是运行时思考字段差异，未覆盖这条终态准入路径。

日志没有记录具体拒绝分支，不能声称已完成现场内存级复现。本卡通过真实历史加载和压缩 hook 的进程内回归确认该代码缺陷；不是修复所有可能的 SourceChanged。

## 验收用例（先于实现，当前均待 CI）

使用临时 Store、已有 MockProvider 和真实生产入口；不启动真实模型、外部进程或桌面应用。不得只测新条件表达式。优先复用现有 agent 测试设施，至少一个正例贯穿新回合的自动压缩触发、摘要请求和后续主模型请求。

| 用例 | 前置状态与操作 | 必须观察的结果 |
| --- | --- | --- |
| T1 停止后继续 | 已停止助手为 interrupted，保留正文、多个工具轮次及完整的 success/cancelled 结果；提交后续用户输入并自动压缩 | 摘要 provider 确实被调用，checkpoint 安装，后续主模型继续；没有伪 SourceChanged，不重执行旧工具；旧助手和工具状态、正文、审计参数/结果不变 |
| T2 失败后重试 | 旧助手为 failed，含完整的 success/failed/rejected 结果；提交新回合并压缩 | 同 T1；失败事实保留，不能改成 done 或把失败工具伪装为成功 |
| T3 重开历史后压缩 | 对 interrupted 和 failed 两种历史关闭并重开 Store，经生产 history loader 读取，再走自动压缩；覆盖空助手正文/相同文本 offset 的多轮工具 | 正常生成 checkpoint；再次重开并加载 checkpoint 后，投影与压缩结果一致，已覆盖的工具不重复，保留的用户请求和当前工具后缀不遗漏；原历史保持不变 |
| T4 正常终态 | 已有 done 回合及当前 live owned 工具组 | 原行为保持；保留已有持续压缩、思考字段和模型切换回归 |
| T5 真实历史变更 | 在合法 interrupted/failed 样本上分别改请求中的旧正文、工具审计参数、结果、调用顺序；另保留新增用户/model/source/CAS 既有反例 | 来源不一致继续拒绝；不得发出摘要请求或安装错误 checkpoint，原始历史不被修改 |
| T6 不完整与活动历史 | 缺失/重复工具结果、工具仍在运行，或旧助手仍为 streaming；若存储允许未知助手状态也覆盖 | 拒绝为既有 SourceChanged/InvalidProjection 分类，摘要请求为零，checkpoint 不变；不靠状态白名单放过不完整组 |

T1/T2 可复用真实运行入口产生终态，或使用与生产完全同形的持久化终态夹具，再经生产历史 loader 和自动压缩 hook 验证。必须另有至少一个真实 run_turn 自动触发的集成正例，断言压缩后主请求正常结束。T3 必须真实关闭/重开临时数据库，不能仅 clone 一个消息 Vec。负例使用有意义的业务变异，不为覆盖率增加公开 API。

## 冻结实现方案

1. 在 `crates/vega_conversation/src/agent/compaction.rs` 的 `normalize_completed_live_groups` 中，把旧助手的终态准入改为明确白名单 `done | interrupted | failed`。可将私有函数重命名为表达终态历史的名称，并同步唯一调用点；不做额外结构重构。
2. 保留 owner 查找及 `owner != current_owner`、assistant role、`validate_complete_projection`、canonical 重建、临时内容脱敏、正文/图片/调用/结果/偏移/顺序校验。已终止的回合不等于任意数据都可信，仍须通过所有原有一致性检查。
3. 不修改 pipeline 的历史加载语义，不改原记录状态；不清除历史、重执行工具、禁用自动压缩或吞掉 SourceChanged。source version/fingerprint、冻结模型、predecessor 与安装 checkpoint 的事务 CAS 原样保留。
4. 在 `crates/vega_conversation/src/agent/tests/` 增加专用 `terminal_history_compaction.rs` 并在 `mod.rs` 注册，或扩展已有 compaction 测试模块。测试名描述业务行为，不加 Issue 编号。复用已有 setup、MockProvider 和真实 hook，不引入依赖。
5. 新测试须在旧的 done-only 校验上失败，正例在修复后通过；失败位置应指向旧历史校验而非预算、凭据等无关前置条件。至少根据静态调用链明确说明旧实现为何拒绝；运行验证仅由 Actions 完成。

允许范围是上述 compaction 生产函数、对应测试及本规格。无 schema、UI、provider 协议、Cache、依赖或模型设置改动。没有产品决策待确认。发现独立缺陷记录并交回主会话，不扩大本卡代码范围。

## 验证、手测与回滚

本地只做 `git diff --check`、差异审阅和快速 rustfmt 检查，不运行 Vega、编译、构建或测试。自动回归随标准 PR gate 在 Actions 执行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

新增 terminal_history_compaction 用例和现有 continuous_compaction、history_permissions、reasoning_replay 中的来源围栏用例均必须被执行。不得删除、ignore、弱化断言或增加自动重试换绿。以最新 PR head 的 required check `check (fmt, clippy, test)` 为准；保留首次失败记录及修复原因。

用户手测：新版安装后，分别继续已停止的长工具任务、重试失败任务，以及重开会话后继续任务，触发自动压缩应继续生成。真实历史修改造成的错误仍可出现。合并不代表已安装，不操作用户的生产数据库或运行中应用。

回滚为 revert 本卡修复提交；不涉及数据库迁移或用户数据回滚。不得以关闭来源保护作为回滚方式。

## 执行交接

- 分支：`feat/443-terminal-history-compaction`；每卡独立 worktree，单一实现写入者。
- 主会话先提交并发布本规格，再委派实现；子 agent 只改允许的生产/测试文件，不改 spec、不 push/开 PR/合并。
- 子 agent 回报修改、commit、静态检查退出码、待 CI 项和偏离；完成后停止写入。主会话审查、提交 PR、等待 Actions 后 squash merge。
- 合并后卡片 In review，Issue 开放，分支/worktree 保留，等待用户手测；不提前 Done 或清理。
