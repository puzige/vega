# NSH 消息页脚

## 范围与验收

本轮仅实现新 `conversation_stream/message_footer.rs`、`message_footer/**` 与本文件；父任务负责 `mod.rs`、`render.rs`、`render_rows.rs` 接线及移除旧可见页脚。冻结 selectors 不修改。不是中途独立审查。

- 复制、复制回执、分支按钮、实际创建中指示及时间戳全部使用 NSH；不复用仍绘制 GPUI 图标的 `CopyReceiptButton`。
- 原用户消息右对齐、助手左对齐不变；用户气泡页脚 bottom 为 `CONVERSATION_USER_BOTTOM - MESSAGE_COPY_FOOTER`，非气泡保留 `MESSAGE_COPY_FOOTER` 底部高度。现有 token：`COPY_ACTION_SIZE = 24`、`COPY_ACTION_GAP = 4`，所以 footer 高度预留 `24 + 4 = 28`，气泡偏移 `48 - 28 = 20`；NSH 控件行高仍为 24，不增加重复 padding。
- 消息 hover、原 timestamp FocusHandle 含焦点、NSH 控件焦点或该消息实际 fork pending 时显示。焦点/hover 不更改内容高度。时间戳有独立完整可访问名称。
- Copy 使用当前原文 `MessageCopy::copy`，仅现有 actions enabled 且有当前文本时可用；回执仅在实际写入剪贴板之后出现，按原 Motion 时长消退，同来源内容版本变化或卸载取消。保留 Copy→Check 交换和 `ui.copy_requested` 可访问反馈，不宣称外部读取完成。
- Fork 保留完整 thread/message 请求，调用原 `request_fork`；运行、权限、preview、父会话、pending 和后台运行门禁由当前控制器重查。pending 仅匹配当前完整请求消息，窗口非活动或 reduced-motion 时静态分支图标，活动时 NSH 旋转加载图标。
- 所有延迟动作携带源/路由 epoch、完整消息 identity、copy id/generation、完整 fork 请求和展示投影。可信端核对 ReadingOwner 根/路由、当前业务源、当前挂载身份、已 paint 的 bounds/clip/viewport/scale/foreground、最新 NSH 控件测量及布局版本。不得用 index、旧 focus 或旧 lease 直接授权。
- 鼠标、键盘和 AX 激活共享实测与可信处理器；未知/陈旧动作拒绝，不重放。真实 Tab、hover、焦点、窄窗、滚动裁剪和三平台原生行为留待集中验证。

## 父侧最小接线

注册 `mod message_footer;`。`render.rs` 原 `render_rows::message_with_copy` 与 `render_rows::MessageActions` 分别改为 `message_footer::message_with_copy`、`message_footer::MessageActions`，构造中增加：

```text
owner: cx.entity().downgrade(),
identity: this.entry_identities.get(index).cloned(),
```

新函数参数仍为 `(body, copy, actions, window, cx)`，末两项改为 `&mut Window` / `&mut App`。其余字段保持原样。父侧移除 `render_rows.rs` 原 MessageActions 与 message_with_copy 可见实现；不移除其他代码块仍使用的 CopyReceiptButton。

## 验证计划与限制

仅限域 rustfmt/static；禁止 Cargo、Git、网络、native、依赖修改和委派。父统一编译/测试，当前 Cargo 曾被权限层拒绝，不绕过。最终 required CI、非实现者独审及用户手测未取消。本文件不将源码存在、格式检查或父侧接线声明为运行通过。

## 本轮实现与验证记录

- 新页脚可见内容仅使用 NSH；原 GPUI 只承担布局、FocusHandle/AX 事件桥、透明 Surface 及 paint 挂载观察。没有复用 CopyReceiptButton 的 GPUI 绘制。
- 源通知立即失效展示 id/意图；焦点离开立即失效待键盘动作。宿主 AX 也携带原快照，再要求 NSH 重新测量；最新 pointer/AX/keyboard 共用语义 Copy/Fork 与 Witness，不由 index 授权。
- 复制仍读取当前 `MessageCopy::copy` 的 `display_text`，未改业务原文/投影策略；非空源核验后写入剪贴板才创建回执。回执仅更新本控件，不修改消息或权限。
- 非活动窗口保留展示快照；只有交互核验要求 ReadingOwner 的活动根/route 当前。这样 actual pending 在非活动或 reduced-motion 时仍有静态分支图标，而不能执行动作。
- 已运行：`RUSTUP_TOOLCHAIN=1.98.0 rustfmt --check --edition 2024 --config skip_children=true`，仅两个新 Rust 文件，退出 0；Python 限域禁止项和行尾空白静态清点退出 0。未增加或执行 Rust 测试，无行为通过声明。
- 未运行：Cargo/check/Clippy/测试、真实 NSH/原生 UI、跨平台、CI、非实现者独审。本路未调用 Git/网络、未修改依赖/vendor/native/共享 render/mod 或冻结 selectors。
- 停写前只读核对父接线：`render.rs:805` 仍调用旧 `render_rows::message_with_copy`，新模块注册尚未发现；父须按上述接口接线并删除旧可见页脚，随后统一编译。原 render_rows 已由父并行更改，不以开工 SHA 覆盖。

Rust 产物 SHA-256：

```text
ed728eb0d950015792ba31c0d796975ff5a10f3a105c2d853fd8c23d9ae37e34  message_footer.rs
5ed8bcb97be09fecea0d512f4342eaf8b22c117b1d62562c6517fb58c8e2141f  message_footer/pixels.rs
```

交接为 READY（实现停写、待父接线与集中验证），不是编译/行为验收通过。集中验证至少覆盖：用户/助手与有无气泡几何、hover/原 FocusHandle/Tab、鼠标/键盘/AX 复制及来源变化失效、连续复制与回执到期、完整 Fork 请求/pending/reduced-motion、滚动裁剪及卸载旧动作拒绝。
