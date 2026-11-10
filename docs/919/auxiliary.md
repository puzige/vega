# #919 辅助写入迁移

> 当前整合状态见末节；下文原实施记录、未迁移清单及验证限制属于历史阶段，不代表当前缺口。历史内容与 hash 保留。

## 范围与验收

仅修改用户指定的 automation/、automatic_titles.rs、composer_predictions.rs、session_reasoning.rs、submission_diagnostics.rs、types/prediction_diagnostics.rs、privacy.rs、plans.rs 及本文档。共享基线由主 Agent 管理；本执行不 fetch/rebase、修改 index、提交或运行 cargo。

所有迁移使用 `crate::database_writer::{DatabaseCommand, Sealed, commit}`，命令为封闭具名 owned 数据，不携带 Fn、连接或任意 SQL。调用方不得持权限/偏好 Mutex 或事务等待 actor。命令执行端保留原锁序及短事务；模型、计价、文件操作、脱敏和用户回调留在 actor 外。用量及其关联状态不得分开提交。诊断队列满不得静默丢弃必要事件。

本次是一次性全库迁移的独立实现范围，不授权分批合并。未完成入口保留为阻塞项，不视为启动或旁路例外。

## 实施设计

- 标题：模型及计价在线程外完成，owned 完成命令携带 title admission，执行端复核数据库身份/线程；用量及标题在同一短事务中提交，通知留调用线程。
- 预测用量：在线程外计价，owned 计费命令执行 SQL。
- 自动任务：普通 command、暂停、匹配暂停、过期刷新、释放和结束迁入具名命令；事务内 helper 继续供主 Agent 的原子运行事件调用，不额外提交命令。recover_startup 在其已有权限锁内直接使用原 finish 事务执行体，避免持锁后再等待 actor；该函数仅保留原启动恢复边界，不豁免任何在线调用。
- 提交诊断：复用原有 journal.events 存储，单 journal worker 按 next_write 顺序读取并等待 actor，journal 锁在等待前释放，worker 持有 Arc 直到排空；不再通过可能满的独立事件队列丢弃必要事件。线程创建/SQL/actor 失败保留 failed_memory_retained 状态，不自动重放未知结果。全局 worker 上限与应用关闭 drain 仍需主 Agent 集成生命周期协议。
- 预测诊断：后台 worker 等待 actor；UI 入口仍保留原有 try_send，满队列不丢失的验收尚未实现，见下方阻塞项。
- 推理偏好：initialize 和 save 的偏好协调锁在执行端取得；保留 with_current_preferences 的闭包锁契约，不在该闭包内等待 actor。
- 隐私：epoch、例外仅携带已计算数据入队；永久脱敏不能把旧 clean(FnMut) 整体搬到 actor。

## 已确认的跨范围协议缺口

- `permission_contracts::DraftCreationGuard::begin` 同时持权限 Mutex 与 ConfigEdit，并读取配置文件，materialize 依赖借用 guard。automation::command_for_draft 需要主 Agent 的 owned prepared draft + 执行端权限/配置版本复核协议。
- 初始盘点的 `capture_fence` 文件依赖已由主 Agent 在共享 worktree 新增的 `capture_fence_for_commit` 解除。已读取该实现：仅从 thread/project/permission SQL 重建工作区和 fence，不执行文件检查。本范围增量使用该接口迁移 authorize_full_access、claim/activate、AutomationPermissionService::save、RuntimeAutomationHook::call 与 plans::review_plan；调用端完成工作区文件检查/准备，执行端取得原权限锁再复核。此基础接口由主 Agent 写入，本执行未修改它。
- `credential_privacy::clean` 在短事务内调用脱敏 FnMut 并遍历历史/offset/JSON。需要 store 层 owned prepared cleanup DTO（source fingerprint、字段值、response/generated notice offsets、compaction、historical delivery 等完整变更）及 actor 内 CAS install；不允许传任意 SQL 或脱敏回调。
- `session_reasoning::initialize` 在线惰性写入已迁移，非启动例外。静态确认唯一生产调用为 threads::load_conversation，外层 window/conversation_loading.rs::start_conversation_load 使用 vega-conversation-load 专用线程。save 在 window/reasoning.rs 的保存 job 中执行；with_current_preferences 的现存闭包只写 reasoning 配置文件，不提交 SQL 命令。后续主 Agent 不得把这些提交接到 UI 同步路径或持偏好锁的闭包里。
- 自动任务工具已迁入 CallAutomation 命令：执行端权限锁 → admission → 短事务 → current_claim_for_commit 的 SQL fence/task/owner 复核 → 原工具事务 helper → cancellation 复核 → commit。工具回调和异步调度保留 actor 外，不把 callback 搬入命令。
- `types/prediction_diagnostics.rs::record_prediction_event` 从 window/composer_predictions.rs 的 UI 同步入口调用。基础 commit 是同步阻塞 API；直接用 send 背压会阻塞 UI，保留 try_send 则仍有满队列丢失。最小所需协议：预持有的有界诊断 drain lease/异步发送端，将必要事件保留到获得 actor admission，并纳入 app 关闭排空；不可用无限线程/无界队列掩盖问题。当前仅迁移 worker 的 SQL 执行端，队列验收明确未完成。

### 未迁移入口清单

- automation/mod.rs：command_for_draft 仍阻塞于 owned draft/config 协议；finish_unsettled_claim 的读后 finish 已通过新命令提交，但其读后竞态语义未扩展。recover_startup 仍只用于原启动恢复边界。
- 自动任务授权、claim/activate、权限保存和工具调用已补迁移，不再列作未迁移入口。
- privacy.rs：permanently_clean_with_replay（需要 store prepared cleanup 安装协议）。
- plans.rs：review_plan 已迁移至 ReviewPlan 命令，审批所需 workspace 准备在调用端，权限协调和 thread admission 在 actor 执行端取得。
- prediction diagnostics：SQL 已迁移，但 UI 入队满队列保留及关闭 drain 尚缺；不能据此宣称全部诊断验收完成。

### 原子性及交接边界

未修改 automation::record_usage(conn, claim, usage_id, input, output) 与 reset/pause 的事务内入口，不把 usage/attribution/automation 状态拆成多个 actor 命令；主 Agent 的运行事件仍须在同一事务中调用这些 helper。新增标题命令将自身用量及标题更新放入同一事务。基础 commit 不提供稳定操作身份去重回执；本执行不重试 outcome-unknown，也不宣称已证明重复交付计费去重，需由全库协议统一验证。

## 验证

按用户要求仅静态检查，未运行 cargo、Clippy、行为/并发测试或 CI；无新增 Rust 测试。编译/类型检查、原子性、权限排队后复核、队列压力/退出排空仍待主 Agent 统一验证。

已执行 `rustfmt --edition 2024 --config skip_children=true`，仅指定本范围已修改的 Rust 文件；工具报告 rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)。对同一文件列表执行 `rustfmt --check`，并对授权路径执行 `git diff --check`；结果及文件 hash 以最终停写回报为准。rustfmt 只证明解析/格式，不冒充编译或业务回归。

实际修改：automation/mod.rs、automation/permission.rs、automation/runtime.rs、新增 automation/writes.rs、automatic_titles.rs、composer_predictions.rs、session_reasoning.rs、submission_diagnostics.rs、types/prediction_diagnostics.rs、privacy.rs、plans.rs、本文档。无 Git/index/基础接口修改。本实现是明确未完成的整合检查点，不可单独合并；以上未迁移项不作为旁路例外缩小一次性全库验收。

## 当前整合状态（源码核对，非独审／最终验收）

本次仅整合文档。主会话提供的基线为 HEAD `7c3ee7c97db83fbec4ce099313661f9cb7325cb3` + 未提交整合，MERGE_HEAD `2a29adf9b2ea78eaf59d591838531b45adc7373e`；本轮 fetch 已由主会话完成，origin/master 与该上游相同。本次未执行 Git/Cargo、未改 Rust 或用户库；以下是相关接口的有限源码核对，不是全库新调查。

- **privacy cleanup 已接线**：`privacy.rs::permanently_clean_with_replay` 释放 permission coordination 后进行 review、投影、脱敏和 `credential_privacy::prepare_clean`；提交 `PermanentlyCleanPrivacy { prepared, token, admission, precompute }`。store `credential_privacy/prepared.rs::PreparedCleanup::install_in_transaction` 在 actor 的事务内验证来源 fence、revisions、permission、epoch、exceptions 和 idle，并以行原值 CAS 安装。脱敏回调留在准备侧，不再将旧 clean(FnMut) 搬入 actor；这不是 SQL 安装耗时恒定的声明。
- **无回调 token 已接线**：`privacy.rs::PreparedRedactionAuthority` 将 policy 与 `RedactionCommitToken` 分离；token 只有 owned 校验数据。`pipeline.rs` 提取 `commit_token`，`pipeline/install.rs::InstallPreparation` 携带 `Option<RedactionCommitToken>`，两次提交端 validate 保留。历史“完整 authority 携带回调入队”缺口不再是当前状态。
- **automation draft owned 协议已接线**：`permission_contracts.rs::DraftCreationGuard` 名称保留，但字段仅为 owned draft/operation/epoch/generation/source，不持 MutexGuard 或 ConfigEdit。`begin/under_coordination` 的配置锁和权限锁在等待 `intents::PrepareDraft` 前释放；配置读取及前后快照检查在 actor 外。`PrepareDraft` 记录 durable intent；`automation/mod.rs::DraftAutomation::apply` 在 permission → SQL 事务内调用 `materialize`，按 operation/thread/authority epoch/configuration generation/source/state/pending 复核并原子创建任务。未据静态接线宣称所有配置竞态已完全验收。
- **pending 提交端已改为 SQL-only 来源／权限复核**：`pending_messages.rs::verify_commit_fence` 使用 `capture_fence_for_commit` 从已注册 thread/project/permission SQL 构造 fence，不再探测 workspace 目录。锁外来源准备仍保留 workspace 检查；“SQL-only”不表示数据库自身 I/O 或公共数据库身份检查消失。
- **预测、MCP 与提交诊断共用有界后台池**：`submission_diagnostics/background.rs` 为 2 workers、最多 1024 个待执行 job，另有至多 2 个 active；不是无限线程或无界 job 队列。`types/prediction_diagnostics.rs::record_prediction_event`、`mcp_settings.rs::schedule_reconciliation` 及 journal worker 使用该池，闭包只在 actor 外协调，数据库提交仍为具名命令。满容量／closing 为接受前显式拒绝并记录 not_accepted，不宣称所有被拒绝事件已保存；已接受 job 的 SQL/actor 失败或 panic 计入 failures，继续排空后 shutdown 返回错误，超时报 queued/active/failed。journal 保留 failed_memory_retained 语义，不自动重放未知结果。
- **关闭已接线**：`vega/src/app_agent.rs` 后台退出路径先处理 precompute/Shell，再 `shutdown_background_writes`、`shutdown_database_writers`、failure sidecar；各阶段失败有真实错误，不冒充 drain 成功。

### 保留风险与文字冲突

`InstallPreparation` 的 Skill 来源末检，以及 privacy token 的 `CredentialStorageVersion::capture`，仍同步发生在 actor 的 permission lock／SQL 写事务内。它们执行路径解析、open/openat 和 metadata（credential 为 canonicalize + symlink_metadata），不读取／解析文件 body、不取得文件锁；调用数或文件大小限制不提供耗时保证。`Store::verify_database_identity` 的数据库自身 metadata／SQLite 身份安全检查与 SQL I/O 保留，不为性能删除。

本卡原承诺是慢**锁外准备**不阻塞父／两子运行，不承诺任意慢盘上的 actor 完全无阻塞。上方历史范围段“文件操作……留在 actor 外”若按所有文件 syscall 理解，比该承诺更强，且与现有安全末检冲突；此处显式交主会话确认措辞／契约归属，不擅自删安全检查或修改验收。

### 已报告验证与仍待门禁

主会话本次提供：中央 conversation **78/78**、store **27/27**、app **7/7** 通过，包含 privacy/pending/diagnostics；store **14** 项 lint 已修且编译通过；conversation **9 项生产 + 1 项 test lint** 修复中。此为中央报告，不是本文档整合者重新执行或独立复验；命令、工具链、日志与版本对应由主会话维护 `validation.md`。Clippy、最终冻结审查、CI 仍待完成；开发测试留痕／合并前清理、最终 head required CI 和用户手测仍为独立门禁。当前不能宣称完整验收或可合并。
