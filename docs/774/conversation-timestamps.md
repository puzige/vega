## 背景与目标

Vega 的对话流目前没有消息级时间展示：翻到旧对话时无法判断这轮是哪天发生的，消息本身也看不到发送时间。参照 ChatGPT Desktop（内含 Codex 界面）26.1021.31345，为 Vega 补两层时间信息：

1. **分组时间分隔线**：按相邻逻辑条目的间隔，在分组边界显示居中的日期与时间。
2. **单条消息时间**：默认悬停可见，与现有消息动作同排；今天只显示时间，1–6 天前加星期，更早加日期；键盘聚焦时同样可见。

**本轮仅修订规格，不开始实现，不改代码、数据库或看板状态。** 下文区分参考实现补证、Vega 当前源码事实、Vega 目标契约和仍待验证的事项；目标与验收项均不代表已经实现。

## 证据版本与可信边界

- Vega 源码基线：`de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420`（本轮已 fetch，并将本地 master 快进到该远端版本）。本轮仅静态阅读，未运行 Vega、编译、构建或测试。
- ChatGPT 证据：用户重新静态核对 `/Applications/ChatGPT.app/Contents/Resources/app.asar` 后提供的补证，版本 26.1021.31345。本轮未独立重新解析 asar，不把补证写成 Vega 的运行验证。
- 参考分块：`timestamp-separator-796153ccf821.js`、`timestamps-85bd12c0e1b2.js`、`local-conversation-turn-709dc5de82d1.js`、`user-message-59fad66de194.js`；另有 `local-conversation-thread-turn-entries-*.js` 与 `app-initial` 中的 `xSr`。

## 已确认的参考规则与 Vega 采用口径

### 1. 展示前缀，而非独立列表行

参考分隔线只接收 `sentAtMs`（可注入 `nowMs`）；非法时间不渲染。它是 turn 容器的首个子节点，不是吸顶行，也不是列表中的独立消息。

- 整行居中、次要色、不可选中；日期部分比时间略重。参考样式 `text-sm leading-5 text-tertiary select-none`，普通上下间距 `py-4`，紧凑变体 `py-2` / `text-xs`；Vega 使用自身主题与布局 token 对齐，不机械照抄 CSS 数值。
- 参考结构有 `role="separator"`、完整 `aria-label` 和 `<time dateTime=ISO>`；Vega 按 GPUI 实际能力提供对应语义，不能把内部保存的 ISO 字段当成已暴露给读屏。
- 参考通过 `data-markdown-copy="exclude"` 排除复制；Vega 应继续从 `MessageCopy` 读取正文，时间不进入正文、Markdown 或选区数据，不照搬 DOM 属性。
- **Vega 优先采用所属逻辑条目的展示前缀，不预设新增 `StreamEntry` 变体。** 前缀仍须纳入实际列表尺寸、缓存失效和滚动锚点处理。

### 2. 日期分级、时区与 locale

按目标时刻与当前时刻的**本地日历日差**分级，不能用毫秒差除以 24 小时替代：

| 天差 | 分隔线日期部分 | 英文连接 |
|---|---|---|
| 0 | 今天 / Today | 空格 |
| 1 | 昨天 / Yesterday | 空格 |
| 2–7 | 星期全称，例如 Wednesday | 空格 |
| 8–365 | 周几 + 月 + 日，例如 Wed, Oct 28 | `at` |
| >365 | 月 + 日 + 年，例如 Oct 28, 2025 | `at` |

- 参考近 7 天模板 `<day>{date}</day> {time}`，更早 `<day>{date}</day> at {time}`；`at` 仅是英文模板，中文按中文语序，不硬编码英文连接词。
- 单条消息：今天只显示时间；1–6 天为星期 + 时间；更早为月日 + 时间。与分隔线的 7 天分界分别处理。
- **时区跟随系统本地时区；文案语言跟随 Vega UI 语言；小时制跟随当前 locale 约定，而不是系统独立小时制偏好。** 当前 `en` 明确映射为 `en-US` 格式约定（12 小时），`zh-CN` 为 24 小时；时间为 hour numeric / minute 2-digit。未来若支持 en-GB，再引入区域维度，不将裸 `en` 描述为天然等同 en-US。
- 英文相对日首字母大写只对英文生效；全部日期词、星期、月份、AM/PM 和组合模板进入 `vega_i18n`。
- 参考 Intl/ICU 对每个时间戳分别转换。Vega 可复用已有 `localtime_r` 路径：分别转换目标时刻和 now，不能把当前 UTC 偏移套在全部历史消息上；转换失败不显示错误时间，不静默回退为未标注 UTC。
- 不增加定时刷新；每次相关投影／渲染使用一致的 `now_ms`。未来时间、异常时钟与格式转换失败的精确降级见“待验证事项”，不能直接把所有负日差标成今天。

### 3. 间隔与列表起点规则

判定单位是**逻辑消息／turn 条目，不是渲染行或每个 `HistoryEntry`**。三条严格大于阈值的规则：

1. 当前已加载逻辑列表中，尚未出现过用户且未被断点抑制的首个用户条目，距 now **>1 小时**时插入；不要求它是会话真实起点。
2. 上一逻辑条目是 assistant，当前是 user，间隔 **>1 小时**时插入。
3. 上一逻辑条目是 assistant，当前是其它有效逻辑条目，间隔 **>10 分钟**时插入。“其它”不包括独立断点和附属渲染行。

恰好 1 小时／10 分钟不插。首用户特例需 now 与当前时间有效；相邻比较需两侧时间有效，缺失或非法不插。阈值使用真实 Unix 毫秒差，不受日历分级或 DST 改写。

**同一自然日允许多个时间分组；同一分组边界不得因分页、重开或重复投影而重复显示。** 撤回原稿“同一天连续对话不重复出现”及前次评审“必须是会话真实首条用户消息”的限定。

断点规则：独立非 turn 条目清空前序，并将“已出现过用户”置真，避免其后重新触发首用户特例。显式 `breaksPreviousTimestampAdjacency` 在相应首逻辑条目上打断与前序的比较；附属投影的合并不是断点。不得把“默认不参与比较的独立条目”和“归属已有消息的附属投影”混为一谈。

### 4. 时间来源不变量

参考对话页使用 `create_time ?? update_time`；Codex 使用 `turnStartedAtMs` / `finalAssistantStartedAtMs`，本地合成 turn 才用一次 `Date.now()`，合并时权威时间优先。对话页还会在同一 turn 内合并连续同角色消息，取组内首个可用时间，不能每个渲染行重新计时。这些是两套参考模型，不应混合成 Vega 已有的数据结构。

Vega 是本地持久化架构，统一称为**会话层／持久化权威时间**，不是笼统的“服务端时间”：

- 用户：使用提交／受理时记录的 Unix 毫秒。普通发送可使用提交事务的权威时间；已排队消息使用原入队受理时间，之后的投递或 steer 接受不改写显示时间。
- 乐观条目：缺少权威数据时最多记录一次临时时间；取得权威值后替换。不得用 UI 收到事件、重渲染、分页加载或重开时刻冒充发送时间。临时值被权威值替换的允许变化，与取得权威值后必须稳定的约束区分记录。
- 助手 turn 多片段：目标使用**最后一段助手响应的开始时间**（正常完成路径为 final 响应开始），不是整个持久化 assistant 消息的 `created_at`，不是响应完成时间，也不是渲染时刻。正文／图片／活动拆分不各自制造新发送时间。
- 助手片段开始时间必须由会话／持久化边界记录并透传实时及历史路径；已取得的同一权威值在重开和 Fork 后保持不变。流式尚无最终段、失败／中断无 final、重试与旧数据缺字段的选择／降级须在实现前固定，不能把消息创建时间伪装成精确 final 开始时间。

## Vega 源码事实与逻辑条目映射

以下映射是**本卡的 Vega 目标设计**，不是当前代码已具备时间分组。表中“参与比较”指是否独立向分组算法提供逻辑条目；“打断”不是删除行或隐藏业务内容。

| Vega 源条目／渲染投影 | 逻辑归属 | 时间来源及当前缺口 | 独立参与比较 | 打断前序 |
|---|---|---|---|---|
| `HistoryEntry::UserText` → `StreamEntry::User`，普通发送／乐观 echo | 一条用户逻辑消息；同一消息拆成多行不拆组 | 普通提交的 `MessageRow.created_at`；历史投影和实时受理投影尚未透传 | 是，user，仅一次 | 否；显式边界除外 |
| `UserImages`（含仅图片消息） | 同一 `message_id`／pending identity 的用户消息，正文为空也保留一个逻辑主体 | 继承所属用户消息；不按图片生成或解码时刻计时 | 否，不重复比较；仅图片主体仍比较一次 | 否 |
| 排队／steer 用户条目及 `PendingMessageDelivered` | 同一用户逻辑消息，投递更新身份而非追加第二个用户条目 | `pending_messages.created_at` 为目标来源；当前 decode 丢弃，投递写入另取 now，需修正 | 是，user；echo、投递、历史去重 | 否；续接显式边界单独处理 |
| `AssistantText` → 多个 `Assistant` 正文片段 | 同一助手 turn／响应归属；Markdown 行和活动拆段不成为新 turn | 最后一段响应开始时间；目前未记录和持久化，不能直接用 message.created_at 替代 | 是，assistant，在所属逻辑分组处一次 | 否；中途遇独立断点时按边界处理，不能跨断点硬并 |
| `AssistantResponseMetadata` / `AssistantResponseStarted` / `AssistantResponseCompleted` | 上述助手的响应边界与时间元数据，不是可见消息 | 需新增权威开始时间并按 response_id 绑定；当前只有 ID、文本范围／类型 | 否，由助手主体消费 | 否 |
| `Tool` → `Tool` / `ToolGroup` / `RunActivity` / `RunActivitySegment`；`AssistantScreenshot` → `Screenshot`；所属产物 `ArtifactGroup` | 有有效 owner 的助手附属投影 | 继承助手逻辑归属，不用 tool.created_at／截图时刻替代助手发送时间；活动耗时仍维持原语义 | 否，不按工具数量和分段重复插 | 否；不得跨独立断点合并；无法建立归属时按独立非 turn 断点降级 |
| `Permission` 请求卡 | 能确认 owner 时归属于助手活动；无 owner 时独立卡 | 不制造发送时间；继承 owner，不以弹窗／审批时刻计时 | 否 | 有 owner 时否；独立卡是 |
| `Plan` | 由 assistant 持久化消息投影的计划主体；不是一律视为压缩／摘要 | 当前 `Plan` 不携带 message.created_at；需透传其创建时间；若有所属响应开始时间，按统一助手时间契约选取，不能把 reviewed_at 当发送时间 | 是，assistant，一次；所属活动不再比较 | 否；其合成批准指令不成为用户消息 |
| `GeneratedNotice` | 消息正文中的受控提示区间／元数据 | 继承所属消息，不独立取时间 | 否 | 否；当前会折入正文投影，不按枚举名猜成独立通知 |
| `CredentialContinuation` | 所属助手消息的失败／恢复元数据 | 不新增发送时间 | 否 | 元数据自身否；真正恢复／续接边界显式标记并打断 |
| `RunContinuation` / `RunContinued`；Fork／恢复续接边界 | 关系元数据，不是独立消息；在实际连接点标记下一逻辑主体 | 不用关系建立时间替换原消息时间 | 否 | 是，在显式连接点；Fork 复制来的每条历史消息不逐条打断 |
| `Subagents` 聚合行 | 父流内独立非 turn 活动；打开的子会话自身按同一消息规则处理 | 父流不拿子 Agent 创建时间生成消息分隔线 | 否 | 是，清前序并抑制首用户特例 |
| `ContextCompaction` 压缩活动 | 独立非 turn 活动 | 状态记录时间不充当消息发送时间 | 否 | 是；零可见行的处理仍须固定，见待验证 |
| `Summary`（`TaskCostSummary`，不是 `SummaryDelta`） | 独立任务结果／成本摘要，即使带 message_id 也不自动并入正文 | 不拿统计时间替代助手发送时间 | 否 | 是；当前部分 outcome 不产生可见行，见待验证 |
| `SkillActivation` 历史技能来源行 | 独立来源审计行，虽绑定 run，仍不是正文消息或工具分段 | 审计创建时间不作为消息发送时间 | 否 | 是 |
| thinking／reasoning、`SummaryDelta` 等助手活动及合成批准指令 | 前者归所属助手；后者为控制能力，不是用户正文 | 不新增发送时间 | 否 | 附属活动否；被过滤控制指令不伪造 user 或断点 |

### 源码依据（固定版本链接）

- [HistoryEntry 完整变体](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/history.rs#L17-L96)；[用户、计划与响应历史投影](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/history.rs#L572-L680)。目前未带出消息时间。
- [StreamEntry 类型与可见行数](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/model.rs#L1258-L1351)；[按响应边界拆分正文](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/history_preparation.rs#L15-L80)；[GeneratedNotice 区间投影](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/generated_notice.rs#L1-L111)。
- [Plan、Summary 与工具水合](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/content.rs#L406-L555)；[恢复子 Agent 行](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/subagents.rs#L208-L218)。

## 时间来源与持久化缺口（已静态确认）

| 路径 | 已有事实 | 实施所需改动／限制 |
|---|---|---|
| 普通消息 | [`MessageRow.created_at`](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_store/src/messages/mod.rs#L16-L30) 为 Unix 毫秒；[普通提交](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/agent/pipeline.rs#L1291-L1470) 在事务路径生成 now 并写 user / assistant | 用户可复用权威值，但需历史和实时透传；assistant 行创建不等于最后响应开始 |
| 排队受理 | [入队保存 created_at](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/pending_messages.rs#L63-L83)；[`PendingMessage` 无时间字段](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/types/pending_messages.rs#L30-L41)，[decode 不保留 row.created_at](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/pending_messages.rs#L180-L204) | 补受理投影与乐观／持久化身份合并，不能只改 HistoryEntry |
| queue 投递与 steer 接受 | [queue 读取 pending 时只保留 content](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/agent/pipeline.rs#L1236-L1262)，[用户消息写 created_at: now](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/agent/pipeline.rs#L1383-L1402)；[steer 新建 user / assistant 均用 now_ms](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/agent/events.rs#L572-L630) | **存在写入语义缺口，不仅是展示透传。** 用户显示时间要保留原受理值，且不改排序／审批／安全权威语义；是复用 created_at 还是独立保存显示时间，实施前结合这些用途确定 |
| 实时事件 | [`AssistantResponseStarted`、`PendingMessageDelivered`、`MessageStarted` 均无 Unix 时间，`RunStarted.started_at` 为 Instant](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/types/events.rs#L536-L588) | 不能用 Instant 格式化日期；需会话权威时间贯通提交／开始／投递与 UI，不用事件接收时间补造 |
| 助手响应开始与完成 | [开始事件当前持久化分支为空](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/agent/events.rs#L560-L571)；[`AssistantResponseMetadata`](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/types/response.rs#L1-L13)、[`AssistantResponseRow` 及 INSERT / SELECT](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_store/src/messages/responses.rs#L3-L107)、[响应表 schema](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_store/migrations/0017_assistant_responses.sql) 均无开始时间 | 需新增按 response_id 记录的权威开始时间、持久化和历史投影；仅在完成时采样不满足“开始时间”。当前表要求非空已完成文本区间，不能假定直接插入未完成响应即可；需兼容开始后崩溃和重试 |
| Fork | [复制消息保留 created_at](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_store/src/thread_fork.rs#L152-L178)；[复制响应显式枚举现有列并重映射 ID](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_store/src/thread_fork.rs#L318-L326) | 新增响应时间需同步复制／校验路径，不得改成 Fork 创建时刻；历史前缀保留原时间和相邻性，仅实际分叉连接点显式打断 |
| 本地日历 | [已有 localtime_r + 本地 civil 日序换算](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_conversation/src/sidebar_organization/calendar.rs#L1-L41)，但只暴露侧边栏分桶 | 提取可复用本地日期时间能力，不直接套用侧边栏分桶。撤回“Vega 完全没有本地时区能力”的原稿；存在路径不等于 DST 已验收 |

**因此本卡不再宣称“数据库数据全部齐备／只需透传、不需 schema 变化”。** 消息表本身已有创建时间；但精确助手响应开始时间是新增数据需求，应采用向前兼容、只增不删的迁移设计。旧数据无法凭创建／完成时间反推出精确开始时间，不得伪造回填。共享时间类型按仓库架构约束放在 `vega_conversation::types`，数据层保存 Unix 毫秒，不保存本地化字符串。

## 分页与交互规格

- [现有双向分页](https://github.com/puzige/vega/blob/de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420/crates/vega_ui/src/conversation_stream/content.rs#L111-L195) 先水合／合并，再恢复子 Agent 行、重排普通消息。**分隔线在合并、身份去重、恢复独立行及重排后计算**，不能在每个网络分页内部独立生成并永久保存。
- 当前已加载列表开头的旧用户消息允许显示日期；加载更早历史后，原页头前缀按新相邻关系正确保留、移除或转为真实间隔边界。列表窗口开头提示不是永久断点；显式业务断点不是分页断点。
- 前缀绑定稳定逻辑身份；重复加载不新增副本。边界变化应失效相关列表尺寸／缓存，并保持滚动锚点，不要求每个 token 全量重建历史。
- 时间与既有消息动作同排右侧，次要色、小字号。鼠标悬停可见，移开隐藏；键盘焦点进入消息或其动作时同样可见，不能只让 copy 按钮获得焦点样式而时间仍透明。
- 不改变既有动作命中区域或让时间截获复制／Fork 点击。覆盖无正文／仅图片、流式中、动作禁用的情况；不能因 `message_with_copy` 对空正文提前返回而丢掉时间。
- 分隔线不吸顶，不增加常驻说明或成功提示。无障碍优先真实 separator 语义、完整可读标签与机器可读时间；GPUI 不支持的部分必须记录实际降级和读屏可见范围。

## 实施前需固定的方案细节与待验证事项

已采用的产品规则不再待裁决（同日多组、加载列表起点特例、附属投影归属与独立断点、用户受理时间、最后助手段开始时间、本地时区、en→en-US / zh-CN 小时制、展示前缀）。以下尚未由本轮静态证据或运行结果解决：

1. **缺时间／旧数据降级**：响应开始时间缺失时不冒充精确值；需在实施方案明确隐藏、可区分的近似值或其他兼容策略，以及历史排队受理时间是否仍能可靠恢复。缺失时间对后续相邻状态如何推进必须确定，不可跨缺口用更早值误插。
2. **助手非正常路径**：流式尚无 final、仅工具、失败／中断、空响应、重试回滚与恢复续接，分别选哪个已记录段的开始时间；从 provisional 到 final 如何更新前缀并保持身份／滚动稳定。已有 `MessageRow.created_at`、执行耗时或 `Instant` 不能自动解决这些问题。
3. **映射边界落地**：将表中逻辑归属落实到稳定 owner / response / continuation identity；同一助手的附属活动穿过独立断点时不得再次制造 turn。零可见行的 Summary / ContextCompaction 是否仍向逻辑层提供断点，需统一实时与历史投影并明确，不能由渲染裁剪偶然决定。Plan 缺响应元数据时的时间选择也须明确。
4. **迁移与权威性**：确定开始时间记录位置及幂等键，持久化失败／崩溃恢复／重试不漂移；核对 queue / steer 时间语义调整对审批、排序、恢复和权限的影响。新时间字段贯通分页查询、历史投影及 Fork 显式列复制；不扩张本卡去修改业务安全规则。
5. **本地日历与异常输入**：复用范围、平台支持与错误返回；DST 春季跳时、秋季重复小时、历史偏移、跨年／闰年；未来时刻、反向时间差、异常时钟和越界值的降级。日差与真实间隔分别验证，不能仅用固定偏移替身声称系统时区转换已通过。
6. **GPUI 实际能力**：前缀高度变化、虚拟列表滚动锚点、键盘焦点联动、仅图片／动作禁用布局、读屏 role / 标签 / 时间语义需要实现后验证；本轮未做 UI 或系统集成验收。

## 验收目标与必要回归边界（全部待实现／验证）

- [ ] 三条严格阈值成立：10 分钟／1 小时恰好不插、超过才插；前序不是 assistant 不误用相邻阈值；首用户使用 now；缺失／非法时间不插；断点清前序并抑制首用户特例。
- [ ] 同一天可多组；同一逻辑消息的正文行、图片、工具、响应元数据不重复插入；独立子 Agent、压缩、摘要、来源审计与显式续接断点按映射处理；仅图片和 Plan 不遗漏。
- [ ] 分隔线日差 0、1、2、7、8、365、366 档位正确；单条消息 0、1、6、7 边界正确；英文 `at` 仅 >7 天出现；跨午夜但不足 24 小时仍按本地日历日，含跨年／闰年。
- [ ] 每个时间戳分别转系统本地时间；覆盖 DST 春季跳时与秋季重复小时，核对当地日期、时刻与间隔阈值不混用；失败／越界不伪装成 UTC 或错误本地时间。
- [ ] `en` 按 en-US 12 小时、zh-CN 按 24 小时；全部文案进入 i18n，无硬编码英文；不读取系统独立小时制偏好，不新增区域／时区设置。
- [ ] 用户受理时间在 queue 投递／steer 接受后不变；乐观条目取得权威时间后替换一次而非重计时；实时、持久化、重开与 Fork 使用相同权威值。
- [ ] 助手最后段开始时间在多片段、实时完成、重开与 Fork 后一致；不能使用整条创建／完成时间冒充。流式、重试、失败／中断、旧数据按实施前明确的兼容方案回归，迁移不伪造精确时间。
- [ ] 双向分页：截断列表起点、加载更早后旧页头前缀的保留／移除／转化、接缝去重、重复加载、重排及滚动锚点稳定；Fork 历史前缀保留原时间，真实连接点才打断。
- [ ] 悬停时间与 copy／既有 Fork 等动作同排，鼠标和键盘可见，移开按状态隐藏；无正文／仅图片、流式与动作禁用仍合理；不改变现有动作命中与布局。
- [ ] 复制正文、原始 Markdown、选区不包含时间文案；时间不会被计入模型正文或控制指令。
- [ ] 无障碍树可识别分隔线及完整可读时间；机器可读时间按 GPUI 能力提供；不足项以实际降级记录，不以 DOM 写法或内部 ISO 值冒称读屏通过。
- [ ] 现有消息、活动、计划、子 Agent、恢复续接与压缩／摘要展示不回归；写入时间调整不改变业务排序、权限和审批约束。

### 验证与交付边界

本地只做快速、低 CPU 的差异／格式／静态检查；不运行 Vega 代码、编译、构建、打包或测试。自动验证交 GitHub Actions；必要时开发期 Rust 测试应覆盖上述可观察行为与边界，留存原始源码／完整 SHA／实际结果，合并前按仓库政策移除且最终 required CI 确认 `no Rust tests`（0 发现／0 执行），不把空集说成测试通过。不为流程造测试，不削弱上述验收要求；真实系统与原生 UI 验收由用户手测，CI／合并不代表用户验收。

**本轮结果仅为静态规格修订：没有功能验证结果、CI 结果或用户验收。**

## 范围与边界

- 不做消息时间编辑、时区选择器、系统小时制设置或相对日期定时刷新；系统时区变化不承诺主动刷新，后续正常重投影／渲染使用当时系统本地时区。
- 不改造侧边栏时间展示；可提取其已有本地日历能力供本功能复用。
- 不引入重量级日期库，优先使用已有 libc / 平台路径；新增白名单外依赖须按仓库规则批准。
- 包含实现精确消息／响应时间所需的最小数据透传、开始记录与兼容迁移；不因原稿“数据齐备”省略必要持久化。
- 与 #770 无关：本卡只加时间展示及其必要时间数据，不改变失败原因或其它业务规则。

---

阶段：规格已修订，实施准备前仍须收敛上述方案细节；**实施未开始**。
当前执行者：待领取；下一执行者：Agent。
方案版本：本轮 asar 补证 + Vega `de2d4b26c8c69ecd72d7a38b4255c7ce07d1b420` 静态核对版。
实现分支／提交／PR：尚未创建。
看板：Vega Desktop / Backlog；label：enhancement（本轮不变）。


## 实施裁决（用户确认）

本节替代上文仅规格阶段声明及待裁决部分；实施基线 `5d042fcd861a00af97486e948f0cc6291f33ca8f`，分支 `feat/774-conversation-timestamps`。

- 旧响应缺开始时间、非法／越界／未来时刻：隐藏时间与前缀，不伪造回填。缺口推进逻辑状态但不跨缺口比较。
- 助手使用最后一个未被回滚的已记录响应开始时间；新响应开始更新，正常结束为 final。无有效响应时隐藏。Plan 无响应元数据时使用自身消息创建时间。
- Summary／ContextCompaction 即使零可见行仍打断，实时与历史一致，清前序并抑制首用户特例。
- 反向时间差不生成间隔分组；格式转换失败隐藏。未来时刻不标成今天。

### 实施顺序及可观察验收

1. 数据：新增幂等响应开始时间记录及回滚处理；排队／steer 保留受理时间；分页、实时事件、Fork 透传同一权威值，旧响应明确缺失。
2. 投影：按 owner 归并正文与附属条目，独立断点显式生效；时间元数据不成为可见行或断点。合并与重排后计算前缀，稳定身份不变。
3. 格式：复用逐时间戳 localtime_r；按本地日序分档，en-US / zh-CN 模板进入 i18n；一次渲染共用 now，不加刷新计时器。
4. UI：前缀在所属列表项内，纳入高度与失效；时间与既有动作同排，鼠标／键盘可见，不进入 MessageCopy、Markdown 或选区。覆盖 Plan、仅图片、禁用动作及失败／中断。
5. 验证：上文十二项验收保留，边界包括严格阈值、DST、分页接缝及滚动锚点；本地仅差异／格式／静态检查，编译和自动测试由 PR Check。原生 UI 和读屏实际效果待用户手测，不以静态证据或 CI 冒充已验收。

最小数据接口：`HistoryEntry::MessageTimestamp` 与 `ConversationEvent::MessageTimestamp` 携带 `message_id`、`sent_at_ms: Option<i64>`；UI 根据稳定 owner 消费，不为每个 HistoryEntry 变体重复加时间。存储与会话层自行保证权威性，UI 不读 SQLite。

### 静态确认的无障碍降级

当前 GPUI re-export 的 AccessKit Role 无 Separator，只有代表可调整分隔器的 Splitter，不能冒用。时间前缀采用 Label 与完整本地年月日时间标签；未发现对应 HTML time/dateTime 的原生公开接线，暂不声称已暴露机器可读时间。真实读屏效果待用户手测。

### 集成补充

- 时间断点按 owner 与正文 byte offset 持久化并随 Fork 复制；独立断点不增加列表行。旧压缩记录没有精确正文锚点，不伪造 offset；旧数据保持缺少精确历史断点的兼容限制，新记录贯通实时与历史。
- Fork 新连接点记录原前缀序号，不对复制历史逐条打断；旧 Fork 没有可恢复前缀元数据时不猜测。
- Codex 后端用户时间透传持久化创建值；无响应开始事件的助手不回退创建时刻。Native 新响应提供精确开始时钟。
- 基线 #694 的 10 个诊断回归由该卡用户特别要求保留（Issue comment-6518598138）；本卡不删除、修改或恢复它们。最终 CI 如非空应报告实际计数，不能冒称 0。本卡自身若增加开发期 Rust 测试仍按现行政策处理。

### 重试身份口径（源码契约）

当前 Runtime 对同一逻辑 response_id 的传输重试只发 ResponseRetrying，不发新响应开始；因此内容回滚不删除该逻辑响应首次开始时钟，只移除被丢弃的后续响应及完成区间。真正新的 response_id 才记录新开始时间。此为同一响应的幂等重试，不在 UI 或完成事件采样。

### 云端验证计划

开发期包含实际生产 TimestampAdjacency 的严格阈值／断点／分页重算回归，以及日期格式器的日历分级、跨午夜、locale 回归。仅由 PR Check 执行；不以 i18n 源码字符串出现代替行为断言。系统时区历史规则、原生 hover/focus/读屏和分页实际滚动为未运行事项。开发测试需留存固定 SHA 与 Actions 结果，再单独清理；基线特别授权测试不动。
