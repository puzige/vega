# 仅图片消息不渲染空气泡

版本：v1，2026-10-26。Issue：[#646](https://github.com/puzige/vega/issues/646)。基线：`9a90ef74`。

## 目标与边界

用户只附加图片、不输入文字时发送消息，会话流里不应出现任何空文本气泡；图片缩略图仍按原有位置和尺寸显示。有文字（含只有空行、只有空白字符）与图片混发时，文字气泡照旧显示。

只改用户消息的渲染与几何估算，不改动存储、runtime、附件校验、图片缩略图、待发送队列、复制文本或 `User` 条目的数据模型；不为流程新增提示文案。

## 先行验收

| 前置/操作 | 可观察结果 | 验证 |
|---|---|---|
| 仅附加 1 张图片发送 | 无浅蓝色空气泡、无多余垂直空白，缩略图按原样式出现 | 静态链路、用户手测 |
| 多张图片、无文字 | 同上，缩略图正常换行排列 | 静态链路、用户手测 |
| 文字 + 图片 | 文字气泡与图片各自正常，布局与修复前一致 | 静态链路、用户手测 |
| 仅图片消息重新打开会话 | 历史水合后同样不出现空气泡 | 静态链路、用户手测 |
| 仅图片消息在运行中排队、投递 | 投递前后都不出现空气泡 | 静态链路、用户手测 |
| 浅/深色主题、消息锚点轨道 | 无空气泡，锚点指向图片消息而不产生额外空锚点 | 静态链路、用户手测 |

## 根因

`StreamEntry::User` 与 `StreamEntry::UserImages` 是两个并列条目。以下路径在正文为空时仍会创建 `StreamEntry::User`，其 `lines` 只有 `UserLabel`、一条空 `UserLine` 和 `Spacer`：`composer::accept_composer_submission`、`pending_messages::append_pending_echo` / `append_pending_user`、`content` 历史水合（后端仍为图片-only 轮次写入空正文 user 行）、`core::apply_composer_history` 的投影补全。`ordinary_submissions::project_ordinary_submission` 已按 `raw.text()` 非空跳过文本条目，不在此列。

渲染层 `render_rows::user_message_item` / `selectable_user_message` 无条件套用 `bg(conversation_bubble)`、`px(CONVERSATION_USER_PADDING)`、`py_2`，空 `UserLine` 又补一个正文行高占位，于是得到约 32×39 逻辑像素的浅蓝药丸（截图实测 64×78 物理像素 @2x，与计算一致）。

同一份空行数据还影响 `message_anchors::estimate_user_height`（虚高占位）与 `message_anchor_projections`（为空文本条目生成无预览锚点）。

## 最小实现

1. `render_rows::user_message_is_visible(copy, lines)`：先看消息文本 `copy.has_text()`，再回退到 `UserLine` span 是否有非空文本；作为「是否有可显示内容」的单一判据。
2. `render_entry_internal` 的 `StreamEntry::User` 分支：不可显示时直接返回零高元素，不建立气泡、不挂复制入口。
3. `message_anchors::estimate_user_height`（及 `text_cache::raw_text_height` 的传参）：不可显示返回 `0.`，避免列表为空气泡预留高度。
4. `message_anchor_projections`：跳过不可显示的 `User` 条目，锚点落到紧随其后的 `UserImages`（其预览已是既有 `ui.image_message`）。

判据以消息文本优先，因此「只有换行/空行」的轮次仍按原样渲染，不会把用户真实发送的内容整条隐藏；只有既无文本也无 span 文本的图片-only 轮次才零高。判据放在唯一入口分支，不分散到 `user_message_item` / `selectable_user_message`。`User` 条目本身保留，因此历史水合、身份键、普通提交重试与复制来源链路都不需要分支改造；纯文本消息行为不变。

## 验证与交付

局部 UI 可见性变化：静态调用链/状态检查、`rustfmt --check`、差异检查、非实现者独立审查与最新 head required CI；不为流程临时造 Rust 测试。本地不运行 Vega、不编译/构建/打包/测试，行为与像素验收由用户手测。

回滚本卡四处判据改动（含 `text_cache` 传参）即可，不涉及用户数据。
