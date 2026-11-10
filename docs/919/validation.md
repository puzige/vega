# #919 开发检查点（未完成，不可合并）

## 内容与验证身份

基线 `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`。当前检查点包含开发 Rust 测试；不得进 master。任务分支 `feat/919-database-writer`，worktree `~/Workspace/worktrees/vega-919-database-writer`。

实际工具链：`cargo 1.101.0 (e611c7a87 2026-11-04) (huacnlee/cargo 0.102.0-9)`；`rustc 1.98.0 (88d9e12ae 2026-08-18)`。所有 Cargo 命令设置 `CARGO_FORK_AUTO_UPDATE=0`、`--locked --offline -j 2`，未改机器配置或 CI。

## 当前实现

运行 persistence 的具名 Snapshot/Event/Diagnostic 共享数据库身份注册的写线程，各运行保留有界发送端和轮转调度，事件唤醒无定时轮询。关闭先断开发送端并唤醒，再等待 drain；调用方丢弃回执不丢已接收命令。快照/用量带操作 ID，回执同事务保存；其他事件暂有 pending 状态防盲重放，**并未满足完整原子回执与恢复验收**。工具提案/批准/运行增加权限 revision 校验，但尚须与最终写入原子化。自动化 missing_usage 回调已移出共享线程。

## 实际执行

- `cargo check -p vega_conversation --locked --offline -j 2`：通过（阶段性编译）。
- `cargo test -p vega_conversation --lib --locked --offline -j 2 -- --nocapture`：检查点 **19 发现、19 执行、19 通过**，执行 16.19 秒；编译日志 `/tmp/vega-919-checkpoint-tests.log`。这些临时路径非永久证据，原测试源码保留在本检查点提交中。
- `cargo clippy -p vega_conversation -p vega_store --all-targets --locked --offline -j 2 -- -D warnings`：通过，2.07 秒；日志 `/tmp/vega-919-checkpoint-clippy.log`。现有 manifest `unused workspace dependency tracing-subscriber` warning 未修改。
- `cargo fmt --all`、`git diff --check`：通过；未运行最终无测试版 fmt/CI。

19 项含第一阶段本任务相关回归及共享线程新回归：慢准备暂停时父任务及两子任务完整终态先提交；followup/recovery；权限撤销；快照冲突重算；取消与预算；晚到工具 ID；大历史计时；同库别名共享/不同库隔离；回执丢失仍提交及 drain；单运行关闭不影响其他运行；重复用量操作只一行；pending 操作不重放；权限 revision 变化拒绝旧提案；满队列取消不入队，另一库可提交。

## 失败与责任层

- 新权限末检使旧并发夹具失败：夹具仅插消息、无 run fence；补生产权限前提后原隔离断言通过，未删除校验。
- 首版满队列回归超时：消费者已取走一条，队列仍余一格，测试写入已经入队。改为实际确认队列 Full，取消保持 200ms 断言，跨库提交保持 200ms；通过。不是放宽时限。
- 临时接口/编译与 Clippy 错误已修；最终此检查点的上述验证已重跑。

## 未决与下一步

完整清单及三个结构调整见 `database-writer.md`。19 项通过**不证明**全库入口迁移、完整事件原子性、提交未知恢复元数据、全局容量/公平性、独立侧录外移、app shutdown 或权限原子复核完成。

上述为旧检查点的状态。用户随后明确选择一次性全库迁移，任务已继续；主 Agent 与具名互不重叠的范围协作者持续实现，不做 native-only 部分合并。尚未最终实现独立审查、云端 required CI、合并、正式打包、安装或用户第二阶段手测。独立子 Agent 的入口盘点不是实现审查；不关闭 #919，不标 In review/Done。

## 后续集成阶段：后台关闭与准入

- 当前基础 HEAD `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b` 加未提交全库迁移差异；协作者仍在写 Codex、隐私、来源校验与 fork，以下不是最终冻结身份。
- `cargo check -p vega --all-targets --locked --offline -j 2`（`CARGO_FORK_AUTO_UPDATE=0`）：成功；11.67 秒，剩余两处迁移后的 dead-code warning，非最终 Clippy 证据。此前的 Request move 错误已在责任层修复；Codex 私有方法、fork 模块缺失均曾阻断中间态编译，不列作通过。
- `cargo test -p vega_conversation --lib --locked --offline -j 2`：30 发现/30 执行/30 通过，执行18.74秒；日志 `/tmp/vega-919-tests-current.log`。含慢准备时父任务与两个子任务完整终态提交、receipt 原子性/恢复、权限、跨库、后台池容量/失败/排空、同库最后生产者释放后重开仍同 actor、关闭等待运行和必要租约且丢回执仍提交。
- `cargo test -p vega_store persistence_failures::tests --lib --locked --offline -j 2`：2 发现/2 执行/2 通过，执行0.37秒；日志 `/tmp/vega-919-sidecar-current.log`。覆盖文件锁不阻塞数据库、物理库替换拒绝、关闭超时不冒充落盘/后续排空/关闭拒绝。
- 上述之后补充服务准入排队取消、排队预算、开始后取消仍等真实结果，以及必要终态失败不冒充关闭成功。首次专项编译被并行 fork 测试文件尚未创建阻断，尚无这些新增测试的通过证据。全库新版本仍需集中复验和最终门禁。
- 本阶段最初两次中央命令沿用了本机默认选项（未显式 locked/offline/auto-update 环境），未安装或修改机器配置；随后恢复前述固定命令约束。实际 rustc/Cargo 版本与本文件前文相同。

