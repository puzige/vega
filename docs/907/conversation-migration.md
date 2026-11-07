# 会话与输入迁移写包

基线：`d4beacac2f01d4c09af7545163393e9d43753fe9`。遵循 `nsh-full.md` v1；本写包只拥有 `conversation_stream/**`、`text_input/**` 和本文件。不修改共享 Surface、依赖注册或 Git。

## 恢复事实

恢复时两个独占目录没有 NSH 实现文件。共享 `Surface<S,C>` 提供容量 64 的有界命令队列与 watch 快照。当前任务只做静态验证，不运行、编译或测试 Vega。

## 实施契约

- 原 TextInput 是文本、revision、UTF-8 mention 范围和撤销的权威；NSH 使用 UTF-16 选择坐标。输入事件串行送达，编辑包含预期旧文本和 revision；只对仍匹配的前置状态进行最小区间替换，避免整条 set_text 清空绑定和历史。
- NSH composition 文本不是提交文本；开始组合即抑制预测/提交，提交文本由 on_change 处理，组合结束不添加第二次文本。原 IME/撤销状态归原对象所有。
- 所有命令通过有界 mpsc 的异步 send，不用 try_send；页面事件泵串行发送，不让不同 spawn 的 send 重排。
- renderer 在组件 owner 生命周期内只创建一次。快照用响应式字段更新，不逐帧重建引擎。
- 普通 watch 快照不携带 masked 输入；凭据输入需要专用安全通道，不得把配置凭据广播到普通会话投影。
- 会话 entry、权限 lease、文件/技能/预测 generation、草稿与附件 owner 不迁移到 NSH 虚拟行 owner；原控制器继续执行业务动作。

## 范围外接口请求（完整会话 Render 切换前置）

当前原卡片没有可供 NSH 使用的完整纯数据/动作接口，不能嵌旧 GPUI 或只显示标题替代。请对应单写者提供 crate 内协议及 NSH 构造器（固定 0.4 API）：

| 文件/类型 | 必需的快照 | 必需的动作及所有权 |
| --- | --- | --- |
| `permission_card.rs::PermissionCard` | request/thread/tool 身份、完整命令/危险项、有效/已决状态、所有允许决策与禁用原因 | 单次响应/拒绝、范围授权、超时；继续由卡片保有 lease。当前 decision 方法及字段为私有，只有 identity/is_resolved 等不足以迁移 |
| `plan_card.rs::PlanCard` | 完整计划、持久化状态、陈旧状态、可用审批动作 | 原审批/修改事件和 owner；目前计划字段私有，公开 status/render_row 不足 |
| `tool_card.rs::ToolCard`、`tool_activity_group`、`run_activity_group` | 完整命令/输出/Thinking/真实终态、分组/segment、展开状态、subagent/artifact 信息 | 展开、复制原文、链接、取消/打开对象；不由页面推断成功或绕过 URL 白名单 |
| `artifact_card`、`branch_selector`、`commit_panel` | 当前对象完整显示投影、脏状态/冲突/忙碌与禁用原因 | 原可信动作及对象 owner，不携带 GPUI Entity 跨 engine |

`SummaryCard::summary()` 已有纯数据访问，除此之外不据组件名称假设等价。权限审批缺口属于安全阻塞，不以隐藏入口降级。

## 验证/交付状态

实施中，未宣称全量完成。最终追加文件身份、静态结果和剩余缺口；CI、三桌面运行、IME/剪贴板/无障碍及用户手测均未取得。
