# #830 用户长消息展开控件对齐与箭头

## 基线、根因与范围

初始基线 `259099b30b01ce23c680615655aa4346eb90a865`；集成基线更新为 `44f8148291ae0059e764ff8a0b708196a0eecf00`，保留 #829 的圆角与规格修订，仅合并文档变更记录冲突。用户已授权按 #830 实施，替代上一轮仅排查建卡的限制。#810 的 `selectable_user_message` 使用 `.justify_end()` 且仅渲染 label；[#804](../804/user-message-disclosure.md) 将参考图误写为“右下方只显示文字”。本规格取代该条视觉契约，不改变其他预览和折叠规则。

仅修改 `crates/vega_ui/src/conversation_stream/render_rows.rs::selectable_user_message` 内的控件布局与装饰图标。用户气泡外层 `.items_end()` 保持不变，正文、附件、时间戳及整条消息仍保持原布局。不改共享 disclosure、依赖、源文、复制、存储、模型输入、折叠阈值或滚动策略。

## 实现契约

- 控件容器左对齐，与气泡正文左边缘同轴；用户气泡整体仍靠右。
- 同一个 Button 内水平排列文字和常驻 chevron，垂直居中、不换行。收起为 `Show more` / “展开更多”加向下箭头；展开为 `Show less` / “收起”加向上箭头。
- 原图标封装返回 `AnyElement`，不暴露旋转；本控件直接使用同一组件库的 `IconName::ChevronDown` SVG，展开时旋转 π 弧度，不新增图标资产或修改共享组件。保持默认可见，不依赖 hover/focus。
- 保留现有 Button role、名称、真实 expanded 状态、Tab、Enter/Space、鼠标传播边界；图标不另设可访问节点、焦点或事件，点击文字和图标均交同一按钮处理。
- 原 `user_disclosure::toggle` 的选区清除、消息独立状态、列表高度失效及刷新逻辑不变。短消息仍不显示控件，模型运行中仍可展开。

## 几何核对

使用现有 token，不新增数值 token。按钮行高显式取用户正文行高，与 `user_disclosure::estimated_height` 已计入的一行一致；文字保留 `Typography::BODY`，图标采用 `Layout::CONVERSATION_CHEVRON_SIZE`，间距采用 `Layout::CONVERSATION_INLINE_GAP`。

| 项目 | 计算 | 结果 |
|---|---|---|
| 按钮与估算行高 | `CONVERSATION × CONVERSATION_USER_LINE_HEIGHT = 13.5 × (23 / 13.5)` | 23px |
| 图标槽 | `CONVERSATION_CHEVRON_SIZE` | 12px |
| 图标上下剩余空间 | `(23 - 12) / 2` | 各 5.5px |
| 文字与箭头间距 | `CONVERSATION_INLINE_GAP` | 6px |
| 控件及其顶部间距 | `USER_MESSAGE_TOGGLE_GAP + 23 = 6 + 23` | 29px，与估算一致 |
| 展开图标方向 | 向下箭头旋转 `π = 180°` | 向上 |

无额外垂直 padding/border，不需要修改高度估算或预览测量。控件宽度随本地化文字和图标自然布局，气泡宽度与内距不变。

## 验证与验收

本地只做 `git diff --check`、rustfmt 和源码链路静态核对；不运行 Vega、编译、构建或测试。此改动仅涉及局部布局和图标，不增加临时 Rust 测试，也不以源码字符串测试冒充交互验证。非实现者独审精确停写版本，现有 Actions 执行完整 fmt / 全目标 Clippy / Python 回归 / Nextest 清点，最终 required check 通过后合并；Rust 空集应标 `no Rust tests`，不称交互通过。

原生验收由用户在包含本修复的版本手测，CI 和静态核查不代替以下结论：

1. 收起时气泡整体靠右，按钮在内部左下方与正文对齐，文字后向下箭头默认可见。
2. 点击文字或箭头均展开；按钮仍左对齐，文字改为收起、箭头向上；再次点击恢复收起。重复切换及多条消息状态独立。
3. Tab 可聚焦且焦点可见，Enter/Space 每次切换一次，辅助功能 expanded 与内容一致。
4. 浅深主题、中英文、窄窗、hover/focus 下无消失、错位、换行或溢出；短消息无控件，运行中可阅读。
5. 长段落、列表、代码和表格预览、完整复制及可见选区复制不变，附件/时间戳不变，无新增重叠或高度跳变；侧栏和工具折叠箭头不变。

用户明确手测通过前 Issue 保持开放，合并后置 In review。回滚撤销本卡代码差异；保留规格的历史更正说明。
