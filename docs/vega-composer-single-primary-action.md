# Composer 单一发送/停止主按钮

2026-10-18 用户明确要求按 Codex 参考图调整（#463）。本契约替代 #299 中运行时同时显示发送和停止按钮的展示方式，原有 queue/steer 与停止语义保持。

## 验收矩阵（实现前冻结）

| 场景 | 预期 | 验证 |
|---|---|---|
| Native 运行中且无草稿 | 右下角只显示原停止按钮 | GPUI 结构/交互，Actions |
| Native 运行中输入文本或已有附件 | 相同主按钮位置显示向上箭头，隐藏停止，尺寸和主要按钮表面与空闲发送一致 | GPUI 结构/几何，Actions；用户手测 |
| 清空文本并移除附件，或发送成功清空草稿 | 恢复停止按钮，不产生多余槽位 | GPUI 状态切换 |
| 默认插话或排队 | 点击/Enter/Space 触发原对应行为一次，不停止任务；可访问名称明确为插话或排队发送 | GPUI 事件 |
| 附件导入、IME、空白、提交 pending、stopping 等不可提交状态 | 保留现有提交 guard，禁止重复提交；停止/准备状态不被发送错误覆盖 | GPUI 回归 |
| Tab/Shift+Tab 与按钮切换 | 焦点只到实际显示的主动作，无隐藏停止焦点；按钮切换不触发错误动作 | GPUI 焦点 |
| 空闲与 Codex backend | 既有发送/停止限制保持，不启用不支持的运行中插话 | 现有回归 |

## 实现计划与边界

在 conversation_stream/render.rs 统一 Native 运行中主动作可见性：存在文本或附件草稿且未处于提交/停止过渡时使用发送，否则保留停止；无运行时保留发送。发送启用状态继续由 can_submit_message 决定，展示不得绕过 guard。

pending_messages.rs 的 render_running_send 使用同尺寸、圆形、主色与 disabled token 的发送按钮，保留 queue/steer 路由、明确的 accessible label 和键盘激活。composer_actions.rs 的焦点列表采用与渲染相同的可见条件，防止隐藏动作获得焦点；必要时复用单一主动作 focus。不得改队列存储、调度、runtime、模型协议、权限与停止实现。已有附件、IME、准备和错误恢复规则不变。

补充或调整现有 composer/pending_messages 进程内回归，禁止本地运行 Vega、编译或测试；仅快速格式与 diff 静态检查。Actions 执行完整 PR check，用户合并后手测两种运行中发送模式及空/有草稿切换。无新依赖、数据迁移、额外界面说明或设置。

## CI 回归适配

云端发现旧 composer_actions 停止测试、issue79 错误布局测试及 utility_bar 跨主题布局测试仍假定运行中有草稿时存在停止按钮。按本展示契约更新这些明确受影响的预期：Native 有草稿验证发送且停止缺席；无草稿与 Codex 仍验证停止。停止保留草稿的 controller 集成测试可通过现有公开 request_composer_stop 发起取消，保留完整的未发送草稿、partial persistence、computer-use stopping、late-tool suppression 等断言；另行维持无草稿停止按钮的真实点击覆盖。不得删除、忽略或弱化这些行为断言。
