# Issue #78 follow-up — 禁用消息悬停复制（实现与验证记录）

## 背景与决策

2026-09-23 用户复验：消息下方常驻的复制动作行「很丑」，且与布局耦合，要求**先禁用**该交互。本卡是 PR #144（hover-copy，已合并 master）的后续收口，只关闭呈现，不删除能力。

关键决策：用一个编译期开关 `conversation_stream::MESSAGE_COPY_ACTIONS_ENABLED = false` 收口，而不是回滚 #144 的代码。这样 `MessageCopy` 缓冲、共享 `Copy` 图标、以及 `message_with_copy` 渲染分支全部保留，待重新设计放置方式后只需翻转该开关即可恢复；同时避免「删了再重写」和随之而来的历史证据断裂。

## Freeze

- Contract: [双侧消息悬停复制](vega-issue-78-hover-copy.md)（顶部状态说明指向本卡）。
- Branch: `feat/issue-78-disable-copy`; base `origin/master` `c926c76`。
- Platform: macOS arm64；本卡独立 worktree 与独立 target，无新依赖。
- 生产改动：`mod.rs` 新增 `MESSAGE_COPY_ACTIONS_ENABLED`；`render_rows.rs` 的 `message_with_copy` 在该开关为 `false` 时原样返回 `body`。开关判断放在函数内部（而非调用点），以保证 `MessageCopy`/`Copy` 图标在编译期仍可达、不触发 dead-code/unused 警告。
- 测试改动：既有 4 个 hover-copy 断言改为按开关分支；新增 1 个禁用态回归，证明「不挂载、不预留、hover 不复制」。

## Results

| 需求 | 证据等级 | 精确命令 | 结果 |
|---|---|---|---|
| 禁用后不挂载动作行、不预留高度、hover 不触发复制（浅/深色 × 用户/助手） | 生产 GPUI render/handler | `cargo test -p vega_ui issue78_hover_copy -- --nocapture` | 6 passed, 0 failed |
| 会话区完整回归（含消息气泡、附件、恢复、时间线） | 生产 GPUI/controller | `cargo test -p vega_ui conversation_stream::tests` | 200 passed, 0 failed（原 199 + 新增 1） |
| Lint（本仓统一入口） | 静态 | `cargo clippy -p vega_ui --all-targets -- -D warnings` | PASS（exit 0） |
| 格式 | 静态 | `cargo fmt --all -- --check` | PASS（exit 0，空输出） |

禁用态回归 `issue78_hover_copy_disabled_mounts_no_action_row` 的断言：

- `message-copy-user` / `message-copy-assistant` 的 `debug_bounds` 均为 `None`（不挂载动作行）；
- 对应 `user-message-bubble` / `assistant-message` 仍存在（正文照常渲染）；
- 指针移入消息中心后动作行仍为 `None`（hover 不显现）；
- 预置剪贴板哨兵 `sentinel` 后 hover，剪贴板保持不变（未发生复制）。

## Residuals

- LIMIT: 本卡为 UI 呈现收口，禁用态由生产 GPUI 渲染/几何测试证明；未重跑原生实机截图（该交互已被禁用，实机可见结果即「没有按钮」，与 #144 的实机证据相反方向）。
- NOT RUN: 云端 required checks 与合并。本地聚焦测试不替代 PR 云端门禁。
- Spec 偏离：无。
