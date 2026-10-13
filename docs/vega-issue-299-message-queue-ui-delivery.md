# Issue #299 UI 交付记录

契约：[运行中消息队列规格](vega-issue-299-message-queue.md) §2.2、Q15–Q18。UI 实现分支 `feat/299-message-queue-ui`；主工作分支负责后端、集成与云端验收。用户截图仅作可见交互参考，原图和私人内容不入仓库。

## 实现前验收矩阵

| ID | 操作 | 预期 | 验证方式 | 状态 |
|---|---|---|---|---|
| Q15 | 运行中 Enter；切换全局默认；空闲再次发送 | 默认排队，切换后插话；空闲提交保持原契约；接收成功前保留草稿 | 生产 GPUI 提交事件 | 待 CI |
| Q16 | 默认切换时已有待发消息 | 已有消息行为不变；单条插话不改变全局默认 | 生产 GPUI 队列操作 | 待 CI |
| Q17 | 插话、垃圾桶、更多菜单；Esc / 外部点击；Tab / Enter / Space | 每次操作准确的稳定 ID；弹层可关闭并回焦 | 生产 GPUI 交互 | 待 CI |
| Q17-edit | 有草稿时编辑；删除失败；删除成功前用户输入新草稿 | 不覆盖新草稿；删除 ACK 后才回填；竞态时保留可恢复取回项 | 生产 GPUI 草稿与附件状态 | 待 CI |
| Q18 | 浅深主题、长文本、附件、4 条待发消息 | 内缩 16、行高 36、覆盖 8、最多 3 行滚动、缩略图 24；主控件可见 | 生产 GPUI 几何；用户像素手测 | 待 CI / 未手测 |
| Q18-narrow | 360 px 窗口、4 张附件、长文本 | 仅第一张缩略图加 +N；插话、删除、更多仍在边界内；composer 和弹层命中正确 | 生产 GPUI 几何和鼠标事件 | 待 CI |
| Q15-settings | 通用页保存默认；模拟同字段外部修改；更新全局 | 保存成功才更新全局；不覆盖冲突；已打开设置同步 | 生产 Settings GPUI + owned config | 待 CI |
| Q13 | 重复收到同一 durable delivery | 历史只出现一次，使用真实 message ID / seq 定位 | 生产事件回归 | 待 CI |

## 实现计划

1. 建立以上用例，再替换初版队列列表为共享 token 驱动的 composer 叠层。
2. 使用同一全局默认投影联通通用页和更多菜单；保存复用既有配置 CAS 路径。
3. 对单条操作、菜单焦点、ACK 后取回、持久历史身份建立生产 GPUI 回归。
4. 本地只执行格式与差异静态检查；编译、Clippy、nextest 由主工作分支 PR Check 执行。

## 验证结果

2026-10-13 本地完成：22 个新增或修改的 Rust 文件经 `rustfmt --edition 2024 --config skip_children=true` 格式化；`rustfmt --check`、`git diff --check` 通过。对新增生产代码的静态扫描未发现新增注释、`.unwrap()`、`.expect()`；未新增依赖，未写入用户截图或私人内容。

新增 10 个 ConversationStream GPUI 回归和 1 个 Settings GPUI 回归，覆盖上述矩阵、成功 ACK 前保留草稿、同文新草稿保护、平台 replacement / IME replacement、编辑 ACK 与新草稿竞态、失败后重试、持久 ID 去重和定位。所有测试尚未运行，待集成分支 CI；未进行本地编译、cargo check、Clippy、test、nextest、打包或未构建界面的截图验证。截图像素相似度仍需运行产物人工验收。

TextInput 所有内容写点已静态核对：`clear`、`set_text`、`complete_at_query`、`replace_text_in_range`、`replace_and_mark_text_in_range` 均推进版本号；历史召回调用 `set_text`，剪切、删除、粘贴和换行调用 replacement。当前 TextInput 没有 Undo / Redo action 或独立撤销历史，不存在另一条撤销写入路径。

## 公共接口与集成约定

- `RunningMessageDefault(PendingMessageBehavior)` 为 UI Global；配置保存成功后更新。已打开的 SettingsView 监听该 Global，同步更多菜单造成的默认值修改。
- `ComposerSubmitted.pending_behavior` 仅在 Native 运行中为 `Some`；提交前冻结 TextInput 版本，`accept_queued_submission(content, cx)` 仅清理版本和文字均匹配的草稿，以及冻结附件 ID。
- `PendingQueueAction` 支持 `Remove(id)`、`Edit(id)`、`Steer(id)`、`ToggleDefault`、`Resume`。请求通过 `PendingQueueRequested { thread_id, action }` 发出；busy 期间阻止重复提交。
- `apply_pending_queue` / `pending_queue` 保持后端约定。`accept_pending_edit(message, cx)` 必须在稳定 ID 原子移除成功后调用，即使后续 reload 失败也必须交回已移除项。失败时不调用；输入竞态由 UI recovery buffer 保留，不覆盖新草稿。
- `PendingMessageDelivered` 消费 durable `message_id` 与 `seq`，建立正文和附件 anchor 并按 ID 去重。

## 交付文件

现有文件修改：

```text
crates/vega_theme/src/lib.rs
crates/vega_ui/src/conversation_stream/attachments.rs
crates/vega_ui/src/conversation_stream/composer.rs
crates/vega_ui/src/conversation_stream/composer_actions.rs
crates/vega_ui/src/conversation_stream/content.rs
crates/vega_ui/src/conversation_stream/core.rs
crates/vega_ui/src/conversation_stream/mod.rs
crates/vega_ui/src/conversation_stream/render.rs
crates/vega_ui/src/conversation_stream/tests/mod.rs
crates/vega_ui/src/icons.rs
crates/vega_ui/src/image_preview.rs
crates/vega_ui/src/lib.rs
crates/vega_ui/src/settings/preferences.rs
crates/vega_ui/src/settings/render_impl.rs
crates/vega_ui/src/settings/state.rs
crates/vega_ui/src/settings/tests.rs
crates/vega_ui/src/text_input/input_handler.rs
crates/vega_ui/src/text_input/mod.rs
crates/vega_ui/src/text_input/state.rs
```

新增文件：

```text
crates/vega_ui/src/conversation_stream/pending_messages.rs
crates/vega_ui/src/conversation_stream/tests/issue299_pending_messages.rs
crates/vega_ui/src/settings/tests/issue299.rs
docs/vega-issue-299-message-queue-ui-delivery.md
```

本分支仅修改 UI / Theme 与本交付文档；未 commit、push、创建 PR 或修改其他 worktree。主 agent 负责复制上述文件并与后端类型、配置、controller 改动集成。
