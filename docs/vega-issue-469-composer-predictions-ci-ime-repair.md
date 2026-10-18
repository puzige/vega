# Composer IME 命令回退修复

Purpose: 修复第四轮 CI 剩余的 IME 控制字符回退。
Type: REVIEW_FIX
Status: DONE
Handoff initiator: Codex
Recipient: composer_predictions_impl
Primary owner: Codex
Reviewer: Codex
Decision entry: [规格 v1.5](vega-issue-469-composer-predictions.md)
Date: 2026-10-18
Time base: Asia/Shanghai

## 10-Minute Take-Over Package

只修应用 action 层错误的 propagate。禁止改输入提交回调来滤掉 Enter/Tab，禁止放宽中文/UTF-16/不发送断言。禁止本地运行代码、编译或测试；只做 diff/格式静态检查。返回未提交 diff 后停止，不 commit/push/merge。

## Current Status

- Worktree：`~/Workspace/worktrees/vega-469-composer-predictions`；分支 `feat/469-composer-predictions`。
- HEAD `b75c0eb5`；2026-10-18 Codex 已 fetch，`origin/master` 仍为 `a608ffa2`，是 HEAD 祖先。
- [PR #472](https://github.com/puzige/vega/pull/472) OPEN，auto merge 为空，用户要求本轮不合并。
- [第四轮 Actions](https://github.com/puzige/vega/actions/runs/39120769717)：fmt/Clippy success；Nextest 1649 个运行，1648 passed、1 failed、3 个既有 skipped。
- 唯一失败：`prediction_ime_and_utf16_queries_never_expose_or_accept_ghost` 第154行，Enter 后左值为换行，右值为原 `中😀文`。
- 脱色日志：`/tmp/vega-469-ci.dhEpAC/fourth-nextest-plain.log`。

## Timeline

- 2026-10-18：上一轮的 SSE、权限/提问和身份/阻塞回归已通过。
- 2026-10-18：Codex 读取锁定版本 GPUI 与 macOS 平台源，定位 Enter 的真实回退路径。

## Scope

1. `conversation_stream/composer.rs::on_send_action`：Composer 输入框聚焦且 composing 时消费 action，不调用 propagate，不提交。非 Composer 输入焦点仍保留 propagate。可将焦点检查移到 composing 判断之前，以保持不属于输入框的动作传播。
2. `text_input/state.rs::insert_newline`：composing 时消费 action 并返回，不 propagate。其它换行行为不变。
3. `conversation_stream/composer_actions.rs::move_composer_focus`：composing 时消费 action 并返回，不 propagate。非合成态 Tab/Shift+Tab 遍历保持。
4. `conversation_stream/tests/composer_predictions.rs`：保留模拟 Enter/Shift+Enter/Tab 后文本、marked/selection、不提交的全部断言，补充 Cmd+Enter、Shift+Tab 及输入焦点未移动；仍在 marked 状态时通过原 `replace_text_in_range(None, committed_text, ...)` 提交文本，确认整段 marked 被替换、composition 清除、真实提交可写入草稿且不发送，随后保留 unmark 与旧预测拒绝断言，不为变绿换成低层键事件。
5. 更新原 delivery 的第四轮失败及修复记录。不要改其它代码、spec、handoff、README、依赖或 CI。

## Non-Scope

- 不改 `EntityInputHandler` 的平台提交语义，不丢弃合法提交的换行/Tab，不加键盘全局拦截或自定义输入法。
- 不改其它 IME guard，不删除/ignore 测试、不改 timeout/重试或安全断言。
- 不本地运行 Vega/编译/Clippy/测试，不 push、开 PR、merge 或操作真实应用/配置。

## Key Entry Points

- 锁定 `gpui-pre 0.3.4`：`src/window.rs::dispatch_keystroke` 第5169行：先 `with_simulated_ime()`，action 仍 propagate 时用 `key_char` 调 `dispatch_input`。
- `platform/keystroke.rs::with_simulated_ime` 第241行将 Tab 映射为制表符、Enter 映射为换行。
- `gpui-pre-macos 0.3.4` 的 `src/window.rs` 第2446–2507行：有 marked range 时先 `inputContext.handleEvent`；输入法不能处理才向 GPUI 派发。第3150行 `do_command_by_selector` 同样是 IME 回调后的 action 分派，不是输入法入口。
- `on_send_action` 的 composing propagate 是本轮第一步的直接原因；上一轮只 guard `insert_newline` 且继续 propagate，无法阻止 `dispatch_input`。

## Acceptance

- 保留 GPUI 高层按键模拟、原中文/emoji、UTF-16、无自动提交断言；新增按键和真实提交回归。
- `git diff --check` 与 `cargo fmt --all -- --check` exit 0；自动行为只由 Actions 执行。
- 交回文件清单、静态检查结果、未运行项和风险后停止写入；Codex review、amend、push 后等待最新 head 完整 CI。

## Risks and Human Decisions

- 本轮只准备 PR，保持开放，不自动合并 master。
- macOS 输入法先处理已由锁定源码验证；真实系统输入法体验仍待用户手测，自动回归不冒充系统验收。

## Result Writeback (2026-10-18)

- Status: DONE（定向实现交接结束，最新 head 自动行为仍待 CI）。
- Executor: `composer_predictions_impl`；Reviewer: Codex。
- Deliverable: 三处 composing action guard 的窄修复、原高层按键与真实提交回归，以及 delivery；平台输入提交回调未修改。
- Verification: 子 agent 的 formatter、`git diff --check`、最终 `cargo fmt --all -- --check` exit 0；没有本地运行、编译或测试。Codex 已逐行审查差异与 GPUI 派发语义。
- Next step: Codex 集成 push，重验最新 head 的完整门禁并记录原始证据；保持 PR 开放，不自动合并 master。
