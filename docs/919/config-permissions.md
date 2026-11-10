# #919 配置与权限独立迁移

## 范围与契约

共享 worktree 以用户指定基线开始；仅修改 permission_contracts、thread_permissions、permission_reset（含子模块）、pending_messages、mcp_settings、skill_settings（含子模块）和本文。不修改 Git/index/基础接口，不执行 Cargo；最终集成、编译及风险回归由主 Agent 统一执行。不单独合并，所有在线入口必须在同一最终版本完成迁移。

具名 owned 命令实现 `crate::database_writer::{DatabaseCommand, Sealed}`，使用 `commit(path, identity, command)`。当前 trait 方法为 `apply(self, &Store)`。提交端不得持权限协调锁；命令内按权限锁→短 SQL transaction 获取。文件配置、Skill 扫描和凭据清理留在 actor 外；提交重新核对数据库身份、来源版本与权限。只读入口不得新增同步 commit。

## 实施边界

- 线程权限保存使用 owned change，命令内重新检查 admission、authority epoch、thread 来源与 permission revision；草稿修改仍为纯读/内存结果。
- Skill 文件发现、预览校验、target binding 在服务端准备，actor 仅接收 owned SQL 参数，使用既有 consent generation CAS。
- Pending 队列保留 authority fence、容量、代际与来源 CAS；序列化/图片准备不进入 actor。
- Permission reset 沿用 durable reset intent，文件保存与 replay 清理留在 actor 外，SQL 阶段重新核对 reset record、settled 与 authority。
- MCP 沿用 pending declarations/outbox 与 cleanup intent，不允许将 `mcp_config::reconcile_with_edit` 整体放入 actor。

## 已确认接口缺口

### MCP 追加授权与实施规格

本轮独占写范围扩展到 `vega_store/src/mcp_config.rs`、`mcp_servers.rs`、新 `mcp_config/*`，及 conversation 的 `mcp_settings.rs`/子模块；其他权限、pending、reset、Skill 与 shared writer 接口不再修改。共享 Git/index 由主 Agent 管理，保持用户指定脏工作树基线，不 fetch/rebase，不运行 Cargo 或全局格式化。

Store 提供受支持 owned prepare/intent/file/install DTO。外部在只读 SQL 快照及锁定文件快照上解析、校验、计算 SQL 投影与 cleanup；actor 重验 accepted digest/generation/pending、完整 rows/identities/schema 后，以短事务登记 durable pending declaration，尚不安装投影。文件导入 intent 使用带原 SQL 来源快照的 JSON envelope，已有 metadata pending 数组保持兼容；恢复重验原 rows/identities、generation 与 accepted digest，不能把变化后的数据库当成新来源。pending 期间 admission 拒绝。外部重新取得配置锁，按 pending base digest 或精确 target 重验来源，保存并重读目标；释放所有 registry/document/config 锁后，actor 再重验来源与新增身份审计，以短事务原子安装投影、cleanup outbox、identities/tombstone、accepted digest 并清 pending。返回前外部重验文件 digest。结果未知不盲重放外部操作，不执行整个 reconcile helper。

在线 store/credential/list 只读且不 migrate/同步等待 actor；文件投影不一致时拒绝旧投影并触发非阻塞后台 reconciliation。list 使用只读 SQL 快照及 registry try_lock，不创建配置锁文件，前后核对真实文件；下一次读取可见后台完成后的投影。显式后台保存与 run 准备执行完整协议，run reconciliation 使用 spawn_blocking。metadata 命令补齐 accepted digest/generation/pending 重验，pending 时不得再改来源。文件、YAML 解析和凭据操作不得进入 actor；无新增 schema、依赖、永久旁路或验收例外。

## 停写交接

本范围仍是未集成实现，不是一次性全库迁移完成，不可单独合并。主 Agent 接管后可继续修改本范围。

### 实际差异

所有路径相对 `crates/vega_conversation/src/`：

| 文件 | 实际变化 |
|---|---|
| `permission_contracts.rs` | `ChangeThreadPermission` 命令；拆出纯 SQL 来源重验 `capture_fence_for_commit`，原外部 fence 仍验证工作区存在 |
| `thread_permissions.rs` | `SaveThreadPermission` owned change、执行端权限锁与 CAS；读/准备/草稿保存使用只读连接 |
| `permission_reset.rs` | Begin/Revoke/Finish 经 actor，文件锁不跨 commit；恢复保持 durable record、settled、文件来源与结果核验 |
| `permission_reset/session.rs` | 只读连接；调度暂停经 actor |
| `permission_reset/writer.rs`（新增） | 四个 reset 短 SQL 阶段，执行端权限→writer 锁序 |
| `pending_messages.rs` | Enqueue/Remove/Promote/Recover/Release/Claim owned 命令；图片、文件引用、Skill/凭据准备及解码留外；事务内 fence/owner/队列来源/序号/generation/cancel 重验；入队锁外校验投影并用完整队列来源 CAS 保留损坏回滚语义 |
| `mcp_settings.rs` | metadata/cleanup 经 actor；store/credential/list 无在线 migrate 或直接 reconcile 写；完整 owned 协议，后台自动发现与 run blocking worker；registry/document 锁不跨 commit |
| `mcp_settings/writer.rs`（新增） | Begin/Confirm 具名命令；metadata 的 registry epoch、revision 与投影来源 CAS；cleanup SQL 重验 accepted digest/generation/pending、outbox 目标与 tombstone 来源 |
| `skill_settings.rs` | 只读 store；文件发现/加载、预览 identity/target-binding 检查后准备 owned mutation，释放 preview 锁再提交 |
| `skill_settings/writer.rs`（新增） | generation CAS 与 SQL；排队后重验预览 active/epoch，重复关联保持 no-op |

使用现存 `DatabaseCommand::apply`（不另造 execute trait）；没有新增 Fn/原始 SQL 提交 API、依赖、DDL 或测试。`skill_settings/composer.rs` 未改。

### 必须由主 Agent 集成的缺口

1. **普通发送/草稿 owned DTO**：`permission_contracts.rs:339` 的 `prepare_submission_permission` 仍持原 guard 执行旧事务。`OrdinaryWorker` 不可克隆；`DraftCreationGuard` 持有非 Send MutexGuard 与 ConfigEdit。需要主 Agent 已负责的 owned preparation/worker fence 生命周期接口；然后拆外部配置/工作区准备和 actor SQL 安装，保留 authority/source/model/workspace/accepted fence/失败 unknown 断言。现有 `DraftCreationGuard` 的 threads/sidebar/automation 调用亦须由主 Agent统一接线，不能持 guard 等待 actor。
2. **MCP 投影 reconcile（本轮已实现，待中央验证）**：旧 `reconcile/reconcile_with_edit` 移除，无调用残留；store owned DTO 及同库 actor 接线已完成。在线 migrate 已删除，依赖既有启动阶段 schema 初始化；不把在线 migration 登记成例外。主 Agent 不再需要提供 private DTO。需中央回归确认自动发现、中断恢复、文件/SQL 来源冲突、凭据 cleanup 和关闭 drain；详见下方 MCP 追加证据。
3. **生产身份重验**：本范围提交均传入原 `Store::database_identity()`。现有基础 `Request::execute` 只比较缓存身份；`DatabaseIdentity::for_connection` 不对 conversation 公开。主 Agent 必须在执行端对实际生产连接/路径 device+inode 重验，尤其原连接打开后文件被替换的情况；本范围未修改基础接口，不能宣称已经解决。

### 尚未验证的集成风险

- 全部同步 commit 调用须由后台保存/worker 执行；pending 公共 API 的所有外部调用线程、MCP async 方法中的阻塞段由主 Agent统一检查，不能在 UI 在线读路径同步等待。
- Reset 保留 admission pause/precompute pause 生命周期，命令执行端检查 database paused + SQL settled；pause 的精确 token/普通发送 recovery worker 在排队后的原子复核仍需主 Agent 的 owned lease 接口与并发回归确认。文件阶段释放后若发生外部修改，后续访问必须依据 reset record/config revision 拒绝旧来源。
- MCP cleanup 文件删除后若 SQL 来源 CAS 失败，outbox 保留，下一次允许按现有 Missing 语义恢复；没有自动重放外部动作。确认 ack 与 finish-remove 是既有两阶段事务，结果未知不能盲重放。文件确认前释放锁是有意边界：actor 不读取文件，最终外部重验与在线 admission/read digest 门禁拒绝后续文件变化；需验证确认排队时外部编辑竞态，不得删门禁。
- Skill 文件验证仍在外部；actor 保留 consent generation 与 preview epoch CAS。外部文件变化、project/root 改绑及 close/clear-preview 排队竞态需要回归，不以静态检查声称安全证明。
- Pending 新增 full-queue 来源 CAS 可返回冲突，调用端不得盲重放；无生产路径/in-memory Store 不再旁路直接写入。原文件工作区验证在准备端，提交端只重验 SQL 推导工作区及完整 fence。

## 最终静态证据

- 源码基线/交接 HEAD：`555b0c0b3c00031ad22b69c7a5622e3c1dc4116b`；分支 `feat/919-database-writer`，未提交、未改 index，未执行 fetch/rebase（共享 Git 状态由主 Agent独占）。
- 按上表 10 个代码文件顺序，以 `relative-path + NUL + file-bytes + NUL` 拼接的 SHA-256：`7a8c1a72a969000cefba69c1cf1c1d71d4317cec127a78acbab9553867e28473`。文档不计入该代码哈希。
- 直接运行 `rustfmt --edition 2024 --check --config skip_children=true`，显式列上表 10 个代码文件，退出码 **0**；工具版本 `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`。
- 对授权范围运行 `git diff --check -- <授权路径>`，退出码 **0**。
- 静态清点新增 diff 的注释/unwrap/expect 匹配 **0**；三个新 writer 模块注释、unwrap/expect、Rust test 接线匹配均 **0**。`rg` 明确发现上述普通发送与 MCP reconcile 残留，未将残留说成通过。
- **NOT RUN**：Cargo/rustc 编译、Clippy、行为/安全/并发回归、CI、独立审查、用户手测；Cargo/rustc 版本本轮未执行取得。按用户要求不跑 Cargo、不添加开发测试，交主 Agent统一风险回归。

## MCP 追加迁移停写证据

以下仅覆盖本轮 MCP；前文旧 10 文件 hash/检查是上一阶段历史记录，不能代替本轮验证。HEAD 仍为用户指定基线，无提交/index/Git 状态操作；代码仅下列五文件及本文 MCP 段落，未修改 permission/pending/reset/Skill/shared writer。

| 文件 | 对 HEAD 的精确 diff 或新增规模 | SHA-256 |
|---|---|---|
| `crates/vega_store/src/mcp_config.rs` | +7/-208；删除旧混合 helper，导出 owned API | `aae46b0919c3bffb5299cff27d6c3dd373eb9f3a5fc2b3e6c7ea1ad382703051` |
| `crates/vega_store/src/mcp_servers.rs` | +14/-9；来源序列化、纯 cleanup refs 准备 | `5f07a261f385ef964a907b56c77d44f37da84909a566a922490539e16ea686ad` |
| `crates/vega_store/src/mcp_config/prepared.rs` | 新增 374 行 | `179c7f9b210d7155c6c742f5c62d43c3d07a02f9fa0e40cdce2c5049239def0e` |
| `crates/vega_conversation/src/mcp_settings.rs` | +355/-101，含上一阶段未提交 MCP 改动 | `dc7b0a1b63892c45a0d5f4f554245be8dc9a599c1b95bf43fcea095eabe54653` |
| `crates/vega_conversation/src/mcp_settings/writer.rs` | 新增文件现 226 行，含上一阶段实现 | `37a6ee70b74e00110ade6894e358dd619aeea7627317e1ad418a11145d8aa317` |

按表顺序使用 `relative-path + NUL + bytes + NUL` 的代码内容 identity：`2ebc3fbee5ad48a42e096f51b5f684d8dcb2c51e8d21e6521455bd4429e13ac4`。文档不计入代码 hash。

### MCP 公开边界

- `mcp_config::prepare_reconciliation(&Store, Option<&[u8]>, validate)` → `PreparedReconciliation`；回调只在外部准备调用，不存入 DTO。
- `PreparedReconciliation::{unchanged, begin(&Store)}` → `ReconciliationIntent`；begin 仅 actor SQL intent，无投影提前安装。
- `ReconciliationIntent::{changed, save(&Store, path, Edit)}` → `PreparedConfirmation`；save 只在外部执行，消费/释放 Edit。
- `PreparedConfirmation::confirm(&Store)`：actor 短事务，来源 CAS 后安装与确认。
- conversation 使用 `BeginMcpReconciliation`、`ConfirmMcpReconciliation`，原 `commit(path, identity, owned command)` 不变；metadata 与 cleanup 仍为具名命令，无 Fn/SQL 提交接口。

### MCP 已执行与待验证

- `rustfmt --edition 2024 --check --config skip_children=true <上述五文件>`：退出 **0**；`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`；只格式化明确授权文件，无全局 fmt。
- `git diff --check -- <本轮授权 MCP 路径和本文>`：退出 **0**。
- 静态 `rg`：在线 `Store::open`/`.migrate`/旧 `reconcile[_with_edit]`、全 crates 删除符号引用均 **0**；actor writer 模块 `yaml_config|keystore|save_fields|std::fs|serde_json` 匹配 **0**。新增 diff 加完整新模块的注释/unwrap/expect/Rust tests 接线匹配均 **0**。
- Settings 既有 `vega_ui/src/settings/mcp.rs` 后台 `vega-mcp-settings` worker 调用保存；本轮未改 UI。run 对账增加 `spawn_blocking`，list 不 commit/不创建锁文件。后台完成后下一次读取会重验文件并取得新投影；当前 UI Reload 本身是 NoData，没有新增完成订阅，首次冲突后的自动重刷/反馈由主 Agent在 UI 范围决定，不能误报本轮已实现刷新通知。
- 已静态观察主 Agent 的共享 `Request::execute` 改为 `store.verify_database_identity(identity)`；本轮未修改基础接口，其实际连接/路径替换回归仍由中央验证。本轮 DTO 另比较原 Store identity。
- **跨范围生命周期集成待验证**：自动发现使用有去重标志的后台 OS worker，没有可写权限接入 app shutdown/join；主 Agent须核对全库关闭/drain 与该 worker 的生命周期，不能仅凭已入队命令 drain 推断未入队外部文件阶段也已完成。无需新增 NecessaryLease 到此任务。
- **中央必测风险，均 NOT RUN**：首次文件导入/禁用投影、metadata pending 兼容、intent 后与文件保存后崩溃恢复、原 rows/identities/digest/schema 变化拒绝、新身份历史审计拒绝、并发 metadata/cleanup、确认排队时文件被改、cleanup Missing 恢复、不同库隔离、只读 list 后台发现与最终关闭。保留既有 assertions，无缩小验收或永久线上旁路。
- **NOT RUN**：Cargo 编译、Clippy、行为/安全/并发回归、CI、独立审查、用户手测；未取得本轮 Cargo/rustc 版本。本轮静态成功不是行为通过，22+1 历史阶段性回归不证明本版本通过。代码与文档交主 Agent集成，一次性全库任务继续，不单独合并。

