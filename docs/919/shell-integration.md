# Shell 集成验收（#919 / #921）

## 范围与基线

共享工作树当前合并中的 Shell 集成，仅修改 conversation 的 `agent/shell_operations.rs`、`shell_recovery.rs`、本文及必要开发测试子模块。Git/index、提交、fetch、迁移注册、store 和 shared writer 由主线负责；Cargo 验证统一由主线运行。本执行者不运行 Cargo，不覆盖合并任一整侧。

## 实现前验收

| 场景 | 必须保留的可观察结果与边界 |
|---|---|
| 在线资源身份登记、运行状态与完整终态 | 全部经 sealed named database writer；登记仍核对 operation/thread/run/generation，不向 actor 传入任意 callback。 |
| #921 Full Access concurrency | 不可变 `concurrent` 从 runtime record 贯穿 durable record 和 pending reconciliation；资源占用与写范围互斥分离，任务局部清理不清除其他沙箱 blocker。 |
| facts 与重复终态/回执未知 | 保留 command result、exit signal、cleanup state、output incomplete、resources closed、process resolved、cleanup reasons；沿原 operation 和完整 facts 精确 verify，不重跑命令，不伪造成功回执。 |
| 并发任务取消后观察 | runtime 仍按 thread/workspace/operation 核对可信 owner；新 cleanup observation 也经现有 sealed `CleanupObservation`，不得保留直接在线 SQL 写旁路。 |
| 重启与历史资源 | `RecoverGeneration` 留在 writer 内；只观察旧资源身份，不复获旧 PID 控制；Full Access 不产生 workspace blocker。 |
| 恢复写入 | `ResumeWrites` 保留 authority/reset、线程修订/模式、draft 来源、project path、expected recovery version、operation 集和 decision id 的提交端核验；外部不持权限锁等待 actor。 |
| 文件 pending commit | 读取 pending journal、runtime 清理/释放、确认文件记录与 ReadState 失效留 actor 外；只有准备好的 record/facts 进入 actor。精确 durable verify 完成后才确认 pending，失败保留可恢复状态。 |
| 已自动合并 facts | 核对 actor 内已有完整 facts，只补遗漏的 concurrency，不重复定义/提交 facts。 |

## 最小方案与验证

1. 在线 hook 冲突保留 writer 接线，在其 `ShellRecord` 内补上 `concurrent`。
2. reconciliation 冲突保留文件操作在外和具名 SQL 命令在内，在恢复 `ShellRecord` 补上 `concurrent`。
3. 将上游新增的 concurrent cleanup observation 直接写入替换为既有 `CleanupObservation` 命令；不改变控制、owner、fence 和 recovery version 语义。
4. 仅执行授权文件 rustfmt、差异/冲突标记静态检查；必要开发行为测试交主线统一运行。编译、Clippy、行为回归、独立审查和最终 head CI 未取得前均不得称通过。

## 结果

实现已完成并停写，尚未编译、运行测试、独立审查或取得最终 CI；本段不是行为通过或合并声明。

### 双侧契约保留证据

- 只读 `git show HEAD:<file>` / `git show MERGE_HEAD:<file>` 核对两侧，未选一侧覆盖。静态逐段比较通过：在线文件相对 HEAD 仅新增 `concurrent: record.concurrent`；恢复文件 `require_same` 至 `ShellRecord` 前全部内容与 HEAD 相同，保留 workspace/version、权限与 draft 来源、project path、decision/operation 集、事务与身份校验。
- 恢复文件上游 `ShellCommandProjection` 及 active/projection/cancel 分支逐段比较通过；该分支仅将新 observation 的直接 SQL 写入替换为 sealed `CleanupObservation`。上游 thread/workspace/operation 的可信 owner 检查、active 中排除 concurrent、未知清理局部重试和释放逻辑均保留。
- 两个 `ShellRecord` 的 facts 已自动合并且各只构造一次，只补 `concurrent`；原 `record_with_facts` / `verify_record` 完整内容精确核对保留。
- 两个生产文件所有写 helper 调用均落在 sealed `DatabaseCommand::apply` 内：`ShellResource`、两个 `ShellRecord`、`CleanupObservation`、`RecoverGeneration`、`ResumeWrites`。没有新增 actor callback。
- `pending_shell_commits`、`confirm_shell_commit`、ReadState 失效、runtime 资源观察/清理/释放仍在 actor 外。当前 runtime pending helper 读取 owner 的内存 pending record；本次没有添加或迁移任何文件 journal I/O，更没有把文件待提交记录当成 durable SQL receipt。

### 静态检查（本执行者已运行）

- `rustfmt --version`：`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。
- `rustfmt --edition 2024 --config skip_children=true --check`（两个授权生产文件及开发测试文件）：退出 0；格式化只作用于这三个文件。
- `git diff --check -- crates/vega_conversation/src/agent/shell_operations.rs crates/vega_conversation/src/shell_recovery.rs`：退出 0。
- 两个授权生产文件冲突标记检查：无残留；逐段比较双侧保留契约的 Python 静态断言：退出 0。这不是行为测试。
- 未运行任何 Cargo 命令、fetch、commit 或 index 写操作；index 的 `UU` 状态留主线 `git add` 解决，不表示文件仍有冲突标记。

### 开发行为测试（未运行，交中央）

新增 `crates/vega_conversation/src/shell_recovery/tests.rs`，只使用临时数据库、实际 named writer 和真实 store 行为，无外部 Shell/进程执行：

1. `concurrent_terminal_replay_preserves_all_facts_and_rejects_changed_content`
2. `concurrent_cleanup_observation_preserves_sandbox_blocker_and_recovery_version`
3. `generation_recovery_expires_concurrent_control_without_workspace_blocker`

中央建议命令：`cargo test -p vega_conversation shell_recovery::tests:: -- --nocapture`。编译、发现/执行/通过计数均未取得；需中央在整合迁移注册后执行并保存实际工具链、版本与结果。在线 hook 的资源所有权和恢复确认失效等扩大回归仍由主线按风险统筹，本组不声称覆盖真实运行时取消/owner 竞争或丢失回执故障注入。开发测试与 `#[cfg(test)] mod tests` 接线须在合并 master 前按仓库政策留痕并清理；不能把本组未运行记为通过。

### 停写 SHA-256

| 文件（conversation/src 下） | SHA-256 |
|---|---|
| `agent/shell_operations.rs` | `4b2d777e017a6013156739fdde446d1068968a0798e6d2d391c4bc322a07e182` |
| `shell_recovery.rs` | `87a836b399491a8a4130fb724552ce436d26cce7080be0259305ed456b603d1f` |
| `shell_recovery/tests.rs` | `c9eebb776331d66b03f6d1efcadbddaff55e3def91110bd0171f31eb053cf2f0` |

未决：中央编译/行为回归、非实现者独立审查、开发测试清理及最终 head required CI；Git/index、迁移编号和其他负责人的集成仍由主线协调。规格偏离：无。
