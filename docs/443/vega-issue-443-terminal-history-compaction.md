# 停止或失败后的历史回合参与自动压缩

- Issue：[#443](https://github.com/puzige/vega/issues/443)
- 版本：v2，2026-10-18；本次基线为 v0.1.150。下文保留 v1 已交付契约，新增部分 checkpoint 的二次压缩修复。
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

## v2：已有部分 checkpoint 的历史再次压缩

### 现场与根因

用户指定继续使用 #443，重复登记 #454 已关闭。v0.1.150 已包含 v1 修复，但 2026-10-18 的一轮继续任务在约 8 分 45 秒后失败：12 次主请求和 24 次工具调用成功，随后压缩预检查在 93 毫秒内返回 `summary_source_changed`。诊断没有记录精确拒绝分支；以下为现场元数据与生产代码的静态对应，运行验证只由 Actions 完成。

已有部分 checkpoint 覆盖旧助手正文的前 3,018 字节及部分工具轮次。旧正文共 3,976 字节；重载只保留后 958 字节，首个保留工具的绝对 offset 3,018 变为相对 offset 0。`pipeline::source_after_checkpoint` 正确切掉正文前缀、移除已覆盖工具并重定位剩余工具；`compact_impl` 的持久化投影使用该 checkpoint，历史归一化却把完整 `source` 交给 `normalize_terminal_history_groups`，因此对同一合法尾部使用了不同坐标和工具集合。v1 测试从无 checkpoint 的历史开始，没有覆盖本次前置条件。

### 验收用例（先于实现）

只使用临时 Store、现有 MockProvider 与生产历史加载/压缩路径，不运行真实模型、网络或外部进程。至少一个用例通过真实自动压缩运行入口，另一个从生产 hook 安装首个部分 checkpoint 后关闭并重开 Store，再做第二次压缩。不得仅手工裁剪 Vec 后测试比较函数。

| 用例 | 前置状态与操作 | 必须观察的结果 |
| --- | --- | --- |
| P1 停止后再次压缩 | 长工具回合已有非零正文和工具游标的部分 checkpoint；旧助手 interrupted；发起新用户回合并自动压缩 | 发出摘要请求，安装后继 checkpoint，主模型继续；摘要包含既有摘要和未覆盖尾部，保留最新用户请求，旧工具零调度；原始历史保持不变 |
| P2 失败重试与重开 | 生产 hook 安装部分 checkpoint；旧助手 failed；关闭并重开 Store 后重试并再次压缩 | 正常完成二次压缩；再次重开后投影与结果一致，无尾部丢失或工具重复；失败事实保留 |
| P3 正常结束与连续部分 checkpoint | 覆盖 done 旧助手；同一助手先后生成至少两个递进的部分 checkpoint 后成为历史，继续并压缩 | 以最新 checkpoint 为准，仅处理尚未覆盖的尾部；每次安装的 predecessor、正文/工具游标连续一致，不把已总结前缀重新插入 |
| P4 UTF-8 与工具边界 | 正文含多字节字符，切点合法且非零；首个保留工具相对 offset 为 0；含同 offset 的不同 response 轮次，保留当前 live owner 的完整工具组 | 正文按字节准确切分，工具分组/调用结果配对保持；当前回合与旧历史分别匹配其合法投影 |
| P5 真实变化仍拒绝 | 基于部分 checkpoint 的合法请求分别变更保留正文、工具参数/结果、调用顺序或 offset；另覆盖不完整工具组 | 以现有 SourceChanged/InvalidProjection 错误拒绝，摘要请求为零，checkpoint 不变；不能通过丢弃工具或比较较短前缀放行 |
| P6 保留既有围栏 | 运行 v1、continuous_compaction、history_permissions、reasoning_replay 的既有回归 | 无 checkpoint、整消息 checkpoint、当前 owner、模型/来源围栏和最终 CAS 保持原契约 |

使用参数化状态覆盖可共享夹具，但不同故障断言须有业务意义。无需扩大生产 public API；新测试名称描述行为，不嵌入卡号。至少静态证明 P1/P2 在修复前会于绝对/相对 offset 校验失败，CI 证明修复后的生产路径通过。

### v2 冻结实现方案

1. 在 `crates/vega_conversation/src/agent/compaction.rs::compact_impl` 的自动压缩来源校验分支，调用现有 `pipeline::source_after_checkpoint(&source, checkpoint.as_ref())` 取得一次 checkpoint 投影后的 source。当前 owned group 的 canonical 构造与旧终态组的归一化使用这个相同投影，避免重复实现字节和工具游标裁剪。
2. `normalize_terminal_history_groups` 对收到的 projected source 保留全部原有 owner、终态、正文、工具参数/结果、顺序与偏移检查。改动应让两侧在相同坐标内严格相等；不能简单跳过已有 checkpoint 的组、忽略 offset、截短比较或吞掉 SourceChanged。
3. 原始 `source` 仍供版本/指纹、模型/source fence、覆盖边界和 checkpoint 安装 CAS 使用；不得用 projected source 替换原始持久化事实。新的持久化 checkpoint 游标继续使用原消息字节/工具序列坐标，不把相对 offset 写回数据库。
4. 扩展 `agent/tests/terminal_history_compaction.rs`，或新增语义命名测试模块并在 `agent/tests/mod.rs` 注册。复用已有连续压缩/历史夹具，不新增依赖、DDL、生产配置、UI 或 provider 协议改动。禁止访问测试环境外的用户数据库。
5. 保留 v1 的终态白名单及所有既有测试断言。若当前 production hook 有额外前置契约导致用例无法到达目标校验，先回报主会话调整夹具/规格，不能绕过校验或扩大范围。

### v2 验证与交付

- 任务分支：`feat/443-partial-checkpoint-compaction`，复用本卡干净 worktree，从最新 origin/master 创建；v1 分支与验证证据保留。
- 用户明确授权 Codex 直接修复；主会话写规格、审查、集成，专职子 agent 是唯一代码写入者。子 agent 不改 spec、不操作生产数据、不 push/开 PR/合并。
- 本地仅快速差异和 rustfmt 静态检查；完整 fmt、Clippy、workspace nextest 由标准 PR Actions 执行。回报最新 head 的原始证据及首次失败原因，不弱化断言，不盲目重跑。
- 用户手测：新版中重开一条曾做过回合内部分压缩的已停止/失败长任务，继续至下一次自动压缩，观察任务继续且历史和最新请求保留。代码合并不代表安装完成，不自动恢复或重试用户正在运行的任务。
- 最新 head required check 通过后 squash merge；#443 转 In review 并保持开放，等待用户手测。回滚仅 revert 本次提交，无 schema/数据回滚步骤。
