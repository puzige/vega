# #535 排队与插话的运行展示

## 状态与基线

实现方案 v2.4（A1–A7 产品验收不变）。生产实现 `be839cc63da09643c852f3bc2496e08ef932eb47` 已推送，接线修正 head `9327fa4fe5b8bb41a3ea01cf2f870ffe634a1566` 的 Actions 实际执行 18 个开发测试：17 通过、1 个工具保留观测 API 误用失败，另有 app unused import 阻断 Clippy；几何对照测试通过不代表用户像素验收。独立实现审查发现的控制器迟到终态遗漏已在停写补丁中修正并获静态增量审查无阻塞；开发源现有 20 个静态测试定义，工具观测、read 事件兼容及 A4 控制器回归已完成，当前集成版的云端结果仍待取得。当前交付流程遵守最新验证政策，不强制 TDD，但不删除未解决的失败测试逃避验证，最终 master 不包含 Rust 测试或独占支持。
初始基线：`83fd51d79940516029cdc1eaf8e81566870c877b`。
原开发测试生产基线：`6615748f91273bfb8ee424011b3b5a09b3777d2b`。当前集成基线：`830a41cb90b4dff585d821344382a31d548b1954`，已合入 master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`，保留上游委派阶段恢复、紧凑权限引导及消息锚点/预览关闭修复。上游已发布的 `0033_delegated_stage_recovery.sql` 保持原位，本任务尚未发布的续接迁移顺延为 `0034_run_continuations.sql`，SQL 内容不变。工具观测错误和 read 完成事件兼容性仅在开发测试层修正，不改生产工具状态机。
Issue：https://github.com/puzige/vega/issues/535

## 用户裁决

- 排队消息发出后上方出现 Completed，用户消息气泡与下方 Working for 间距不一致。
- 不使用 CUA；本地不运行、构建或测试 Vega，自动验证只交现有 Actions。
- 同一次运行只保留一个计时。插话不是任务完成，不以 Completed / Worked for 表示插话边界；普通排队开始的独立运行仍各自计时。
- 只修排队切换的不一致，保留普通消息的既有间距，不全局收紧。

## 已确认事实

`vega_conversation::agent::events::process_runtime_events` 在处理 `RuntimeEvent::SteeringAccepted` 时，依次发出旧消息 `MessageFinished(duration=None)`、用户投递、新消息开始及沿用原起点的 `RunStarted`。UI 的 `finish_run_activity` 将旧 group 置为 Completed，`RunActivityGroup::label` 因缺少 duration 显示 Completed。持久化也将旧助手消息结束且不写耗时；因此只替换字符串或补上旧段耗时不符合本次裁决。

普通用户气泡和可选择气泡共用顶部 24、底部 48 的布局；复制按钮绝对定位在既有底部空间中。复制槽为 24 + 4 = 28，距行尾为 48 - 28 = 20。上述数值是现行结构，不是本轮像素测量，也不是已证实的间距根因。必须对实际列表的投递、原位转正与普通路径做行为比较。

## 验收矩阵（先于实现）

| 编号 | 场景 | 预期 |
|---|---|---|
| A1 | 同一次运行接受一条或多条插话 | 只显示一个当前运行计时，不出现虚假的分段 Completed / Worked for；起点不重置 |
| A2 | 普通 queue 在前一运行真正完成后启动 | 前一运行真实终态保留，新运行独立计时，不合并不同运行 |
| A3 | 插话前已有正文、思考、工具、图片或最终回复元数据 | 历史内容、过程展开、复制来源与归属完整；不因移除旧计时标题隐藏过程或串联回复 |
| A4 | 插话后正常结束、失败、用户停止 | 当前运行真实终态正确，具体错误与恢复入口保留；重复或旧消息终态不污染新运行 |
| A5 | 切换会话、历史重载及分页 | 显式运行/续接归属维持单计时；不靠相邻文本、缺时长或时间戳猜测同次运行，不将未知历史伪造为插话 |
| A6 | 普通发送、排队直接投递、插话 echo 原位转正 | 相同内容和窗口下，用户气泡到后继运行行的真实布局一致；无重复消息、额外空行、重叠或无效占位 |
| A7 | 长文换行、图片、窄窗口、复制 hover/focus、列表重新测量 | 维持各自普通路径的几何与交互；图片按同一普通附图基线比较，不把附图高度当成文字间距 |

## 范围与非目标

修正运行展示归属与排队布局一致性。保留队列消费时机、顺序、去重、持久化事务、安全与权限、输入内容、复制文本以及普通消息几何。不得用修改全局间距、隐藏全部旧过程或猜测同一运行替代修复。不新增依赖、真实外部 E2E 或 CUA 门禁。

## 具体方案与先行测试包

### 真实续接关系

现有 `pending_messages` 的 delivered + steer 不能证明实际插话：`window/pending_messages.rs::continue_pending_queue` 在空闲时也会取该消息独立启动，`agent/pipeline.rs` 使用同一个 `mark_delivered`，保留旧 behavior/generation。generation 是独立 ULID，不是旧 assistant ID。因此不按这些字段或缺时长猜测关系。

最小新增一个 nullable 字段 `pending_messages.accepted_after_assistant_id`，用递增 migration `0034_run_continuations.sql`，不新建运行表。仅 `agent/events.rs::persist_runtime_event` 的真实 SteeringAccepted 事务写旧 assistant ID；普通独立投递保持空值。该事务已验证 user seq，并原子插入 successor assistant（user.seq + 1）。store 在同一快照中按准确 user ID、线程、role 和 successor seq 校验关系，不用上一相邻行推测 predecessor。保留 pending 删除与线程级联生命周期；异常关系不得合并。

旧记录无法可靠回填，保持原有未知历史展示；修复后新接受的插话在 live 与 reload 中一致。测试必须覆盖“曾标为 steer，最终独立投递”不被误合并。

### 事件与 UI

在 `vega_conversation::types` 定义明确续接 payload / event，包含前后 assistant ID 和 user ID。`process_runtime_events` 用续接事件替代旧普通 MessageFinished，后续用户投递、MessageStarted、RunStarted 顺序与同一计时起点不变；两个 runtime 接受入口都沿用此转换。需核对 debug、meter 及其他事件消费者的穷举匹配，但不改变计费/队列调度。

UI 收到续接时仅结束旧消息段的 parser/presentation，不标整个运行 Completed。仍为新 assistant 建独立 `RunActivityGroup`，避免 final 收起、隐藏正文和复制集合跨消息污染。旧组进入 continuation 前段模式，停止独立计时；有被收起内容时保留简短 Activity 展开入口，无内容时不留标题占位。新组位于最新插话（含图片）之后，显示唯一当前运行计时。旧正文/工具/复制/错误不移动、不丢失。重复事件幂等，旧终态不得重新唤醒旧计时。

`MessagePage` 各分页读取路径应携带明确 continuation 关系，`history::project_rows` 投影到 UI；查询须涵盖页外 successor，补载旧页不生成第二计时。UI hydration 根据明确 predecessor 关系使用相同展示模式，不依赖当前页相邻关系。同步渲染高度、锚点估算、列表失效和键盘可达性；不改全局48px留白。

### 应用控制器的终态归属（v2.3）

生产 ingress 在 `window/artifact.rs` 先调用 `AppAgentController::observe_terminal_message`，再调用 ConversationStream；仅 UI 忽略旧终态不足以保护队列释放和完成通知。控制器须在匹配的 generation/thread/stream 内记录显式 RunContinued 的 predecessor，在任何 queue_ready、completion_outcome、terminal_message_id、terminal_failure 变更前忽略该 predecessor 的 MessageFinished / Interrupted / Error(Some id)。不靠历史邻接推断；重复续接幂等，不结束当前运行，也不影响独立运行。

保留当前 successor 的真实成功、失败与停止，以及无 message ID 的启动错误；不因过滤 predecessor 丢失合法错误或恢复诊断。必要回归覆盖一次/多次续接、当前终态前后到达的 predecessor success/error/interruption、当前成功后的队列可释放与结果归属，以及当前失败/停止和未启动错误。沿用真实 controller 边界，不扩建整窗/系统 E2E。

### 文件边界

- `vega_store/migrations/`、`vega_store/src/lib.rs`、`pending_messages.rs`、`messages/mod.rs`：字段、真实接受事务写入、页内外关系查询。
- `vega_conversation/src/types/`、`agent/events.rs`、`history.rs`：typed 续接事件、事件转换、历史投影；必要调用点只做接线。
- `vega_ui/src/conversation_stream/{content,run_activity,phase,core,message_anchors,pending_messages,render,render_rows}.rs`：段生命周期、单计时、历史、真实布局与失效；只改验收所需路径。
- `vega/src/app_agent.rs`：仅补齐显式续接 predecessor 的控制器终态过滤；不改 queue 调度、通知文案或权限逻辑。
- 开发测试及最小 cfg(test)/dev feature 接线；不扩大 production public API。

### 测试与观测

先写齐 A1–A7 的有效测试。store 使用内存/隔离临时数据库，验证真实事务结果、独立发送负对照与跨页投影；conversation 验证真实事件转换而不是测试镜像算法。GPUI 使用当前 `gpui-kit 0.6.1` 的 test-support、TestAppContext 和真实 ConversationStream，观测真实 header 的 label/bounds、气泡 painted quads、list bounds、展开行为和 clipboard。观测支持仅最小接线，不改生产决策；snapshot label 不冒充像素文字。

几何加入普通发送阳性对照、直接投递、echo转正、echo后旧run继续输出、零高行及重新测量。相同内容/窗口下比较真实气泡到后继运行行的距离；若中间仍有合法旧内容，记录其实际顺序而非删除内容强行相等。当前尚无间距根因证据，测试若均一致，不得虚构间距修复或改token；报告未复现并保留回归。

v2.1 明确测试边界：保留真实接受事务/事件转换、真实 store → history → UI、真实 controller 终态与队列门禁三组可组合的进程内测试，不为了单条全链路测试扩大生产 API 或构建含 Sidebar/定价等外部依赖的完整 VegaWindow。应用 ingress 的续接事件接线仍须逐项静态审查，不能把分层结果称为整窗 E2E。错误恢复观测实际 `ComposerSubmitted` payload，不仅检查 pending 私有字段；模型执行由用户手测。

几何收尾须同时比较文字气泡底部与最后图片底部到后继 header 的距离，图片组保留独立的普通发送对照。真实可滚动夹具在滚动前后断言位移，再回到目标行比较尺寸与间距；短内容 resize 可以证明重测，但不冒充滚动证据。上述是既有 A4/A6/A7 的观测收尾，不改变用户行为预期。

现有 `.github/workflows/pr-check.yml` 运行云端 fmt、Clippy、Nextest，无新 workflow。此前接口缺失 RED 只证明最小目标接口缺失，未执行行为断言；无关接线错误不算产品失败。按当前政策直接完成最小实现、在现有开发测试上验证，并做实现独立审查。

按最新仓库规则：专职子 Agent 最小实现，停写版本由非写入者独立审查并交现有 Actions。已有开发测试及断言继续用于本轮回归，不为流程追加测试；测试自身错误修责任层并留证，不放宽预期。固定可恢复源码及实际云端结果后清理 Rust 测试与独占支持，最终 head 静态清点及 required CI 确认 0 发现/0 执行后才合并。真实 UI 外观由用户手测，静态调查或进程内测试不冒充像素验收。

v2.2 仅同步基线及 #569/#573 的验证政策，不改变运行归属、计时、内容/复制、历史或几何验收。v2.3 根据实际 Actions 与独立审查补齐原 A4 的应用控制器归属遗漏，并将 A3 工具保留观测修正为前后同一实体、保留实际目标/成功状态及真实展开输出；不再误用仅适用于待审批重复提案的 matches_call。全部原有计时、复制、工具展开与输出断言保留，不更改产品取舍。v2.4 仅同步上游基线、迁移顺序和成功 read 新增 ToolReadFinished 的开发观测兼容；call ID 与真实工具输出的预期不变。
