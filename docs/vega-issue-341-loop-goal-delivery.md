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
| Conversation 运行接线 | `crates/vega_conversation/src/agent/entry.rs`；`persistence.rs`；`pipeline.rs`；`tests/automation.rs`；`tests/mod.rs` |
| Runtime | `crates/vega_runtime/src/agent/automation.rs`；`loop_.rs`；`mcp_registry.rs`；`mod.rs`；`tools_exec.rs` |
| App | `crates/vega/src/automation_controller.rs`；`app_agent.rs`；`main.rs`；`window/automation.rs`；`window/agent.rs`；`window/mod.rs`；`window/pending_messages.rs`；`window/render.rs`；`tests/automation.rs`；`tests/composer_actions.rs` |
| UI | `crates/vega_ui/src/conversation_stream/automation.rs`；`composer_actions.rs`；`core.rs`；`mod.rs`；`render.rs`；`tests/automation.rs`；`tests/mod.rs` |
| 文档 | `docs/vega-issue-341-loop-goal.md`；`docs/vega-issue-341-loop-goal-delivery.md` |

未改依赖、Cargo 配置、CI 工作流、用户配置、既有 provider 实现或其他卡片文件；未新增代码注释、Issue 编号测试名。现有 schema 精确断言保留并由 36 张更新为 38 张，包括 `permissions.rs` 的独立断言。

## 验收矩阵与证据层级

下表是**已编写但尚未执行**的回归映射。D 为 conversation 领域/SQLite 测试，不经过 provider/controller；P 为真实 conversation/runtime/PersistenceActor 入口加 MockProvider（个别标明仅 actor 或 fingerprint）；A 为真实 VegaWindow/composer/生产 worker/controller 加 MockProvider；U 为 ConversationStream 的 GPUI 控件/键盘测试；C 为共享 owner/epoch 的纯 controller helper；M 为保留待用户手测。

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
| R1 | store 四处 schema 精确断言；两表 FK；重复启动恢复和 open/migrate 不暂停；原子 draft 创建回滚 | Store、D；待 CI |
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

- 代码实现范围已完成；类型检查、Clippy、自动测试、真实 provider/系统 UI 集成尚未执行，不能将本报告当成验收通过证据。
- `0026` 是当前基线的下一个迁移号；若并行 PR 先占用，主 agent 集成时需重编号并保持现有 schema 断言精确。
- 崩溃和已开始但未能正常结算的回合保守记录 usage 未知；有预算的目标需要清除后新建。这是防止未核实用量绕过预算的既定行为。
- token budget 是新回合准入边界，已提交 provider 请求可以超过上限；没有硬计费限额承诺。
- 自动调度只存在于运行中的 Vega 原生应用，退出后不执行；重启后需手动恢复。没有系统后台服务或通知通道扩展。
- 无已知规格偏离；无有意省略的产品实现项。CI 与上述 M 项仍待实际验证。

按任务要求仅交回本 worktree 和实现提交，不 push、不建 PR、不改看板、不合并；交付后停止写入，等待主 agent 审查。
