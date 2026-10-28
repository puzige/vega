# #667 — Claude Code 上下文压缩全路径对齐

## 状态、目标与来源边界

- Issue：[#667](https://github.com/puzige/vega/issues/667)。阶段：按用户补充裁决修订规格并进入实现；尚未验证或合并。
- Vega 基线：`d884a9a36160a15e4ab460e6ea1958a9e3dc4d2c`；任务分支 `feat/667-claude-compaction`。原规格提交 `10ba07f7cc7d4444d6cff167b9782900e52ee513` 已发布为历史证据；同步基线后规格提交为 `57d1db681fb8e7f1268fe9921d8575cd395877d9`。新增 #608 的 Native interrupted-task resume 行为必须保留，不能让压缩重写绕过其 admission/unknown-effect 围栏。
- 参考：本机 Claude Code **2.1.292** 的静态可验证客户端实现。版本指纹、调用链及默认/实验/平台差异见 [参考矩阵](claude-2.1.292-reference.md) 与 [脱敏证据索引](claude-2.1.292-evidence.json)；未完成核实的项目不得标成已对齐。
- 用户要求是重写完整压缩机制，不是只改续接提示词，也不是沿用旧规格排除 micro、partial、reactive、precompute 后宣称全量完成。后续明确选择 **Vega 原生实现全部机制**，并补充：**“仅实现压缩相关的机制 hooks生命周期太重了”**。因此不建设通用 hooks 生命周期、Claude 外部插件兼容或新增 Anthropic 协议平台；压缩自身的准备/生成/恢复/安装/取消阶段仍必须完整。
- 对齐行为契约，以 Vega 原生实现适配 SQLite、可取消运行、多 provider 与 GPUI；不复制整段专有实现，不假装复现不可观察的服务端算法或私有实验分配。
- 本规格先列验收，再定义实现。主要客户端路径已由 62 项有界证据固定，参考矩阵保留上游 hooks/平台事实，但本卡验收以最新用户裁决为准。原平台范围阻碍已解除；不添加无消费者的扩展框架。

## 1. 已证实的问题与取代关系

### 1.1 续接事故

脱敏事故具有以下因果链：一个真实用户请求明确要求继续未完成任务；同一用户任务中多轮工具调用后发生压缩；partial-assistant 覆盖游标越过该用户消息，原文只剩在摘要中。摘要同时把摘要器自身的“生成检查点”元任务写成当前请求。主循环恢复后虽记得任务和限制，却重新询问是否继续。

当前源码对应：

- `crates/vega_conversation/src/agent/compaction.rs::compact_impl` 用最后一个完整 response 工具组作为保留段。存在更早工具组时，把 `covered_through_seq` 移到活动 assistant 前、`suffix_index` 移到最后工具组；用户任务不再独立存在于主模型投影。
- `direct_summary_request` 把摘要指令作为最后一条 user 消息，并添加标为 untrusted reference 的 continuation context。摘要任务与被摘要任务的语义没有结构化隔离。
- `pipeline.rs::historical_summary_message` 统一把摘要包装成非指令历史；即时安装和重启重建都以该包装、历史图片和后缀组成上下文。
- `runtime/agent/loop_.rs` 安装压缩消息后不另存可恢复的真实任务身份。

这不是“原始数据库记录被删除”，也不是“所有知识丢失”。修复必须同时保护来源与执行连续性，不能把所有摘要提升成用户授权，也不能禁止长用户任务中的压缩。

### 1.2 取代旧规格

1. 本卡取代 [旧 Claude parity](../vega-claude-compaction-parity.md) 的 **ordinary full 子集范围**、固定 80%/60% 策略和明确排除其他路径的条款；旧交付证据仅证明旧版本。
2. [连续压缩](../vega-continuous-compaction.md) 的单用户任务内多次压缩、完整 response 分组、原始审计和即时/重启一致性继续有效；本卡会取代其单一 prefix + partial-assistant 投影模型，以支持上游 full/partial/reactive 的不同保留策略。
3. 保留来源版本/指纹/前驱 CAS、权限与凭据保护、取消、工具配对、真实用量、图片来源、reasoning replay 等安全边界。是否摘要覆盖某段必须由投影计划记录，不能靠摘要文本猜测。
4. 本卡与 [composer 入口规格](../vega-composer-entry-simplification.md)（#644）共用 `/compact` 的 owner/epoch/trusted-action 围栏与状态呈现，不接管 `/goal`、`/fork`、技能提及、加号菜单或整套 composer 重构。全量参考新增自定义摘要要求、partial 选区/方向，与 #644 §7.1 的“仅独立 token、不得新增服务/数据/后台任务”存在明确取代关系：后者只约束 #644 的入口精简，本卡重写压缩引擎不受其实现范围限制。`/compact` 无参数仍不发送普通用户消息；带参数及 partial 入口须在实现前完成相应 UI 合同，不默默按普通消息发送或即时执行参数文本。不新增第二个常驻压缩按钮。
5. 本地执行禁令及当前开发测试留痕/清理规则优先于所有旧规格中的本地脚本、构建、安装与 TDD 要求。

## 2. 验收矩阵（实现前合同）

`CLOUD` 表示在现有 Actions 中运行的必要开发回归；`STATIC` 只证明源码/契约；`MANUAL` 是用户对合并版本的真实模型/应用验证。下表当前全部 **NOT RUN / NOT IMPLEMENTED**，不把设计当结果。

| ID | 前置状态与操作 | 可观察结果与必要反例 | 验证层级 |
|---|---|---|---|
| A01 | 用户要求继续；一个用户任务内多个完成的 API 工具组；触发自动压缩 | 原始任务身份与未完成运行状态保留；不把摘要器请求变成当前任务；恢复到下一模型回合，不重复询问继续、不重放已执行工具 | CLOUD + MANUAL |
| A02 | 摘要包含“当前任务只是生成摘要”、历史授权、外来文档指令或已完成任务 | 合成消息不能创建人类权限、复活旧任务、改变 stop/暂停/接管；当前权限按真实 owner/admission 重新校验 | CLOUD |
| A03 | 同一模型的 policy window、硬窗口、输出能力不同；auto on/off、显式未知容量、合法/非法 override | 阈值按参考契约计算；策略触发和硬阻塞分开；未知能力不伪装已知，不静默改用户持久化设置；小窗口无下溢/死循环 | CLOUD |
| A04 | 可清理工具结果、已持久化/已清理结果、最新保护结果、图片/文档、失败结果混合；context hint 触发 | 只改 eligible 完成结果的模型投影，保留调用 ID/状态与外来内容来源；省量不足时 no-op；保留数由该路径决定；原始审计可回读 | CLOUD |
| A05 | idle/server context edit 的独立 gates、未支持 provider、shadow/off 状态 | 不把 context-hint 条件套到 idle；不向未知 provider 发送专属字段；provider edit、本地清理、遥测分别记录，不把预测省量记成真实 usage | CLOUD + MANUAL |
| A06 | 普通 full：有旧摘要、user/assistant/tools、图片与当前任务；工具禁用摘要调用 | full 的保留段遵守参考（不是假称永远保留最近原文）；产物含有来源的 boundary/summary/restored state，明确续接模式；摘要限额和失败均不安装半成品 | CLOUD |
| A07 | idle 手动 `/compact`、自定义摘要要求；运行中外部手动请求；取消后晚返回 | 无自定义/新增 hook 指令时可校验复用 precompute，否则走尾部保留式 reactive；不是固定 ordinary full。手动入口按 owner 排他，不据此禁用自动中途压缩；附加要求只约束摘要；晚结果不能安装 | CLOUD + MANUAL |
| A08 | 手动 partial 两方向、选中边界、平行工具组、reasoning-only/空可见文本回合 | 只摘要指定区间；保留前缀或后缀准确；不会切断 assistant/tool 配对；重启后的 splice 与即时投影一致，ID 不漂移 | CLOUD + MANUAL |
| A09 | primary 拒绝输入过长/typed token-gap 或本地硬窗口溢出；普通压缩不可用 | 走 reactive 的独立尾部选择与有界缩减；不能无限重试同一源；不是把所有 provider 错误当溢出 | CLOUD |
| A10 | reactive 最后兜底必须舍弃最旧未摘要组，或单组仍无法容纳 | 只有参考允许的兜底可 head-truncate，并持久化精确遗漏范围、给续接明确提示；不可假称全部历史已摘要；无法安全配对则失败不安装 | CLOUD + MANUAL |
| A11 | 提前阈值启动 precompute，同时主任务继续产生完整新消息 | 主运行不被无关后台等待阻塞；候选只覆盖捕获前缀，新后缀完整保留；只有 install owner 可提交；成功只计一次已报告 usage | CLOUD |
| A12 | precompute 期间改模型、删/改覆盖消息、hook 改写、边界丢失、新回复无法 splice、重启、stop | pending 与 ready 分别处理：模型切换取消 pending，ready 在消费时复核；重启可读取完整 ready candidate，但必须验证格式/大小/年龄/session/UUID/当前 fit 并取得新安装 owner；中断生成不能复活 | CLOUD |
| A13 | 压缩前存在已读文件版本、动态工具发现、技能快照、计划/任务、会话上下文 | 按参考恢复模型所需状态；不把 read-before-edit 凭据或权限从摘要制造出来；恢复内容有来源、预算与过期规则；失败不宣称恢复完成 | CLOUD + MANUAL |
| A14 | 续接需要摘要未含细节/被清理工具输出；原始 transcript 保留 | 有有界只读回读通道，可定位消息/工具组/附件元数据；遵守当前会话访问与凭据过滤，不自动重跑有副作用工具，不泄露其他线程 | CLOUD + MANUAL |
| A15 | 压缩内部准备、摘要、状态恢复、安装失败/取消/源变化 | 内部阶段按顺序完成并校验；不调用外部插件，不实现通用 PreCompact/PostCompact/SessionStart 生命周期；恢复数据不自带新权限，失败不安装半成品 | CLOUD |
| A16 | control/lean/short/capped 摘要变体、auto/reactive/manual 续接模式 | 变体选择可追溯；保留安全约束、最新纠正、未完成工作；不输出内部分析草稿；不会根据 summary role 推断人为新指令 | CLOUD |
| A17 | 自动摘要连续失败、成功重置、短周期反复回填；一次用户任务含很多 API 回合 | 失败熔断与 rapid-refill 熔断分别计数；参考 turn 单位明确；停止循环并有可恢复错误，不把失败隐藏成“完成” | CLOUD |
| A18 | 使用已报告/缺失/缓存 usage；摘要失败、后台丢弃、取消；图片/reasoning 预算 | 所有实际请求用量保留，未知仍未知；不把估算当账单；安装后 accounting anchor/replay frame 同步失效重建 | CLOUD |
| A19 | legacy checkpoint、同 seq 编辑、重复安装、模型切换、并发手动/自动安装 | 向后读取；新增 schema；前驱/来源/owner/取消原子校验；重复提交幂等；不改原始对话、不跨线程安装 | CLOUD |
| A20 | UI 默认/运行/完成/失败/禁用/键盘；选中消息局部压缩与导航切换 | 入口可发现、状态真实；成功不叠加长说明；必要失败和恢复动作保留；导航不会让旧结果落到新线程 | CLOUD + MANUAL |
| A21 | 空闲会话级压缩的 off/log/compact，1h cache 寿命、prefix 改变、用户活动、其他执行者、额度约束 | 与 3900s idle micro 分开；未知缓存期限不得伪造调度；到期前回调复查 source/owner；log 不调用模型；有真实进行状态及取消 | CLOUD + MANUAL |

## 3. 全路径目标与已核实 Vega 差距

| 机制 | 当前 Vega | 本卡目标 / 不能偷换的边界 |
|---|---|---|
| 触发与预算 | `ContextBudget` 使用输入预算的 ceil(80%)/floor(60%)；`model_context_budget` 从用户配置生成 | policy window、硬上限、输出 reserve、warn/compact/block/precompute 各自表达；适配已知 model snapshot，不伪造未知 provider 能力 |
| micro | `ToolContextProjector` 已做 8 KiB 文本预览与重复输出引用，并非 Claude microcompact | 保留现有审计与预览，增加真实按 eligible/keep/savings/gate 选择的生命周期及 reload；两种能力不能互相冒充 |
| full | 单一 `compact_impl`，最近 user 或最后工具组后缀；32 阶段摘要、溢出缩阶段窗口 | ordinary full、运行内 full 与其他路线分开；只执行参考证实的保留合同 |
| manual/partial | 有 manual owner、可信动作与固定 source；请求只含 thread/model/request id；无方向/区间 | 原生命令和 partial 两方向；选择基于稳定 ID/完整组，不用文本 offset 充当所有分组身份 |
| reactive | primary typed overflow 触发同一 compact hook；摘要输入过长最多三次缩 stage limit | 独立可观测的恢复策略、尾部/last-resort/遗漏记录；保持 typed provider error 边界 |
| precompute | 未见独立候选/后台 install 生命周期；当前整体 source CAS 拒绝任何后续变化 | prepare/generate/validate/install 分离；安全 append-only 后缀可 splice，编辑/权限变化必失效；预算与取消不借主循环终止掩盖 |
| durable reload | checkpoint 的 covered prefix + 可选 partial assistant cursor | 版本化 projection plan 可表达 prefix/suffix/range、summary/boundary、micro、恢复附件与遗漏；原始历史保持 append-only 审计 |
| 续接 | untrusted summary wrapper；无区分 auto/manual 的结构化 continuation mode | 源自真实运行 owner 的续做意图与摘要文本分离；摘要器自身任务不会覆盖用户任务 |
| 状态恢复 | 系统/工具/skills 在运行层已独立装配；ReadState 是会话共享内容 hash | 明确压缩时各状态保留/清空/重载矩阵；不盲目清除安全读版本，也不把被压缩的工具发现当永久权限 |
| 内部服务/provider | 现有 `ContextCompactionHook` 是内部 conversation 服务；ModelCompat 无 context edit 能力 | 复用内部服务，不新增通用 hooks/插件框架；保持现有 provider 路由，专属服务以明确的客户端策略适配，不发送 Anthropic-only 字段 |
| 熔断/usage | 三次自动失败、同 source/live revision 去重；阶段 usage、取消和 300s inactivity/600s operation deadline | 区分失败与快速回填熔断；每条新路径均带实际 usage 与 owner/cancel；deadline 适配需记录，不撤销已有超时保护 |

## 4. 设计约束

### 4.1 任务、摘要与权限分离

- `task identity` 来自真实 durable 人类消息的身份、顺序及当前运行 owner；`continuation mode` 来自调用入口；摘要只提供历史内容。这三者不能都压进一个自由文本 summary。
- auto/reactive 成功安装后继续已有未终止运行，不再向用户索要同一“继续”授权。被 stop/接管/权限撤销终止的运行不能因摘要提示词复活。
- 普通 full 不承诺保留所有用户原文；安全约束必须按参考提示词要求保留，同时当前执行授权仍由真实来源和当前权限系统决定。必要的最新任务锚点是 harness 来源证据，不是给历史工具结果提权。
- 摘要请求使用显式的历史数据边界与摘要器元任务排除规则。最新任务、完成状态与下一步只能指被摘要会话；出现旧摘要中的元任务污染时不得继承。
- 历史附件、工具输出、外来 artifact 及 hook 输出保持数据来源标记。拒绝把 `role=user`、`isCompactSummary` 或“verbatim”文字当人类授权证明。

### 4.2 计划与原子安装

把目前 `compact_impl` 中的源捕获、区间选择、模型生成、完整性校验和安装拆成内部阶段，避免另建互不相容的压缩系统。

1. **Capture**：一次源快照，记录线程/模型/owner、前驱 checkpoint、来源版本和内容指纹、完整 API response 分组、当前权限快照。
2. **Plan**：选择明确的压缩策略、覆盖/保留区间、候选状态恢复与预算。所有区间以稳定消息/工具/response 身份描述；legacy offset 只能在已有受校验 fallback 内使用。
3. **Generate**：工具禁用的有界摘要；流式取消、合理输出活动计时、共同硬 deadline、真实 usage 收集。缓存共享路径必须通过 provider 能力，不伪造 wire cache 命中。
4. **Validate**：非空/合法摘要、工具配对、图片/reasoning 规则、实际重建请求预算、覆盖完整性、hook 变更与来源复核。
5. **Install**：事务内验证来源/前驱/当前 owner/取消；即时投影和 restart 共用一个 plan interpreter。拒绝部分写入；旧 checkpoint 不覆盖或删除。

预计算允许“捕获前缀仍完全相同、之后只追加可拼接后缀”，不允许把现有整体 source CAS 简单去掉。需要单独验证 covered prefix 与 retained anchors，并在最终事务重新验证当下源和整个拼接结果。取消和资源释放用 `CancellationToken`，不用 abort。

#### 生成／安装分离的内部边界（2026-10-28）

前台 `compact` 先取得 `GeneratedCompaction`（候选 checkpoint、已验证投影及 usage），再显式 install；生成阶段不得写 `context_checkpoints` 或改变活动投影。前台为保持既有串行语义，可随生成结果返回本次 admission 与同一 600 秒 deadline，但 admission 不属于可持久化候选，也不能成为后续后台 ready 状态长期占用线程的方式。安装必须核对线程／模型身份与当前数据库 admission，再复用事务内完整 source fingerprint／前驱／模型／取消校验；等待期间有同序号编辑、追加或竞争 checkpoint 时仍拒绝，直到独立的 append-safe 消费逻辑明确验证并重建当前来源。生成成功不等于完成安装，不触发完成状态，不提前丢失 usage；安装失败、取消和超时返回原生成 usage。普通压缩入口、摘要来源检查、凭据过滤和 retry／omission 策略保持不变。

开发回归通过真实 SQLite 和合成摘要 provider 验证：生成不落库，显式安装与 reload 投影一致；生成后取消／期限届满／同序号编辑／模型变化／前驱竞争均不增加 checkpoint，且保留已发生 usage；未安装候选被丢弃后 admission 释放。此分离只是后台生命周期的共用基础，不声称已实现后台并发、ready 持久化、append-safe 消费或恢复。

#### 后台候选的 append-safe 来源围栏（2026-10-28）

候选另外保存版本化 `ContextAppendFence`，只含 thread、捕获时消息与工具各自的最大 seq、旧来源 digest 和可选 streaming owner 的稳定身份／UTF-8 长度／最后完整 response cursor，不持久化原始消息、工具输出或凭据。捕获必须来自同一 SQLite 快照；除末尾已知 owner 外，既有消息和调用均已终态。owner 只在空白且无调用，或文本末尾恰好对应完整已结束响应组时可扩展，不能冻结半个响应。

消费重新读取完整当前来源，再分别按消息／工具各自捕获 seq 范围，对消息／调用及其历史投递元数据、工具回执、图片计算同一有界 digest；历史投递可能遮住的 user 原文也参与候选 digest。只允许该 owner 在原 UTF-8 前缀之后追加文本和新的完整响应组，状态由 streaming 变 done 可归一；新增工具不得挂到其他旧消息，旧响应不能追加同 response ID 的调用。新消息、调用和相关图片／历史元数据可在捕获 watermark 后追加，不应让有效候选无条件过期；同 seq 编辑、删行、旧图片或历史元数据变更、micro 标记变化、旧调用路径或 response 身份变化必须拒绝。此来源围栏不是授权：消费仍须当前 owner／模型／前驱／预算校验，重新构造投影后在最终事务内复核候选围栏并用当下完整来源 CAS 安装，不能拿捕获 digest 替代最终事务检查。原整体来源指纹算法及旧 checkpoint 兼容性不变。

开发回归覆盖正常追加与重载、空／有工具的 streaming owner、保留锚点和覆盖来源篡改、图片／历史元数据／micro／调用路径变化、旧消息挂新调用、同响应跨边界及无效围栏。该存储原语不单独宣称后台调度、候选 ready 持久化和消费接线已完成。

增量审查发现 `29af6dc5e96fae589676043a9ecccde85f640942` 错将两张表的独立 seq 当作共同时间轴；真实 `messages::next_seq`／`tool_calls::next_seq` 各取自身表的 MAX，首个工具 seq=1 可以小于所属 assistant 消息 seq=2。修正为独立消息／工具边界，调用只按真实 message ID 归属，不比较跨表 seq 大小；保留所有内容／响应边界校验。fixture 使用真实分配器，并覆盖消息领先、工具领先、连续无跳号追加及同 owner 新调用。该提交 Actions [40709384606](https://github.com/puzige/vega/actions/runs/40709384606) 的 Nextest `122890170904` 原始汇总 80 发现／80 执行通过／0 跳过，Clippy `122890170383` 和 required `122892377437` 成功，但不能替代这项已确认的产品修复；修正后需要新版本审查和 CI。

生成／安装拆分提交 `eaee6266a4896d69aabfa3b0114a4f741e7a7116` 的 Actions [40702321829](https://github.com/puzige/vega/actions/runs/40702321829) 已成功：Nextest `122865129665` 原始日志 73 发现、73 执行通过、0 跳过；Clippy `122865130027`、required `122867516709` 成功。非实现者独立增量审查无发现，包含前一 scripted reply 修正；仅此版本已验证，不覆盖后续 append-safe 修改。PR 仍为草稿。

### 4.3 持久化与回读

- 新增递增 migration，保留旧 `context_checkpoints` 可读；新投影 metadata 包含 schema/version、策略与 continuation mode、源/owner fences、稳定 covered/kept 范围、head-truncated 标记及遗漏范围、恢复状态来源。
- 新模型投影不得写回 `messages` / `tool_calls` 原始正文；micro 清理、摘要、附件恢复是派生数据。
- 回读应从 conversation 的当前线程访问边界获取已保留 transcript/tool output，并执行现有凭据过滤和有界分页。不可要求模型读取本机整个 SQLite 文件；不可自动重执行旧 Bash。
- precompute candidate、completed summary、installed checkpoint 是不同状态。ready candidate 可持久化并在重启后重新校验，不能直接视为已安装；schema/体积/年龄/session/边界/保留 UUID 先校验，实际安装仍经过当前 owner/source/前驱/预算事务。参考恢复处的 model/CLI 版本比较是遥测，不把它误写成其硬拒绝条件；Vega 的当前模型适用性在消费/安装时重新核实。中断的 pending 生成不复活。
- usage 独立于是否安装：失败、丢弃与取消也可能收费；缺失 usage 保持 unknown。

### 4.4 provider 与本机能力

- 使用实际冻结模型定义中的 `context_window`、`max_output_tokens` 以及用户覆盖。既有配置的 input-limit / reserve 语义要显式迁移或适配，不把 input cap 偷换成 total window；不直接改用户本机配置/数据库。
- Anthropic provider context editing、cache-sharing、私有 feature assignment 分别列 capability。未支持 route 使用已声明的客户端等价策略或明确不可用，不静默发送未知字段、不伪报服务端缓存节省。
- reference feature flags 不是 Vega 可调用服务。客户端分支可移植；远端 rollout 默认值无法静态证明时保留未知和验证条件，不用“默认启用”措辞。
- 不新增白名单外依赖。新增跨层 UI/service 类型经 `vega_conversation::types`；runtime 保持 headless，复用既有 runtime→conversation hook 方向，禁止循环依赖。

### 4.5 Vega 原生适配决策（用户裁决后实现合同）

- 不复制上游实验分配系统。默认使用 control 摘要合同、客户端 keep-recent micro、阈值驱动压缩和 reactive 溢出恢复；预计算使用明确可计算的提前阈值。专属 provider hint/server edit 不发送，客户端清理独立标为估算，不宣称缓存账单收益。没有 cache expiry 的当前路由不运行伪造的 idle-cache 定时器。
- 保持已保存模型 input limit 的含义作为硬输入上限，不重写数据库配置。运行时将预算分为硬输入容量、auto policy 输入容量、summary reserve 和主响应上限；参考的 20000-token reserve 只用于压缩预算，不把用户原来更大的主响应能力静默截成 20000。已知 model snapshot 进一步限制真实硬窗口；未知时沿用标为假设/显式设置的本地能力。
- 通常 auto 阈值为 policy 可用输入减 13000，block 为硬输入减 3000，precompute 为 min(policy 输入的 80%, auto 阈值)，安装结果必须严格低于 auto 阈值。小窗口的 auto buffer=min(13000,floor(policy 输入/5))，block buffer=min(3000,floor(硬输入/20))，使阈值随容量单调且为正，避免在 13000/3000 边界骤降为 1 或每轮死循环；开发回归覆盖边界两侧。此小窗口规则是 Vega 多模型适配，不声称上游原式。
- 本卡只做压缩内部生命周期；删除外部 hook 指令合并、消息替换和通用插件接口验收。当前 user 的自定义压缩要求仍支持，但只在摘要请求中作为约束，不变成工具执行权限。
- 源覆盖/保留计划在存储中版本化保存，新增字段只存来源游标/ID/模式/遗漏标志，不复制凭据正文。原始消息仍由既有审计与凭据策略管理；即时重建和重启重建使用同一解释器。

### 4.6 已核实的具体参考合同

- **预算**：令 `R=min(maxOutputTokens,20000)`、`W=policyWindow-R`、`H=hardContextWindow-R`。通常 compact=`W-13000`，warn=`compact-20000`，block=`H-3000`；auto 关闭时 warning 基准为 W。合法 pct override 取其阈值与 `W-13000` 的较小值；block override 独立。precompute=`min(W-round(W*f),compact)`，f=0.2 只是 scalar fallback，可被窗口/REPL/SDK 表覆盖。例：policy=200000、hard=1000000、maxOutput=32000，则 R=20000、W=180000、H=980000、compact=167000、warn=147000、block=977000、fallback arm=144000。该例仅验算公式，不代表任一模型实际配置。小窗口在 Vega 必须有边界检查，不以整数下溢模拟参考 JS 负数。
- **partial 选点**：`up_to` 摘要 `[0, selected)`，保留 selected 起的后缀，安装 summary→kept；`from` 摘要 selected 起、保留之前的前缀，安装 kept→summary。后者摘要请求可见前缀作为上下文，但不能把保留区重复标成 covered。选点不是工具组内部任意字节。
- **分组/反应式**：以 assistant response 身份区分回合（不是只看可见文本偏移），不足两组失败；初始保留一组，token gap 可增加保留组数以缩小摘要输入。last-resort 可改为 summarize-all/完整组头截断；最多三次头截断不等于最多三次总 API 请求。precompute 禁此 last-resort；reactive 的一次受限 deeper-pass 与单次 fork `maxTurns=1` 分开。
- **后台所有权**：pending 结果只可由同 controller/generation 发布；正常消费移除候选，fork 借用不移除；只取消前台等待时后台可继续，必须由明确的生命周期取消控制，不能把导航/等待取消与用户 stop 一概合并。模型切换取消 pending，ready 的适用性延后复核；Vega 需要在当前权限下重新取得安装权。
- **后台恢复**：参考 sidecar schema=3、上限 8000000 bytes、最长 7 天，包含 session、边界及保留 UUID。Vega 用自己的版本化 SQLite 候选实现同等生命周期，不要求照搬文件名或 JSON 作为数据库。模型/CLI 版本匹配在参考 rehydrate 处仅遥测。
- **空闲会话压缩**：独立 off/log/compact 状态，参考 fallback off；1h cache 活动到过期时间的 90% 处调度，fraction 限 0.5–0.95；默认至少 200000 token、配置下限 100000、迟到超过 60s 不执行。还要验证 prefix、活动、owner、额度与缓存仍 warm。没有可信 cache expiry 不得凭空调度；这不是 3900s idle micro。
- **摘要变体**：control/lean/short/capped 的选择先 env 后 feature，非法值回 control；reactive 使用该选择器。capped 的 2000 words 是提示词而非确定性截断。Vega 不展示或持久化分析草稿，保留任务所需摘要；该安全适配与参考 control 的输出形状差别明确记录。

### 摘要变体原生接线（2026-10-27）

`compaction/summary_variant.rs` 将 control/lean/short/capped 解析与提示约束集中管理。Vega 使用 `VEGA_COMPACTION_SUMMARY_VARIANT`，不读取 Claude 私有实验配置；未设置、非 Unicode、空值和非法值回 control。hook 构造时冻结选择，所有预估与实际分阶段摘要请求使用同一变体，避免中途环境变化造成估算/请求不一致。各变体均保留原有任务来源、用户更正、停止/权限、未知副作用与附件真实性约束；lean 强调去重和操作性，short 强调紧凑交接，capped 请求不超过 2000 words，但不按字符或词数硬截断结果。control 不生成分析草稿是已声明的安全适配，不声称提示文本逐字复制 Claude。开发回归检查解析回退与实际请求构造；真实模型摘要质量仍待用户手测。

### 版本化投影安装与重建（2026-10-28）

新增可空 `context_checkpoints.projection_plan`；空值继续使用旧前缀游标合同，schema 1 保存压缩策略、调用方续接模式、owner/真实 task ID、覆盖/遗漏范围及有序投影片段。范围端点记录最后消息身份和可选完整 assistant response 末工具身份、response ID、UTF-8 游标；序号和字节偏移只是经过身份核验的坐标。片段表达原始来源范围、当前摘要、同线程旧摘要引用、历史任务引用和历史附件；不持久化 provider message、reasoning 或权限凭据。开放尾区间接纳之后正常追加的消息。摘要引用必须指向更早同线程 checkpoint，并由有界读取加载；未知 schema、缺失/跨线程引用和失效锚点拒绝，不能静默重放未校验区间。

安装事务校验完整 covered/omitted 组、来源/模型/前驱/取消及计划自身；新请求身份包含计划与前驱，允许同源上先后安装不同范围，绝不移除最终整体 source CAS。即时安装前的预算/replay 检查与重启重建共用同一 plan interpreter；即时保留区间可叠加调用者现存的 live messages，但必须逐个对应同一稳定范围，并核对 durable role/content/images/tool ID/calls（仅合并相邻无工具 assistant 文本以适配持久化分段），不借叠加扩展覆盖或授权。reasoning、live bindings、response layout、tool context 等运行态仅留内存，重启按相同 durable 片段冷恢复；不能为“立即/重启一致”而抹除仍需发送的 live replay。原始 transcript 不变。旧摘要引用继续受既有凭据扫描和清理约束，元数据不存正文。先将现有前缀路线真正迁入该机制，再接两方向 partial/controller；结构可表达不等于产品入口已交付。

### 派生附件区间边界（2026-10-28）

当前 `context_compaction::load_source*` 只加载、指纹化真实用户图片；assistant 截图通过独立 artifact/audit 读取，不进入此来源，因此现有压缩入口不会触发“被裁掉截图调用但图片仍留在来源”的报错。通用 `SourceRange` 仍须保证带 `artifact_call_id` 的派生附件只随同消息内被选中的所属调用保留，普通用户附件继续按消息归属保留。不能为了消除孤立附件而写回原始 attachment/tool 行或把不明 assistant 图片默默当用户图片。开发回归从真实截图插入和有界附件读取取得 provenance，分别检查来源加载合同、两侧区间附件和审计不变；不把构造来源的能力误称为当前压缩加载行为。

### 自定义摘要要求的请求边界

自定义要求只作为本次摘要转换的偏好，最多 8192 UTF-8 bytes；超限在 provider 请求前拒绝。所有分阶段输入估算与实际摘要请求必须携带同一冻结值，使用 JSON 字符串编码明确数据边界；不得覆盖摘要系统安全规则、触发工具、改变原任务身份或产生执行权限。不写入投影计划；无要求时保持原请求行为。该请求构造能力不代表 `/compact` 参数或局部压缩 UI 已接通。

### 两方向局部压缩接线合同

消息入口使用 durable message ID 和当前 source fingerprint，不以 UI 行号或文本选区字节作为锚点。up_to 不含选中消息，from 包含选中消息，均保留完整消息内的 response 组。只允许当前活动投影中的完整终态消息选点；旧摘要覆盖区不可重新选作原始来源。请求沿现有手动 owner、trusted action、取消、usage 和整体 source CAS 传递。已有摘要位于保留侧时引用原 checkpoint，位于压缩侧时纳入新摘要输入；不得恢复已覆盖原文或将保留前缀重复标为 covered。安装后开放尾区间保留后续追加。连续局部压缩以当前有序投影片段中的选点位置切分，而不是把所有旧摘要统一前置：压缩侧的原文与旧摘要按原投影顺序组成摘要输入，保留侧的摘要保持原位引用。新摘要只替换选中侧，不能把晚发生的摘要移到较早事件之前；legacy 先转换为同一计划再执行此切分。开发回归使用不同原文／摘要标记同时断言模型输入和安装结果的先后顺序。

### Full／reactive 原生入口与规划补充

- 显式区分 ordinary full 与保尾式入口，不再从 `require_source_fence` 推断策略；该字段仅用于来源校验。主 query 的阈值／技能预检和手动服务走保尾式规划，delegated teammate 的普通阈值走 full；typed primary overflow 覆盖普通选择走 reactive，并把可选 token gap 传给规划器。局部选区独立且不扩大范围。
- Full 对当前有序活动投影摘要，原位纳入旧摘要，不重新恢复已覆盖或遗漏的原文。安装后不保留捕获的普通消息原文，只保留来源标记的恢复内容与未来追加的开放 Source；task ID/continuation 保持，必要的原任务引用是显式恢复附件，不是假装保留普通原文，也不能授权。运行中的 assistant 只覆盖完整已落库 response；若存在不能安全覆盖的尾部，失败不安装，不将整条 streaming 行伪称终态。Source 在 assistant cursor 之后没有剩余正文／工具／附件时不生成空 assistant 片段；后续追加内容仍从同一游标正常回读。
- 保尾式规划以完整 response/source 游标为界，初始留一组；summary PTL 的已分类 gap 决定向前移动分界，无 gap 至少移动一组，每次输入必须真正改变。完整请求最终 fit、较原输入缩小、取消、前驱与全源 CAS 均在安装前检查。既有 prior covered／omitted 与摘要顺序必须保留。
- Last-resort 只在明确允许的 full／手动／typed PTL／本地硬阻塞路径使用；普通 API 错误、partial、precompute 和媒体错误不能获得丢弃资格。最多三次完整最旧组截断，记录精确 omitted，不把省略说成摘要覆盖；保留尚未处理的用户请求和至少一组。共享 32 次摘要请求、600 秒操作／300 秒空闲界限不放宽。一次相邻 deeper-pass 只由 fresh typed PTL 成功且重建至少三组授予；重试再 PTL 时消费，不能无限重置。deeper 使用更少的尾部组且最终必须更小，无法形成安全更深边界则失败。
- 必要开发回归：full／manual 区别、partial→full 的摘要顺序、空保留后的追加／reload、同偏移不同 response、typed-gap/no-gap 输入前进、完整组遗漏与审计不变、取消/失败 usage，以及相邻 deeper 的一次性与部分输出禁止恢复。当前本节为待实现合同，不表示云端已通过。

### Reactive 有序规划实现约束

保尾候选从当前活动 fragments 顺序切分，不从完整审计历史重新构建前缀。Source 的切分点复用 SourceCursor 的完整 response 校验；Summary／PreviousSummary 在压缩侧原位进入摘要输入，在保留侧保持引用。既有 covered 和 omitted 不丢失、不互换。候选最终安装继续经过完整分区校验；不得以全源 prefix_plan 覆盖既有 partial 分区。规划、摘要重试与最后恢复共享一次操作的取消、期限和调用计数。

响应组规划使用当前 fragment 的原始完整 response／完整消息游标，摘要引用也是有序输入单元；空的开放未来区间不计为保留组。fresh typed PTL 的成功结果返回实际保留的原始组数，至少两组（加新摘要至少三组）才能授予相邻 deeper；deeper 必须从前驱 Reactive 投影减少原始保留组数。运行态只在结果通过完整请求校验并应用后授予，下一次主请求的非 PTL 结果或任何已输出内容使资格失效，deeper 自身不能再次授予。

Last-resort 的三次限制计最旧完整来源组截断操作，不计 provider 调用；无 gap 每次去掉剩余可摘要组约 20%（至少一组），已知 gap 以各组估算量选择。被跨过的旧摘要／任务／附件引用不能伪称遗漏的原始来源，而须保留在新摘要之前的原有顺序；原 covered／omitted 不互换。尚未处理的最后用户消息及开放未来区间始终保留。输入推进失败、真实缩减失败、预算不满足或没有至少一个可摘要组时，不安装 checkpoint。保留侧 live 段必须逐范围唯一匹配并验证 durable 内容，不能为绕过组合问题抹掉 reasoning。

Reactive 恢复图片仍使用既有有界 HistoricalImages 投影：新进入摘要／遗漏区的完整来源消息，以及进入摘要区的旧图片引用，都必须在安装／reload 中恢复历史图片证据；摘要请求只接收显式无视觉内容的 provenance。已有保留图片引用维持原位置并扩展到需要的截止点，不再另加重叠引用；没有保留引用时放在新摘要之后。截止点不得越过仍按 Source 保留的图片消息，原始附件和审计不变。被 head 截断跨过的任务引用已保留时，不重复注入同一任务引用。

### 客户端 micro 生命周期（2026-10-28）

原生 context hint 在自动压缩开启且主请求达到 precompute 提前阈值时检查，不依赖不存在的 provider hint／cache expiry。仅清理当前活动投影中已有完成结果的 `read`／`Read`、`grep`、`glob`、`bash` 调用；这是当前 Vega 工具集合的明确映射，不把任意 MCP、子 agent 回执、编辑／写入、桌面截图或权限结果纳入。候选按真实调用顺序，保留最新五个 eligible 结果，已标记清理项不重复产生节省；失败结果保留其失败状态而非改称成功。所有候选替换后按完整 system＋模型消息＋工具 schema 的 wire 估算，净节省不足 20000 tokens 则 no-op；只作估算，不增加／冲抵 usage。

清理只替换结果正文／媒体为不具权限的历史清理引用，保留调用身份、配对、状态以及其他消息、live reasoning／bindings。原始 `tool_calls.output_text`、消息和附件不改；新增 `context_micro_compactions` 保存 thread/call ID、原始完整调用内容 digest 与时间，不复制正文。来源加载验证 digest 后标记结果；修改／删除原记录使标记失效或级联删除，重启和摘要来源重建复用相同占位规则。当前线程权限／模型／完整来源／前驱 checkpoint／取消在写事务内复核，标记参与来源指纹，旧异步摘要不能越过 micro 的更新安装；没有经过校验的持久化成功不替换 live 内容。一般摘要继续从活动的已清理投影读，不重新搬回已清理结果正文。

Runtime 通过既有内部 compaction service 的独立 micro 方法取得结果，默认未支持实现 no-op；不调用摘要 provider、不发起通用 hooks。先校验只改清理目标、真实净节省及 source identity 再应用；清除 input accounting anchor／warm replay candidate，但保留消息分段与 live reasoning。micro 不重置摘要失败熔断／rapid-refill、不授予 deeper，也不伪报一次摘要完成。取消或来源冲突不继续发送。开发回归覆盖 keep-five／不足省量／重复 no-op／失败状态／reload／后续摘要／原始审计／取消与来源变更；历史原文的有界凭据过滤回读通道仍属本卡后续工作。

Micro 增量审查补充：来源校验按当前 projection plan 的 retained Source 做完整内容比较，不再用 legacy covered-through 截掉 partial-from 留下的早期助手组；只有无 plan 的旧 checkpoint 保留该 cutoff。桌面观察／截图等合法临时 overlay 在校验副本中按既有规则归一化，两侧等价后才继续；原始 live 图片、观察和绑定不因 micro 被抹除。micro 的独立请求明确携带 system ReplayFrame，服务在落库前和 runtime 应用前均对同一个 framed wire 投影计算触发阈值及净省量；不能把 GLM 已不发送的历史 reasoning 算入触发量。新增非 fenced partial/disjoint、live 截图与 GLM frame 门槛回归。

Micro 回归夹具修正：`d89e0594029ce3e29484128958fd800d016532dc` 的 Actions `40692740896` 实际执行 70 项，69 通过、1 失败、0 跳过，Clippy／格式通过；独立增量审查确认三个生产修复均已处理。失败发生在 partial/disjoint 场景准备的水位断言：唯一用户消息 seq=1，partial 的 legacy 水位按最后用户之前计算为 0，而不是按选中尾部 seq=100 计算。夹具增加 seq=99 的后续用户消息，使旧助手 seq=2 保留且确实早于水位 98；保留非 fenced 校验、篡改拒绝、36 个精确目标、live 结果／checkpoint 不变与无额外摘要请求的验收，不修改生产水位规则或放宽断言。

第二轮 `9d827511e81bc9e488742d8f17a8e302f519016a`／Actions `40695779457` 同为 70 执行、69 通过、1 失败、0 跳过，Clippy／格式通过。单选区分支已通过 micro 的来源拒绝／精确目标／live 与 checkpoint 检查，随后在替身 `requests()` 的剩余响应检查失败：该分支只应消费一次准备摘要，却预装了两条。按 disjoint 与否分别准备一／两条响应，保留队列耗尽和请求数断言，下一轮仍须实际执行两种分支。

Micro 当前验证：`dfb965795f54b038e1a3e05ad846c597a3e87f6e`／Actions `40699159283` 的 Clippy `122853943726`、Nextest `122853944249` 和 required `122856560420` 均成功；原始日志 2026-10-28T13:47:02Z 记录 70 发现／70 执行／70 通过／0 跳过（22 binaries）。该结果不覆盖随后生成／安装拆分的新代码；未做本地 Vega 执行、编译或测试，PR 继续草稿。

### Reactive 增量审查修正（2026-10-28）

Partial 的“不允许遗漏”不等于“不允许 PTL 重试”：固定选区、顺序和 covered／retained 分区不变，typed summary PTL 时缩小本次完整摘要组批次并分阶段纳入全部选区；每次实际 wire 输入必须改变，不能丢组。与其他路线共用 32 次请求／600 秒／300 秒 idle／usage 记账，单组仍不适配或普通错误则失败不安装。保尾规划只在实际 reactive 或需要移动边界时构造；普通阈值成功路径仍允许将 offset 未知但已完成工具结果的 owned streaming 尾部原样保留。不能因此允许 full 覆盖该尾部或凭空制造其内部边界。

### 集成与终态回归补充

Full 未完成尾部回归区分两道拒绝边界：live 请求中无法归属到完整 response 的尾部 assistant 文本先被来源校验拒绝（SourceChanged）；精确 source version/fingerprint 的 fenced 请求进入 full 规划后，被不完整尾部拒绝（InvalidProjection）。两条路径均不得调用摘要 provider、安装 checkpoint 或改变原始审计。`f51230683de9232d25b0d8095a1011b68be5c064` 的 Actions `40648493811` 执行 42 项、41 通过、1 失败，Clippy 通过；失败用例以 live 请求断言规划层错误，修正开发夹具覆盖两层，不放宽生产来源栅栏或接受不完整响应。

Full 首批 `2a5fa2ad209aef326dad1e06444791a3f15acf9a` 独立审查发现 `PersistenceActorConfig` 移动后读取 delegated 标志的静态编译阻断；最小修正是在移动前冻结该布尔值，不克隆整份配置。该提交的 PR 因 master `29bea083a7d955da7c64d96101ae8f4b8f1e2b12` 的 UI 用量入口增量出现冲突，未产生 Actions run；整合保留 master 的无有效用量时隐藏入口规则及本卡 partial 菜单／状态清理，不撤销 #720/#722。修正与 rebase 必须在新 head 重新验证。

同步 master 的协议诊断迁移后，本分支尚未交付的投影迁移顺延为 `0047_context_projection.sql`，保留 `0046_provider_protocol_reason.sql`，不修改既有正式迁移。开发期通过仅测试 feature 暴露 UI 终态快照，直接断言状态、失败码、usage、generation 和 busy，而非借一次状态写入间接判断。完成处理提取为同一生产方法，测试可重复提交已退出的旧完成；新任务的 owner／request／lease、取消令牌及可见状态必须保持不变。路由切换后的旧完成只释放其原租约，不应用业务结果；返回同一会话也不能恢复旧 epoch 的安装权。旧 worker 退出时按 thread／model／request 精确退役其临时 Compacting 行、活动组引用与对应 pending 状态，保留单调编号屏障，不能让缓存 stream 永久 busy，也不能改写新终态。即使模型切换已经重置控制状态，也必须清除原模型的临时行；新模型或较新请求的记录、pending、取消状态和活动组阶段不能被旧请求清理。开发回归覆盖切走后结束／返回后才结束两种时序，以及控制状态已重置的模型切换，返回后应可发起新请求。需要的持久化结果只通过当前 owner 的新读取恢复，不从旧回调跨路由应用。

### Controller/UI 竞争请求补充

同一 owner 的手动压缩尚未退出时，无论请求号相同或不同，后续请求都不得启动 worker、替换租约或向原 stream 写入较新失败状态；原任务必须保留 busy 并能应用自身终态。不同请求号的拒绝不能污染 UI 的单调 generation 屏障。

### Controller/UI 进程内回归边界（本轮用户批准）

优先覆盖真实窗口和 ConversationStream 的压缩请求接入、独占租约、取消、切换会话／模型、过期完成拒绝及终态应用，再推进 full/reactive。开发期允许启用现有 gpui-kit 的 test-support feature 及其传递测试依赖；仅 Actions 执行，合并前连同开发测试专用接线移除。测试窗口不启动 pricing/updater；可控 worker 仅替换配置／凭据／网络等外部执行，不替换 controller 的准入、归属、租约、取消和结果应用决策。拒绝请求不得启动 worker；旧结果不得释放新租约或更新另一会话／模型；有效结果应更新原请求终态并清除 busy，取消必须传递至该次 worker。原生菜单／焦点及真实服务联调仍由用户手测，不以进程内回归冒充。保留现有 service 的存储、usage、取消、CAS 回归，PR 保持草稿，本地不编译测试。

## 5. 分阶段实现与验证顺序

所有阶段属于同一卡完整范围，任何单阶段完成都不能宣称全量对齐。

1. **参考合同**：完成版本绑定矩阵、默认/gate/外部能力分类；补齐本规格的数值与方向语义，先消除会改变数据模型的未知。固定可恢复文档版本。
2. **核心投影/预算/续接**：版本化 plan 与共用 reload、策略预算、任务来源、full/manual/partial、安全安装和摘要变体；用开发回归验证任务元污染、完整工具组、旧 checkpoint、取消/CAS。
3. **轻量与恢复**：micro 各路径、只读历史回读、reactive/last-resort、状态恢复、独立熔断；复核 wire 与 usage。
4. **后台与生命周期**：precompute 的准备/生成/消费/失效/取消与原生状态恢复；覆盖 append-vs-edit race、model/owner 变化、重启和停机。不建设外部 hooks 生命周期。
5. **产品入口与交付**：只接压缩所需 `/compact`/partial/状态；独立审查绑定停写版本；现有 Actions 验证开发回归；固定源码/完整 SHA/原始结果后删除开发 Rust 测试及独占接线；最终 head required check 与 Rust 空集清点；然后合并，保持 Issue OPEN / In review 等用户手测。

一次卡默认不超过三次提交：规格基线；实现与必要开发测试；留痕后的最终测试清理。实际验证失败必须修责任层，不因提交数量而放宽断言或忽略失败；若确需超出提交策略，先记录原因并按项目规则处理，不继承其他卡的特殊许可。

## 6. 当前证据、未知与下一步

### 局部压缩开发云端记录（2026-10-28）

用户手动推送后已核实 PR #690 为 OPEN / DRAFT，远端 head 为 `dae7260b28cb704d260724a4c30e5d4939225f77`。Actions [40616417701](https://github.com/puzige/vega/actions/runs/40616417701)：Clippy job `122579848836` 成功，Nextest job `122579848522` 失败，required job `122581448127` 失败。Nextest 原始汇总为 31 执行、20 通过、11 失败、0 跳过；失败均停在测试 fixture 的 `full_access_grant IS NULL OR json_valid(full_access_grant)` 约束。修正仅将不执行工具的压缩 fixture 设为只读／无 grant，不改生产约束，不重跑旧失败换绿。后续顺序修复及不同标记回归须验证新 head，不能继承此轮 Clippy 结果。该历史版本当时尚缺隔离 pricing/updater 和可注入 worker 的 controller 测试入口；后续记录如下，hook/service 回归仍不冒充原生手测。

后续开发版本 `5e577ad68e607402c71193f501e023de93384d79` 的 Actions [40629705761](https://github.com/puzige/vega/actions/runs/40629705761) 全部成功；Nextest `122622976395` 原始汇总 36 执行、36 通过、0 跳过，Clippy `122622976617` 与 required `122624509587` 成功。其中四项 controller 进程内回归覆盖请求、取消、路由变更与错误模型拒绝。更早版本的 unused import／type complexity 已修正，不通过 lint suppression 规避。

当前增量补充竞争请求、精确终态、过期回调不释放新租约、路由返回／模型切换退役临时行，以及新请求可恢复；重新同步到 master `ea68313dcd9686dfd24d3c7fcaaf8c16df7b5a7a`。本地仅 rustfmt、diff 与静态检查。该增量版本 `46111a53c2659e715e8b9917078d1fe36c5be88a` 已成功直接推送，独立静态审查无确认发现；Actions [40642209153](https://github.com/puzige/vega/actions/runs/40642209153) 格式通过，但 Nextest `122662698766` 和 Clippy `122662699251` 均因开发 fixture 缺少 master 新增的 `Thread.draft_authority_epoch` 编译失败，Rust 测试未执行。修复为与普通非草稿 Thread 一致的 `None`，不改生产权限边界或断言；同时同步 master `0d42e3ef43881f2db89cf759c1050e61e36c9490` 的窄栏消息导航修复。后续修正版本 `7bec38cc0ca833fb9c7d8fca313e0cec6cd758a6` 的 Actions [40643480294](https://github.com/puzige/vega/actions/runs/40643480294) 成功：Nextest `122666787435` 原始汇总 40 执行、40 通过、0 跳过，全部 8 项 controller 回归成功；Clippy `122666787036`、required `122668375725` 成功。fixture 增量独立审查无发现。该结果只覆盖此提交，不覆盖后续 full/reactive 开发修改。PR #690 仍 OPEN / DRAFT，Issue OPEN / In progress；整卡 full/reactive/precompute/micro/restoration/idle 等后续能力未交付。

已完成静态核实：当前预算、partial-assistant 覆盖、summary 请求/包装、实时安装/reload、工具预览、manual owner、SQLite CAS、模型能力字段。参考取证共 62 项有界片段，原研究执行在报告整理阶段失败，已确认释放写入权并保存产物；后续只整理既有证据，不重复全量调查。未进行模型调用、本地 Vega 运行、编译或测试。

已消除的重要误推：手动 `/compact` 不等于 ordinary full；自动/错误恢复可发生在用户任务内部；reactive 也保留尾部；micro 的 hint/idle/server 不共享全部门槛；precompute 支持校验恢复且前台等待取消不必取消后台；model switch 的 pending/ready 不同；rapid-refill 的计数来自 query/API 回合推进而非人类消息。

真实实现前提：Vega 的 `ModelApi` 和 `ProviderApi` 目前只有 ChatCompletions / Responses，没有 Anthropic Messages、首方 beta/context-hint/server edits/缓存寿命来源；也没有可承载 `session.compact` 消息替换的外部插件平台。移植客户端选择/生成/恢复逻辑可以在现有路由实现，但不能证明 Anthropic 私有服务或远端实验分配同效。完整“功能行为对齐”与“原协议/插件兼容”必须明确区分，不能只加 capability=false 的空接口就声称后者完成。

产品边界已由用户明确：Vega 原生实现全部压缩机制，仅限压缩相关机制，hooks 生命周期太重而不做。不新增 Anthropic Messages provider/Claude 外部插件平台，也不复刻私有实验服务。保留完整客户端预算、micro、full/manual/partial、reactive、precompute、状态恢复、持久化、回读、续接、安全与计量路径。原 [BLOCKED] 平台选择已解除，下一步按此合同实现。

规格阶段静态检查包括 Markdown 链接、21 项验收编号、62 项证据编号与 JSON 结构、脱敏路径和空白检查。卡片保持 OPEN / In progress；不标 Ready、In review 或 Done。

### 自动压缩快速回填实现合同

对应 A17、参考 E01/E08/E09/E29/E37/E58。runtime 的一次运行内共享独立 rapid-refill 状态，普通自动压缩和技能预检压缩都在真正调用 hook 前检查。第一次成功压缩建立基准；此后距离上次成功压缩不足三个主 provider 请求时，每次再压缩累计一次，第三次拒绝并返回已有上下文超限错误。经过至少三个主请求再压缩则清除快速回填计数。只有成功安装才重置回合距离，失败不伪造成功；连续失败计数仍单独维护。回合在主 provider 请求派发前递增，不统计摘要请求、传输内部重试或用户消息数量。手动压缩和未来 precompute 独立，不共用该熔断。

开发回归覆盖首次成功、连续第三次快速回填拒绝、三回合恢复、失败不重置距离；后续入口接线审查确认没有绕过状态的自动压缩调用。本地仅静态检查，执行交 Actions。

### 基础实现开发检查点

基线已更新到 `07a9b355`，保留 #680 的连续 assistant owner 修复。当前基础实现分离主响应/摘要输出上限，接入冻结模型能力和有界策略阈值；摘要任务直接引用捕获源中的最新用户行。轮内摘要覆盖原任务时，即时与重启共用来源标记的历史任务引用，不更改原始消息、不生成权限。GLM 无保留真实 user 后缀时全段冷恢复，末尾边界只允许完整冷历史；持久化前与运行时使用同一 replay 校验。

非实现者对基础差异静态审查发现的两项阻断（未显式指定输出 cap 时预算不约束实际请求、GLM 边界指向 assistant）已修正，增量审查未发现剩余确认代码阻断。审查不代表整卡实现完成或行为验证通过。开发回归包括预算边界、实际 agent 出站 cap、replay 边界、任务引用及真实 hook 连续两次轮内压缩/持久化恢复；测试源码仅留开发分支，须取得 Actions 结果并固定可恢复版本后按仓库政策清理。本地仅格式与差异检查，没有执行 Vega、编译或测试。云端结果、PR 与最终审查另行记录。

尚未完成：版本化通用 projection plan、完整 micro/full/manual/partial/reactive/precompute/idle 能力适配、恢复附件与历史回读、熔断和对应 UI；不能将此基础检查点称为全量对齐。
