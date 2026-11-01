# 纯图片用户消息的时间对齐

基线 `173e71558c6aaddcba9c315d8e71b43d2e0b56b5`；用户截图显示图片靠右而自身时间 10:54 靠左。不是居中时间分隔问题。

## 根因与最小方案

render.rs 的 MessageActions.user 只匹配 User，UserImages 使用非用户页脚的左对齐。图片行没有文字 User 气泡的 CONVERSATION_USER_BOTTOM 留白，因此不能只将整个 user 开关改为 true，否则还会失去 MESSAGE_COPY_FOOTER 留白并套用气泡内部底部偏移。

本轮把用户角色与文字气泡布局分开：user 匹配 User/UserImages，用于横向靠右、复制命名；user_bubble 仅匹配 User，保留原有底部留白和偏移。非气泡图片页脚继续使用底部零偏移及 MESSAGE_COPY_FOOTER 留白；文字及助手的行为不变。不改图片尺寸、间距、时间来源、hover/focus、分组阈值或复制源，不新增文案/依赖。

## 验收与验证

- 纯图片用户消息自身时间在图片下方靠右；仍在 hover/focus 时显示，时间可访问名称保留，图片无文本不新增复制操作。
- 文字用户消息的时间及复制仍靠右，原气泡底部定位不变；图文消息时间落在文字主体且不重复；助手时间仍靠左。
- timestamp_extra_height 对 UserImages 保留 MESSAGE_COPY_FOOTER，与非气泡渲染留白一致。
- 低风险布局分支，使用静态调用点/样式核查、独立审查和完整 required CI，不为布尔转发补镜像或源码字符串测试。按仓库限制本地不执行 Vega、编译或测试。实际绘制、悬停、键盘和窄宽度由用户更新后手测，不能把 CI 当视觉验收。

## 结果

待独立审查和最终云端门禁。Issue 开放，用户未手测确认。
