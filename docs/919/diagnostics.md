# 手动压缩 buffered diagnostics

## 本轮范围与验收（实现前）

只迁移 `agent/diagnostics.rs` 的 buffered flush 及 `agent/compaction.rs` 手动压缩调用；不修改 precompute、共享 writer、privacy、pipeline、pending、Git/index 或 Cargo。一次性全库迁移要求不变，本页不是 native-only 或永久在线豁免。

1. buffered 事件在锁内一次性 take 为 owned batch，立即释放 mutex；注册、排队、等待 actor 回执均在锁外，阻塞等待移到 blocking worker。
2. 具名 sealed `RecordDiagnostics` 只携带 owned admission、thread/run 标识与事件，不携带 Store、连接、guard 或任意 closure。复用 precompute 同名命令的提交契约：校验 database identity、thread admission 和每条事件所属 thread/run；单个短 Diagnostics 事务按 FIFO 顺序写入，全部成功后才报告成功。
3. SQL、writer/回执、buffer 或 blocking worker 失败返回真实错误，不忽略 INSERT 失败、不将已接受或提交结果未知表述为 persisted；不自动重试不确定提交。
4. 手动压缩开始记录后，正常成功、取消、生成失败及 hook 配置失败都经过终态记录及尽力 flush。flush 不沿用已取消的生成 token。
5. 压缩成功而 flush 失败：返回可报告的诊断持久化失败并保留已发生用量；已安装 checkpoint 不回滚，不声称压缩安装没有发生。压缩本来失败/取消且 flush 也失败：原始错误、取消分类及用量优先保留，次要 flush 错误明确记录，不用次要错误掩盖原始错误。
6. 开发回归覆盖真实临时 SQLite 库的成功/顺序、批次中途 SQL 失败与整批回滚、actor 阻塞时 buffer mutex 可用且另一数据库可提交，以及返回错误优先级。不得接触用户库，不新增依赖。

## 验证与交付边界

本轮测试由主 Agent 中央运行；实现者仅做局部格式及静态核对，不将未运行测试记为通过。Rust 开发测试及独占支持合并前按仓库规则留痕后清理。最终独审另行绑定停写版本。

## 实现与停写交接

- 读取基线及交付时 HEAD 均为 `7c3ee7c97db83fbec4ce099313661f9cb7325cb3`，MERGE_HEAD 为 `2a29adf9b2ea78eaf59d591838531b45adc7373e`；工作树仍由主线并行集成，本页 hash 仅绑定本次授权产物，不冻结其他文件。
- `diagnostics.rs` 新增通用 sealed `RecordDiagnostics`，未修改 precompute 的专属同名命令。buffer 的 VecDeque 在 mutex 外转为 Vec；actor 仅负责归属校验及短事务 INSERT。无 Store/closure/guard payload，无文件解析、全历史准备或文件侧录。
- 手动 hook 配置纳入 async 结果块，开始记录后不再因配置 `?` 跳过终态 flush。flush 在 blocking worker 提交，不沿用生成取消 token；真实 SQL 错误保留为 `VegaError::Store`，其他错误保留来源为 `VegaError::Io`。已有失败/取消时原 failure 对象原样返回，次要 flush 错误用 tracing 明确报告；原本成功时转为诊断失败并保留全部 usages/usage_complete。
- 已运行：`rustfmt --version` 为 `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`；四个授权 Rust 文件 `rustfmt --edition 2024 --config skip_children=true` 及同参数 `--check` 均退出 0。Python 只读检查冲突标记、行尾空白、专属测试无注释/ignore，退出 0。这些均不是类型检查或行为通过。
- NOT RUN：Cargo/rustc 编译、Cargo 版本探测、check、Clippy、全部测试、CI、独立审查及用户手测。按本轮授权由主 Agent 统一测试；未执行 Git 命令、未修改 index/Cargo/用户库，未新增依赖。

### 中央运行的开发测试

`agent::diagnostics::tests`：

- `buffered_diagnostics_commit_in_recorded_order`
- `buffered_diagnostics_sql_failure_is_reported_and_rolls_back_entire_batch`
- `buffered_diagnostics_reject_cross_run_batch_without_writes`
- `buffered_diagnostics_flush_cancelled_and_failed_terminals`
- `buffered_diagnostics_wait_releases_mutex_and_other_database_can_commit`

`agent::compaction::diagnostics_tests`：

- `manual_compaction_reports_flush_failure_after_success_without_losing_usage`
- `manual_compaction_preserves_original_failure_and_cancellation_when_flush_fails`
- `manual_compaction_preserves_success_after_diagnostics_acknowledgement`

测试文件分别为 `agent/diagnostics_tests.rs`、`agent/compaction_diagnostics_tests.rs`，仅开发接线。并发用专属 gate 命令确定 actor 已暂停，确认 flush 不能绕过 actor、mutex 可取、迟到 record 不阻塞、另一临时数据库可提交，再释放 gate 核对最终顺序；无用户库或真实 provider。hook 配置失败汇聚目前仅静态核对，中央集成/现有手动压缩测试仍需补足调用链验证，不能将本组 helper 回归冒充完整 provider 集成。

### 停写内容 SHA-256

以下路径相对 `crates/vega_conversation/src/agent/`：

| 文件 | SHA-256 |
|---|---|
| `diagnostics.rs` | `e72848eeae12f62cb68d36f7e4b975406bf1d162445af7803d801d05ff447d47` |
| `compaction.rs` | `1f9a3479b4ae7a65c16112f7fcf7ba238ebadc31180661aa5fa3bda3aefa7888` |
| `diagnostics_tests.rs` | `adecd7f20585c815a35794be2807b023dccd56bd64305d4e377bb347dbf3a138` |
| `compaction_diagnostics_tests.rs` | `d442e8952bc091862cc5c2dcca5e1e9a28d151763f391476ddb7618b53a56708` |
