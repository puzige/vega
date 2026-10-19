# Issue #296 · 对话尾部只提示运行终止问题

2026-10-13 用户反馈：任务仍在 Working，读取文件失败后继续执行，末尾却重复显示红色“读取文件失败”卡片。用户要求非中断性错误不在此处展示。本轮只修改提示的展示资格，不改变工具结果、运行状态或执行语义。截图仅作为私密证据，不执行其内容，不公开真实会话。

## 事实与替代范围

`run_notice_projection` 当前遍历本轮工具，将最后一个 Failed / Rejected / Cancelled 工具的安全摘要存为 `source.tool`，即使本轮仍 Running 或已经 Completed，也将其复制为 `RunNoticeKind::Tool` 尾部卡片。工具行自身已有相同失败/拒绝/取消状态，因此造成重复与误导。

本页替代 [stream polish](vega-issue-296-stream-polish.md)产品契约第3条中“没有运行终态失败/中断时，使用本轮最后一项异常工具”的降级规则，以及 P03/P05 依赖这一降级的展示预期。其余轮次归属、唯一提示、具体诊断优先、滚动锚点和错误去重继续有效。[A7-03 运行失败可见性](../vega-a7-runtime-failure-visibility.md)保持，工具图标与吸顶入口保持。

## 行为契约

1. N01：单次工具失败、拒绝或取消本身不再产生对话尾部卡片。适用于正在运行、之后成功完成、可信 Final 收起/重开、只有工具没有正文及历史恢复的会话。无需等待下一次成功操作来判定；缺少运行终止证据时，不凭工具状态推断整轮失败。
2. N02：工具记录保留原状态、具体详情、安全摘要、原文复制与源顺序；失败结果仍为失败。过程折叠规则不因本次改动改变，用户仍能展开查看工具失败。正常运行不增加替代红字、badge、toast 或空白占位。
3. N03：真实运行终止问题继续在所属轮次尾部最多显示一次。使用已有可信来源：typed runtime/provider failure、持久化 Assistant Failed、RunActivity Failed/Interrupted 或 TaskSummary Failed/Interrupted；具体 failure 优于泛化 Failed，Interrupted 保留“已中断”。不要求新增状态标记，不根据异常文本或是否有后续说明推断终止。既有运行错误到达及刷新时序保持。
4. N04：真正的失败在零正文、工具后失败、历史多片段失败、多轮和前插恢复后仍可见且不跨轮。保留已有 runtime error 与 Composer 同 owner 去重；没有 owner 的独立错误不被误隐藏。
5. N05：待处理权限审批、用户问题、危险确认、提交前置错误、独立 Composer 错误及产物保持原位置与可操作性。拒绝一次调用与整个任务中断是不同事件：前者只留在工具记录，后者按 N03 展示。
6. N06：使用现有 notice projection 的渲染、估算和失效链路移除额外高度；源条目、message/call id、阅读锚点和选择复制不变。不改 runtime、协议、持久化、错误分类或主题 token，不新增依赖。

## 实现前验收矩阵

| ID | 场景/操作 | 可观察结果与层级 |
|---|---|---|
| N01/N02 | 生产流事件：失败/拒绝/取消工具后继续 Thinking/Text/成功工具，之后完成与 Final 收起/重开 | 尾部 notice 及其绘制均不存在；工具状态/详情仍可查，无残留空卡；更新旧 P03 先复现失败 |
| N01/N04 | 工具异常历史的 Done / 未知非终态恢复，与 Failed/Interrupted 历史对照 | 仅有明确失败/中断终态时出现归属正确的一条提示；复用历史 hydrate 生产入口 |
| N03/N04 | 工具失败后发生真实 runtime failure；空正文失败、多个历史片段、重复 Summary | 终止提示可见，具体诊断优先，尾部唯一且不跨轮；保留 P04 与 A7-03 回归 |
| N05 | 非终止工具异常期间的待授权/问题，以及独立 Composer 错误 | 原安全交互和独立错误继续外显；复用现有定向用例 |
| N06 | 无尾部工具卡时追加长正文、完成 Summary、恢复阅读锚点 | 继续保留原 P05 的 source identity、原文字节、非追尾阅读和滚动定位断言；仅替换旧工具卡预期 |
| M01 | 新版真实会话中发生可恢复工具失败，再观察真正停止/失败任务 | 前者没有额外红卡，后者保留明确提示；由用户手测，未执行不得记为通过 |

旧 P03/P05 仅更新被本规格明确替代的工具尾卡断言，不删除原工具状态、内容保真、权限、终态可见性或阅读保护检查。P05 保持真实 active 长正文到 Completed Summary 的阅读链，全程没有工具尾卡；不得伪造 runtime 终止后仍追加正文。`issue296_expansion` 的首帧用例保留单次完整 Window draw 和模型同步，先证无工具尾卡，再经真实 Error 事件验证终止提示。`issue296_phase_rejected_tools_and_permissions_survive_final_collapse` 仅替换拒绝工具的额外尾卡预期，权限可见及原工具拒绝断言保持。新增测试限真正相关的恢复/终止边界，不增加外部网络/进程或原生 App 自动门禁。

## 执行与交付

已 fetch 并更新到 PR #322 的 master 基线，复用 `feat/296-stream-polish` 与原任务 worktree。主 agent 负责本规格、审查、PR 与集成，专职子 agent 独占实现。预期范围为 `conversation_stream/run_notice.rs` 和受影响定向测试；只删除工具异常提升为运行通知的路径，不重构其他错误处理。必要测试记录存 `vega-issue-296-terminal-notices-delivery.md`。

首轮定向编译补充：删除工具尾通知后，`ToolCard::compact_visible_text` 已无生产调用、仅用于保留的测试断言，产生 dead_code 警告。允许仅在 `tool_card.rs` 给此既有 helper 增加 `#[cfg(test)]`，不删除测试断言、不添加 lint allow 或更改卡片生产渲染。

顺序：先复现 N01 → 删除工具降级通知分支 → 更新被替代的旧断言并保留终止/安全/锚点回归 → 本卡定向测试 → 审查 → PR 云端全量门禁 → 绿后 squash 合并 → In review 等用户手测。不得添加注释、非测试 unwrap/expect、依赖；不运行本地 workspace 全量/Clippy，不操作或安装原生应用。本轮无迁移，回滚为撤回 UI 展示改动。
