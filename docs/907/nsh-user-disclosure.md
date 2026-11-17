# NSH 用户消息展开与收起

## 实施范围与验收（实现前固定）

本路只写 `conversation_stream/user_disclosure.rs`、新增 `user_disclosure/**` 与本文。共享 `render_rows/render/mod/core` 由父侧写入。基线主文件 SHA-256 为 `d8ca5b5f59a75da8098452c0db9920c6e3c64f2c5c1aad59bf51deae66609baa`。父侧已确认远端更新和祖先关系；本路不运行 Git、网络、Cargo 或原生应用，不改依赖、vendor、Kit/Fast、权限业务或持久化。

1. 原 `preview`、截断、链接投影、代码块/表格处理、`estimated_height` 与完整 Markdown 选择算法保持字节不变；展开状态仍属于实际 `MessageCopy.user_disclosure`，成功切换仍使选择失效并使对应真实条目重新布局。
2. 展开/收起标签与旋转 ChevronDown、hover/pressed/focus 可见反馈由固定 NSH 0.4 构造。保留原 `ui.show_more` / `ui.show_less` 本地化、BODY 字号、用户消息行高、间距、圆角与原焦点 key。GPUI 只作透明宿主、布局、事件与 AX/Tab 桥接，不绘制可见标签或图标。
3. 从真实 `MessageInteraction.stream` 获取 owner；在 owner 当前 User 条目中解析完整 `StreamEntryIdentity`。动作绑定真实 MessageCopy 身份及共享 disclosure 对象、content generation、disclosure revision/expanded、root owner、navigation epoch、独立 mount/presentation/intent 标识。行号只作当前身份查找后的布局定位，不成为授权。
4. 鼠标、Enter/Space 与 AX 均经过 NSH 当前叶子的异步测量；测量前后复核 layout revision 和 NSH presentation 存活，执行端再复核来源、root、当前路由、窗口激活、挂载、已绘制投影、viewport、scale、clip、列表可见范围和测量。隐藏、部分裁剪、来源替换/内容 A→B→A、展开 A→B→A、导航离开返回、卸载重挂及延迟旧动作全部拒绝。
5. 保留逐控件 Tab 和 AX 按钮/expanded；焦点延续使用原 key。键盘只接受当前焦点上的无修饰、非 repeat、非 IME Enter/Space，不截获 Tab，不能把旧 AX Focus 或键盘动作迁移到新来源。执行后保留当前控件焦点，并使旧动作立即过期。
6. 开发回归直接调用生产来源/状态或测量决策，不做源码文本断言或 mock 决策；测试保留供父侧集中执行、留痕及合并前清理。限域格式和静态检查不替代编译、行为测试、独审或用户手测。

## 集成现状

现有生产调用位于 `render_rows::selectable_user_message`，旧 GPUI toggle 在 `.when(preview.collapsible, ...)` 中；`MessageInteraction` 已携带真实 `Option<WeakEntity<ConversationStream>>`，不需要修改上游 render/core API。父侧负责替换此分支，精确入口签名与最终验证记录在实现交付时补充。本文不把独立叶子完成称为生产已接线或已验收。
