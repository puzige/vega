# 问答按钮与模型胶囊文字居中

## 来源与范围

用户反馈问答 Skip 与 Composer 模型选择器文字未居中，建议使用胶囊按钮。任务 [#739](https://github.com/puzige/vega/issues/739)，基线 `0b31956674ec7315828a87f65a9654c460083d51`。不公开原始截图及会话。

仅改 `UserQuestionPanel::button` 与 `ConversationStream::render_model_selector`。不改权限按钮、全局 Interaction、模型请求、问答提交、焦点事件链、依赖或 CI。模型胶囊沿用 [#546 v2.1](../546/vega-issue-546-capsule-state-correction.md) 的透明默认与交互状态。

## 静态诊断与方案

- 问答按钮只有最小高度和内边距，无 flex 内容居中；标签可能受祖先行高影响。设置固定 token 高度、显式控件行高、水平/垂直居中、禁止收缩，并以高度的一半作为四角半径。同助手的 Back、Skip、Next、Submit、Add note 一起一致化。
- 模型触发器外层非 flex，内层通过 `h_full()` 做垂直对齐。改为外层 flex 负责居中，内行使用自然高度、显式收缩边界；保留预留标签宽度与截断，标签在预留槽位内水平居中。避免像素偏移补丁与百分比高度依赖。

## 几何与验收

| 项目 | 数值依据与计算 |
|---|---|
| 问答按钮高 | `TITLEBAR_CONTROL_SIZE` = 28px；固定高度，不因 hover/focus 改变 |
| 问答四角半径 | 28 / 2 = 14px；左右 padding 各 12px，最小宽 28px 避免半径钳制 |
| 问答文字 | SIDEBAR 13px × BODY_LINE_HEIGHT 1.55 = 20.15px；1px 边框下可用高 26px，剩余 5.85px 由 flex 上下平分，不另加偏移 |
| 模型胶囊 | 原 token 28px 高、14px 半径、左右内距 12px 不变；文字行盒 20.15px，小于按钮高，箭头使用原图标槽位 |

1. Back / Skip / Next / Submit / Add note 的文字在命中框内水平、垂直居中，四角对称；操作栏仍 Back 居左，Skip 与主操作居右，不把整组挪到卡片中央。
2. 模型名、选择强度、保存中、空模型标签及箭头在胶囊内垂直居中；标签在预留槽内水平居中。保留预留宽度，切换文案不导致按钮跳动。
3. 默认背景透明，hover 用 `bg_hover`，按下/展开用 `bg_active`，禁用无误导 hover；不新增永久灰底、阴影或产品文案。
4. 不改问答 disabled/提交/跳过/翻页/备注、键盘激活、焦点边框；不改模型请求 guard、Slider/List、外部关闭、Escape 与回焦。
5. 浅深主题、英文/CJK、长模型名、窄窗口下模型入口与箭头可见，标签截断且不挤占发送按钮。外观、真实键盘交互与字体光学效果交用户手测。

## 验证与交付

本轮为局部布局修复，不新增开发 Rust 测试或源码字符串测试。本地仅格式、差异和静态调用链检查，不运行 Vega、构建、编译、测试或打包。非实现者独立审查绑定停写 SHA；最终 head required Actions 通过后 squash merge，Issue 保持开放并置 In review，待用户验收。CI 结果不代替像素验收；不自动安装或重启应用。
