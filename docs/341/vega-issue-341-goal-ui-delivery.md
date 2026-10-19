# Goal / Loop 状态条与编辑面板交付

日期：2026-10-18。规格：`docs/341/vega-issue-341-goal-ui.md` v1.2。原始基线：`6382c908`，最新集成基线：`a91b8be8`；rebase 后规格提交：`7478e41b`、`5ef92665`（原 `7f3be373`、`72510963`）。实现分支：`feat/341-goal-ui`。

## 交付状态

本轮规格范围已实现并补充自动回归；本地仅完成格式、差异和源代码/fixture 静态核对。未运行 Vega、编译、Clippy、测试、nextest、构建或打包。下列测试全部待最新 PR head 的 Actions 执行，浅深主题、真实键盘与像素外观待用户手测。本报告不将已编写用例视为通过。

## 行为与接线

- Goal / Loop 使用紧贴 composer 的单行状态条，展示真实指令摘要、状态、时间与独立图标动作。排队、提问和必要错误位于条组上方。普通暂停不重复展示“已由你暂停”；普通用量移入编辑面板。
- 宽态采用规格中的 36/16/12/28 几何 token。composer 小于 360 时使用 8/6 内收和 padding、短状态，时间移入详情；固定动作与类型图标保留。Tab 顺序与清除、暂停/恢复、编辑的可见顺序相同。
- 右侧 Workspace 提供原生多行编辑、保存/Cmd-S、还原、冲突后的载入最新、Esc 回 composer。草稿按窗口内 thread/kind/identity 缓存；关闭、隐藏和切换会话不自动保存。新身份显式载入会关闭旧 tab，旧缓存不写回新任务。窄窗临时展开会恢复先前的 Workspace 最大化状态。
- 输入区使用实际分配的 TextElement 高度限制可见行并跟随光标；输入和只读详情分别滚动，工具栏固定。普通 TextInput 保持原有最多 8 行行为。编辑器空白/过长、失败与冲突在相关区域显示，保存主按钮具有明确主次层级。
- Edit 在原有事务中校验 Active/native owner、identity、revision、非空和 UTF-8 字节上限，受检查的 revision 加一仅更新 instruction/updated_at。保留最新持久状态、计量、预算、间隔与有效期；相同文本不更新版本。Create 与 Edit 复用 provider/MCP 凭据扫描。
- 保存使用现有 mutation lane/epoch，成功后只取消该项旧 revision 所拥有的自动 run。失败不取消现行 run；用户独立 run 和另一 kind 所拥有的 run 继续。终态用量按 identity 幂等结算，旧 revision 报告不能改写新内容或状态。
- 只有当前可见的脏编辑器阻止新自动回合，已在执行的回合可结算；通过现有全窗口 stream readiness 生效。关闭、隐藏、切换 tab/路由和被另一 dock 最大化遮挡均释放该窗口的编辑阻挡。
- 临时耗时来自已接收的 RunStarted Instant，并校验共享 owner/generation 与未结算 claim。worker 在同一事务中读取任务累计与 active ledger；结算后不再叠加时钟。过期未结算快照不覆盖已显示耗时。时间标签使用独立实体更新，不按秒通知整段对话历史。Loop 等待不累计执行时间，也不把未归属的 0 tokens 显示为 Loop 用量。

## 修改文件

| 范围 | 文件 |
|---|---|
| 领域命令与一致读取 | `crates/vega_conversation/src/types/automation.rs`、`crates/vega_conversation/src/automation/mod.rs`、`crates/vega_conversation/src/automation/tests.rs` |
| 原生编辑器 | `crates/vega_ui/src/automation_editor.rs`、`crates/vega_ui/src/automation_editor/tests.rs`、`crates/vega_ui/src/lib.rs` |
| 输入视口窄接缝 | `crates/vega_ui/src/text_input/mod.rs`、`crates/vega_ui/src/text_input/state.rs`、`crates/vega_ui/src/text_input/element.rs` |
| 状态条与主题 | `crates/vega_ui/src/conversation_stream/automation.rs`、`core.rs`、`mod.rs`、`render.rs`、`tests/automation.rs`；`crates/vega_ui/src/icons.rs`；`crates/vega_theme/src/lib.rs` |
| 保存、Workspace 与运行时钟 | `crates/vega/src/automation_controller.rs`；`crates/vega/src/window/automation_editor.rs`、`automation.rs`、`agent.rs`、`artifact.rs`、`mod.rs`、`render.rs`、`workspace.rs` |
| 应用回归与严格 fixture | `crates/vega/src/tests/automation.rs`、`crates/vega/src/tests/controller-git-fixtures.json` |
| 交付记录 | 本文件 |

无依赖、migration、CI、用户配置或无关运行时变更。原有菜单顺序、预算/重启/归档/问答/Stop 等断言保留；原 inline 详情断言按新规格替换为打开编辑器事件断言。

## 验收矩阵与测试层级

以下均为已编写、待 CI；涉及真实 UI 外观的项目另外待用户手测。

| 编号 | 自动回归及证据入口 | 层级与边界 |
|---|---|---|
| V1 | `compact_strips_join_composer_keep_actions_on_one_line_and_match_tab_order` 断言无任务/外部 backend 条组高度 0；保留 `automation_menu_preserves_draft_keyboard_selection_and_escape` 和 Codex 显式错误用例 | GPUI 实际布局/键盘；既有菜单索引断言不变 |
| V2 | 上述 compact 用例验证双条 36 高、内收、与 composer 连续、动作 28 同行；`narrow_workspace_strip_keeps_every_action_inside_its_bounds` 在实际 295 宽窗口验证 composer 231、strip 215、摘要至少 33 和所有动作在边界内 | GPUI bounds；长中文、URL、多行输入；浅深像素外观待手测 |
| V3 | `strip_status_matrix_and_clock_updates_leave_history_quiet` 的 7 状态矩阵；保留未知计量不可恢复和预算焦点断言 | GPUI 真实投影/控件；必要错误继续可见 |
| V4 | `timing_projection_rejects_stale_run_and_settlement_never_adds_the_clock_twice`、`timing_snapshot_excludes_settled_claims_and_keeps_unknown_late_usage_enforced`；GPUI clock 更新不 notify history；运行中编辑 app 用例检查结算时间投影 | 一致数据库读取 + 共享控制器投影纯函数 + GPUI + 真实 app worker/mock provider；无 clock 不推算、旧快照不归零、结算不双计 |
| V5 | 原状态动作用例扩展 Space 编辑事件；compact 用例实际 Tab/Shift-Tab；app 保存用例点击展开、Esc 返回真实 composer 焦点、窄窗展开/拉宽恢复 | GPUI 事件/焦点 + 生产 Workspace；原生手测待完成 |
| E1 | `editing_preserves_every_status_and_latest_accounting_without_resuming`；`multiline_keyboard_save_and_late_callback_preserve_newer_draft`；`invalid_editor_content_is_visible_and_toolbar_survives_small_pane_scroll`；两个 app 保存用例覆盖 Goal/Loop | 领域事务 + 原生 TextInput/Enter/Cmd-S；272 高 pane 的实际 painted caret/viewport 可见，滚轮后继续输入，拉高后至少 12 行 |
| E2 | `editing_running_goal_cancels_only_after_commit_and_next_run_uses_new_instruction`；`editing_attached_goal_does_not_cancel_user_or_loop_owned_run`；旧报告事务回归 | 第一个实际走 controller → native worker → mock provider → persistence，断言失败不取消、成功取消、新上下文与计量；第二个用合成已启动 claim 注册共享 owner，走生产保存/poll 验证 source=None/Loop 不被 Goal 编辑取消 |
| E3 | `editing_preserves_every_status_and_latest_accounting_without_resuming` 逐项覆盖两 kind、全部 7 状态及完整 Task 等值；`edit_uses_latest_terminal_status_and_late_usage_stays_with_identity_once` 覆盖保存前已完成 | 领域事务；Paused 的真实 app Save 也断言不派发、不恢复，Loop 间隔和 expiry 保持 |
| E4 | `edits_compare_identity_revision_and_reject_invalid_text_without_writes`、`editor_conflicts_failures_and_replaced_identity_never_discard_input`、`visible_peer_editor_blocks_dispatch_and_conflict_reload_resumes_eligibility` | CAS 事务 + editor + 双 VegaWindow 生产保存；显式新身份载入后旧 tab 不再渲染，新 tab 与缓存正确 |
| E5 | `edits_are_atomic_on_storage_failure_and_version_limit`；运行中编辑用例用 owned SQL trigger 制造保存失败；`workspace_editor_saves_paused_goal_retains_drafts_and_rejects_credentials` 拒绝 provider/MCP 凭据并保留输入 | 数据库原子失败 + 生产异步保存、凭据扫描；无真实外部 provider/MCP 请求 |
| E6 | `multiline_keyboard_save_and_late_callback_preserve_newer_draft` 对提交后继续输入、重复/迟到回调保持脏草稿；app 保存用例覆盖关闭重开、切换会话、保存后立即导航 | editor submission fence + 生产 Workspace/OpenedThread/异步 callback |
| E7 | `visible_peer_editor_blocks_dispatch_and_conflict_reload_resumes_eligibility` 验证另一窗口脏编辑器阻止 provider，载入最新后恢复；app 保存用例验证 Esc/导航释放 gate；运行中编辑用例验证单纯修改不取消 | 双窗口生产 streams_ready + 真实 native worker/mock provider；共享窗口 owner 断言保留 |
| E8 | `edit_uses_latest_terminal_status_and_late_usage_stays_with_identity_once` 与 timing snapshot 用例断言旧 revision 的迟到 usage 只计一次、超预算/未知停止续跑 | 领域真实事务/ledger；不复制计量体系 |
| R1 | 保留原有 composer actions、cua_menu_stability、pending messages、user questions、Workspace 文件/图片/Review/终端及自动运行回归；本轮新增布局和显式编辑 tab 分支 | 全部由既有 Actions workspace Clippy/Nextest 执行，未本地运行 |

新增 17 个测试：5 个 conversation 事务、4 个 editor（含 1 个耗时格式纯函数）、3 个 strip GPUI、4 个 app、1 个共享时钟控制器测试；另外扩展既有 UI 交互断言。Git fixture 仅增加 4 个精确测试键，复用同一 owned 仓库状态的完整只读命令录制；所有既有键值静态逐项相等，未放宽 strict 匹配。

## 本地静态检查

| 命令/检查 | 结果 |
|---|---|
| `cargo fmt --all` | 退出 0；仅格式化 |
| `cargo fmt --all -- --check` | 退出 0 |
| `git diff --check` | 退出 0 |
| Python 标准库解析 `controller-git-fixtures.json`，与 `git show HEAD:<path>` 比较全部既有项，核对 4 个新增键对应真实测试函数 | 退出 0；所有既有项相等，精确新增 4 项 |
| `rg`/源代码静态核对新增 enum 初始化、跨 crate public 导出、GPUI receiver/回调类型、workspace 分支、SQL 类型、代码注释及生产 unwrap/expect | 已完成静态审阅；无本轮新增生产 unwrap/expect 或代码注释；不替代编译结果 |

## 待验证、风险与偏离

- Actions 第四轮 Nextest 3053 个测试全部通过、5 skipped，包括真实窄条、窄窗 Workspace 和小 pane 光标回归；Clippy 仅因两处测试绑定 `unused_mut` 失败。两行修复已准备，仍须最新 PR head 的 Clippy/Nextest 与 required check 全部通过。
- 未运行本机 Vega，未生成实际客户端截图。主题反差、中文字体实际宽度、极窄 Workspace、滚轮/IME/原生键盘最终体验待用户手测；GPUI 用例仅提供进程内布局与交互证据。
- 运行时钟在 RunStarted 事件抵达共享 controller 后显示，并使用已有轮询频率；等待或无该证据时只显示持久值。该保守表现符合规格。
- 规格偏离：无。原始预算、用户优先、跨重启手动恢复与权限边界不变；本轮只扩展状态条、编辑器及必要保存/时钟接缝。

交回主 agent 审查、推送同分支、创建 PR 并等待 Actions。实现 agent 不自行 push、发 PR、改看板或合并。

## PR #458 首轮 CI 修复

2026-10-18，run `39084447894`，head `7ff5f02fe5e2750de88522ba3328475e9630a342`：Clippy 和 Nextest 均在 `vega_ui` lib test 编译阶段失败，退出 101；没有运行测试。错误为 `automation_editor/tests.rs:70` 的 `recursion limit reached while expanding #[test]`。完整日志保留在 `/tmp/vega-341-ui-ci-39084447894.log`，去控制符版本为 `/tmp/vega-341-ui-ci-39084447894.plain.log`。

根因是父模块的 `use gpui_kit::*` 经测试模块的 `use super::*` 引入同名 `test` 属性宏，使 GPUI 测试宏展开生成的 `#[test]` 再次解析成 GPUI 宏。修复沿用 `command_palette.rs` 的既有写法，在编辑器 tests 显式 `use core::prelude::v1::test`；不提高递归上限，不改测试体或断言。静态核对本轮涉及的 conversation stream、TextInput、app 和控制器测试导入链，没有新增同类未限定名称污染。

修复仅 amend 实现提交，基线与两笔规格提交不变。再次执行 `cargo fmt --all -- --check`、`git diff --check`，均退出 0；未本地编译或执行测试。修复效果待下一轮 Actions。

## PR #458 第二轮 CI 修复

2026-10-18，run `39084872854`，head `308ee58583ccc910980185ca6fe3d081066802dd`：宏递归错误已排除。Clippy 与 Nextest 均因 `conversation_stream/tests/automation.rs:455/574/607` 的三处 E0425 停在编译阶段：新增布局断言使用 `size(...)`，但测试模块和父模块均未导入该函数；尚无运行断言结果。完整日志为 `/tmp/vega-341-ui-ci-39084872854.log`，去控制符版本为 `/tmp/vega-341-ui-ci-39084872854.plain.log`。

最小修复为在该测试模块的既有具名 GPUI 导入中添加 `size`。静态检查新增测试中的 `size`、`point`、`px` 和项目 helper：编辑器测试从父模块获得 GPUI 函数；状态条测试的 `px` 来自 stream 父模块、`size` 现已显式导入；app 的新增几何调用使用完整 `gpui_kit::` 路径，项目 helper 均在本模块或既有父测试模块定义。没有修改测试断言或生产行为。

再次执行 `cargo fmt --all -- --check`、`git diff --check`，均退出 0；仅 amend 最后一笔实现提交，总计仍为三笔。未本地编译、运行测试或 push；等待下一轮 Actions。

## PR #458 第三轮 CI 与尺寸事件修复

2026-10-18，run `39085299344`，head `0485fde1fce794d0ef00e4d6f88d14a0c641bf2f`：Clippy 成功；Nextest 执行 3031 个测试，3030 passed、1 failed、5 skipped，退出 100。本轮新增 17 个用例中，其余 16 个通过，唯一失败为 `narrow_workspace_strip_keeps_every_action_inside_its_bounds:597`：composer shell 实测宽 768，期望 231。总数已含 master PR #456 的 4 个新增测试。完整日志为 `/tmp/vega-341-ui-ci-39085299344.log`，去控制符版本为 `/tmp/vega-341-ui-ci-39085299344.plain.log`。

静态读取 GPUI `gpui-pre-0.3.4` 源码确认：测试平台的 `Window::resize` 仅改 bounds，没有触发 resize callback，故新测试并未改变实际 GPUI viewport。改用 `VisualTestContext::simulate_resize` 并 pump，额外断言真实 viewport 为 295×900；原有 composer 231、strip 215、摘要至少 33、动作 28 与边界断言全部保留。已通过的编辑器 272 高 pane 滚轮/光标用例原本使用正确事件，无修改。

同步修正 app 编辑器用例中的 800/1200 两次 resize。800 宽会触发默认 sidebar 自动收起，因而测试通过已有 `SidebarExplicitlyShown(true)` 表达用户明确显示侧栏的场景，再发送真实 800×600 resize，断言 viewport、主内容宽小于 610、编辑器与主内容等宽、Save 位于边界内，且 docked pane/composer 不渲染。关闭后发送 1200×900 resize，再打开编辑器，断言面板恢复原宽度、窄于主内容区且 composer 恢复。保存、草稿、Esc 焦点与无自动派发断言均保留；之前假 resize 的通过不作为真实响应式布局证据。

修改前 fetch 并将三笔提交干净 rebase 到 `origin/master`，实际新基线为 `a91b8be8`（包含 `2fe39e63` / PR #456，以及更新的 PR #457）；无冲突，无生产行为修改。再次执行 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check`，均退出 0。静态确认本轮新增测试不再调用不触发 viewport 事件的 `window.resize`。仅 amend 实现提交，仍为两笔规格加一笔实现；未本地编译、测试或 push，修复后结果待 Actions。

## PR #458 第四轮 CI 与最后 lint 修复

2026-10-18，run `39086229728`，head `ddaa31c686f581bd753f35cd383d5b1bb7609a62`：Nextest 完整成功，`Summary [176.699s] 3053 tests run: 3053 passed, 5 skipped`。窄条实际尺寸用例通过（0.055s），生产 Workspace 真实窄窗/恢复用例通过（2.235s），编辑器小 pane 滚轮与光标可见用例通过（0.229s）。完整日志为 `/tmp/vega-341-ui-ci-39086229728-nextest.log`，去控制符版本为 `/tmp/vega-341-ui-ci-39086229728-nextest.plain.log`。

Clippy 唯一阻塞为两处 `unused_mut`：状态条窄窗测试的 resize-only `visual`，以及 app 恢复宽窗的 resize-only `visual`。两者仅调用接收 `&self` 的 `simulate_resize`/`run_until_parked`；删除这两个 `mut`，保留其他使用 `debug_bounds(&mut self)` 的可变绑定。Clippy 日志为 `/tmp/vega-341-ui-ci-39086229728-clippy.log` 和 `.plain.log`。没有修改生产代码、测试断言或 lint 配置。

本轮先完成两行静态修复，等待上述 Nextest 完整结束后再统一 amend 实现提交。`cargo fmt --all -- --check`、`git diff --check` 均退出 0；未本地编译、测试或 push。基线保持 `a91b8be8`，仍为两笔规格加一笔实现。最后 lint 修复后的完整 required check 待主 agent 推送最新 head 后确认；用户原生手测仍待完成。
