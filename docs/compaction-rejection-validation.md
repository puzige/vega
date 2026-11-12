# 隔离压缩诊断验证记录

## 版本与范围

- 工作树：`~/Workspace/worktrees/vega-compaction-diagnosis`，分支 `feat/compaction-isolated-diagnosis`。
- 基线：v0.1.416 / `24f1d9397215db9e3258be0a679192be183349c0`；本轮 fetch 后 origin/master 相同，fast-forward 返回 Already up to date。
- 实现/测试未提交，精确内容由本机 `.diagnosis/review-manifest.json` 固定，SHA256 `a50ddeaba5e570bc6c34453c0ee32612e33a99c3082850c11a6013b9298bac0a`。清单 13 个文件含规格、上一轮临时校验源码；本验证记录在审查后新增，不改变审查版本。
- 初始阶段仅授权本地隔离实现与回归；后续授权整理提交、创建 PR 并完成最终 CI。本轮停在 PR，不合并或发布。未访问或修改远端应用与数据，未升级、重启、重试任务、调用模型或重放历史工具。私有数据、现场校验源码与原始日志不上传。

## 实现

新增固定 compaction_rejection 分类：所有权、源围栏、历史投影/当前 owned 组/整体消息/工具分组/调用结构/文本偏移/工具结果、微压缩准备/提交围栏、共享写入器准入和阻塞任务 join。

只保存固定枚举；不新增正文、工具参数/输出、路径、源指纹、凭据、原始错误。使用配对 Started/Failed 的既有 ContextSummary 诊断；原 SourceChanged、Cancelled、Store 返回保持不变。新增 0067 递增迁移和 API/SQLite 双层白名单，保留旧 runtime 字段约束，不新增 UI 文案。

## 实际本地验证

工具链：Cargo 1.101.0（e611c7a87 2026-11-04，huacnlee/cargo 0.102.0-9，执行设置 CARGO_FORK_AUTO_UPDATE=0）；rustc 1.98.0（88d9e12ae 2026-08-18）。

- `cargo test -p vega_conversation rejection_tests --offline -- --nocapture`：10 发现 / 10 执行 / 10 通过 / 0 忽略；3 项上一轮私有现场校验被过滤，未在本轮重跑。最终测试执行耗时 5.60 秒（编译 6.41 秒）。
- `cargo fmt --all -- --check`：通过。
- `git diff --check`：通过。
- `cargo clippy -p vega_conversation -p vega_store --all-targets --offline -- -D warnings`：通过，最终耗时 2.46 秒；manifest 既有 unused workspace dependency tracing-subscriber 提示未改。

回归涵盖规范历史/真实微压缩成功、历史结果/调用参数/偏移/分组/用户消息差异、活动 owned 工具差异、同 seq 指纹变化/未来源版本、准备前与准备后真实 SQLite 变化、真实 shared writer 数据库身份拒绝、错误线程 admission、取消保持取消、任意诊断文本 API 与数据库拒绝。冲突后检查未发生微压缩；诊断实际通过 writer 落库读回，检查无不完整 attempt、无正文/凭据 canary 或路径。Provider 若被调用立即 panic。

最终日志本机 SHA256：
- `.diagnosis/rejection-final.log`：`ef0c8be0ca091f437121e0535d2adfccd6a7265c5eb2b5817e7a00ad99e97fcb`
- `.diagnosis/clippy-final.log`：`3fa11f66d2f8ba84819360b5750a54cefaf9feea21954f65ab5c836e478c4f56`

## 失败与修正

原始日志保留在本机，不以重试代替修正：
1. 新参数漏接预计算调用、DiagnosticMetrics 显式初始化漏字段：修编译接线。
2. 测试误假定 vega_store 重导出 rusqlite：改用已有 tuple 参数 API，不加依赖。
3. 新诊断漏 Started：落库拒绝 attempt_id，修生产诊断生命周期；保留原冲突断言。
4. 旧 runtime 字段有 SQLite CHECK：真实落库失败后改为专用枚举字段和递增迁移，未放宽旧约束。

## 独立审查与限制

非实现者只读审查 execution `01M74VR72BF5KH4B24RC4N5VX8` / agent `01M74VR72BHKVN0H2GVV7BCWYB`，绑定上述 manifest，13/13 哈希一致，结论无阻塞。审查是静态检查，没有重跑测试或读取私有会话副本。

未复现历史 185 毫秒失败，缺少当时内存请求/所有权状态；不宣称根因已修复。未运行完整 Agent、真实模型/工具、native UI 或远端验收。初始隔离阶段未运行 CI；后续 PR 最终门禁另行记录。Rust 开发测试在可恢复的开发提交留痕，最终 PR 文件树按仓库规范清理后才进入最终 CI。诊断缓冲/持久化不可用仍受既有生命周期限制，不保证每次拒绝必然可持久化。

## PR 整理阶段

fetch 后 origin/master 仍为上述完整基线，fast-forward 无新增提交。生产实现与独立审查版本一致；移除私有 isolated_diagnosis 模块接线，将其源码留存本机私有目录，不进入 Git。公开测试仅使用合成 SQLite 数据。

公开开发版本重新执行：10 发现 / 10 执行 / 10 通过 / 0 忽略 / 0 过滤，执行 4.24 秒；fmt、diff check、受影响双 crate 全目标严格 Clippy 通过。三组 Python 测试分别 5/5、15/15、40/40。工具链与前文相同。日志保留本机，不将私有数据上传。

下一步固定开发提交与公开可恢复源码，移除 rejection_tests 及独占模块接线；清理差异独立复核、最终 head 本地静态/Clippy/Nextest 空集验证与 required CI 尚待完成。原始故障未复现，不宣称修复。
