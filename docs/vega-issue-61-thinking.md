# Issue 61 · 展示真实 Thinking 输出

关联：https://github.com/puzige/vega/issues/61 。2026-09-18 用户授权修复，需区分模型未返回与 Vega 丢弃输出。

## 已知链路与任务契约

OpenAI-compatible SSE 已将 `reasoning_content` 映射成 ThinkingDelta，conversation 会转发该事件；当前 ConversationStream 明确忽略 ThinkingDelta，导致即便模型返回推理文本也不可见。须复核完整生产链路后实现展示。

- I61-R1：只在当前活动消息实际收到非空 ThinkingDelta 时渲染独立可折叠「思考过程」块；无返回时不伪造思考内容、不制造空白占位，也不把普通回答猜成思考。
- I61-R2：以 message/run 归属过滤过期事件，保持思考、正文与工具发生顺序。工具调用前后的不同思考片段不能挤到最终回复末尾；final/error/interrupt 正确结束活动思考块。用户可展开/收起，使用共享颜色、字体、图标及现有流式重绘策略。
- I61-R3：Thinking 文本不得计入普通回答的字符 token 估算，不混入正文或发送成用户消息。保持 runtime 既有有界 reasoning buffer 与 provider replay 策略；UI 同样有界，不接受单条/累计无上限增长。
- I61-R4：本轮优先修复真实 live stream 展示。现有存储不持久化 thinking 时，不从历史回答伪造或重新生成；在报告中明确历史重开限制，不擅自扩展敏感正文存储范围。若已存在合法的历史 thinking 数据源，应复用。
- I61-R5：复用已有 Thinking 组件（若存在）或新增最小独立组件；不改 #58 权限或 #59 表格 renderer。无新依赖；不要为测试扩大生产 public API。

## 显示边界

- 默认折叠，标题「思考过程」常见可见；点击或键盘 Enter/Space 展开与收起。
- 单分片最多接收 64 KiB、单块最多 256 KiB，与 runtime 既有上限相同；单个 ConversationStream 生命周期最多保留 1 MiB，不在下一条消息开始时重置，避免连续运行累积无限内存。
- 最多保留 256 个思考块，防止极小思考/正文交替绕过字节预算制造无界实体。截断在 UTF-8 字符边界进行；超过字节或块数上限时在最后一个思考块显示「思考内容已达显示上限」，包括恰好达到上限后的后续事件。
- 文本仅保留于当前流实体；历史重新水合不会恢复这些思考块。现有持久化没有 thinking 数据，不增加 schema 或将思考混入普通回答。

## 验收

1. 真实 SSE/ProviderEvent → runtime/conversation → production UI 入口（可用 MockProvider 替代网络）证明带 ThinkingDelta 的回答可展开并看到完整分片合并内容；正文/工具仍正常展示。
2. 覆盖无 thinking、仅 thinking、thinking→正文→tool→thinking→正文、重复/过期事件、切线程、停止/失败、上限截断、收起/展开和默认可见标题；真实布局/可见内容断言优于仅 helper 状态测试。
3. provider 未返回 reasoning 字段时无空块；不要把 mock 通过当成真实 provider 已返回 thinking 的证明。
4. 运行相关 focused tests 与最终 workspace fmt/Clippy/test/build；主 agent 负责代码审查、集成和原生界面检查。
