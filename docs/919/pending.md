# Pending / steering 准备与提交分界（#919）

## 基线与范围

共享树 HEAD `7c3ee7c97db83fbec4ce099313661f9cb7325cb3`，合并 `2a29adf9b2ea78eaf59d591838531b45adc7373e` 中。本轮仅修改 conversation 的 `pending_messages.rs`、专属开发测试及本文；不改 permission contracts、pipeline、threads、shared writer、Git/index 或 Cargo。由主线统一执行 Cargo，本执行者只做 rustfmt / 静态检查，不 fetch / commit。

## 验收（实现前记录）

1. 外部准备仍核对目录存在和输入来源：`validate_authority` / `prepare_dispatch` / steering `take` 与 `acknowledge` 保留 filesystem-aware `require_authority`；缺失目录拒绝，不生成新的可信 pending authority 或提交 claim。
2. actor 内 `verify_dispatch`、`verify_steering_acceptance` 使用 SQL-only authority：保留数据库身份、thread/id、完整 pending source 等值检查、有效 fence、native/active、registered workspace 身份、权限 epoch/revision/exact rules 及 admission/reset 判断。不得调用 `capture_fence` / `existing_task_workspace_root`。
3. `PendingWrite::apply` 仅 SQL / identity / admission / cancellation / generation / CAS；Enqueue 保留队列快照 CAS 与容量限制，Claim 保留 source row、generation、seq、skill consent generation 和 claim CAS。准备输入解析/图片/引用/skill 验证不移入 actor。
4. 权限锁 → writer transaction 顺序不变；外部 `acknowledge` 在等待 actor 前释放 prepared mutex 与 permission guard。不得新增 arbitrary callback 或 actor 文件操作。
5. 行为测试：准备时缺失目录拒绝；完成准备后删除 workspace，外部重新准备拒绝而执行末检/具名提交仍成功；等待执行期间权限或来源变化拒绝且无部分写。目录消失不等于 SQL 权限撤销：后者仍必须拒绝。

## 已核实调用链

- `prepare_dispatch` 对 dispatchable 行 decode 并比对 content/images/references，调用 filesystem-aware `require_authority`，返回绑定数据库身份及完整 message 的 `PendingInputAuthority`。
- actor `InitializePreparation::apply_with_admission` 在 permission guard 与 immediate transaction 内调用 authority 验证，再经 `verify_pending_sources` → `verify_dispatch`。
- steering `take` 锁外解析引用、验证 skills、读取凭据；`acknowledge` 重验 prepared input/owner/authority/skills，构造具名 Claim，显式 drop 两个 guard 后调用 writer。
- runtime Event actor 在 permission guard 与 immediate transaction 内经 `persist_runtime_event(SteeringAccepted)` → `verify_steering_acceptance`。
- `PendingWrite::apply` 已在 permission guard 后取 immediate transaction；Enqueue/Claim 的 `verify_commit_fence` 使用现存 `capture_fence_for_commit`。其 workspace 重建为 registered SQL identity，随后同一 `capture_fence_at_workspace` 核对 admission、reset、epoch、revision、exact rules；无目录探测。thread admission verify 为内存 identity/ownership 校验，skill consent generation 为 SQL 查询。

## 实现与 API

- 新增私有 `require_authority_for_commit(&Store, &PendingMessage) -> Result<PermissionFence, ConversationError>`：核对 saved fence 的有效性及 thread 绑定后，复用 `verify_commit_fence` → `capture_fence_for_commit`。不新增公共 API。
- 仅 `verify_dispatch` 与 `verify_steering_acceptance` 改调该 helper。外部 `require_authority`、`validate_authority`、`prepare_dispatch`、steering `take/acknowledge` 不变；两处提交端来源比对也不变。
- `PendingWrite::apply` 无生产修改：Enqueue/Claim 已使用 SQL-only fence；Remove/Promote/Recover/Release 仅操作 SQL，Claim 的 owner/admission、cancel、skill generation、pending row/generation、seq 与 claim CAS 保留。
- 完整 pending message 等值检查属于 dispatch；steering acceptance 保留既有 content/images 校验，generation/source-row CAS 属于前置 Claim。本轮不把 runtime input 中不存在的 generation 伪称为 acceptance 已校验项，不扩大 runtime/pipeline 接口。
- 本轮移除的是 workspace 探测/准备 I/O，不是 SQLite 自身持久化 I/O，也不移除共享 writer 的数据库文件身份校验。

## 开发测试

专属 `pending_messages/tests.rs`，只使用临时目录/真实 SQLite、真实 steering take/acknowledge 准备及现有 writer 队列。测试 Gate 只暂停队列，不替代权限或来源决策；末检在具名开发命令的 permission guard + immediate transaction 内调用，Claim/Enqueue 执行生产具名命令。无 production callback/seam。

1. `missing_workspace_rejects_dispatch_and_steering_preparation`：删目录后外部 validate/dispatch/steering take 拒绝，未创建 prepared input，未 claim / 写 messages。
2. `prepared_commits_use_registered_workspace_without_directory_probe`：真实准备后删目录；外部 validate 与 acknowledge 拒绝且无 claim，提交端两种末检及已准备 Claim/Enqueue 成功，目录未被重建。此场景与静态调用链共同验证准备/提交分离，不冒充 syscall tracing 或完整 runtime Event 安装回归。
3. `queued_permission_changes_reject_sources_and_claim_without_partial_write`：在真实 writer Gate 后排队末检和 Claim，再改变 revision / authority epoch / exact rule / registered workspace identity；两种末检均拒绝，Claim 拒绝，pending 状态和 messages 无部分写。
4. `queued_source_changes_reject_stale_dispatch_and_claim_without_partial_write`：同样实际排队后改变 payload / generation；dispatch 与 Claim 均拒绝，payload 变化亦被 steering acceptance 拒绝；保持修改后的 source row、pending 状态及零 messages。generation 不冒称 acceptance 独立检查。

## 验证记录与交付边界

- 已执行 `rustfmt --edition 2024` 定向格式化两个 Rust 文件，随后同路径 `rustfmt --edition 2024 --check`，退出 0。工具 `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`，仓库固定 toolchain `1.98.0`。
- 已执行限定授权路径的 `git diff --check`，退出 0；最终另检查新建文件 whitespace / 冲突标记及生产末检调用链。
- **未运行** Cargo / Clippy / Rust 行为测试 / CI，未取得编译或测试结果。按用户分工由主线执行：`cargo test -p vega_conversation pending_messages::tests:: -- --nocapture`（预期 4 项具名开发测试，不以预期数冒充发现或执行数）。Cargo/rustc 运行版本与耗时由中央实际执行时记录。
- 待中央编译/行为回归、非实现者独立审查及最终 CI。Rust 开发测试、fixture、具名测试命令和 `#[cfg(test)] mod tests` 接线须留痕后在合并 master 前清理；不在本轮未验证时提前清理。未变更 Git/index、fetch 或 commit。
