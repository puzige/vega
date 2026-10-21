# #421 最新进展与完成态保留

状态：实施规格 v4，用户手测反馈后的独立 TEST-FIRST 周期，完整有效 Actions RED 已固定并冻结测试，最小实现已交付并通过独立生产审查，待同套测试 GREEN。关联 [Issue #421](https://github.com/puzige/vega/issues/421)。本轮基线为已合并 [PR #513](https://github.com/puzige/vega/pull/513) 的 `edc944040b66cfaf196272a6d5577f5c8fde4d70`，任务分支 `feat/421-retained-completion`。以下 v4 为当前契约；下半部 v3 仅保留已交付历史，旧 GREEN 不证明新行为。

## v4 目标与替代关系

快速命令完成后立即回到类别标题，会让具体动作一闪而过。用户选择保留完成态而非显示时长阈值，并进一步确认**命令和可读 thinking summary 都保留**：真实活动有 Shimmer，明确完成后立即停止 Shimmer，标题暂留，等待下一条有效进展直接替换。成功命令使用既有安全投影的 `Ran <command>`，不是改写 `Running` 字符串或把完成伪装为运行。

本轮仅替代 v3 中“命令／summary 完成且无活动就立即恢复类别”的规则及对应 A1／A2／A5／A7／A8 预期。保留 #513 的生命周期信号、请求身份、源文和详情、单项直显、工具类别、手动开合、Final 单次收起及安全入口。整轮结束仍恢复既有类别收纳。用户截图仅用于讨论，不上传公开仓库，不把正文小标题的外观当作 summary 来源证明。

## v4 先行验收矩阵

在本轮生产修改前写齐必要的进程内合成事件／GPUI 测试；不运行真实命令、网络、模型或应用，不整批恢复已删除的 v3 测试。以下为目标而非已取得结果。

| ID | 前置与操作 | 可观察结果 |
|---|---|---|
| B1 | 同阶段已聚合；命令 Proposed → Approved → Running → Success，整轮保持开放；重绘并推进模拟时间 | 只有真实 Running 激活具体命令和 Shimmer；成功后立即为 `Ran <command>`、命令图标、无 Shimmer，不插入类别帧，无超时退回 |
| B2 | 保留命令后依次收到新响应开始、工具提议／批准、空白／不可读 summary，再收到真实工具 Running 或有效 summary／详细 thinking | 前几类不冒充新活动或抹去保留态；真实新活动直接替换。仅详细 thinking 显示 `Thinking`，原文留在详情；它结束不保留 `Thinking` 或复活更早的已替换完成态 |
| B3 | 可读 summary 更新并明确完成，含 reasoning-only、匹配 AssistantResponseCompleted、可见正文／新提议边界；仅空白正文及流式停顿对照 | 有效 summary 活跃时 Shimmer，明确结束后同一可读标题静止保留；空白／停顿不结束。新 summary 替换旧标题，已结束块的迟到增量／重复或旧请求完成不夺取新进展 |
| B4 | 并行工具 A 后 B 真实开始；B 完成仍有 A；随后 A 完成；交错 summary／thinking 活动以及旧工具输出／重复 Running／完成／计时 | 任何真实活动优先于完成内容；B 完成先回 A；无活动时保留最近有效激活的可保留项 B，而不是按最后到达的完成事件改成 A。旧更新不重新排序或盖住新活动／完成态 |
| B5 | 新命令真实 Running 后 Failed／非零退出码／Cancelled；无 Running 的 Rejected／reused／损坏结果；新非命令工具运行后完成 | 已运行命令可保留真实失败／取消标题但不显示 `Ran` 或 Shimmer；拒绝／复用／损坏不凭结果造“执行过”。非命令工具仍沿用既有完成展示，不复活已被它替换的旧命令／summary；必要错误和待处理入口不变 |
| B6 | 展开／收起阶段，在命令和 summary 的运行／保留／替换之间切换；检查实际标题、图标、aria、焦点、估高及源文 | 开合状态保持；标题、图标、Shimmer 和去重使用同一选择。保留 summary 在外层时仍不在详情重复；被替换后在原位置恢复一次。单项直显、两项聚合、直显完成工具详情、类别顺序／复数／首图标／96 字符规则不变 |
| B7 | 检查点前已有保留项或真实运行工具；请求内产生新活动后重试回滚；可见正文关闭阶段并创建新阶段 | 回滚清除本次新活动／保留态，恢复检查点前合法展示；检查点前 Running 的工具若已真实完成，回滚后仍保持终态、无 Shimmer，不复活已结束思考或伪造工具状态。阶段保留不串到新阶段，不改变正文边界或详情顺序 |
| B8 | 保留或运行时整轮 Completed／Failed／Interrupted、无归属 Error、worker 释放；历史载入；权限／待回答、压缩、隐藏／失焦／Reduce Motion | 终态／所有权释放清除临时保留并停止动画，恢复既有类别或无工具回退，不伪造 durable 成功。历史静态；权限／重连／错误独立可用；绘制门禁不降低 |

## v4 精确行为与边界

1. **活跃优先，完成其次。** 同一阶段先按 #513 规则选择最新且仍活跃的工具／thinking；没有活跃候选时才展示可保留的最近有效进展。有效进展顺序来自真实工具首次 Running 或思考有效激活／可读摘要变化，不来自完成到达顺序。并行剩余活动退场后，先在所有类型中选最近有效激活项，再判断它能否保留，不先过滤为“历史最新可保留项”；已被较新有效进展替换的旧项不作为保留态重新出现，仍在活跃的旧项可按上述优先级显示。
2. **保留范围。** 本轮保留经过真实 Running 的 Bash 命令，以及已通过 summary 路径形成可读预览且明确结束的摘要。完成命令沿用安全投影，失败、取消、非零退出仍是真实失败／取消文案；损坏、拒绝、无 Running 的复用结果不成为完成候选。已保留命令若因非法转换变为损坏，立即失去保留资格，不缓存旧 `Ran` 或复活更早完成项，沿用真实损坏展示。其他工具活跃时照常接管，完成后保留原展示规则；详细 thinking 只在活跃时显示 `Thinking`，结束后不把其原文或 `Thinking` 当作保留摘要。
3. **有效替换而非任意事件。** 新响应开始、提议、批准、token 暂停、空白、未形成可读预览的 summary、重复 Running、旧输出／计时／重绘不提升顺序，不清除合法保留。新提议仍按既有入口明确结束当前思考；它自身尚不是工具运行，因而已完成摘要可留到实际新进展。新详细 thinking 或其他工具的真实活动会替换旧完成项；若它们完成时没有可保留内容，使用既有非保留回退，不恢复旧项；特别是无工具的 summary S → 新详细 thinking → 结束，在开放 run 内回退 `Thought` 而非历史 S，整轮终态仍按第 5 条处理。
4. **完成与来源真实。** 使用既有匹配请求的 `ReasoningCompleted`、合法响应完成、可见正文、新工具提议和终态边界；不添加超时／阈值，不改 provider 的 thinking／summary 分类。完成摘要保留其结束时可读标题，迟到片段不让已结束标题变为新进展；底层源文继续按既有规则保留。摘要图标不挂无关工具图标，命令使用自己的工具图标。
5. **生命周期有界。** 保留只属于当前 run 内原阶段；新阶段不继承前阶段标题。检查点／回滚记录并恢复必要选择来源、顺序和保留状态，移除尝试内新增项；不快照或回写工具生命周期，检查点前已真实完成的工具不得被恢复为 Running／Shimmer。整轮完成／失败／取消或 presentation ownership 释放时清除临时保留，不改 durable 工具结果；阶段恢复类别（无工具沿用原非工具回退），历史载入不重建临时保留。
6. **同位置、同投影。** 不新增说明行、常驻图标提示、等待阈值或完成 toast。聚合标题、静止／动态 StageText、图标、aria、摘要去重和估高共用相同决策。保留用户开合、焦点／键盘行为与源文复制；直显完成工具沿用既有详情展开标题，不为保留策略改写内层标题。待用户处理时停动画但不丢失必要提示；压缩和可见性门禁沿用现状。

## v4 根因、最小计划与允许范围

已静态读取 `edc94404`：`RunActivityGroup::stage_headline` 只在 `tool_starts` 中选仍 `is_running()` 的工具；`finish_thinking` 直接移除 `current_thinking`，没有完成摘要选择，因此无活动立刻进入类别回退。`ToolCard::build_bash_summary` 已有 `Ran` 和真实失败／取消文案；`StageText::set(..., None, ...)` 已能静止绘制。无需再改 runtime／conversation 的完成信号、命令执行或 provider 来源。

1. 专职子 Agent 先在未改生产上写 B1–B8 所需最少有效测试及仅测试接线；独立审查后建立新 draft PR，通过现有 Actions 取得行为 RED，再冻结目标／断言／源码指纹。
2. 有效 RED 后，专职子 Agent 最小修改 `crates/vega_ui/src/conversation_stream/run_activity.rs` 的候选选择／必要状态、检查点／回滚／release，保持活跃优先、阶段所属与完成静止。`tool_card.rs` 仅在需要时增加 crate 内只读资格查询，不扩公共 API／改安全投影。`thinking.rs`／`content.rs`／`network_retry.rs` 只在必要生命周期接线时修改，不重做 #513。
3. 实际渲染、aria、summary 可见去重／估高继续用统一 headline；保留 observer 既有完整高度失效机制，不增加重复框架。复用 Shimmer／Motion／样式，避免新增任务和 token 全量渲染。
4. 生产实现经非写入者独立审查，固定相同测试 GREEN 的完整 SHA／源码／原始 patch／run／job／准确计数，独立删测并独立核对生产等价，最终最新 head 完整门禁确认 `no Rust tests`（0 发现／0 执行）后转正式 PR、squash merge、In review 等用户手测。

非目标：全工具完成态策略扩展、provider／GPT 摘要来源修改、运行时或共享事件重做、命令执行、权限／数据库／配置／数据、依赖升级、增加 CI 调度基础设施、已合并的统一 Agent 工作流重写。没有新依赖授权。历史夹具只可逐项复用必要基础设施并记录来源，不恢复 v3 的冲突断言或整个测试集合。

## v4 验证与恢复记录

- 本地仅快速差异、格式和静态检查；不运行 Vega、不编译、构建、打包或执行测试。云端沿用现有 `pr-check` 的 fmt／Clippy／三组 Python／Nextest，5 分钟查询，通知／用户即时要求除外。
- 当前：在生产基线 `edc944040b66cfaf196272a6d5577f5c8fde4d70` 未改时完成先行测试与独立复审，夹具中性修正后的完整 Actions 行为 RED 已经独立确认，冻结目标／数据／断言／预期。专职子 Agent 随后交付最小生产补丁，非写入者独立审查已批准；同套测试 GREEN、删测及最终门禁尚未取得，不引用 v3 的 27／27 作为本轮结果。
- 测试独立预审（未冻结、未运行）：暂定源码 SHA-256 `89e5308722090f75d1bc65b1b43fe7faf159f82a3a5b2e4003b52677065244d4`，静态 30 个定义，不是发现／执行计数。审查发现三处必须在生产修改前补齐的区分场景：同阶段且无工具的旧 summary → 新详细 thinking → 结束必须 `Thought`；先有效成功并保留的命令随后收到非法转换必须失去成功保留；检查点前 Running 工具在 retry 前真实完成，回滚不复活其 Running。此暂定版本不接受为完整冻结测试；不以夹具失败替代产品 RED。
- 三处预审缺口已由专职子 Agent 在生产未改时补齐，保留原有初始损坏／真实存活工具对照。最新先行源码 `crates/vega_ui/src/conversation_stream/retained_headline_tests.rs` 为 SHA-256 `4e85e8da81db7eccd89bae64e03c0b044fba92e97030a868847fea451a440262`，静态 32 个测试定义，不是执行计数。目标覆盖 B1–B8；来源为本轮新行为断言，逐项复用 `ce2b3cef60904fb5cf2a0e761bca9fad000d1f86` 的自然高度 host、实际 aria／StageText 观察、帧门禁、模拟时钟和焦点基础夹具，不恢复旧 suite／冲突的立即类别回退断言。测试观察模块置 EOF，Cargo 增量仅 GPUI dev-only 支持与 runtime 类型访问，锁文件 914 → 924，保留包元数据／checksum 不变。
- 主会话本地静态：定向 `rustfmt --edition 2024 --check --config skip_children=true`、`cargo metadata --no-deps --offline --locked --format-version 1`、`git diff --check` 均退出 0；不代表类型检查、行为或像素验收。源码生产投影在移除精确测试接线后与基线一致；runtime／conversation／workflow 未改。最终独立复审已核对三处补齐、32 个定义与源码指纹前后一致，无静态阻断；仅批准进入 draft PR 的 Actions RED，不代表编译／行为已通过。以下保留首次 Actions、夹具中性修正及完整有效 RED 证据；后续保持同一冻结测试，不按实现输出改写。
- 首次完整先行提交 [`15521a36d176390785fb15871f85b18ae5ce209c`](https://github.com/puzige/vega/commit/15521a36d176390785fb15871f85b18ae5ce209c)／[原始 patch](https://github.com/puzige/vega/commit/15521a36d176390785fb15871f85b18ae5ce209c.patch)，[draft PR #520](https://github.com/puzige/vega/pull/520)。[Actions 39525834372](https://github.com/puzige/vega/actions/runs/39525834372) 的 [Nextest 118980135649](https://github.com/puzige/vega/actions/runs/39525834372/job/118980135649) 实际 32 发现／32 执行／8 通过／24 失败／0 跳过；Python 5／15／14 通过；[fmt／Clippy 118980135813](https://github.com/puzige/vega/actions/runs/39525834372/job/118980135813) 成功，[required 118981711605](https://github.com/puzige/vega/actions/runs/39525834372/job/118981711605) 失败。非接口缺失；所有测试已编译并执行。
- 独立逐项日志分类：23 个真实产品 RED（9 个命令保留、13 个 summary 保留、1 个无工具旧摘要复活）；其他断言在首次失败之后尚未执行，不声称全部边界已验证。另 1 个 `a_single_completed_command_keeps_its_existing_expanded_detail_title` 在普通 window update 调用 render-scoped disclosure focus，经 keyed state → `current_view` 触发 GPUI phase panic，不算产品 RED。[首次失败记录](https://github.com/puzige/vega/pull/520#issuecomment-6305659384) 公开保留。
- 夹具中性修正已在编辑前独立批准：仅从既有 cfg(test) 工具卡渲染观察器保存实际 `.track_focus` 使用的同一 FocusHandle，在测试取回它代替 render-scoped 查询；保留 focus／render_frame／Enter、全部断言／事件／预期／32 个用例。不能直接设置 expanded 或造替代焦点。该修正已委派完成并经独立逐字复审批准，测试仅查询替换，生产仍未改。修正后测试 SHA-256 `c5ebcddf52b11db5727ce5acb0ebc19d384e5a16f074486115687cf3eba44501`，tool-card 文件 `d872470de6e681a80171bf6fda7a20669a9589270b39bdf22e22615c8a9675e2`；定向 rustfmt／diff check 退出 0，不运行本地测试。修正提交 [`517fd1a3c5f21c3144b36e2898751bd110694d12`](https://github.com/puzige/vega/commit/517fd1a3c5f21c3144b36e2898751bd110694d12)／[原始 patch](https://github.com/puzige/vega/commit/517fd1a3c5f21c3144b36e2898751bd110694d12.patch)。
- **完整有效行为 RED／冻结**：[Actions 39527668640](https://github.com/puzige/vega/actions/runs/39527668640)／[Nextest 118986034377](https://github.com/puzige/vega/actions/runs/39527668640/job/118986034377) 在生产未改 head `517fd1a3c5f21c3144b36e2898751bd110694d12` 实际 32 发现／32 执行／8 通过／24 失败／0 跳过，三组 Python 5／15／14 通过；[fmt／Clippy 118986034785](https://github.com/puzige/vega/actions/runs/39527668640/job/118986034785) 成功，[required 118987436166](https://github.com/puzige/vega/actions/runs/39527668640/job/118987436166) 失败。独立逐项确认全部 24 个失败均为统一实际渲染标题断言（10 命令、13 summary、1 历史摘要复活）；已无夹具 phase panic。单工具键盘展开及原有完成标题断言已执行通过，随后聚合才在 `Run commands` 对 `Ran printf single` 处有效失败。冻结测试指纹 `c5ebcddf52b11db5727ce5acb0ebc19d384e5a16f074486115687cf3eba44501`，允许开始专职最小实现；每个首次失败之后的后续断言仍待同一套测试 GREEN，不将 RED 当边界全通过。
- 最小实现交付：仅 `run_activity.rs` 与 `tool_card.rs`，生产差异 +106／−8。每阶段记录最近有效进展，先选真实活动，再判断最近进展能否保留；完成摘要冻结标题并保留原 entity；命令通过真实 Running 的阶段记录及当前合法终态判定，不缓存 `Ran`。检查点只恢复选择来源，不回写工具生命周期；release 在结束思考后清除保留。测试源码指纹仍为 `c5ebcddf52b11db5727ce5acb0ebc19d384e5a16f074486115687cf3eba44501`，测试观察／接线未改，stream／provider／Cargo／CI 未改。定向 rustfmt 与 `git diff --check` 退出 0；非写入者独立生产审查无阻断并复核指纹稳定：run-activity `bd9fe8ffda1bf3d623301de86a43a9d9ded0399d3d1957a049bce2034fb2fac8`、tool-card `e9e32caf4cccb1254b7a80af9e9d0ed947547f3486d50943d94e0090b7254e27`，生产 diff `b1299590ae1af7077368d2fb1e5d73dd2a3732749463d30fac3d1f2886368733`。编译和行为仅待同套测试云端验证。
- 用户手测目标（尚未执行）：真实会话快速命令不再闪回类别，完成命令为 `Ran` 且静止，完成可读摘要静止保留，下一进展直接替换；并行回退、权限／失败、手动展开及 Final 收纳正常。合并不等于应用已安装，未明确通过前不关闭 Issue／Done。
- 回滚：本轮集成提交的 revert PR，原始测试及 RED／GREEN 留在新 PR 可恢复；不撤销 #513 的无关生命周期与工作流改动。

---

# v3 已交付历史（不是 v4 当前验收）

历史状态：v3 已通过独立删测和最终空集门禁，并经 [PR #513](https://github.com/puzige/vega/pull/513) squash 合入 `edc944040b66cfaf196272a6d5577f5c8fde4d70`。最终任务 head 为 `f4a1e0d9b5276ed8eea79f0bb783486d42b819e4`，与集成 tree 相同；[最终 Actions 39415626514](https://github.com/puzige/vega/actions/runs/39415626514) 为 `no Rust tests`（0 发现／0 执行），不是新周期 GREEN。下文“待删测”等措辞是当时阶段记录，后续完成证据见 [PR 最终记录](https://github.com/puzige/vega/pull/513#issuecomment-6289993271)。原始生产未改基线为 `e93518cf76a24df921a1af2c2663ebce75f614fd`，RED 前集成 `9baca7519d97995267de31eb9c2e213f0c171e67`，原任务分支 `feat/421-live-progress-agent-delivery`。

## 目标与替代关系

同一个外层阶段标题，运行时表示**现在正在做什么**，无活动时表示**这个阶段做过什么**。最新且仍在运行的具体工具动作、可读 summary 或 `Thinking` 临时占据标题，并使用既有 Shimmer；活动全部结束即恢复现有工具类别标题，不残留虚假运行态。不增加额外进展行、完成提示或解释文案。

本规格替代本卡旧的“summary 接管直到下次工具提议”契约，以及 [#343](../343/vega-issue-343-live-thinking-title.md) 中与本轮运行标题冲突的部分。旧 PR #423 与摘要来源投影交接仅为历史，不能作为本轮验收证据。用户本轮取消厂商角色分工：统一由 Agent 分析、委派实现、审查、PR/CI、合并与收尾，不等待另一身份接手。

## 先行验收矩阵

所有自动用例均为进程内合成事件／GPUI 测试，不运行真实命令、网络、模型或应用。以下目标在生产修改前写齐并冻结；矩阵不代替实际测试。完整先行提交已有 Actions 接口缺失 RED；实现后的相同目标／断言在独立审查的接线修正后，已取得 27 发现／27 执行／27 通过／0 失败／0 跳过的 GREEN；真实 UI 为待用户手测。

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

## 本轮测试冻结与 RED 记录

- 完整先行源码提交 [`35d51e369db7ca347be4e5444fa8885cff357926`](https://github.com/puzige/vega/commit/35d51e369db7ca347be4e5444fa8885cff357926)，[UI 原始补丁](https://github.com/puzige/vega/commit/35d51e369db7ca347be4e5444fa8885cff357926.patch)；生命周期源码与接线提交 [`fd0647c7e7ff62f31041bf4c686a86edd11c2acc`](https://github.com/puzige/vega/commit/fd0647c7e7ff62f31041bf4c686a86edd11c2acc)，[原始补丁](https://github.com/puzige/vega/commit/fd0647c7e7ff62f31041bf4c686a86edd11c2acc.patch)。静态定义 27 个（runtime 5、conversation 2、UI 20），不是执行计数。
- 完整提交 [Actions 39401665593](https://github.com/puzige/vega/actions/runs/39401665593)：[Nextest 118557592186](https://github.com/puzige/vega/actions/runs/39401665593/job/118557592186)、[Clippy 118557592454](https://github.com/puzige/vega/actions/runs/39401665593/job/118557592454) 的编译诊断仅为最小目标 `ReasoningCompleted`／`AssistantResponseStarted.response_id` 缺失。属于接口缺失 RED；行为断言未执行，发现／执行／通过／跳过计数未取得。早先部分提交的 RED 不替代本完整记录。
- 独立静态审查在生产未改时修正了合法正文 metadata、暂停控制配置、真实计时更新、请求身份、详情回退、断连清理，以及传输重试同 ID／无第二次 Started 的 fixture。没有放宽验收、忽略用例或本地执行。测试目标、断言和源文指纹在 RED 后冻结，实现中不得按结果改写。
- SHA-256：runtime `4e7075f33804f0a96c2bcacd3e91940353ebddc8b4448afb70000a8857024a4d`；conversation `94e8868711395235cc5dd152a8d672fe58a11bb48fe4fc34b9a5ad88c32fc431`；UI `4e70ab5d810d3c0c21e179480a1b21d8d5488cad4a4ee65b0e19f29fc967476e`。原始测试与过程保存在 [PR #513](https://github.com/puzige/vega/pull/513)。原始源码不改写；后续仅允许已留证并独立审查、保持目标和断言不变的测试自身接线修正。
- 实现提交：runtime／conversation [`9588d0d399b038793c1ba07db98290449788100f`](https://github.com/puzige/vega/commit/9588d0d399b038793c1ba07db98290449788100f)；UI [`de614570d471f5409ca30b965620530fbfe99226`](https://github.com/puzige/vega/commit/de614570d471f5409ca30b965620530fbfe99226)。两部分均由非写入者独立静态审查；独立子 Agent 提议入口的清理遗漏已在实现中修复并关闭 finding。
- 测试自身接线修正：[`1aa8c762d42e2fe61df2a4756d0814ab69d6a0a3`](https://github.com/puzige/vega/commit/1aa8c762d42e2fe61df2a4756d0814ab69d6a0a3.patch) 仅移除未用 import、克隆同一焦点句柄后调用 focus；[`ce2b3cef60904fb5cf2a0e761bca9fad000d1f86`](https://github.com/puzige/vega/commit/ce2b3cef60904fb5cf2a0e761bca9fad000d1f86.patch) 仅使用自然高度测试子容器、等待异步窗口激活回调、原样后移两个测试观察模块。每次均先留证并独立批准、逐字重建读回，未改断言／容差／预期／业务事件／用例数或生产正文。保留编译失败和随后 22 通过／5 失败／0 跳过的 [run 39409350936](https://github.com/puzige/vega/actions/runs/39409350936)，不将夹具错误算产品 RED 或隐去失败。
- 带测试 GREEN：[`ce2b3cef60904fb5cf2a0e761bca9fad000d1f86`](https://github.com/puzige/vega/commit/ce2b3cef60904fb5cf2a0e761bca9fad000d1f86) 的 [Actions 39411606422](https://github.com/puzige/vega/actions/runs/39411606422)，[Nextest 118590464255](https://github.com/puzige/vega/actions/runs/39411606422/job/118590464255) 实际 27 发现／27 执行／27 通过／0 失败／0 跳过；Python 5＋15＋14 通过；[fmt／Clippy 118590463990](https://github.com/puzige/vega/actions/runs/39411606422/job/118590463990) 与 [required check 118591769579](https://github.com/puzige/vega/actions/runs/39411606422/job/118591769579) 成功。修正后 UI SHA-256 `467aef1ed563779a7318e4fb3d25724b457bc0cc0ed9b63b4dadd19390ff32ae`；runtime／conversation 指纹未变。源码／完整 SHA／原始补丁及原始 run/job 已在 [PR 证据](https://github.com/puzige/vega/pull/513#issuecomment-6289427564) 固定并读回，独立删测与最新 head 空集门禁仍待完成。

## 验证、交付与恢复

- 本地只做快速差异、格式、静态检查；不运行 Vega、编译、构建、打包或执行测试。
- 云端命令沿用 `.github/workflows/pr-check.yml`：fmt、全目标 Clippy、三组 Python 检查、`cargo nextest list --workspace --message-format json`、非空时 `cargo nextest run --workspace`。完整门禁为 `check (fmt, clippy, test)`，CI 每 5 分钟查询，通知／用户即时请求除外。
- 有效 RED 后冻结本轮测试目标与预期；最小实现后同一测试云端 GREEN，固定完整 SHA、原始源码／patch 与 run/job、发现／执行／通过／跳过计数，再以独立提交删除 Rust 测试与独占支持。最新 head 的完整门禁必须确认 `no Rust tests`（0 发现／0 执行），不以空集替代 GREEN。
- 正式 PR 不使用自动关闭 Issue 的关键字。最新 head required check 绿后 Agent squash merge，Project 才转 In review，Issue 保持开放等用户手测。合并不等于安装或手测通过。
- 用户用真实 GPT／DeepSeek 会话观察：具体动作 Shimmer、工具完成收纳、summary／Thinking 切换、并行回退、展开保留及错误恢复。截图和私有会话不上传公开仓库。
- 回滚使用本轮集成提交的 revert PR，保留门禁；原测试源码、完整 SHA／patch 与证据留在 PR 可恢复。必要时从证据恢复相同冻结测试再次云端验证，禁止以删测逃避失败。
