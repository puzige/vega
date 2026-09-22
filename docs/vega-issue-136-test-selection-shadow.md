# Issue #136 — 慢测试优化与影响范围影子验证

> 2026-09-22 [Issue #140](vega-issue-140-ci-test-throughput.md) supersedes the shadow selector and per-shard compilation topology: Python shadow reporting is removed; full-suite native nextest archives are built once and reused across workers. Existing safety assertions, resource weights and ignored inventory remain.

## 用户决策与边界（2026-09-22）

用户批准按默认全测、已审查慢测试按影响选择的方向继续优化。第一阶段只生成选择报告，PR 实际仍执行所有既有非 ignored 测试；不因报告而放行失败。正式选择执行须经后续影子证据审查，届时必须具备每晚全量和发布提交全量验证。当前全量 PR 门禁已经覆盖这些测试，本阶段不另增加重复定时任务。

本规格是 Issue #123 C9 的增量：保留四分片、资源权重、零重试、required check 名称和 fmt/clippy/doc-tests；允许专用 CI 影子分析脚本（不恢复本地 hooks/verify/调度器），不改变生产 API、timeout 或安全语义。无产品 UI 修改，原生截图与应用安装不适用。

## 验收矩阵（实现前冻结）

| ID | 风险/操作 | 预期结果 | 证据 |
|---|---|---|---|
| A1 | 已知 crate 改动 | 从 Cargo metadata 计算受影响 crate 与反向传递依赖；详细说明每个慢测试组运行/候选跳过理由 | 选择器针对性测试与报告 |
| A2 | 修改配置、清单、锁文件、工具链、CI、选择规则，未知路径或 diff 读取失败 | 回退全量并说明原因；不把错误当空 diff | 故障案例 |
| A3 | 慢测试改名、匹配为空、坏配置或清单不可用 | 回退全量；新测试未列入可跳过集合时默认运行 | 清单案例 |
| A4 | 删除/重命名路径 | 分析旧路径和新路径；不能漏掉原归属 | Git fixture 案例 |
| A5 | 每次 PR | 全量四分片仍原样执行；有可下载/查看的影子 JSON 与摘要，包含 base/head、路径、规则、选择统计，实际运行范围明确 full | 云端日志/summary |
| A6 | 拆分原 commit_proof 11 场景 | 11 个可独立发现/分片测试，原每场景所有断言完整，真实 owned repo/一次提交/重复调用拒绝均保留 | 逐场景对照、nextest list、定向运行 |
| A7 | 自动标题纯超时测试 | 若虚拟时钟可通过既有 Tokio test-support 实现，则走同一生产 collect 入口，仍验证生产 15 秒 deadline、fallback、无 retry；不改变生产 timeout。无法可靠证明则保留原测试并记录原因 | 定向生产入口测试与时长 |
| A8 | 集成 | fmt/clippy/doc-tests 与完整云端 tests 成功；不增加 ignore/retry，不削弱安全断言 | PR CI |

## 实现计划

1. 在版本化配置中仅登记已审查的 Git 故障矩阵和自动标题超时测试，记录保护行为、关联 crate/额外触发路径和精确测试标识。其余测试全部默认运行；保留关键代表测试无条件运行。
2. 从真实 Cargo metadata 构建依赖图，并使用 nextest 测试清单确认规则匹配有效。跨 crate 隐式业务依赖由显式触发项补足；第一版宁可过选。不能仅以慢测试所属 crate 是否被修改判断。例如 Git 提交入口也触发 Git 安全组。
3. 专用 CI 脚本只做分析与报告，不执行测试、不提供跳过门禁的输出。使用标准库/现有依赖，不引入第三方运行时依赖。diff 使用可信 base/head 参数及 NUL 安全路径处理，不将路径拼入 shell。报告文本处理 Markdown/特殊文件名，限制公开信息为仓库相对路径和测试标识。
4. CI 先进行影子分析再保留原全量 nextest 命令；避免新增 macOS job 占用测试并发。报告阶段失败不得阻止实际全量测试（失败明示）；聚合门禁不接受跳过的实际测试。完整 CI 验证仍覆盖选择器自身测试。
5. 提取只在测试代码中的共用断言 helper，把 commit_proof 原 11-plan 循环拆成独立命名测试，不复用可变仓库状态，不降低 threads-required。
6. 检查自动标题定时器是否可使用虚拟时钟；允许仅在现有 Tokio dev-dependency 启用 test-util feature（不新增 crate），保留原断言并精确验证虚拟经过时间为生产 15 秒。真实 Git/PTY/process 超时不套用虚拟时钟。
7. 主代理审查场景/断言对应、回退安全、报告真实性与 CI；专职子代理负责实现。成功后 squash 合并、清理本任务 worktree、回写 Issue。

## 衡量与后续启用

记录本次完整 CI 中拆分测试的最长单项/累计耗时、自动标题耗时、分片 runtime、job 时长和排队时长。不能把测试累加耗时当流水线墙钟时间；影子阶段不宣称省略测试收益。正式启用前需跨不同模块的真实 PR 影子记录，核对候选跳过集合与全量失败交集；无漏选观察不等于数学完备。将来发生漏选立即恢复对应组必跑，再修规则。
