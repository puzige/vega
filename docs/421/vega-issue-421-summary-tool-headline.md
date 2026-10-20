# #421 最新运行进展与完成收纳

状态：2026-10-20 用户重新确认、生命周期审查修订后的实施规格 v3（尚未冻结测试）。关联 [Issue #421](https://github.com/puzige/vega/issues/421)。本轮生产未改基线为 `e93518cf76a24df921a1af2c2663ebce75f614fd`；任务分支 `feat/421-live-progress-agent-delivery`。

## 目标与替代关系

同一个外层阶段标题，运行时表示**现在正在做什么**，无活动时表示**这个阶段做过什么**。最新且仍在运行的具体工具动作、可读 summary 或 `Thinking` 临时占据标题，并使用既有 Shimmer；活动全部结束即恢复现有工具类别标题，不残留虚假运行态。不增加额外进展行、完成提示或解释文案。

本规格替代本卡旧的“summary 接管直到下次工具提议”契约，以及 [#343](../343/vega-issue-343-live-thinking-title.md) 中与本轮运行标题冲突的部分。旧 PR #423 与摘要来源投影交接仅为历史，不能作为本轮验收证据。用户本轮取消厂商角色分工：统一由 Agent 分析、委派实现、审查、PR/CI、合并与收尾，不等待另一身份接手。

## 先行验收矩阵

所有自动用例均为进程内合成事件／GPUI 测试，不运行真实命令、网络、模型或应用。以下目标在生产修改前写齐并冻结；矩阵不代替实际测试。自动证据当前均为待 Actions，真实 UI 当前为待用户手测。

| ID | 前置与操作 | 可观察结果 | 层级与证据 |
|---|---|---|---|
| A1 | 同阶段已聚合；命令从提议、批准进入 Running，随后 Success，整轮仍未结束 | 仅 Running 时外层为具体命令并有 Shimmer；完成即类别标题、无 Shimmer；提议／批准不冒充执行中 | 真实 ConversationStream 事件与实际阶段渲染 |
| A2 | read → command → summary → 新工具 → summary，各自开始和结束 | 运行标题依最新仍活跃活动切换；结束的命令／summary 不因整轮未结束或重绘继续占据运行态；收纳类别去重、顺序、复数与首工具图标不变 | 合成事件、标题／图标／动效渲染 |
| A3 | 仅 ThinkingDelta；随后有效 SummaryDelta；再追加详细 thinking | 仅详细 thinking 时标题为 `Thinking`，原文只在详情；有效 summary 用可读预览；已有 summary 不被同块详细正文替换；持续思考有 Shimmer | 事件到 UI；源文仍完整 |
| A4 | 空白、未完成 Markdown、围栏、重复可读预览与逐段增量；推进模拟时间但无完成事件 | 不可读内容不抢占已有有效进展；同一 summary 的有效增量更新标题；流式短暂停顿不当完成；不把围栏或原始 thinking 当摘要 | 合成事件与真实预览解析 |
| A5 | 并行工具 A 后 B 开始；B 完成；A 完成；反向提议顺序但开始顺序不同 | 最新真实开始且仍 Running 的 B 优先；B 完成回 A；都结束才类别收纳；依据执行开始而非记录位置 | 真实工具事件／阶段渲染 |
| A6 | 活跃 summary／thinking 期间旧工具收到输出、重复 Running、计时、完成或普通通知 | 旧工具更新不终止或抢走真正最新思考；真正新开始的工具可以成为最新进展；重复 Running 不改变先后顺序 | 交错事件／重绘回归 |
| A7 | 活跃 summary 收到真实响应流结束（含仅 reasoning 无正文）、AssistantResponseCompleted 或非空可见 TextDelta；仅空白 TextDelta 对照 | 有真实完成／可见正文边界才结束思考运行态；无正文的工具轮不能遗漏结束；已结束阶段不再闪动；下一阶段独立决定标题，空白不制造切段；不制造空正文元数据 | runtime → conversation → UI 合成事件与 feed／阶段边界 |
| A8 | 先展开或收起；重复经过活动开始、增量、结束和类别回退 | 用户开合状态不被这些切换重置；底层详情条目与源文不增删改或重排；外层显示摘要 S 时详情不重复 S，回退工具标题后 S 在原位置显示一次 | GPUI 渲染与键盘开合／焦点／aria／估高 |
| A9 | 一条工具或一个 thinking，随后第二逻辑条目 | 一条仍直显、不造文件夹；第二项才聚合；直显运行工具也表达真实运行 Shimmer，完成后保留现有直显规则 | 实际渲染与聚合门槛 |
| A10 | ResponseStarted 检查点后产生工具／summary，随后重试回滚，再有新事件 | 回滚去掉本次临时活动和条目；检查点之前仍真实运行的工具可恢复；新进展不继承被撤销的顺序／标题；类别、详情、开合保持检查点契约 | 真实检查点／回滚事件 |
| A11 | 运行中收到工具失败／取消／拒绝、整轮失败／取消／完成、无 message_id 的 Error、worker 无最终事件时释放运行所有权，以及历史载入 | 终态不继续 Shimmer 或复活；仍有别的活动时回退，整轮真实终态／所有权释放全部停止；不伪造持久化结果；历史记录无虚假运行态；真实错误保留原入口 | 事件、worker 展示清理与历史投影 |
| A12 | 权限／待回答、重连、压缩、失焦窗口／隐藏内容／Reduce Motion | 不以运行标题替代外层权限、重连或错误；待用户操作和终态无误导动效；既有可见性、Reduce Motion 约束不降低 | 进程内状态与 Shimmer 既有门禁；系统视觉由用户手测 |

## 行为契约

1. **真实活动，不是回合状态。** 工具仅在真实 `ToolCallRunning` 生效后算运行；PendingApproval／Approved、已完成、拒绝、损坏、取消均不是正在执行。已运行工具的输出、计时和重复状态不重新排序。并行工具按真实开始顺序选最新仍运行项，结束后回退到剩余最新项。
2. **思考生命周期。** 可读 summary 的有效增量可以刷新最新进展；只有详细 thinking 时使用 `Thinking`，不投影正文为摘要。不可读的 summary 不抢占已有有效工具或摘要。空白／重复预览不构成新的标题优先事件，流式停顿不结束活动。响应完成、可见正文、新工具提议所标记的真实阶段转换、重试／取消／整轮终态结束相关思考；旧工具状态更新本身不结束新思考。不得用超时猜测完成。
3. **标题、图标、动效同源。** 活跃工具沿用其现有具体动作与对象标题（命令保持具体命令，不因详情展开改成泛称）；使用该工具图标。活跃 summary／Thinking 不挂无关工具图标。无活跃项时沿用现有累计类别、首次出现顺序、复数、首工具图标和 96 字符规则；无工具时沿用非工具回退。
4. **既有详情不动。** 不新增、重排、改写或删除底层条目与源文；保留既有当前摘要去重逻辑：摘要占外层时详情不重复同一块，外层回退后该摘要在原位置显示一次，允许由此产生的可见集合和估高变化。完成只恢复标题，不自动关闭手动展开的详情。单条目直显、两项聚合、正文切段、持久化、源文复制、Final 收纳、安全入口与独立 feed 行不变；Final 的既有单次收起不属于本卡移除范围。
5. **无虚假动效。** 复用共享 Shimmer、Motion 与样式 token；仅真实活跃标题提供动效，无活动立即停止。隐藏、窗口失焦、Reduce Motion 继续受既有绘制门禁约束，不增加动画任务或每 token 全量重绘。

## 当前根因与调用链

已读取基线代码，以下为代码事实而非运行复现：

- `crates/vega_ui/src/conversation_stream/content.rs::apply_event` 在工具 Running／Finished 时分别统一设置 `RunPhase::Tool`／`Working`；`RunActivityGroup::set_phase` 会调用 `finish_thinking`。因此旧工具更新会清除新思考，单一 phase 不能表达并行活动。
- `run_activity.rs` 的 `headline_summary` 只记“summary 后是否有新工具提议”，没有真实完成和并行运行先后语义。`stage_title` 与 `stage_aggregate_tool_title` 沿用该旧优先级。
- `render_segment_with_top` 中 `shimmering = !aggregate && stage_thinking_active(...)`，明确排除聚合标题的动效。工具紧凑行也未把真实 Running 状态绑定到标题 Shimmer。
- 独立审查推翻了 v2 的“已有完成事件足够”假设：`vega_runtime/src/agent/loop_.rs` 仅在 `final_text.len() > response_text_start` 时发出 `AssistantResponseCompleted`，仅 reasoning 的工具轮无该事件；`ToolRoundCompleted` 又被 `vega_conversation/src/types/events.rs::from_runtime_event` 丢弃。必须增加不含正文的成功响应流结束投影，不能把现有持久化正文 metadata 伪造为空区间；不改 reasoning 来源语义或 schema。
- `content.rs::finish_agent_message`、`composer_actions.rs::finish_composer_run` 在 worker 无最终事件时只释放 stream 所有权，未停止保留的 group 运行展示；本卡须在该真实终态清理临时活动，但不伪造 durable 工具结果／成功状态。

## 最小实施计划与允许范围

先由专职实现子 agent 在生产未改时写齐上述行为测试和最少测试接线，主 Agent 独立审查；保存基线、完整测试 SHA／patch，建立现有 Actions 的 draft PR 确认有效 RED。禁止在 RED 前改生产或用无关接线／编译失败冒充行为 RED。

确认 RED 后，仅在以下边界实施：

1. `tool_card.rs`：提供 crate 内真实运行态、开始顺序及具体标题访问，复用现有安全投影；直显标题接入既有 Shimmer，不改详情输出、业务状态校验或通用公开 API。
2. `run_activity.rs`：以最小必要活动顺序／来源状态选择最新仍活跃项；所有外层标题、图标、Shimmer、aria、估高共用一致选择；检查点／回滚和终态清理相应状态。保留类别累计与详情顺序。
3. `content.rs`、`thinking.rs`、必要的 `network_retry.rs`／`composer_actions.rs`：只调整活动事件接线，区分旧工具更新和真实思考／响应边界；不可读增量不复活旧标题。真实 worker 释放时停止临时运行展示，保留权限／重连／错误 feed。
4. `vega_runtime/src/agent/mod.rs`／`loop_.rs` 与 `vega_conversation/src/types/events.rs` 及必要事件消费者：提供成功响应流完成的不含正文信号并投影到共享事件，在仅 reasoning／无正文轮次同样结束临时思考；失败／重试仍走原失败和检查点路径。不改变 provider 数据、请求、持久化响应区间或业务执行。先用合成 provider 流验证真正结束才发出，再验证投影及 UI 停止。
5. 上述模块本轮开发测试、必要 Cargo dev-only 接线；不恢复已清除的历史测试集合。测试从本轮验收独立编写，行为命名，不在代码写任务编号或新增注释。

内容无关信号的接口契约：`RuntimeEvent::ReasoningCompleted { response_id }` → `ConversationEvent::ReasoningCompleted { message_id, response_id }`；`AssistantResponseStarted` 同时携带同一请求标识，复用现有 attempt 标识，不以可能重复的正文 byte offset 充当身份。成功流完全耗尽且原校验通过后发出一次；不在收到 Done、静默、失败或取消时提前发成功结束。仅将纯响应校验和新的临时结束信号安排在流结束后的 `wait_ready` 等待之前；保留暂停／停止／取消控制门禁，不提前既有持久化 `ToolRoundCompleted` 或 `AssistantResponseCompleted`，不改工具执行和最终正文 metadata。完成仅终止匹配请求的思考候选，不停止并行工具；重复或旧请求完成不得结束新请求（含相同文本 offset）。现有 `ToolRoundCompleted` 保留原用途，不能用伪造空工具轮代替通用结束。

必要消费者包括 runtime/shared event 的 Debug 与投影、`vega_conversation/src/agent/events.rs`（此信号不落库）、`types/meter.rs` 与 UI `core.rs::feed_meter`（不结束计量／整轮）、UI `network_retry.rs`（成功的空正文响应可解除重连）及 `content.rs`。本轮先行测试须覆盖源流正常／空正文／工具／错误／暂停／重试，真实事件适配顺序、请求身份／幂等、不新增持久化记录、不改变业务／工具状态、不结束计量或整轮（允许停止匹配思考、解除重连等指定临时展示变化）、UI 回退与无最终事件清理。仅缺少这些最小目标接口可标为接口缺失 RED，未执行的行为断言须明确标注；不能把辅助接线编译错误算作有效 RED。

非目标：provider／GPT Chat Completions 摘要来源投影、模型配置、API 语义、数据库、命令执行、权限策略、用户配置或数据、依赖升级、额外 UI 文案与无关重构。若现有 API 或事件事实导致验收无法实现，主 Agent 记录具体缺口并修订规格，在生产未改基线上重开先测后修，不按实现回填预期。

## 规格审查记录

v2 在生产尚未修改、测试尚未冻结和 CI 尚未启动时被独立审查纠正为 v3：补齐 reasoning-only 响应完成与 worker 断开清理这两个已授权目标的实际来源路径。新增验收仍以原生产基线先写测试、确认有效 RED；不使用旧 v2 或历史 PR 的绿色证明 v3。

## 验证、交付与恢复

- 本地只做快速差异、格式、静态检查；不运行 Vega、编译、构建、打包或执行测试。
- 云端命令沿用 `.github/workflows/pr-check.yml`：fmt、全目标 Clippy、三组 Python 检查、`cargo nextest list --workspace --message-format json`、非空时 `cargo nextest run --workspace`。完整门禁为 `check (fmt, clippy, test)`，CI 每 5 分钟查询，通知／用户即时请求除外。
- 有效 RED 后冻结本轮测试目标与预期；最小实现后同一测试云端 GREEN，固定完整 SHA、原始源码／patch 与 run/job、发现／执行／通过／跳过计数，再以独立提交删除 Rust 测试与独占支持。最新 head 的完整门禁必须确认 `no Rust tests`（0 发现／0 执行），不以空集替代 GREEN。
- 正式 PR 不使用自动关闭 Issue 的关键字。最新 head required check 绿后 Agent squash merge，Project 才转 In review，Issue 保持开放等用户手测。合并不等于安装或手测通过。
- 用户用真实 GPT／DeepSeek 会话观察：具体动作 Shimmer、工具完成收纳、summary／Thinking 切换、并行回退、展开保留及错误恢复。截图和私有会话不上传公开仓库。
- 回滚使用本轮集成提交的 revert PR，保留门禁；原测试源码、完整 SHA／patch 与证据留在 PR 可恢复。必要时从证据恢复相同冻结测试再次云端验证，禁止以删测逃避失败。
