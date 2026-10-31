# 排队与问答短动作胶囊化

## 来源与范围

用户在 #788 第一批合并后追加授权：排队区文字动作改为 `rounded_full()`，问答短动作统一 full 写法；高度和其他样式保持不变，不影响图标按钮和菜单行。

基线 `d1e9b61bf78551e17040ca8c526393415c5a62fb`，分支 `feat/788-queue-question-capsules`。本规格仅扩展[第一批范围](composer-capsules.md)，模型与权限已交付的 32 高契约不变；不以第三方解包内容作为输入。

## 验收与实现

- `conversation_stream/pending_messages.rs`：仅 `pending-resume`（Continue queue）、`pending-recover-edit`（Continue editing）、`pending-steer-*`（Steer）三个文字动作实例追加 `.rounded_full()`，不修改 `pending_button()` 共用 helper。
- `user_question_panel.rs`：短动作 `button()` 中的半高固定圆角替换为 `.rounded_full()`，覆盖 Back、Skip、Next、Submit、Add note。
- 保留高度 28、宽度及最小宽度、内距、间距、文字、字号、颜色、默认/hover/pressed/禁用/焦点背景与边框；仅圆角变化。宽度不小于高度时 full 的实际半径为 28 / 2 = 14；队列从 8 变为 14，问答从 14 变为 14，不改变尺寸。
- 保留现有工具提示、可访问名称、键盘/鼠标处理器、IME guard、忙碌/草稿占用保护、队列顺序及答案提交逻辑。
- 删除、更多、编辑消息与排队开关菜单行、问答答案卡/Other、主输入表面、安全授权组及 Retry/Reload 均不修改。所有共享几何 token、全局交互 helper 不变。不新增界面文案、依赖、阴影、动画或密度变体。

## 验证与手测

本地只做 `cargo fmt --all -- --check`、`git diff --check` 和差异/调用点静态检查；不运行 Vega、不编译或执行测试。不为纯圆角变更建立临时 Rust 测试。停写精确版本后由非实现者独立审查，最新 head 通过既有 required CI 后 squash merge；CI 的 Rust 发现/执行计数如实记录，空集不表示行为测试通过。

用户手测：暂停队列的 Continue queue、恢复草稿的 Continue editing、运行中的 Steer，在默认、hover、焦点、禁用及适用运行状态下保持原尺寸和交互；删除/更多及弹出菜单仍为原形；问答 Back/Skip/Next/Submit/Add note 的尺寸、答案选择/备注、键盘导航与提交不变。浅深主题及窄窗口检查；无真实运行证据时不声称外观或系统交互已通过。

回滚只撤销三个队列实例的 full 调用并恢复问答半高圆角，不涉及数据或权限策略。Issue 保持开放、合并后 In review 等用户手测。
