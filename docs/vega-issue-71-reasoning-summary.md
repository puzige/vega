# Issue 71 · 实时思考摘要标题

来源：https://github.com/puzige/vega/issues/71。2026-09-22 用户确认了实时 reasoning summary 范围；该范围随后随 PR #145 一起暂缓。2026-10-04 用户重新授权完成 #71，本规格现按 R1–R7 执行。只更新 thinking 活动块标题和预览，不回退 #151 的展开/收起行为，也不恢复 #141 的独立改动。

## 事实与范围

当前 Chat Completions 把 reasoning_content 转为 ThinkingDelta，UI 标题固定为「思考过程」。同一已配置 CPA 模型的真实探测：普通 chat 与带 reasoning.summary 的 chat 均只返回 reasoning_content；Responses 返回 reasoning_summary_text.delta。CPA translator 可将 reasoning_content 包装为 summary 事件，不能宣称它额外生成了精炼摘要。

PI 本体统一接收 summary/thinking，但折叠标签仍固定；社区 pi-thinking-steps 通过标题、段落和列表形成实时预览。采用自主实现的有界提取规则，不引入扩展依赖、不新增模型总结请求。

参考：
- https://developers.openai.com/api/docs/guides/reasoning
- https://developers.openai.com/api/docs/guides/function-calling
- https://github.com/earendil-works/pi/blob/main/packages/ai/src/api/openai-responses-shared.ts
- https://github.com/crustyhacker/pi-thinking-steps

## 行为契约

- R1：只更新 thinking 活动块的实时标题与预览，不改变当前展开/收起行为；尤其保留 #151 的“最新活动默认展开、旧活动收起”规则。以非空 summary 为最高优先级；缺失则使用 thinking 内容。标题从选中来源的当前 Markdown 小标题（ATX 或独立粗体标题）提取；没有小标题时用当前非空段落的简短片段。随着新片段/新标题更新，不永久停留在第一句。空白/未完成 Markdown 标记不抹掉已有可读标题；尚无可读内容时保留「思考过程」。无 thinking/summary 时仍不制造空块。
- R2：标题为单行、省略溢出，最多 96 个 Unicode 标量；清理标题 Markdown 标记与控制字符，正确处理 UTF-8 和跨 delta 边界；code fence 内文本不得被当成标题。一个来源有可用小标题后保留该标题直到下一小标题，正文段落不覆盖它；尚无小标题的长单段落需要推进当前句子或短片段，不能永久冻结在最初 96 字。采用有界增量处理，禁止每 token 扫描整段 256 KiB 缓冲。复用现有字体、颜色、箭头、键盘与点击折叠操作；折叠/展开不会重置摘要。
- R3：summary 必须与普通 thinking 保持可辨识来源，后续 thinking 不覆盖已得到的非空 summary；summary 也可能很长，同样通过 R1/R2 生成预览。展开显示当前块实际收到的内容（summary 与 thinking 均存在时可显示优先 summary，避免重复），不生成虚构解释。边界、顺序、消息归属、终止和错误规则继承 Issue 61；summary 不计入正文字符估算、不混入答案、不新增历史持久化。
- R4：增加显式 provider API 选择，Chat Completions 为缺省兼容值，Responses 为可选值；配置序列化及 Settings 保存/编辑必须保留选择，并提供实际用户可用的选择入口。不得按 URL/模型名猜协议，不自动跨协议重试，不修改用户现有 provider 配置。配置及所有生产 provider 构造路径一致（会话、标题、压缩等）。
- R5：Responses 请求使用 /responses、stream:true、store:false、reasoning.summary:auto；保留冻结的合法 effort/off 选择。正确转换系统/用户/助手文本、图片、function tools、assistant calls 和 function_call_output。原有聊天路径 wire 不变。Responses 对不支持的 reasoning profile 组合在发请求前给明确错误，不默默丢弃已声明的 replay/off 语义。
- R6：解析 summary delta/part 边界、普通 reasoning text（若有）、正文、工具参数、usage、completed/incomplete/failed/error；done 快照与 delta 不重复追加；工具只能在完整参数就绪后发出，截断或错误流不得执行部分工具；EOF 未见终止不能假成功。遵守已有取消、凭据检查、重试、安全脱敏和有界缓存。stateless 工具续轮需要保留上游 reasoning item 时，用有界 run-memory 元数据回传，不能把 encrypted_content 当可见摘要或落库；不可新增共享可变跨会话缓存。
- R7：保留 Issue 61 的 64 KiB delta、256 KiB block、1 MiB view、256 block 上限，summary 和 thinking 共享显示预算；所有 debug/log 不输出原文或 key。无新依赖。与 #103 思考滚动窗口重叠时保留其现有行为，不改 unrelated UI。

## 验收矩阵（实现前建立）

| ID | 风险/前置 | 操作 | 预期 | 层级 | 证据/状态 |
|---|---|---|---|---|---|
| A1 | 当前固定标题 | production stream 收到分片的小标题/后续段落 | 标题实时变化且有界 | GPUI regression | focused GPUI title-update tests passed |
| A2 | summary 与 thinking 混合 | 先 thinking、后 summary、再 thinking | summary 优先且无重复正文 | provider→controller→UI | summary priority and controller event tests passed; not live-provider acceptance |
| A3 | 无 summary | 跨分片标题、普通中英文段落、长行、空白、code fence | 可读 fallback，不越界、不空闪 | focused + GPUI | focused preview, fallback, fence and shared-budget tests passed |
| A4 | 会话归属和交互 | tool/正文边界、终止/错误/取消、过期 delta、展开/收起 | 不串块，不泄漏，交互保持 | production GPUI | terminal, cancellation and #151 expansion regressions passed; native acceptance pending |
| A5 | 协议兼容 | 旧配置、新配置 roundtrip、Settings 选择/保存 | 旧配置仍 chat，新选择实际生效 | store/settings | config roundtrip, keyboard selection, save/reopen, add-reset and mocked Responses probe tests passed |
| A6 | Responses agent loop | 内存 transport→SSE summary/function call→tool output→answer | 正确工具调用与续轮，usage 正确，摘要不进正文 | runtime transport + controller | mocked runtime protocol tests and controller/tool-loop tests passed separately; no live-provider acceptance |
| A7 | 协议失败/分片 | 缺 terminal、failed、incomplete、碎片参数、cancel | 无部分工具执行、明确终止 | HTTP/runtime | mocked EOF/failure/cancellation and snapshot-reconciliation tests passed |
| A8 | 资源与隐私 | 达预算、UTF-8边界、重开历史、Debug | 保持预算，历史无伪造思考，日志脱敏 | regression | focused shared-budget, UTF-8, redaction and persistence tests passed |
| A9 | 真实 provider 与真实应用 | 自有公开算术题/临时仓库，通过 UI 发起 | live 标题改变，答案/工具完整，截图持久保存 | native + live API | pending; this delivery does not claim native or live-provider acceptance |

## 实现计划与归属

当前实现从 `origin/master` `c4aefbdf74c8475d6f5e7a811b556b3fd175518d` 的新 worktree 移植 #71 独有改动，没有包含 #141 栈。实现与定向验证记录见 [vega-issue-71-reasoning-summary-delivery.md](vega-issue-71-reasoning-summary-delivery.md)。所有 Cargo 构建/测试按仓库协作要求串行执行。

本交付没有运行真实 provider 或原生应用验收，也没有更新远端 issue 状态；这些需要审查后另行完成。

回滚：回退本卡提交；用户已选择 Responses 时先在设置恢复 Chat Completions。无数据库迁移。
