# Goal 与 Loop 交付记录

日期：2026-10-17。规格：[v1.1](vega-issue-341-loop-goal.md)。实现分支：`feat/341-loop-goal`，基线：`3705c687`，规格初始提交：`54b73c8`。

实现代码及回归用例已完成，交回主 agent 审查、PR 和 Actions。**本地没有运行 Vega、编译、构建、打包或执行测试；以下所有自动回归均待 CI，不代表已经通过。** 用户手测仍待合并后进行。规格偏离：无。

## 行为与架构

- 原生 composer 支持 `/goal`、`/loop` 的创建、状态、暂停、恢复和清除，以及 `+` 菜单入口。非法命令、附件和外部 Codex backend 保留原草稿并显示局部原因；不会发给 provider。首次创建将原 draft identity 与任务放在同一事务 materialize，成功清除 draft route 和命令文本并更新侧栏。
- 迁移 `0026_automation.sql` 新增任务和运行账本两张表，线程外键级联，未完成账本每线程唯一。Store 只处理存取；共享类型、命令与领域状态在 conversation。两种归档领域入口都会暂停自动任务并提升 revision。
- 应用启动只恢复一次：结算崩溃前已持久化用量、将其完整性标为未知、释放在途 claim，并暂停原 Active 项。Store open/migrate、普通 worker 和第二窗口不会触发恢复。恢复失败通过共享 readiness gate 阻止自动运行并显示错误。
- App controller 使用现有 worker、权限、排队、取消和预检链。共享窗口 owner、weak stream、mutation lane、epoch、持久 claim 共同防止双跑和迟到回写；其他窗口的草稿/预检也使自动任务让位。关闭窗口后释放未开始 claim 和 owner，终态清理对应 run 句柄。
- Goal 成功回合至少让步 1 秒后续跑，Loop 优先于 Goal，真实用户输入优先。Loop 首次和恢复立即执行；后续从完成时刻计算固定间隔或动态延迟，不追赶漏过 tick。动态默认 300 秒、范围 60–3600 秒，7 天有效期仅由用户创建或恢复重设。
- Pause/Stop 即刻在本机撤销，保存失败仍保持禁止自动运行。其他 kind 的创建、普通 worker 和已有 Loop 到期前先兑现待暂停。activate 时再次剔除已撤销的非 trigger 项，返回持久快照供模型上下文、工具权限和用量归属使用。
- 专用 headless hook 将 `get_goal`、`update_goal`、`schedule_wakeup` 接到既有工具注册、prepare、执行与 audit。报告先存本 run，只有成功终态才改变完成、进度及唤醒状态。仅当前 Loop tick 暴露唤醒工具；实际任务 identity/revision/data 参与 replay fingerprint。失败工具历史按真实错误前缀验证，保留严格来源和 approval 校验。
- PersistenceActor 在同一事务中插入真实 `token_usage` 并将精确 row ID 归入当前 run，排除自动标题和外 run。多次 steer、压缩 usage 均归同一账本，终态幂等；暂停提升 revision 后仍按 identity 记历史用量，clear 后新 identity 不串账。`get_goal` 包含本轮已持久用量的只读投影，finish 不重复计费。
- 预算累计 input+output，不重复加 cache 分项，不估算缺失 usage。有预算且完整性未知时暂停，不能凭完成报告或恢复绕过。已完整计量的取消与业务失败不自动变为未知；已经启动但未正常结算的兜底按未知处理，尚未启动的预检失败仍可恢复。累计耗时只计运行，不计 Loop 等待。
- UI 使用已有主题/布局/按钮，紧凑状态行提供文本动作与按需详情；预算耗尽或未知时不提供无法成功的 Resume，保留原因和 Clear。Loop 显示分钟级预计时间，仅显示值或投影变化时通知重绘，运行中隐藏等待文案；错误持续可见，没有额外成功通知。

## 文件清单

以下列出本卡全部变更，规格 v1.1 正文由主 agent 维护，一并纳入实现提交。

| 层 | 文件 |
|---|---|
| Store | `crates/vega_store/migrations/0026_automation.sql`；`crates/vega_store/src/automation.rs`；`crates/vega_store/src/lib.rs`；`crates/vega_store/src/permissions.rs` |
| Conversation 类型与领域 | `crates/vega_conversation/src/types/automation.rs`；`crates/vega_conversation/src/types/mod.rs`；`crates/vega_conversation/src/automation/mod.rs`；`parser.rs`；`runtime.rs`；`tests.rs`；`crates/vega_conversation/src/lib.rs`；`crates/vega_conversation/src/threads.rs` |
| Conversation 运行接线 | `crates/vega_conversation/src/agent/entry.rs`；`events.rs`；`persistence.rs`；`pipeline.rs`；`tests/automation.rs`；`tests/mod.rs`；`tests/stream_persistence.rs`；`crates/vega_conversation/tests/s7_acceptance_e2e.rs`；`todo_e2e.rs` |
| Runtime | `crates/vega_runtime/src/lib.rs`；`agent/automation.rs`；`loop_.rs`；`mcp_registry.rs`；`mod.rs`；`tools_exec.rs` |
| App | `crates/vega/src/automation_controller.rs`；`app_agent.rs`；`main.rs`；`window/automation.rs`；`window/agent.rs`；`window/mod.rs`；`window/pending_messages.rs`；`window/render.rs`；`tests/automation.rs`；`tests/composer_actions.rs`；`tests/controller-git-fixtures.json` |
| UI | `crates/vega_ui/src/conversation_stream/automation.rs`；`composer_actions.rs`；`core.rs`；`mod.rs`；`render.rs`；`tests/automation.rs`；`tests/mod.rs` |
| 文档 | `docs/vega-issue-341-loop-goal.md`；`docs/vega-issue-341-loop-goal-delivery.md` |

未改依赖、Cargo 配置、CI 工作流、用户配置、既有 provider 实现或其他卡片文件；未新增代码注释、Issue 编号测试名。现有 schema 精确断言保留并由 36 张更新为 38 张，包括 `permissions.rs` 的独立断言。

## 验收矩阵与证据层级

下表是初次交付时的回归映射。最新已完成 CI 的真实失败结果和修复见文末；**本轮修复后的矩阵仍待最新 head 的 CI 验证**。D 为 conversation 领域/SQLite 测试，不经过 provider/controller；P 为真实 conversation/runtime/PersistenceActor 入口加 MockProvider（个别标明仅 actor 或 fingerprint）；A 为真实 VegaWindow/composer/生产 worker/controller 加 MockProvider；U 为 ConversationStream 的 GPUI 控件/键盘测试；C 为共享 owner/epoch 的纯 controller helper；M 为保留待用户手测。

| 编号 | 已编写的验证与具体边界 | 层级 / 待验证 |
|---|---|---|
| G1 | `loop_capability_is_only_available_on_its_tick_and_ordinary_runs_create_nothing` 中普通请求不创建任务或自动工具；UI 无任务不渲染状态 | P、U；待 CI |
| G2 | `composer_goal_persists_runs_continues_and_completes_through_native_worker`；`new_task_command_materializes_the_existing_draft_identity_before_running` 从侧栏新草稿提交；`invalid_draft_command_rolls_back_thread_materialization` | A、D；待 CI |
| G3 | composer Goal 用两次工具报告及两个最终回答驱动真实两轮 worker，controller 注入一致时钟越过 1 秒；保留同一目标、真实累计 48 tokens | A、P；待 CI |
| G4 | `goal_tools_use_normal_lifecycle_and_meter_only_owned_provider_rows`；完成只在成功最终回答后生效，保留证据/用量；再次 poll 不请求 | P、A；待 CI |
| G5 | `failure_never_applies_completion_and_resume_resets_streaks`、动态报告相关用例覆盖相同 blocker 三次、进展/恢复清零 | D；待 CI |
| G6 | `three_missing_reports_block_and_progress_clears_streak` 覆盖第三次无报告停止和进展重置 | D；待 CI |
| G7 | `progress_and_completion_only_apply_after_success_and_once`；`budget_keeps_inflight_usage_after_pause_and_rejects_bypass`；多 steer 同 run 和自动标题排除；本轮 get_goal 投影与 finish 只计一次 | D、P；待 CI |
| G8 | parser 覆盖零/负数/溢出/空值/超长/Unicode 间隔；`invalid_commands_preserve_draft_without_provider_or_storage` 检查原草稿、零 provider 请求、零消息落库 | D、A；待 CI |
| L1 | `loops_use_finish_time_expire_and_never_catch_up` 用假时钟验证完成后间隔与单一到期；真实 Loop 首次、Stop/Resume、共享输入让位用例 | D、A；待 CI |
| L2 | `dynamic_wakeup_is_scoped_and_stop_preserves_goal` 验证默认/合法动态时间；Loop tick 的生产注册与执行用例 | D、P；待 CI |
| L3 | 固定间隔和越界拒绝、外线程/旧 revision 报告拒绝；`failed_wakeup_tool_history_reopens_and_runs_again_with_strict_audit_validation` 走真实失败 audit 并重开再运行 | D、P；待 CI |
| L4 | 动态 stop 保留 Goal；固定到期与恢复；clear/new identity 隔离；实际 controller Clear 结束后无后续请求 | D、A；待 CI |
| L5 | tick 实际历史和 usage 经 P/A 落库；noop 领域报告不抹除历史，下一次到期仍由 finish 决定；UI 无成功 toast 接线 | D、P、A，通知/视觉 M |
| S1 | `explicit_creation_and_claims_are_atomic_and_single_flight`；`queued_input_and_pending_plan_prevent_automatic_claim`；`pending_automatic_claim_yields_to_user_but_started_claim_does_not`；两个真实窗口的 draft/preflight 让位 | D、A、U；待 CI，IME 真实输入 M |
| S2 | `stop_revokes_running_loop_and_resume_is_explicit`；失败 pause 保持本机撤销、创建和已有另一 kind Loop 都先兑现暂停，provider 无旧 Goal 工具/上下文、Goal 用量不增加 | A、D；待 CI |
| S3 | clear/recreate、重复 finish、旧 revision 用量和旧失败快照；`activation_removes_revoked_secondary_task_from_context_tools_and_metering`；窗口关闭 claim 清理、迟到终态、mutation epoch | D、A、C；待 CI |
| S4 | 缺 usage / 已计量取消 / 预检失败；`unsettled_started_run_keeps_usage_unknown_but_preflight_failure_can_resume`；凭据报告被 prepare 拒绝；失败不应用暂存完成 | P、A、D；待 CI |
| S5 | `restart_reconciles_claim_once_and_open_does_not_pause`；startup readiness gate；两个归档领域入口及 delete cascade；真实自动 worker 归档/删除取消；窗口关闭后另一窗口接手 | D、A、C；待 CI；真实重启/切换 M |
| S6 | `compaction_usage_is_persisted_with_claim_and_unknown_summary_blocks_budget` 直接走 PersistenceActor；`replay_capability_fence_includes_goal_identity_revision_and_instruction`；失败工具历史重新打开；继续使用 worker 每次新冻结的模型/权限配置 | P（actor/fingerprint）、P（重开）；待 CI；模型切换 M |
| U1 | `automation_menu_preserves_draft_keyboard_selection_and_escape`；创建/错误的真实 composer 提交；`codex_has_no_automation_menu_but_explicit_command_error_is_visible` | U、A；待 CI；平台输入/鼠标 M |
| U2 | `automation_status_actions_details_and_budget_focus_are_accessible`；完整详情、文本动作、错误、未知/耗尽预算不展示无效恢复、Complete 保留；`unchanged_loop_poll_does_not_notify_history_and_running_hides_waiting` 验证相同 poll 不通知历史、运行中隐藏等待 | U；待 CI；浅深主题/窄窗 M |
| R1 | store 四处 schema 精确断言、conversation 三处完整表名列表及迁移版本；两表 FK；重复启动恢复和 open/migrate 不暂停；原子 draft 创建回滚 | Store、D、P；待 CI |
| R2 | 生产工具生命周期、Readonly/Once audit、credential redaction、cancel、compaction、replay fingerprint 与失败 replay；runtime 专用 trait 不依赖 Store/UI | P；待 CI；静态依赖检查已执行 |

其他重要 P 用例：`multiple_steered_responses_share_one_goal_ledger`、`automatic_title_provider_usage_is_separate_from_the_goal_run`、`stale_preclaimed_entry_releases_ledger_and_never_calls_provider`、`automation_report_credential_is_rejected_before_durable_tool_input_or_goal_report`。A 用例还包括 `successful_durable_finish_does_not_cancel_pending_ui_terminal`，避免正常账本已 finish 而 UI 尚未消费终态时误取消。

## 已执行的本地静态检查

在本 worktree 执行，均未运行项目代码：

| 命令 / 检查 | 退出码 | 结果 |
|---|---:|---|
| `cargo fmt --all` | 0 | 格式化结束 |
| `cargo fmt --all -- --check` | 0 | 无输出 |
| `git diff --check` | 0 | 无空白错误 |
| `python3` 只读 staged diff 静态扫描 | 0 | 新增注释 0、Issue 编号测试名 0、依赖或 workflow 修改 0 |
| `rg` 新生产模块的 `unwrap(`/`expect(`/注释/Issue 编号 | 1 | 无匹配；1 为 rg 无结果，不是执行失败 |
| `rg` 所有 `activate_claim` 调用点与 schema 断言 | 0 | Option 返回值接线一致，36→38 精确断言更新 |

静态检查不能证明 Rust 类型、Clippy、测试或真实界面运行正确。没有运行 `cargo check`、Clippy、test、nextest、Vega 或任何构建/打包命令。

## 待 CI 与用户验收

主 agent 创建 PR 后，以最新 head 的 required check `check (fmt, clippy, test)` 为门禁。待 Actions 执行：

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

有失败时保留首次云端输出并修复实现，不降低断言。没有新增真实外部进程或服务依赖；provider 通过已有 MockProvider，UI 使用进程内 GPUI。

用户手测按规格 §8：Goal 至少一次续跑及完成、Pause/Resume/Stop、固定 1 分钟 Loop、动态安排/stop、输入让位、切换会话、退出重启后手动恢复、Clear 无再次执行、浅深主题/窄窗口/键盘和错误恢复。

## 风险与未达项

- 代码实现范围已完成；云端已编译并执行回归，第四轮 Nextest 有 20 项失败，本轮已按根因修复但尚未取得最新 head 的 CI 结果。真实 provider/系统 UI 集成仍待用户手测，不能将本报告当成验收通过证据。
- `0026` 是当前基线的下一个迁移号；若并行 PR 先占用，主 agent 集成时需重编号并保持现有 schema 断言精确。
- 崩溃和已开始但未能正常结算的回合保守记录 usage 未知；有预算的目标需要清除后新建。这是防止未核实用量绕过预算的既定行为。
- token budget 是新回合准入边界，已提交 provider 请求可以超过上限；没有硬计费限额承诺。
- 自动调度只存在于运行中的 Vega 原生应用，退出后不执行；重启后需手动恢复。没有系统后台服务或通知通道扩展。
- 无已知规格偏离；无有意省略的产品实现项。CI 与上述 M 项仍待实际验证。

按任务要求仅交回本 worktree 和实现提交，不 push、不建 PR、不改看板、不合并；交付后停止写入，等待主 agent 审查。

## 首轮 CI 失败与修复

PR [#451](https://github.com/puzige/vega/pull/451) 首轮 [Actions 39046384638](https://github.com/puzige/vega/actions/runs/39046384638) 对实现提交 `de0bbdc670aff0d64af3dc5f69c47627926a5370` 执行。Clippy 和 Nextest 都在编译 `vega_store` 时退出 101，尚未执行回归测试。首次完整失败日志保留在主机 `/tmp/vega-341-ci-39046384638-failed.log`，没有用重跑替代修复。

已证实的同一错误为 `crates/vega_store/src/automation.rs:26` 的 E0277：`u64` 不实现 `rusqlite::ToSql`，`save_task` 将共享类型的 `revision: u64` 直接交给 SQL 参数。

本轮修复及补充静态审视：

- `save_task` 在执行任何 SQL 前用 `i64::try_from(revision)` 校验，超界返回 `ToSqlConversionFailure`，没有截断转换。正数约束继续由原 schema 保留。
- Store 回归 `revision_preserves_sqlite_maximum_and_rejects_invalid_values_without_writes` 验证 `i64::MAX` 原值存储、0/`i64::MAX + 1`/`u64::MAX` 拒绝，以及失败 insert/upsert 不改变原 identity/revision/data、不生成 Loop 行。
- 领域回归 `revision_storage_limit_rejects_pause_without_changing_the_task` 验证边界 revision 的 Pause 返回错误，事务回滚后原任务与 Active 状态完整保留。
- 静态核对新增 config 初始化和 enum 匹配：PersistenceActorConfig 显式初始化继续使用 default/base update；RuntimeToolConfig 由既有构造器初始化新 hook；`activate_claim` 的 Option 返回与调用一致。核对实际 GPUI facade 和 AsyncAppContext 源码中的 Global API、compaction event 字段及 PersistenceActor 的调用签名。
- `PendingAgentRun::Automation` 使用 `Box<AutomationRunClaim>`，避免较大的 claim 报告/向量撑大整个枚举；同步更新调度构造、worker 和共享 run 快照提取，不增加 lint allow。其余分支保持完整匹配。

本轮只修改 Store 转换及测试、conversation 边界测试、app 枚举接线和本报告。所有原有断言保留，没有降低测试或扩大通用等待时间。新回归仍待下一次 CI，本地继续禁止执行项目代码、编译和测试。

本轮本地执行：`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 及只读源码/API 静态核对；格式和差异检查退出码均为 0。最终修复提交由主 agent 推送后再检查最新 head 的 Actions；本报告不声明 CI 已通过。

## 第二轮 CI 失败与修复

第二轮 [Actions 39047173888](https://github.com/puzige/vega/actions/runs/39047173888) 对 `c54a74cb2157a70a81ef218639c7681d9a9768a5` 执行。Clippy 和 Nextest 都在编译 `vega_conversation` 时退出 101；required check 退出 1。日志保留在 `/tmp/vega-341-ci-39047173888-failed.log`。三个 E0603 指向 `automation/runtime.rs`、`agent/entry.rs`、`agent/persistence.rs` 对私有 `vega_runtime::agent` 模块的访问，测试尚未执行。上一轮静态审视遗漏了完整的 crate-root 导出链，不能替代编译证据。

本轮先执行 `git fetch origin` 和 `git merge --ff-only origin/feat/341-loop-goal`（均退出 0，分支已同步），再修正：

- runtime 的 `agent/mod.rs` 只 re-export `RuntimeAutomationHook`，`lib.rs` 经已有 `pub use agent::{...}` 在 crate root 导出该 trait；`agent` 和 `automation` 内部模块保持私有。
- 三处生产引用和领域测试 trait import 全部使用 `vega_runtime::RuntimeAutomationHook`。现有 Binding、工具调用和测试实际依赖此根路径，不开放内部模块来消除错误。
- 静态检查发现 UI `automation_button` 原声明返回 `Stateful<Div>`，而 `InteractionExt::interaction_surface` 定义返回 `Interaction`。按既有 privacy button 的模式改为 `AnyElement` 并在完整交互链末尾转换，保留点击、键盘、焦点和无障碍属性。
- 主审指出菜单键盘测试原 `up up enter` 与生产 saturating 导航不符。当时改为 `down` 九次后 Enter，但误将 Loop 视为 index 9；第四轮 CI 证实该索引实际是 Goal，修正见文末。`/loop check`、Escape、草稿及菜单关闭断言均保留，没有改变生产导航。

本次静态核对的具体声明链：

| 使用边界 | 核对依据 |
|---|---|
| runtime hook 到 conversation | `vega_runtime/src/lib.rs` 的根导出 → 私有 `agent/mod.rs` 的 trait re-export → 私有 `agent/automation.rs` 中的 public trait；跨 crate 代码不再出现 `vega_runtime::agent::` |
| runtime 配置与测试 API | `RuntimeToolConfig::with_automation`、`replay_capability_digest` 在 `agent/mod.rs` 都声明为 `pub`；`RuntimeToolConfig` 已从 `lib.rs` 导出；compaction status/usage、`RuntimeTokenUsage`、`CredentialReader` 都在该根导出列表中 |
| conversation 领域与共享类型 | `vega_conversation/src/lib.rs` 声明 `pub mod automation`；`types/mod.rs` 对 automation types 使用 `pub use automation::*`；parser 从领域 mod re-export，app 所用领域操作逐项声明为 `pub` |
| Store 边界 | `vega_store/src/lib.rs` 声明 `pub mod automation`、re-export `Connection`；automation 存取方法为 `pub`。新增调用的 threads/token_usage/config/keystore/recovery API 在所属公开模块中公开 |
| UI 到 app | `vega_ui/src/lib.rs` 公开 `conversation_stream`；其 mod re-export `ConversationStream`、`AutomationRequested`。`automation_projection`、`apply_automation`、`automation_can_run`、`accept_automation_command` 均为 `pub`，内部 UI 状态仍是 `pub(crate)` |
| GPUI trait 与返回类型 | `automation_controller.rs` 显式导入 `BorrowAppContext`；window/mod 和 conversation_stream/mod 使用 GPUI prelude。缓存的 gpui-kit 0.6.1 源码 re-export `gpui::*`，gpui prelude 包含 BorrowAppContext、AppContext、Styled/Interactive/StatefulInteractive/ParentElement；AsyncAppContext 的 `update_global` 是 public 固有方法。仓库 `interaction.rs` 返回的是 `Interaction` 并实现上述元素 traits |
| 测试 helper 可见性 | app 的 automation 测试由 `tests/composer_actions.rs` 作为子模块挂载，因此可访问父模块私有 Fixture/fixture/edit/draft/click。UI automation 测试是 tests/mod 子模块，可访问父模块 StreamHarness/open_controller_stream。conversation agent/tests 可经 crate 内导出访问 `pub(crate)` 的 PersistenceActor/AutomationBinding；仅 hook trait 跨 crate 并使用公开根路径 |

本轮没有运行项目代码、编译、Clippy 或测试。`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 和新增源码静态扫描均完成，格式/差异检查退出码为 0。修复 amend 到第三个提交，不增加第四个提交；仍须由最新 head 的 CI 证明编译与回归结果。

## 第三轮已确认的 Clippy 失败与修复

[Actions 39047932082](https://github.com/puzige/vega/actions/runs/39047932082) 的 Clippy 对 `8057ff0fe53799957428339cf6661046703d3c6d` 报 E0282：`vega_ui/src/conversation_stream/automation.rs:155` 的 `cx.listener` 键盘回调无法推断 `event` 引用类型，lib 与 lib test 编译失败，退出 101。完整日志为 `/tmp/vega-341-ci-39047932082-clippy.log`。收到修复任务时 Nextest 仍在运行，本节不推断其测试结果。

先 fetch 并执行分支 ff-only 同步（均退出 0，已同步）。键盘回调显式使用 `&gpui_kit::KeyDownEvent`，同一按钮的鼠标点击回调也显式使用 `&gpui_kit::ClickEvent`。控件行为和现有 GPUI 断言不变。

核对依据：GPUI `app/context.rs` 中 `Context::listener<E>` 接受泛型事件引用；`elements/div.rs` 中 `on_key_down` 要求 `KeyDownEvent`，`on_click` 要求 `ClickEvent`。本卡新增键鼠 listener 只有该按钮这两处；其余新增 action handler 是有显式 action 参数的方法引用，app 与测试的订阅回调均已有 `&AutomationRequested` 参数。`Interaction` 实现 InteractiveElement 和 StatefulInteractiveElement，按钮保留 focus、role、tab-stop、点击与 Enter/Space 路径。

本轮仅格式、差异及源码检查：`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均退出 0；没有本地编译、运行或测试。继续 amend 第三个修复提交，最新 head 的 CI 结果仍待主 agent 核实。

## 第四轮已确认的 Clippy 失败与修复

[Actions 39048525678](https://github.com/puzige/vega/actions/runs/39048525678) 对 `91a77b93cbe9a4c1e264d371502d5a656e9ae0c2` 的类型检查推进至 `vega`。Clippy 因两条 lint 退出 101：`window/automation.rs:557` 的 `collapsible_if`，以及 `app_agent.rs:1622` 的 `items_after_test_module`。完整日志保留在 `/tmp/vega-341-ci-39048525678-clippy.log`。收到任务时 Nextest 仍在运行，主 agent 等待本轮测试结果，不用提前推送中断已有运行。

先 fetch 并 ff-only 同步分支（退出 0，已同步）。将 `Some(claim)`、当前 epoch 和 eligibility 条件合为同一 let-chain，保持短路顺序与 claim 清理行为；将 `automation_trigger` 移到 `reasoning_replay_tests` 前，函数内容不变，没有 lint allow 或断言修改。

同时静态检查本卡新增 inline test modules：`vega_store/src/automation.rs`、`vega/src/automation_controller.rs`、`vega_conversation/src/automation/parser.rs` 的 test module 均位于文件尾，没有后续生产 item；conversation 领域与 agent 的测试为外部 `mod tests;` 声明。本轮 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 退出码均为 0；未本地编译、运行或测试。继续 amend 第三个修复提交，测试结果待本轮 CI 完成及主 agent 核实。

## 第四轮 Nextest 完整结果与修复

同一 [Actions 39048525678](https://github.com/puzige/vega/actions/runs/39048525678) 的 Nextest 随后完成，命令为 `cargo nextest run --workspace`，退出码 **100**。2026-10-17 16:24:56 UTC 的原始汇总为 `2989 tests run: 2969 passed, 20 failed, 5 skipped`，执行时间 150.721 秒。完整首次日志保留在 `/tmp/vega-341-ci-39048525678-nextest.log`，可检索副本为 `.log.plain`。本节记录实际失败，不将已修代码或静态检查写作测试通过。

收到结果后先 `git fetch origin`（退出 0），按主 agent 指定在已 amend 的第三提交 `8acccb69` 上继续；远端功能分支仍是它替代的旧修复提交，因此保留本地提交，不合入重复历史。最终静态核对发现共享 `origin/master` 已到 `29863cc9`（PR #450），本 worktree 基线仍为 `3705c687`；新 master 未改 migration/Store lib。本轮先交回修复，由主 agent 接管新基线集成和相同工具/controller 接缝审查，不将未集成状态写作已同步。

| 失败组 | 已确认根因 | 本轮修正与保留的断言 |
|---|---|---|
| 10 个 app automation 测试 | composer fixture 调用严格的 `GitCommandFixture::for_current_test`，新测试完整名称未登记，全部在 root 0 初始化时 panic，尚未运行其 controller 断言 | 在 `controller-git-fixtures.json` 登记 10 个精确名称、11 个 root；`failed_pause...` 独立两次创建目录。复用同一 composer 文件状态的既有九条录制记录，包括初始化和只读 Git 输入/输出；phase、args、input、env、files 仍严格匹配，未知调用继续报错，没有 fallback，旧 fixture 值完全未改 |
| 4 个 conversation provider 测试 | `strict approval audit failed`：live proposal 不接受这三个工具的对象参数；`approval_source_matches` 未接其 ReadonlyTool 授权 | 新共享投影校验只接受 `get_goal`、`update_goal`、`schedule_wakeup` 的 32 KiB 内对象，get_goal 限空对象；live、replay、mixed batch 接同一约束。自动工具授权严格 Once/ReadonlyTool 且无 note/danger/remember，未知工具对象仍拒绝 |
| 同一工具执行链静态遗漏 | 成功结果原 `truncated=None` 不满足既有终态契约；执行 tuple 的 bool 实为 `cancelled_while_running`，原成功 true 会把正常结果误改为 Cancelled | 成功明确 `truncated=Some(false)`，正常成功/失败返回 false，执行前已取消返回 true。执行中取消仍由 loop 的真实 token 状态中断，暂存报告不能在取消终态应用；保留原成功、失败、用量、续跑和重开断言 |
| 3 个既有 schema 测试 | stream_persistence/todo_e2e 的完整表名列表未包含两张新表，s7_acceptance_e2e 版本仍是 25（其后完整列表同样遗漏） | 三处完整列表精确更新为排序后的 38 表，s7 版本精确更新为 26；不改为包含判断，不移除任何外键或生命周期断言 |
| 新 UI 键盘测试 | `PERMISSION_ORDER` 有四项，先前索引估算遗漏 FullAccess，所以九次 Down 选择 Goal | 测试先确认所有 12 个实际 selector 的可见与垂直顺序，再用实际 Down/Enter 到 Loop；保留 `/loop check`、菜单关闭、Escape 和草稿断言，不暴露生产私有方法给测试 |
| 2 个既有 CUA 菜单稳定性测试 | Goal/Loop 原插在 Images 与 Takeover 之间，改变旧键盘 index 和关闭再打开后的相邻行几何；打开期间的冻结行检查已通过 | Goal/Loop 追加在动态 Takeover 与 CredentialReview 后，旧行相对顺序/index 与冻结 `menu_rows` 保留。`cua_menu_stability.rs` 内容和精确 bounds/highlight 断言没有修改 |

新增 `automation_tool_audit_persists_only_valid_proposals_approvals_and_terminals` 直接调用生产 `persist_runtime_event`，覆盖三个工具的 Proposed→Approved→Running→Finished→reused；malformed/non-object/oversized 输入与未知对象拒绝且不留行；未授权 Running、跳过 Running 的成功、伪造 User/Auto/Always/Deny/note 授权、缺 truncation、非法输出或进程 metadata 均保持原持久状态。额外覆盖执行前合法 Timeout/Deny 拒绝及伪造来源/输出不被接受。

新增 `malformed_automation_inputs_are_redacted_rejected_and_reopen_without_authority` 经真实 provider/runtime/PersistenceActor 入口，验证非法 JSON、非对象、超长、get_goal 多余字段及未知工具统一保留安全拒绝历史，不产生 Approved/Running，重开后能继续正常回合。原 Goal 成功工具测试增加重开运行并确认已完成任务用量仍为 36；原失败 wakeup 重开断言保留。它们补充矩阵 G4/G7/L3/S4/S6/R2 的生产接线覆盖，仍待 CI 执行。

本轮本地仅静态操作：`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 退出码均为 0；只读 Python 检查确认 10 个新 fixture 名称/11 个 root 与原录制值一致、所有旧 fixture 不变、三处 schema vectors 都精确含 38 个排序表名。新增源码差异扫描未发现代码注释、lint allow、Issue 编号测试名、依赖或 workflow 修改。没有本地运行 Vega、编译、测试、扩大全局等待或取消任何断言。全部修复继续 amend 第三个提交，由主 agent 推送并等待最新 head 的 Clippy/Nextest；本轮实现无已知规格偏离。

## 集成 AskUserQuestion 新基线

主 agent 随后授权由唯一实现 agent 完成新基线集成。先 fetch（退出 0），确认 `origin/master` 为 `29863cc94339cb01d22b322bb71f700a3295824c`（PR #450），远端本任务分支仍为 `91a77b93`；第四轮修复已在本地第三提交 `54f4fba`。随后 rebase 到该 master，最终保留本卡三个提交，不引入重复远端历史或第四个提交。

冲突位于 conversation 的 `agent/persistence.rs`、`types/mod.rs` 及 runtime 的 `agent/mod.rs`、`agent/tools_exec.rs`。解决时保留两侧的配置字段、builder、公开类型、工具准备与执行分支：自动回合沿用普通回合的 UserQuestionQueue，同时保留 automation claim/hook。master 的整批问答隔离和 `USER_QUESTION_BATCH_REJECTED` 审计规则与自动工具的限定输入投影共存。

新增生产条件仅在 `automation_can_run` 中排除 `has_pending_user_question()`；既有跨窗口 stream guard 因而同时覆盖队列待答和答案已提交但工具终态尚未落地的面板。Steer、排队提交与 Stop 的 master 问答生命周期保持完整。补充回归如下，均待 CI：

- UI eligibility 回归经过真实问答队列、点击选项、Enter 和工具终态，验证等待及提交后禁止自动运行，终态落地或取消后恢复准入。
- 现有跨窗口 controller 回归增加另一窗口待答时不派发、取消问题后恢复；现有 Loop Stop 回归让实际自动 worker 调用 ask_user_question，Stop 后确认持久工具为 Cancelled、任务 Paused、没有多余 provider 请求，显式 Resume 才再执行。沿用原 fixture 名称，仍为 10 个名称、11 个 root。
- 新 provider 回归 `question_batch_rejects_automation_report_before_execution_and_replays` 同批返回 update_goal 与 ask_user_question，验证两个工具都在执行前拒绝、没有 Approved/Running、Goal 不被完成、用量准确，并能重开继续。这补充 S1/S2/S6/R2 的生产入口覆盖。

集成后 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均退出 0。只读静态检查确认 merge base 为 `29863cc9`、本卡提交数为 3、冲突标记清零、原 Git fixture 值不变、现有问答取消和调度 guard 接线保留。未执行本地编译或测试；最新实际 CI 证据仍为第四轮旧 head 的 20 项失败，本次修复与集成尚待主 agent 推送后的最新 head 验证。

## 第五轮编译失败与修复

[Actions 39050935032](https://github.com/puzige/vega/actions/runs/39050935032) 对 `4cce62130614f0fafb415ce0fbbc199873c0ad16` 的 Clippy 和 Nextest 都因同一个 E0277 编译失败：`agent/tests/automation.rs:370` 使用 `row.get::<_, usize>(0)`，rusqlite 0.40.2 没有实现 `usize: FromSql`。Nextest 尚未执行测试。日志为 `/tmp/vega-341-ci-39050935032-clippy.plain` 与 `-nextest.plain`。

先 fetch 并 ff-only 同步任务分支（均退出 0，已同步；master 仍为 `29863cc9`）。仅将该 SQL COUNT 读取改为 `i64`，期望值改用 `i64::try_from(calls.len()).unwrap()`，保留精确相等断言。静态扫描本卡变更的 Rust 文件并核对新 SQL 的推断类型：其余新计数读取为 `i64`，状态/JSON 为 String，存在性为 bool；唯一新 `u64` SQL 写入 revision 已在绑定前 checked conversion 为 i64。未发现其他新增的 SQL usize/u64 直接读写。

本轮 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 及静态检查均退出 0；未本地编译或测试，未扩展测试范围或修改其他断言。修复继续 amend 第三个提交，等待主 agent 推送后的最新 head CI。

## 第六轮 CI 结果与三项回归修复

[Actions 39051474433](https://github.com/puzige/vega/actions/runs/39051474433) 对 `d47be18445c0552d2ea0765048f7ae7d43ea9ba6` 的 Clippy 已成功。Nextest 于 2026-10-17 17:04:19 UTC 汇总 `3010 tests run: 3007 passed, 3 failed, 5 skipped`，执行时间 188.094 秒，日志保留为 `/tmp/vega-341-ci-39051474433-nextest.plain`。先 fetch 并 ff-only 同步（均退出 0，已同步；master 仍为 `29863cc9`），再按首次失败证据修复。

- Goal 续跑测试在 `build_snapshot` 的 cached raw diff 处缺严格 Git 录制，后续锁 poison 是同一 panic 的结果。静态核对完整 snapshot/verify 链后，给本卡 10 个 fixture 名称的 11 个 root 各补 cached/worktree 的 raw/numstat 四条精确只读记录。输出复用既有 `diff_controller_real_finish_drops_superseded_result_and_global_switch_closes_route` 的同 tracked.rs 内容及 index 状态，文件匹配额外包含真实 `context.txt`。保留每个原始九条记录和所有其他 case；参数、phase、stdin、env、files 匹配保持严格，没有 fallback。
- 问答 Stop 测试的完整 backtrace 指向真实 `vega-agent` 线程经 UserQuestionQueue 的 watch sender 唤醒 GPUI 前台 listener。按既有 app 问答和真实 worker 测试的边界，仅该测试调用 `cx.executor().allow_parking()`；不改 scheduler 或通用 pump，不模拟掉 worker，原 Stop/Resume、持久 Cancelled、问题清理和 provider 请求次数断言保留。
- 归档测试只恢复数据库，Finished 已把 Archived 投影到 stream，Active 准入 guard 因而继续阻止运行。进一步静态检查发现自动任务保留的 stream 在真实重新打开时同样只同步标题。新增同 id/project 的 status-only 投影，所有 VegaWindow 的 OpenedThread observer 都更新各自 retained automation stream，不覆盖模型或 pending owner，不放宽 Archived guard。原回归改经 visit_thread→OpenedThread→刷新窗口，增加另一个窗口持有的 Archived stream；确认两边准入恢复、仍为同一主 stream、任务仍 Paused/provider 仍 1，再显式 Resume，并保留删除后取消及 ledger 清理断言。

本轮 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 和只读静态检查均退出 0。静态确认 fixture 原记录不变、四种 snapshot 输入输出精确、全局 observer 覆盖每个窗口的 retained map，新增 status API 可见性及 ThreadStatus 的 Copy 类型成立。没有本地执行编译或测试，没有扩大 pump 上限或减少断言；三项修复继续 amend 第三个提交，等待最新 head CI 验证。

## 第七轮 CI 结果与等待条件修复

[Actions 39052915348](https://github.com/puzige/vega/actions/runs/39052915348) 对 `a1f8a887fd758ba58a2a1910a0dac47776d0b87e` 的 Clippy 成功，Nextest 汇总 `3010 tests run: 3009 passed, 1 failed, 5 skipped`，执行时间 262.782 秒。归档恢复和问答 Stop 回归已通过；唯一失败为 Goal 续跑测试在第二个 pump 超时，日志 `/tmp/vega-341-ci-39052915348-nextest.plain` 没有 fixture 或 scheduler panic。原日志未输出最终 task 状态，因此不能据此宣称其已 Complete。

静态核对确认等待条件必然为假：predicate 首先 `refresh`→`bind_automation`→`poll_automation_thread`，后者不是发现已有 pending 就返回，就是同步插入 pending 后才派发后台 worker；同一 predicate 中无 yield，紧接着 `idle` 必定为 false。修复把 now_override 设置后的单次 refresh 移到 pump 外，predicate 仅读取 Complete 与 idle，保持真实调度及既有时钟边界。使用原 `pump_test_app_with_diagnostics` 输出失败时的状态/token/next_due、provider 请求数、活动 run、polling、mutation 和 claim，不更改通用 pump。保留精确 4 次请求、48 tokens、完整 usage、无 claim/共享 run 和不再派发的断言，并显式重申最终 Complete。

本轮先 fetch/ff-only 同步（退出 0，已同步；master 仍为 `29863cc9`）。仅修改该测试及本记录；`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 和静态检查均退出 0，没有本地编译、运行或测试，没有延长等待或降低断言。继续 amend 第三个提交，最新修复仍待 CI。
