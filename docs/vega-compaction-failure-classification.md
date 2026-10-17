# #413 压缩失败的原因传播与历史恢复

## 冻结、范围与证据

- 规格 v1.1，2026-10-17；基线 `4eb22a1d776155be00a2e940889f8da15d125f55`，分支 `feat/413-compaction-failure-classification`。v1.1 仅补明确 F3 的非压缩具体错误优先级与乱序断言，范围不变。
- 执行必读 [AGENTS](../AGENTS.md)、[执行总纲](vega-exec-guide.md)、[Issue 工作流](vega-issue-workflow.md)、[设计守则](vega-design-guidelines.md)、[#397 错误展示](vega-compaction-error-presentation.md)、[#171 诊断](vega-issue-171-run-diagnostics-spec.md)。
- 已确认：v0.1.128 / v0.1.138 中 context_summary 记录 `summary_source_changed`，根 run 记录却为 `unknown_safe_failure`，用户同时看到具体压缩提示和通用运行环境提示。只读证据见 #413 评论；没有上传真实对话、思考、原始请求或截图。
- 根因链：`compaction.rs::summary_diagnostic_failure` 已有精确码；`agent/diagnostics.rs::run_failure` 仅识别少数 metrics.failure_kind，SourceChanged 等降成 Unknown；`RunFailureKind::from_runtime` 对这些 Context 变体落入 Runtime；历史恢复只看有限的 budget code 和 failure_kind；UI 的同原因去重仅覆盖 context/reasoning limit。
- 本卡修类型化分类、根诊断、实时/恢复一致性与同原因去重。保留 #397 位置/样式及恢复按钮，不改 #409 来源比较、不改压缩算法/预算/超时/重试/取消策略，不处理 #427 无 assistant 行的发送失败位置。
- 与 #409/#434 没有执行依赖：使用既有错误/状态类型，在当前基线即可独立实现和故障注入验收，不必等待另一卡制造或消除该错误。共享文件按单一写入者串行集成。

## 验收矩阵（先于实现计划，全部待实现 / 待 CI）

| ID | 前置、操作 | 必须观察的结果 / 验证层级 |
| --- | --- | --- |
| F1 | 对下表每个 Context 变体注入真实 runtime→conversation 错误链 | 保留对应根 failure_code；安全 RunFailureKind 与实时对话原因一致，不出现通用运行环境替代；生产事件/诊断 |
| F2 | 自动压缩失败记录与同原因 run Error 以两种顺序到达 | 所属回合底部只显示一份同类失败，无 Composer 额外重复；工具/过程收起后仍可见；GPUI 投影 |
| F3 | 同一回合另有独立 provider 授权/限流等真实错误：分别按 provider→compaction→generic 和 compaction→provider→generic 到达；另测其他回合的压缩失败 | 两种顺序都保留非压缩具体原因与独立压缩原因，不能按“有压缩错误”吞掉另一真实原因；保持不同回合、operation、generation 隔离；UI 正反例 |
| F4 | SourceChanged 后 token 取消或后到 Interrupted/笼统 Failed；相反顺序；用户真的主动取消 | 已知失败保留且耗时不重置；真正 Cancelled 仍中性停止，不造失败或改变运行恢复策略；runtime / conversation / GPUI |
| F5 | 新格式根诊断落库、关闭重开，再加载 context projection | 同一 failed owner 恢复同一原因，未借给相邻轮次；无须读取模型正文或敏感日志；存储/生产 hydration |
| F6 | 旧根 Unknown + 完整可信失败操作；再分别去掉 start/归属/failed status、插入后续成功请求、改 root/线程/代际或制造冲突 | 仅唯一、完整、最后有效失败证据允许补回；其余保持未知，不借旧失败解释新失败；存储表驱动反例 |
| F7 | 历史缺原因、损坏字段、无关敏感 payload、成功/取消的压缩与手工独立失败 | 无依据不猜原因；存储验证不放宽；成功/取消不复活提示；手动失败保留原位置；不泄露正文/思考/参数/凭据 |
| F8 | 压缩失败后同一对象更高 generation 成功，另有独立 run 错误 | 已解决的旧压缩提示消失；独立错误继续可见；不能让失败原因丢失或跨线程覆盖 |

进程内测试使用 MockProvider、临时 Store、已有事件管线和 GPUI TestAppContext，不新增真实外部进程/网络/应用 E2E。断言同时覆盖根诊断 code、message owner、上下文失败状态、渲染 notice 数量/文字与重开结果；不只测孤立字符串函数。

## 冻结的分类表

保留现有 `DiagnosticFailureCode` 的精确闭合码，不新增自由字符串、failure_kind/scope 值或 DDL。根诊断的 failure_code 是本卡新增恢复路径的权威字段；现有 failure_kind 仍承载其已有信息，不能单凭其 unknown 否定已知根 code。

| ContextRuntimeError | 根 DiagnosticFailureCode | 对话展示分类 |
| --- | --- | --- |
| SourceChanged | SummarySourceChanged | Compaction(SourceChanged) |
| SummaryTimedOut | SummaryTimeout | Compaction(TimedOut) |
| SummaryOutputTruncated | SummaryTruncated | Compaction(InvalidSummary) |
| SummaryEmpty | SummaryEmpty | Compaction(InvalidSummary) |
| InvalidSummary / SummaryFormatInvalid | SummaryFormatInvalid | Compaction(InvalidSummary) |
| SummaryProjectionInvalid / InvalidProjection / SystemMessageInResult | SummaryProjectionInvalid | Compaction(InvalidSummary) |
| NoCompactablePrefix | SummaryNoCompactablePrefix | Compaction(NoCompactablePrefix) |
| SourceTooLarge / AggregateTooLarge | SummarySourceTooLarge / SummaryAggregateTooLarge | Compaction(TooLarge) |
| ImagesUnsupported | SummaryImagesUnsupported | Compaction(ImagesUnsupported) |
| AlreadyAttempted | SummaryAlreadyAttempted | Compaction(Unavailable) |
| OverLimit / SummaryInputOverLimit / ResultOverLimit / Estimate(InputTooLarge) | ContextOverLimit | 既有 ContextOverLimit |

`MissingHook`、其他 Estimate 错误和未列明错误保持既有安全兜底，不从文本猜更细原因。Provider HTTP/传输/输出限制/额度/权限、ReasoningLimit、凭据与项目错误保留原类型和已有映射；Cancelled 不变成 Compaction 失败。UI 粗分类不覆盖诊断里空摘要/截断/格式错误等细码。

### 共享安全文案与去重

1. 在 `vega_conversation::types::RunFailureKind` 新增 `Compaction(ContextCompactionFailureCode)`；仅放在 conversation 共享类型，runtime 不依赖 UI 或 conversation。
2. 在 `types/context.rs` 为既有 ContextCompactionFailureCode 提供一个共享静态英文失败文案方法。将 `vega_ui/.../context_control.rs::status_label_for` 现有 Failed 分支的固定文案移入该方法，RunFailureKind::message 同样委托它；ReasoningLimit/OverLimit 保持已有分类/文案。保留当前 SourceChanged 的 `History changed. Retry compaction.` 等已批准文字，不增加原因计数、内部字段、常驻说明或新错误卡。
3. 在 `run_notice.rs` 加私有同原因谓词，统一用于 `compaction_reports_run_failure` 与 `run_notice_projection`：Compaction(code) 只与同一 owner 下的相同 code 匹配，原 OverLimit/ReasoningLimit 对应也保留。相同原因只呈现一份；不同原因仍按 #397 同底部区域显示。
4. `remember_run_failure`、active assistant.failure 和历史恢复统一使用以下优先级：具体原因优先于 Runtime/Persisted；已有具体的非 Compaction 原因不能被后到 Compaction 替换；新具体的非 Compaction 原因可以替换已记 Compaction，但对应压缩 status 记录仍保留并组成 `FailureWithCompaction`。同因只保留一份；二者都为 Compaction 时沿用同 operation/generation 的最新有效状态，不能跨对象借用。恢复也遵循同一规则，不能让直接 `insert` 或 active segment 赋值绕过它；后到 generic/Interrupted 不覆盖具体原因，成功结束的其他运行不被旧错误降级。无需把既有 run_failures 改成多错误账本。
5. 不增加自动重试或强制恢复：SourceChanged 仍拒绝本次 checkpoint，用户重试必须重建最新来源且再经全部围栏；不复用失败快照、不能强行安装。现有手动重试入口与新消息继续方式保留。

## 根诊断与历史恢复的具体修改

### 单一闭合诊断映射

将 `agent/compaction.rs::summary_diagnostic_failure` 的现有匹配逻辑移到 `agent/diagnostics.rs` 的 crate 内私有共享函数（例如 `failure_code(error) -> Option<DiagnosticFailureCode>`）。更新原三处摘要调用点及 `run_failure` 共同委托；Cancelled 返回 None，run_failure 的未知兜底保留。按分类表覆盖 Context 变体，不新增 runtime 诊断枚举或扩展 SQLite CHECK 白名单。保留原 provider/reasoning/usage 分类断言。

`types/events.rs::RunFailureKind::from_runtime` 对上表 Context 变体明确匹配；添加 crate 内 `from_compaction_diagnostic_code` 将既有存储细码折成对应对话分类。保留 from_diagnostic(metrics) 对 provider 等既有字段的处理，不能用失败文案或 raw payload 做分类。

### 新根与旧证据恢复

在 `crates/vega_store/src/run_diagnostics/failure_ownership.rs` 新增 `compaction_failure_message_ids`，并从 `run_diagnostics.rs` 的现有模块出口导出。返回 `(failed_message_id, DiagnosticFailureCode)`，沿用 `failed_assistant_owner` 的真实归属检查，不改公共数据表。

- 新记录：仅取本线程 `phase=run,state=failed` 且根 code 在上述 Summary* 闭合集中的记录；code 已明确，不依赖 metrics 是否已填写。ContextOverLimit/ReasoningLimit 继续走现有 budget / classified 路径。
- 旧记录：仅对根 code 为 UnknownSafeFailure 且 metrics 缺失或明确为通用 runtime/unknown 的记录尝试补回。取根终态之前同一 thread/run、parent_attempt_id 等于该 root attempt 的最后一条有效子终态（所有 primary_model/tool/context_summary 阶段共同排序，不能只筛失败）。它须为已知 Summary* 的 context_summary failed 操作，有同 identity 的 started 且顺序正确，并有同线程、同 operation_key 的 `context_compaction_status` failed 记录；其中 operation key 必须指向该 failed owner 的 `:context-generation-...` 操作，粗分类必须一致。
- 若最后子终态是成功、取消、其他失败，存在缺失/冲突/无法确定 owner，或同操作有更高代际成功解决，则不把旧失败借作根因。不得跨 thread/run/root/operation/generation 借证，也不按相近时间、正文关键词或截图推断。
- 复用现有 steer/pending 归属围栏。新起的独立 run、非 failed assistant、断链或不一致 generation 均不归因。查询只读，不补写/修复旧数据库。
- 在 `agent/compaction.rs::read_context_projection_with_provider` 的既有 failure 读取之后合并此结果：明确的根 Summary code 应恢复对应 Compaction 类型；旧证据仅升级 Runtime/Persisted 或缺失原因，不覆盖已知不同的 provider/凭据/预算原因。发生冲突保持既有具体原因，不编造“更确定”分类。

## 文件、顺序与允许范围

1. 先在 `types/events.rs` 测试模块、`agent/tests/failure_paths.rs` / `compaction_timeouts.rs`、`agent/diagnostics.rs` 测试和 store `failure_ownership.rs` 测试补 F1/F4–F7，使用业务测试名 `compaction_failure_...`；把现有 SourceChanged==Runtime 断言按本规格改为准确新分类，不削弱负例。
2. 实现共享 diagnostic code 映射及新的 Compaction 类型；重接终态映射和精确 code 的生产持久化断言。`entry.rs` 的 root 调用/取消策略按原样保留，除非为调用新函数作最小机械修改。
3. 实现上述 store 只读查询与 production hydration，增加“新记录即使 metrics unknown 也可恢复”以及旧证据拒绝矩阵。
4. 在 `vega_ui/src/conversation_stream/{context_control,run_notice,content}.rs` 和对应 `tests/context_control.rs` 补同因去重/乱序/恢复回归；只改共享文案委托、失败优先级与等价判断，不改布局/颜色/计时/滚动/Composer 发送行为。
5. 允许修改：上述文件、必要的 `types/context.rs` / store re-export、当前规格和 #397 文档的分类入口链接。禁止数据迁移、新依赖、用户配置/运行数据库、全局重试、来源比较或无关工具投影改动。

## 验证、手测与回滚

本地只运行快速 `git diff --check` 和格式/差异静态检查，不编译、不执行测试。以下命令仅供后续 Actions：

```sh
cargo nextest run -p vega_store -E 'test(compaction_failure_) | test(failure_ownership)'
cargo nextest run -p vega_conversation -E 'test(compaction_failure_) | test(run_failure_tests) | test(issue397_)'
cargo nextest run -p vega_ui -E 'test(compaction_failure_) | test(context_control)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

required gate 沿用现有 `pr-check.yml` 的 `check (fmt, clippy, test)`，包含现有 Python 检查；不新增工作流、真实 E2E、ignore、自动重试或放宽保留断言。当前没有实现、PR、CI 或手测通过证据。

用户手测（合并发布后）：重开一个已有可信压缩失败记录的 native 对话，底部显示具体压缩原因且不叠加通用运行环境提示；其他回合的失败不被带入。新的正常压缩成功/取消不留下错误；继续消息后若发生新的真实失败，原因仍属于正确回合。不会要求制造历史并发修改或手改 DB；真实 SourceChanged/超时尚未发生时如实记该现场手测未覆盖，进程内故障注入由 CI 验证。

回滚：revert 本卡实现即可回到旧分类。无 DDL 或数据补写，原始失败诊断仍保留；不删除记录以消除提示。

## 交接

- 固定版本 spec 提交并推送后，在 #413 发布链接、基线、任务分支/commit、静态检查与未验证项，才置 Ready。当前执行者待领取，下一执行者 Vega。
- Vega 复用 `~/Workspace/worktrees/vega-413-compaction-failure-classification`，领取前 fetch、保护修改并更新到当时 master；若共享合同漂移先交回 Codex。#409 不阻塞此卡，不并行覆盖共享文件。
- Vega 完成或遇阻在 #413 回报，保持 In progress、下一执行者 Codex、当前待领取并停止写入，不开 PR/合并/关闭。回报精确 commit、文件、静态退出码、待 CI 矩阵、限制、耗时和规格偏离；缺依赖或与现有错误/归属保护冲突按执行总纲 §6 上报，不放宽断言取得通过。
