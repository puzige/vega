# #918 验证记录

## 版本与工具链

基线 `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a`，任务分支 `feat/918-preparation-concurrency`。开发验证版本为本文件同提交的源码；后续清理版须重验。macOS aarch64；cargo 1.101.0 (e611c7a87 2026-11-04), huacnlee/cargo 0.102.0-9；rustc 1.98.0 (88d9e12ae 2026-08-18)。`CARGO_FORK_AUTO_UPDATE=0`，`--locked --offline -j 2`，不修改机器配置。安装的本地 nextest 0.9.151 未用作测试执行；CI 固定 0.9.146 不变。

## 实际本地结果

- 基线 `cargo check -p vega_conversation --locked --offline -j 2` 通过。
- 原实现＋本轮开发暂停钩子：`cargo test -p vega_conversation slow_foreign_history_preparation_allows_other_persistence_before_resume ... -- --nocapture` 失败。慢准备暂停时，其他线程 3 秒内未能提交。修后保持相同断言通过。
- 当前 `cargo test -p vega_conversation concurrency_tests --locked --offline -j 2 -- --nocapture`：12 发现、12 执行、12 通过、0 失败/忽略，13.29 秒。
- `cargo clippy -p vega_conversation -p vega_store --all-targets --locked --offline -j 2 -- -D warnings`：通过，2.87 秒。manifest 的既有 unused workspace tracing-subscriber 提示未修改。
- `cargo fmt --all` 和 `git diff --check`：通过；最终树另执行 fmt check。
- 开发过程编译/夹具失败：工具类型/运行事件字段/批准审计/final 响应及嵌套事务接线已按真实 API 修正；新增恢复循环 checkpoint 作用域与 SQLite FromSql i64 编译错误已修正。未放宽安全断言。

12 项行为覆盖：真实持久化 actor 的父任务及两个子任务在准备恢复前提交快照、工具提案、批准、运行与成功终态；长全库工具准备计时；权限撤销拒绝且无 assistant 部分安装；取消；任务输入变化拒绝；普通历史版本变化重算且 assistant 只一条；followup＋恢复 checkpoint；准备完成后新增跨线程重复工具 ID 拒绝且不覆盖；BEGIN 短暂 busy 恢复且只成功开启一次；引擎 busy / FIFO 等待可取消；预算耗尽无部分写；非暂态嵌套事务错误仅尝试一次。

## 同规模性能（合成数据，不是用户现场）

180000 条其他线程工具记录，每条约 2 KiB 合成 output；6 个样本，当前线程少历史。基线完整准备 ms：986.666,530.242,589.354,437.769,445.002,461.863；writer 持有 ms：390,239,253,223,231,241。当前完整准备 ms：1268.870,430.090,421.242,420.166,421.714,424.842；writer：4,1,1,1,1,1。首样本冷态不剔除，不能宣称整体准备加速；关键改善是全库读取不占 writer。

另执行 `VEGA_PREPARATION_LONG_HISTORY=1 ... cargo test -p vega_conversation full_preparation_with_large_foreign_history_reports_writer_hold ...`，180000 工具＋本线程1000条约1.4 KiB历史。1 发现/执行/通过，9 filtered，11.99 秒。6 个完整准备 ms：1329.132,630.707,656.294,658.561,659.203,662.891；writer 均1ms。此长历史测量早于最后 busy_timeout 参数类型修正，生产控制流相同，仍须在冻结版复跑。观测精度1ms，样本量小，不推算真实应用 p95。

## 边界与待完成

暂态重试仅发生在 BEGIN 成功之前，提交回执未知不自动重放；现有事件持久化/用量/工具幂等路径未改写。最终权限、来源、Skill 与凭据校验保留。不宣称已覆盖真实外部模型/工具执行、安装二进制内容或所有主运行显式 Skill 的性能；真实系统集成由用户在合并后手测。

待独立审查、Rust 开发源码固定可访问 SHA 后清理、最终 head 本地验证与 required CI。尚未合并或用户验收；#919 未实施。
