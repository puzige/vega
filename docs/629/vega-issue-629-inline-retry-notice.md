# 重试提示去除卡片样式

## 问题与根因

网络中断自动重试时，会话流显示一条 `正在重新连接… n/5`。该行由 `RunNoticeKind::Retrying` 渲染，复用 `render_rows::status_notice_with_icon`：带 1px 边框、8px 内边距和 8px 圆角，视觉上是一张独立卡片，与同一轮里无边框、无内边距的工具提示行和活动阶段标题不一致。用户要求参考对话内工具提示的样式，去掉卡片。

## 验收

- 重试行改为工具提示式纯文本行：16px Wi-Fi 图标加 13.5px 文本，行内 6px 间距，无边框、无内边距、无圆角，左侧与同轮工具行、阶段标题对齐。
- 文字与图标使用次级文字色，不再使用警示色；重连仍是可恢复的临时状态，恢复、结束或失败时该行照旧移除。
- 行为不变：同一消息的重试在原位更新尝试次数、`assistant-network-retry-*` 稳定 id 与可访问名称保留、不进入 disclosure、不新增成功提示。
- 其他共用状态卡片的地方（Composer 附件提示、待发消息提示、权限卡）不受影响。
- 行高估算与该行实际渲染一致，避免虚拟列表与消息锚点位置漂移。

## 实现方案

1. `render_rows` 新增 `inline_status_notice`：沿用工具行的排版常量（`CONVERSATION_TOOL_HEIGHT` 最小高度、`CONVERSATION_INLINE_GAP`、`ICON_SIZE`、`Typography::CONVERSATION`），不绘制边框、内边距和圆角。
2. `RunNotice::render` 的重试分支改用该函数并传入 `colors.text_secondary`；外层保留原有 `py_1`，继续与相邻内容留出间距。
3. `RunNotice::estimated_height` 的重试分支按 13.5px 文本在扣除图标与间距后的可用宽度换行，以工具行高度为下限再加外层上下间距；非重试分支保持原有正文排版估算。

## 验证与交付

本地只做格式、差异与静态调用链检查，不编译、不运行 Vega、不跑测试。完整检查交现有 required Actions fmt/clippy/test 门禁；本轮不引入临时 Rust 测试。由非实现者独立审查。合并后由用户手测重连中的行样式、窄窗口换行、深浅主题与恢复后消失；CI 与合并不等于原生界面验收。
