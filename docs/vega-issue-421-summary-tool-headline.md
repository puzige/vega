# #421 聚合阶段标题在 summary 与工具之间切换

状态：2026-10-16 首轮实现已由 PR #423 合入；同日 GPT Chat Completions 手测未通过，以下追加修订待执行。关联 [Issue #421](https://github.com/puzige/vega/issues/421)，追加修订以 `8c69ef64` 为基线。本文件最初只修订 [#343 的思考与工具标题规则](vega-issue-343-live-thinking-title.md)中“阶段已有工具时 summary 永不重新占据外层标题”的部分；追加修订再补齐 Chat Completions 的来源分类。其余阶段边界、单条目门槛、工具类别、展开、错误和权限规则保持不变。

## 问题与目标

当前 `RunActivityGroup::stage_title` 在聚合阶段只要有工具就选工具类别，渲染又单独优先使用 `stage_tool_title`。因此工具后新收到的可读思考 summary 只会进入展开内容，外层仍显示 `run commands, edit files, read files, write files` 一类活动标题，用户看不到最新进展。

外层标题改为反映**最后到达的可读 summary 或新工具调用**。新可读 summary 到达时暂时显示 summary；随后有新工具调用时立即恢复已有工具类别标题。重复执行这一切换，不追加状态、次数或其它前缀。工具类别的历史累计、首次类别顺序、首个工具图标及 96 字符截断均沿用现状。

## 行为契约

1. 聚合阶段有工具时，默认沿用当前类别标题。每次新的 summary 增量使思考块产生**新的可读预览**时，外层立即显示该预览，直到下一次**新的工具调用提议**。同一个 summary 后续可读增量继续刷新外层标题。下一次新工具调用到达即切回工具类别标题；之后的另一次可读 summary 可以再次接管。
2. 空白、尚未成形的 Markdown、围栏代码和没有改变可读预览的重复 delta 不抢占标题；原始 thinking content 不抢占工具类别，也不直接作为外层标题。工具开始、结束、成功、失败、拒绝、计时和渲染通知都不是“新工具调用”，不能自行切走仍有效的 summary。
3. 新工具调用之后，旧 summary 不能因重绘、ThinkingChanged 的非标题变动或工具状态更新而重新出现。只有**此后**新的可读 summary 更新才能重新接管。阶段切分后，新阶段独立决定标题，不继承上一阶段的显示来源。响应重试回滚要恢复检查点时的标题来源，不保留本次失败响应的临时 summary/工具优先权。
4. 外层正在显示 summary 时，展开区不重复显示**该同一个思考块**的 summary 行；既有其它思考、工具和产物保持原顺序、原内容。新工具把外层切回类别标题后，该 summary 在展开区出现一次。手动展开状态、焦点、aria 名称及估高随标题来源同步更新，不因标题切换自动开合。
5. 只有一个逻辑过程条目时仍直接显示原条目，不造聚合外壳；第二个条目出现才聚合。纯 thinking 且无可读 summary 仍显示 `Thinking`/`Thought`；仅有可读 summary 的阶段继续使用现有标题与详情规则。GPT 仅 summary、DeepSeek 仅详细 thinking 两种来源都不能因本次修订失效。
6. 外层显示工具类别时保持现有小写复数短语、不计数、不加 `Worked on`；图标仍取本阶段第一个工具。summary 占据标题时不保留不相符的工具类别图标，恢复工具标题时恢复该图标。权限请求、连接重试和整轮错误仍在独立 feed 行，工具级结果仍在展开详情；本次不改 provider 事件、持久化、原文复制或最终答案。

## 合成事件验收

| 场景 | 预期外层标题与展开区 |
|---|---|
| `summary A → read` | 单项时 `A`；工具作为第二项到来后聚合并显示 `read files`，展开可见 `A` 一次。 |
| `read → command → summary B → 工具状态变化` | 依次显示 `read files, run commands` → `B`；状态变化后仍为 `B`，展开不重复 `B`。 |
| 上述过程再来 `edit → summary C → write` | 依次切回累积的类别标题 → `C` → 更新后的类别标题；同类类别只出现一次，图标始终取首个 read 工具。 |
| `summary B` 后仅追加空白／不完整标记／thinking content | 外层保持 `B`；没有新的可读 summary 就不切换。 |
| 用户先展开，再经历多次切换 | 聚合区保持展开；内容顺序、当前 summary 去重、焦点与 aria 同步；收起后继续保持收起。 |
| 工具／summary 在重试响应中产生，然后回滚 | 检查点前的条目与标题来源恢复；回滚后的新事件按到达顺序重新接管。 |
| 纯 thinking 或单条工具 | 维持既有单条目和 `Thinking`/`Thought` 行为，不显示空聚合。 |

测试在 `crates/vega_ui/src/conversation_stream/tests` 使用合成事件覆盖以上切换及标题实际渲染，修正与新规则冲突的 #343 旧断言。检查点需要覆盖新增的标题优先状态；不要仅以 `stage_title` 的单元断言代替实际标题、图标、展开内容验证。历史载入按现有持久化能力验收，不凭空恢复未持久化的 thinking 文本。

## 交付与验证

代码由专职实现 agent 完成，主 agent 审查与集成。Vega 本地仅运行快速格式、差异和静态检查；不在本地编译、构建、运行测试或启动应用。完整 fmt、Clippy 与测试由 PR Check 执行，合并后用户以真实 GPT/DeepSeek 会话手测标题切换。公开 Issue、PR 和测试仅使用合成文本，不包含截图中的私人会话内容。

## 2026-10-16 手测修订：Chat Completions 的 GPT 思考摘要

### 失败证据与根因

用户在包含 PR #423 的 Vega 0.1.134 上用 GPT API 手测：工具记录之后出现新的思考短标题，外层聚合标题却仍是工具类别。该会话为 Native 后端，所选 GPT 模型的提供方配置使用 Chat Completions。这里没有另一个显式的 `reasoning_summary_text` 通道：`openai/sse.rs::SseAssembler::absorb` 把 `delta.reasoning_content` 无条件映射为 `ProviderEvent::ThinkingDelta`；agent loop 和 `from_runtime_event` 保持此类型，`ConversationStream::append_reasoning` 因 `summary=false` 只更新详细思考预览。内层 `ThinkingBlock::title` 可显示该预览，但 #421 的 `RunActivityGroup::mark_summary_preview` 只接受 `SummaryDelta` 形成的 `summary_title`，所以外层仍展示工具类别。此前只用合成 `SummaryDelta` 验证了 UI，漏掉真实 GPT Chat Completions 来源。此结论来自手测、实际选中模型与 API 类型的只读元数据，以及代码路径；未抓取或公开私有 API 原文。

Chat Completions 的 `reasoning_content` 字段本身没有标明它是可展示摘要还是详细思考，不能对所有提供方统一改成 summary。此轮按**已声明的精确提供方／模型能力**决定展示语义：对声明了 `openai_chat_completions` 的 GPT 模型档案，缺省把这一字段按用户可见 summary 投影；档案可以显式改回 detailed。其他模型、缺档案及未知协议缺省仍按 detailed，DeepSeek 现有行为不变。这是用户确认本次 GPT API 行为后的受限规则，不允许仅凭任意会话标题、正文样式或提供方别名猜测。Responses API 的显式 `reasoning_summary_text` 和 `reasoning_text` 仍各按原事件类型处理。本节只在该来源投影上取代上文“本次不改 provider 事件”的旧范围句及 #343 对已声明 GPT 档案完全不设缺省语义的旧假设，UI 其余规则不变。

### 追加验收矩阵（先于实现）

| 前置与操作 | 可观察结果 | 自动验证层级 |
|---|---|---|
| 已声明的 GPT Chat Completions 档案，收到 `reasoning_content: A` → 两个工具 → 新 `reasoning_content: B` | `A` 到工具后显示累积工具类别；`B` 到达后外层立即显示可读的 `B`，展开区不重复同一块；随后新工具提议使外层恢复累积类别，再来的摘要可再次接管 | 合成 SSE → runtime/conversation 事件，加 #421 GPUI 阶段断言；GitHub Actions |
| 同一 GPT 档案，summary 空白、未闭合 Markdown 或没有改变可读预览 | 不抢占外层；下一次有可读预览才切换；工具状态更新不提前清掉有效 summary | 事件与 GPUI 断言；GitHub Actions |
| DeepSeek 或未声明／未知 Chat Completions 档案输出 `reasoning_content` | 仍是详细 `ThinkingDelta`；有工具时外层显示类别，详细内容只在展开区；单条目不多造外壳 | provider/runtime 与 GPUI 断言；GitHub Actions |
| GPT 档案显式设 detailed，或非 GPT 档案显式设 summary | 以前者的详细规则、后者的 summary 规则投影；无效声明拒绝或安全降级，不静默改变请求 | 配置解析／冻结与事件断言；GitHub Actions |
| GPT Chat Completions 档案开启 `preserve_reasoning_content`，summary 后执行工具并继续下一轮 | UI 按 summary 展示，同时下轮请求仍带原始 `reasoning_content` 的**精确字节**，只在既有内存生命周期保存；关闭该选项时不回传 | runtime 两轮 mock 与 wire body 断言；GitHub Actions |
| Responses API 同时收到显式 summary 与详细 `reasoning_text`；任一 API 流重试回滚 | 显式 summary 优先、详细思考不替换它；回滚恢复标题来源和工具检查点，没有重复或残留摘要 | parser/runtime 与 #421 GPUI 回归；GitHub Actions |
| 工具之后 GPT 摘要出现时阶段已由用户展开，或当前未展开 | 标题即时刷新、手动开合状态不变；摘要作为标题时同一块不重复，工具结果与权限／重连／整轮错误仍在原有层级 | GPUI 焦点、aria、可见子项及估高断言；GitHub Actions；用户手测 |

### 实现切片与边界

1. 在 `vega_store::reasoning::ReasoningProfile`、`vega_conversation::types::ReasoningProfileProjection`、`vega_runtime::provider::FrozenReasoning` 增加用于 **Chat Completions `reasoning_content` 的展示语义**（summary/detailed）。仅已声明 `openai_chat_completions` 的 GPT 档案在字段缺省时采用 summary；其它缺省 detailed；显式声明优先，非法值不得悄悄回落。只影响事件展示分类，不改变思考档位、请求体、提供方选择或现有原文回放开关。
2. 在 `openai/sse.rs` 保留 `reasoning_content` 的 Chat Completions 来源信息，不要直接用现有 `ProviderEvent::SummaryDelta` 抹去它的回放身份。可增设来源专属事件；`agent/loop_.rs` 根据本轮冻结档案投影为 `RuntimeEvent::SummaryDelta` 或 `ThinkingDelta`，同时按原有 `preserve_reasoning_content` 精确保留该字段供下一工具轮回放。Responses 的显式 summary 不应因此进入 Chat Completions 原文回放，Responses 的详细 reasoning 不应误升为 summary。处理重试、截断、取消和预算的同一事件分支均须覆盖新来源。
3. 保留 #421 的 `RunActivityGroup` 标题切换实现；补 **从 SSE 字段到外层聚合标题** 的合成路径断言，不能只再造一个 `SummaryDelta` 证明已有 UI。测试在 `crates/vega_runtime/src/openai/tests`、`crates/vega_runtime/src/agent/tests`、`crates/vega_conversation/src/agent/tests` 与 `crates/vega_ui/src/conversation_stream/tests` 的相应层覆盖上表。若实现证明某层无需新增测试，应在交回说明已有哪条断言穿透了该层。

范围仅为这条来源投影和必要回归，不重做工具汇总、折叠、错误 feed 或 API 选择，也不修改用户机器的提供方配置／运行数据来制造通过。Vega 仅做快速静态检查，不在本地运行代码、测试、编译或打包；交回 Codex 审查后由 PR Check 的 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace` 执行验证。用户在合并后的 GPT Chat Completions 与 DeepSeek 会话手测。若回归失败，回退本次追加修订 PR，保留已合入的 #421 基线。
