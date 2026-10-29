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

增量审查发现 `29af6dc5e96fae589676043a9ecccde85f640942` 错将两张表的独立 seq 当作共同时间轴；真实 `messages::next_seq`／`tool_calls::next_seq` 各取自身表的 MAX，首个工具 seq=1 可以小于所属 assistant 消息 seq=2。修正为独立消息／工具边界，调用只按真实 message ID 归属，不比较跨表 seq 大小；保留所有内容／响应边界校验。fixture 使用真实分配器，并覆盖消息领先、工具领先、连续无跳号追加及同 owner 新调用。修正提交 `bca7e338ca1c3ef1c758a7e51be8a95730c5699d` 的 Actions [40713706790](https://github.com/puzige/vega/actions/runs/40713706790) 已成功：81 发现／81 执行通过／0 跳过，Nextest `122905534396`、Clippy `122905534134`、required `122909045215` 成功；非实现者增量复核确认原 P2 已修复、无新增发现。原缺陷提交 Actions [40709384606](https://github.com/puzige/vega/actions/runs/40709384606) 的 Nextest `122890170904` 原始汇总 80 发现／80 执行通过／0 跳过，Clippy `122890170383` 和 required `122892377437` 成功，但它未覆盖这项已确认的产品缺陷，故另行修复并取得上述新版本审查和 CI。

生成／安装拆分提交 `eaee6266a4896d69aabfa3b0114a4f741e7a7116` 的 Actions [40702321829](https://github.com/puzige/vega/actions/runs/40702321829) 已成功：Nextest `122865129665` 原始日志 73 发现、73 执行通过、0 跳过；Clippy `122865130027`、required `122867516709` 成功。非实现者独立增量审查无发现，包含前一 scripted reply 修正；仅此版本已验证，不覆盖后续 append-safe 修改。PR 仍为草稿。

### 4.3 持久化与回读

- 新增递增 migration，保留旧 `context_checkpoints` 可读；新投影 metadata 包含 schema/version、策略与 continuation mode、源/owner fences、稳定 covered/kept 范围、head-truncated 标记及遗漏范围、恢复状态来源。
- 新模型投影不得写回 `messages` / `tool_calls` 原始正文；micro 清理、摘要、附件恢复是派生数据。
- 回读应从 conversation 的当前线程访问边界获取已保留 transcript/tool output，并执行现有凭据过滤和有界分页。不可要求模型读取本机整个 SQLite 文件；不可自动重执行旧 Bash。
- precompute candidate、completed summary、installed checkpoint 是不同状态。ready candidate 可持久化并在重启后重新校验，不能直接视为已安装；schema/体积/年龄/session/边界/保留 UUID 先校验，实际安装仍经过当前 owner/source/前驱/预算事务。参考恢复处的 model/CLI 版本比较是遥测，不把它误写成其硬拒绝条件；Vega 的当前模型适用性在消费/安装时重新核实。中断的 pending 生成不复活。
- usage 独立于是否安装：失败、丢弃与取消也可能收费；缺失 usage 保持 unknown。

#### Ready 候选与后台计量持久化（2026-10-28）

新增 SQLite migration 分开保存 ready 候选和 generation/stage 计量身份。每线程最多一个 ready，候选 schema=1、总编码不超过 8000000 bytes、创建时间不可在未来且年龄不超过 604800000ms（7 天）；包含 generation、线程、原模型、原始捕获版本／完整指纹／独立 seq 围栏、前驱、合法 Precompute plan、摘要、估算及摘要偏好标识，不保存 live reasoning、临时桌面 observation、provider 或 admission。只有完成且已结算的生成可发布 ready；pending 不落候选表，重启不能复活中断生成。

每线程另存不含内容的当前 generation slot，启动时按期望前代 CAS；它只防旧结果发布，不提供中断恢复能力。ready 发布、读取、移除和失效都按线程／generation 身份进行，已消费／失效 generation 退役且不可再发布，旧异步结果不能覆盖／删除另一 generation 的候选。恢复先限制字节和版本，再校验 age、来源围栏、plan／摘要引用及前驱；前驱摘要及其引用内容另存摘要依赖 digest，原记录脱敏而 ID 不变也必须失效；原模型与当前模型不相等不在恢复时一刀切删除，真正消费须按当前模型重新估算并复核安装权。正常消费移除，借用只读；源编辑／删除／凭据脱敏使候选不可恢复。发布与恢复均不安装 checkpoint、不修改原始对话；最终消费接线仍需后续实现。

每个实际摘要阶段按不可复用的 generation ID＋stage（1..32）独立事务结算，与候选是否成功、丢弃、取消或过期无关。已报告 usage 写 thread-level `token_usage` 并保存幂等结算标记；缺失 usage 只保存 unknown 标记，不编造 0 tokens。相同身份相同内容重试返回原结果，内容冲突或跨线程／模型复用 generation 拒绝并回滚，不覆盖账单；删除候选／线程不会级联删除用量身份或账单。ready 的 stage 数及 usage_complete 必须与账目一致，恢复和消费不再次写账。进程被外部强杀前尚未收到／未落库的 provider usage 无法推断，不能宣称能恢复服务端未知账单。

开发回归覆盖重开数据库后的 ready 读取、合法追加、过期／未来／超限／错 schema／错线程／来源修改、旧 generation 不能替换或删除新候选；计量重复／冲突／unknown／跨线程／取消后丢弃及删除后的账单保留。本批仅交付持久化边界，不把存储 API 当作完整后台调度或产品 UI 接线。

#### 临时摘要查询分支（2026-10-29）

真实摘要生成入口优先尝试继承父请求前缀的单响应临时分支，不新建持久化对话或通用 agent fork 平台。仅当本次摘要历史与父投影前缀逐项一致、无媒体剥离或聚合改写、存在冻结 provider model snapshot，且完整 framed wire（含原工具 schema）装入摘要输入预算时使用；继承父 system／工具／冻结模型与 reasoning，附加有界摘要指令，tool_choice=None 且 collector 拒绝任何工具调用，不进入工具执行循环。独立摘要仍是范围不相容／超预算时的路径；临时分支空白、格式失败或 provider 错误可在剩余阶段／期限内回退独立摘要，取消、凭据检查失败等安全错误不得被回退吞掉。每次实际请求独立结算，失败尝试的用量不丢失；不宣称 provider 缓存命中或账单节省。

完整父历史均纳入本次摘要且没有活跃 source owner 时，临时分支可在已有当前 admission 下只读借用兼容 Ready：同一快照验证候选、来源、前驱、偏好和当前模型，使用生产 projection interpreter 保留未覆盖后缀；借用不安装、不退役、不新记原候选账单。范围不完全匹配、partial、后台生成自身或不适用候选不借用，不能将摘要范围之外的后缀偷偷纳入摘要。借用保留完整请求预算与当前凭据过滤；原始源／候选／账单须保持不变，最终前台安装仍走独立的当前来源 CAS。此增量不冒充任意业务查询 fork、状态恢复附件或整卡完成。

开发回归覆盖真实生成入口的父前缀／schema／model snapshot 保留、错误／空白／工具响应回退及逐请求用量、取消／凭据错误不回退、改写前缀与超大 schema 拒绝；真实 SQLite／worker 覆盖重复借用后仍可正常消费、取消等待不取消后台，以及实际摘要入口借用后候选／账单／checkpoint 不变。无实际 replay frame 的借用不接受活跃 owner，不制造 replay 权限。本地仅 `cargo fmt --all -- --check` 和 `git diff --check`；编译、Clippy 与行为执行待现有 Actions。

首版提交 `2559a7299046844c676999e2ea7f5a2c66cf561b`（tree `67e4e3994ffe7e524cff97aa8be957712ab7d767`）的 Actions [40812130766](https://github.com/puzige/vega/actions/runs/40812130766) 在测试编译阶段失败：Nextest `123244654858`、Clippy `123244655138` 与 required `123245654438` failure。E0063 指向既有 GPUI `context_tests.rs` 的 `Thread` 构造缺少 master #744 新增 `reasoning_preferences`，未取得成功 discovery 或测试执行，不是 no Rust tests。修正仅补默认字段，不修改生产实现、测试断言或 CI。同步 master 后的修正须重新验证。

字段补齐提交 `4801a74e39c992d16e08865a41bed74d84286b53`（tree `8468e12eb9bb778ba140c2038d6a960633b02320`）的 Actions [40812918319](https://github.com/puzige/vega/actions/runs/40812918319) 中 Clippy `123247215270` success；Nextest `123247215458` 实际 150 执行／144 通过／6 失败／0 跳过、22.410s，独立 discovery 数未取得，required `123248312133` failure。五项原 GPUI 生命周期回归因 `reasoning_preferences=None` 表示尚未加载，production submit gate 正确拒绝，worker 未启动而超时；fixture 改为已加载空列表 `Some(Vec::new())`，不改门槛或超时。新 fork 前缀回归揭示真实实现错误：构造器错误附加整段 cold replay frame，collector 的生产投影将父 assistant／tool 改写为历史记录，违反前缀保留合同。修复只取消该伪造 cold frame，保留原父消息的生产投影、安全过滤与工具禁用，原精确前缀断言不变。其余新增四项回归已执行通过；修复仍须新 head 云端验证。

修复提交 `7a88f001e256d2c0d9dcd4664f050288457d6efd`（tree `df5cc1d41105af1214f823bc3bf0e5ab020a9d2b`）的 Actions [40813921199](https://github.com/puzige/vega/actions/runs/40813921199) 中 Clippy `123250432186` success，Nextest `123250432371` 150 执行／149 通过／1 失败／0 跳过、22.096s，required `123252284374` failure，独立 discovery 数未取得。五项 GPUI 已恢复通过。剩余 fork 断言错误比较 provider 观测值和未投影 durable 消息：正常父请求也必须经过 `project_model_messages`，历史工具的 `live_tool_calls=None` 保护会将其转为无权限审计记录，不能为前缀相等绕过。回归改为严格比较正常父请求生产投影的完整前缀，并额外在投影前断言 fork 构造器保留原父消息，既检测伪造 cold frame，也保留历史工具安全语义；不改生产投影或工具权限。前述 cold frame 缺陷修复仍保留，不能据此把原始历史工具身份强行发送为活跃调用。

摘要分支验证提交 `e362fcf5d64a6702a83ec18e8d4e6f2f07e84a11`（tree `eeb15c7a5a7a4da3ba7b7301313f42e070a003b0`）的 Actions [40815811270](https://github.com/puzige/vega/actions/runs/40815811270) 全部成功：Nextest `123256469404` 原始输出 150 发现／150 执行通过／0 跳过、19.654s，Clippy `123256469907`、required `123257969822` success；两层前缀回归的增量独立静态审查 Clear。该结果不代表整卡完成。

#### 文件上下文恢复（2026-10-29）

验收：压缩覆盖成功的 read／Read 后，即时续接和重启投影均提供有工具调用来源的文件引用；排除仍在 Source 保留段中的调用，按调用 seq 最近优先、路径去重，最多 5 文件。路径只能从持久化成功读调用的 JSON 输入取得，不从摘要解析；引用本身不提供读权限或编辑凭据。恢复只读取当前文件，不重放旧工具或信任旧输出；删除、不可读、路径变化、权限拒绝和取消不得显示为已恢复。

运行态在主请求预算检查之前，经现有 prepare_read_call／authorize_call 路径重新校验路径与当前权限，使用独立 ReadState 的工具副本读取，不能更新本会话 read-before-edit 版本。恢复结果仅存在 ChatMessage 的运行态附件，原始正文、审计和来源比较保持不变；重启重新读取，不持久化文件正文。provider 投影以无权限 JSON observation 表达文件来源及当前内容，按当前凭据策略过滤路径和内容，凭据检查不可用时 fail closed。普通权限拒绝保留引用，不循环弹窗或假报成功；取消遵守原运行取消边界。

独立静态审查首版发现两项缺口：暖候选选择发生在文件恢复之后，可能覆盖本轮结果并重发上一请求的旧正文；权限拒绝分支丢弃了规格要求保留的引用。修复在每次运行入口与采用跨请求暖候选时清除恢复结果，先选择最终冷／暖消息再按当前权限读取与估算；同一运行内的失败／拒绝不反复弹窗。拒绝及不可读分支保留经当前凭据策略过滤、无正文的 compact_file_reference；不得恢复旧正文。回归须实际命中暖候选，覆盖文件更新、删除、外部 Once→Deny，并检查真实 provider wire 与请求数，不以 helper 模拟候选选择。

每文件内容以保守 UTF-8 bytes/4 上取整计 tokens，上限 5000，总内容上限 50000；文本截断或超过预算只发送 compact_file_reference，不发送伪称完整的半份文件。文件读取还设置独立 1 MiB 输入上限，避免恢复阶段沿用普通 Read 的 1 GiB 最大分配；二进制／不支持编码不恢复。所有附件进入生产 projection 和 framed-wire estimator；最终请求仍受当前模型预算约束。开发回归须覆盖真实临时文件的更新／缺失、外部权限拒绝、超限引用、凭据脱敏、读版本不变，以及来源投影不变与实际 provider 观测。技能、动态工具、任务／计划等恢复另续，不以本节冒充完成。

文件恢复首版 `bd2915cd44d4604dfa5191539034cfb72d0dcd88`（tree `8cbcb90cfda589c088c28efa99144050583e1cd0`）的 Actions [40820445411](https://github.com/puzige/vega/actions/runs/40820445411)：Nextest `123271422684` success，原始输出 155 发现／155 执行通过／0 跳过、18.943s；Clippy `123271422404` failure，`vega_tools/src/read.rs:164` 新增测试模块之后仍有 `truncate_line`，触发 `items_after_test_module`；required `123272882215` failure。修正只将原函数移至测试模块前，不改函数行为、测试断言或 CI，不加 lint 豁免。独立审查因用户暂停中断，恢复后继续针对冻结首版；尚无结论，不把测试成功当作整项完成。同时普通合并 master `0df693835a290caf3e655ef30138a734253df428`（#747 工具操作描述），不重写任务历史；新 head 须重新完整云端验证。

lint 修复版本 `d7c18c442c9488b488aa9937e57d6d8907576dec`（tree `902b5ff2a326c36a3678a1c78211b1c94717692f`）的 Actions [40822195895](https://github.com/puzige/vega/actions/runs/40822195895) 全部成功：Nextest `123276983450` 原始输出 155 发现／155 执行通过／0 跳过、19.263s，Clippy `123276983713`、required `123278312602` success。恢复后的独立审查并非 Clear，确认上述暖缓存旧正文与拒绝引用两项问题；本轮最小修复及新增实际暖命中回归仍待新 head 云端验证和增量复核，不以此绿色覆盖审查问题。

文件恢复修复版本 `7cef43739e7ea727ef72d2163a033cd8a30c8e6b`（tree `6e8705c49ada48d6deb0fed17c30ed040fcd4260`）的 Actions [40823415559](https://github.com/puzige/vega/actions/runs/40823415559) 全部成功：Nextest `123280864615` 原始输出 156 发现／156 执行通过／0 跳过、12.967s，Clippy `123280864786`、required `123281747696` success。原审查者对 `bd2915cd44d4604dfa5191539034cfb72d0dcd88` → 本修复的冻结增量独立静态复核 Clear，两项发现闭环；不是整卡验收或合并许可。

#### 已调用技能正文恢复（2026-10-29）

同一运行的已激活 SkillRun 仍由当前权限探针保护，系统消息重建保留其冻结正文；本增量不改其激活、source 审批或工具执行规则。跨运行／重启只恢复历史可读上下文，不将旧 SkillRun 重新绑定或激活为新运行权限。

来源只取共享投影解释器中压缩覆盖且未完整保留的真实 assistant 记录，最近优先，最多 8 个 run；不解析摘要或旧工具输出制造来源。每轮最终冷／暖选择后，使用 Conversation 提供的只读读取器重新查询同一线程的持久化快照，沿用当前 consent／revocation 世代、独立绑定、1 MiB 快照上限、整体 digest 和内部格式验证；失效、缺失、损坏或无权限快照不发送正文。读取器不可用则不假报恢复，取消等实际运行错误仍传播。查询期间不保留 SQLite 连接跨 await，不重放 provider／工具或更改 snapshot／审计／checkpoint。

模型附件使用 authority:none 的 restored_skill／compact_skill_reference JSON，保留 source_run_id、技能名、source label 和内容 hash；正文仅来自已校验冻结 snapshot。按技能名去重，最近优先；当前运行已激活的同名技能不再重复历史正文。每技能 UTF-8 bytes/4 上取整最多 5000 tokens，总正文最多 25000；超限保留无正文引用，不发送不完整正文冒充完整技能。所有字段经过当前凭据过滤，检查不可用 fail closed。附件不改原消息正文或来源比较，不进入历史审批或 read-before-edit；暖缓存不能沿用上一请求正文，每轮重新验证。附件进入实际 production projection 与 wire estimator，预算不足时可移除可选历史附件，不放宽主请求预算。

完整保留判断须同时核对正文行、该 assistant 的完整工具调用及图片来源，不能仅以 MessageRow 相等判断：空正文的不同工具 response 被部分压缩时，行本身仍可能相等。开发回归须覆盖零文本工具边界的来源保留、真正完整保留的排除和即时／重载一致。

首版 `133afcb61f80fe839b9425b6cad721662a4f3ece`（tree `4cecc637fa891d2511e8316385858b8c08563f12`）的 Actions [40826881638](https://github.com/puzige/vega/actions/runs/40826881638) 在测试编译阶段失败：Nextest `123291846907`、Clippy `123291847241`、required `123293391893` failure。新 SQLite fixture 误访问 SkillRunSnapshot 的私有 bytes／sha256 字段（E0616），runtime fixture 向 Result<_, ()> 传入 VegaError（E0308）；修正使用现有 bytes()／sha256() 与 Err(())，不扩大生产 API 或削弱断言。未取得成功 discovery／执行结果，不是 no Rust tests。独立静态审查另确认上述工具边界遗漏，修复需同时覆盖工具／图片来源并补回归，新 head 仍待 CI 和增量复核。

开发回归覆盖实际 SQLite 快照的来源／绑定／digest、当前世代变更、同名去重与正文限额，以及实际 runtime provider 观测、暖缓存旧正文清除、凭据过滤和预算；只由 Actions 执行。技能资源原文读取、动态工具、任务／计划与其他恢复仍单独续接，不以本节声称完成全部恢复矩阵。

技能恢复修复与普通主线整合版本 `d610be934bbfdfd791f3e7e5a10fa59d48acde7c`（tree `af1de81acbe3ce60f8dc098746eb09d2fb709e8e`）的 Actions [40828520701](https://github.com/puzige/vega/actions/runs/40828520701) 全部成功：Nextest `123297056928` 原始输出 160 发现／160 执行通过／0 跳过、22.871s，四项新增技能回归均实际 PASS；Clippy `123297057204`、required `123299046330` success。原审查者对首版至本修复的冻结增量独立静态复核 Clear，部分工具边界遗漏闭环；整卡后续恢复及交付门禁仍未完成。

#### 当前计划历史恢复（2026-10-29）

压缩后保留最新真实完成计划的来源 ID：只取 ContextSource 中同线程 assistant／kind=plan／status=done 的最新一条，且它确实被本次 covered 范围覆盖、未在 Source 中完整保留；有更新计划已保留时不恢复旧计划。即时安装与重启由同一解释器选取，不从摘要文字推断计划或批准。

每轮最终冷／暖历史选择后，以及前台压缩后的预算检查前，专用只读读取器按 ID／线程重新检查真实计划行、有效 plan_status 与 review 元数据。只读取有界正文（最多 UTF-8 20000 bytes，即约 5000 tokens）和最多 4096 bytes 复审备注；超限正文返回 compact_plan_reference，无正文的引用不伪称完整恢复。已删除、来源变化、无效状态或不可用读取器不发送旧正文；每轮清除上次恢复结果，暖缓存不能保留过期 approved／pending 状态。SQLite 连接不跨 await，读取不写入计划、审批或消息。

附件为 authority:none 的 restored_plan／compact_plan_reference，标明 source_message_id、当前计划复审状态及必要备注；它只是历史状态，不是当前执行许可，不产生批准指令、不切换 Plan／Execute 模式、不绕过原 approved_authority 与用户确认流程。所有字段经过当前凭据过滤，检查不可用 fail closed；实际 wire estimator 纳入附件，预算不足可移除，不放宽主请求预算。开发回归覆盖真实计划状态变化／缺失／线程隔离／超限、生成安装重载来源一致、暖命中去旧状态及生产 provider 观测，保留既有审批回归与安全断言。

首版 `636e09ea87f217496716d10e9cde87e15e0f6438`（tree `fd9611dd39da5073ed73a90fb6a451df9c6e4636`）的 Actions [40831022418](https://github.com/puzige/vega/actions/runs/40831022418)：Nextest `123305020016` success，原始输出 164 发现／164 执行通过／0 跳过、13.142s，四项计划回归实际 PASS；Clippy `123305019797` 与 required `123306121707` failure。两个新增 Option<String> 扩大 ChatMessage，导致 RuntimeEvent::SteeringAccepted 变体至少 616 bytes、次大变体 400 bytes，触发 large_enum_variant。修正将计划来源与正文合并至 Option<Box<ContextPlan>>，完整计入驻留内存，保持原附件行为与断言；不豁免 lint 或改 CI。首版独立增量静态审查 Clear，体积修正后仍须新 head 验证及增量复核。

计划存储修正及普通主线整合版本 `137ff4d105e8475c81d3f50e586525dcdc7fd096`（tree `983f21ac76522d0b301e9a542d5a81965da0ac0b`）的 Actions [40832413257](https://github.com/puzige/vega/actions/runs/40832413257) 全部成功：Nextest `123309411018` 原始输出 164 发现／164 执行通过／0 跳过、19.763s；Clippy `123309411302`、required `123311202748` success。存储修正增量独立静态审查 Clear；不是整卡交付。

#### 委派执行状态恢复（2026-10-29）

共享解释器只从本次 covered 中成功的 spawn_agent／followup_agent／resume_agent 持久化调用取得来源，排除 Source 中保留的调用，最近优先、去重、最多 16 个；不解析摘要或旧回执制造 agent ID。即时安装与重载保留相同来源。每轮最终冷／暖选择及压缩后预算检查前，按来源调用 ID 和父线程重新查询对应精确委派执行；不能换成该子 agent 的其他执行。

附件只提供持久化执行身份、来源调用、状态与独立 release_state，不含任务正文、结果、checkpoint 内容或旧权限。它不是活跃 worker 探测或完成证明：running 不表示进程仍活着，completed 不表示已释放；不标记结果已取回，不替代 wait_agent／原执行核实，不自动恢复、取消或重放。查询只读、有界且连接不跨 await；缺失、删除、线程不符或非法状态不发送旧附件。每轮清旧正文，暖缓存不得复用旧状态。每项序列化最多 2048 bytes，合计最多 32768 bytes，所有字段经当前凭据过滤且不可用 fail closed；进入真实 wire 估算，可在预算不足时移除。开发回归须核对真实存储绑定／状态变化／删除、生成安装重载、实际暖命中出站、过滤与预算；本地不执行。

委派状态版本 `2a3213bc43b16c98bee42db99c9175f46d4cd04b`（tree `0d3cbdd3f66f5202b39607bf8ae9f75f16a4888a`）的 Actions [40837168801](https://github.com/puzige/vega/actions/runs/40837168801) 全部成功：Nextest `123324559284` 原始输出 168 发现／168 执行通过／0 跳过、14.889s，四项新增回归实际 PASS；Clippy `123324558909`、required `123326589596` success。冻结增量独立静态审查 Clear；不代表整卡完成。

此增量不代替完整阶段 handoff／结果取回、后台 shell、动态工具、原始详情读取或 idle/cache 的后续评估与实现。

#### 当前线程原始详情回读（2026-10-29）

新增只读原生工具 read_conversation，只在当前运行配置具备绑定读取器时提供。输入 kind 为 messages／tools，source_id 可空（指定消息或调用 ID），after_sequence 为非负游标，limit 为 1–8，content_offset 为非负 Unicode 字符偏移。source_id 与 after_sequence 均约束同一查询，不接收数据库路径或线程 ID。返回来源身份／序号／状态，消息正文或工具 input／output，以及消息附件数量与已编码总字节数；不读取图片正文，不跟随 output_full_path，不读审批／凭据／checkpoint 表，不重跑旧工具或制造 ReadState 凭据。历史内容统一包在 authority:none 的 observation 内，不是当前用户命令或权限。

每次调用在 blocking worker 中打开当前只读数据库、核对当前线程存在并重新查询；SQLite 连接不跨 await，无持久缓存。每字段原文只在不超过 1 MiB 时物化，先用当前凭据完整过滤，再按 content_offset 返回最多 1024 字符；超限字段只返回 unavailable，不对未过滤切片回传。页面最多 8 条且序列化不超过 65536 bytes；包含字段是否完整与下一字符偏移、next_after_sequence 和是否还有后续记录，不能把截断称为完整。缺失／删除／跨线程来源返回空记录，读取失败给内容无关错误，取消前后复查；当前凭据不可用 fail closed。通用工具结果过滤与真实 wire 预算继续有效。

元数据同样必须在 SQL 物化前限长：消息／调用／归属消息 ID 最多 128 bytes，role／kind／status 最多 128 bytes，工具名最多 256 bytes；超限元数据行不进入回读结果。取消回归须同步确认 blocking reader 已进入，运行中取消不能提前完成或脱离工作器，释放后返回 Cancelled 且不交付内容。

开发回归覆盖真实 SQLite 的精确定位／分页／附件元数据／删除与线程隔离、先过滤后切片、超限正文与元数据及返回量、无文件跟随或写入，以及实际 runtime 工具注册、执行、当前凭据失败和取消；只交 Actions 执行。此通道不代替删除／脱敏向既有摘要和候选传播、完整 handoff、技能资源与 idle/cache 等剩余合同。

首版 `3e4716bc22d33e0e11148d81765435bc5ff2832d`（tree `52e356d82c2d1d551b75ca973a9ac19ec8bc0021`）的 Actions [40841209263](https://github.com/puzige/vega/actions/runs/40841209263) 因会话模块直接引用未声明的 rusqlite 编译失败，未完成 Rust 发现／执行；改用已有 Store 接口与 tuple 参数，不新增依赖。独立静态审查发现元数据物化前未限长及运行中取消缺少回归，两项随修正处理；不重跑旧 head 换绿或放宽断言。

回读修正版 `871f1bdf919463c549d1a56811067b50a1a2e89e`（tree `897449ee9722f4672557f9b6d6f88408c8e1ba01`）的 Actions [40842430335](https://github.com/puzige/vega/actions/runs/40842430335) 全部成功：Nextest `123341786482` 原始输出 173 发现／173 执行通过／0 跳过、28.610s，五项回读回归实际 PASS；Clippy `123341786142`、required `123343992961` success。修正增量独立静态审查 Clear。

#### 历史技能资源原文检索（2026-10-29）

read_conversation 增加 kind=skill_resources，沿用 tools 的身份、序号、分页、字段限额、当前凭据过滤和取消合同，仅查询当前线程真实 assistant 所属、tool 精确为 read_skill_resource 且 status=success 的审计记录。既可无 source_id 分页发现，也可按原调用 ID 回读；返回冻结的历史 input/output，不解析其路径、不跟随输出文件、不打开当前技能资源或重建 SkillRun，不授予当前技能激活或执行权限。历史 reference 正文仍为低信任证据，assets 只返回原审计中已有的 metadata，不推断或读取资产正文。删除／编辑立即反映，跨线程／非资源／未成功调用为空；暖缓存不代替每次只读查询。

开发回归覆盖真实 SQLite 中混合工具的过滤、精确身份、分页、更新／删除／跨线程和零文件跟随，并经实际 runtime/provider 回合验证新 kind 的注册、请求解析、分页及凭据过滤。复用现有上限与物理读取取消回归，不将此历史回读冒充当前文件读取或技能重新授权。本地仅静态／格式检查；增量独审暂按用户要求不派子线程，保留合并前未完成门禁。

#### 永久隐私清理的压缩状态失效（2026-10-29）

永久清理在复核快照之前取得数据库 admission pause，要求普通提交与既有运行全部排空；再暂停数据库预计算，确认真实 worker 已退出。任一未排空返回既有待处理错误，不阻塞 UI 或伪称清理完成；持有暂停直至清理结束，阻止旧 lease 再发布。应用侧在同一围栏内显式清除暖 replay 缓存；无缓存调用方仍受来源指纹校验。

原文脱敏与衍生状态退役在同一事务中完成。范围包含当前线程、其直接子线程，以及涉及当前线程的委派执行父／子线程；这些线程的 checkpoint 删除，全部预计算 accounting 标记 retired，Ready／slot 删除。不解析候选旧正文，不删除 stages、token_usage 或已有诊断，不重复计费；同库无关线程的持久化候选不受影响。范围覆盖现有隐私清理会更新的委派 task/result/stage 元数据，不把历史摘要当作恢复权限。事务失败必须回滚原文与退役，快照不符不得清理。开发回归使用真实 Ready/账目，覆盖相关线程失效、无关线程保留、失败原子性与活跃 admission 拒绝；继续复用已验证的 worker pause/drain 机制。此增量不宣称实现了任意历史内容删除传播或全恢复矩阵。

隐私退役版本 `43224cbfe4aade02784fa52210bf57c89c1cde67`（tree `2a5c74b3c65a67f7c48a1c38eb4e9b6ee6f3474c`）的 Actions [40845332829](https://github.com/puzige/vega/actions/runs/40845332829) 全部成功：Nextest `123351306543` 原始输出 175 发现／175 执行通过／0 跳过、17.462s，两项新增回归实际 PASS；Clippy `123351306178`、required `123353553262` success；冻结增量独立静态审查 Clear。

#### 当前动态工具定义与撤销（2026-10-29）

Vega 现有运行在开始时冻结已批准的完整 MCP 定义与精确 dispatch 身份，无 deferred ToolSearch 池；压缩后继续保留这份原生运行能力，不从摘要或旧发现回执重新建授权。同一运行只允许撤销使能力收缩，不静默替换同名工具或添加新定义；重连／重加／定义变化由新运行重新冻结。

每轮预算、技能回填／压缩输入与主模型请求使用当前未撤销的定义子集；压缩或等待期间撤销后，发送前再次移除相关定义。保留冻结 alias→执行身份映射作为拒绝旧调用的边界，已有执行前和等待期间撤销检查不放宽。暖 replay 环境指纹只纳入当前有效候选；撤销改变能力指纹，不能因旧已撤销条目导致指纹构造失败或把旧 schema 留在暖缓存请求。未知 provider 不发送 Claude-only deferred/context-edit 字段。开发回归覆盖当前定义／环境指纹收缩、原 alias 拒绝且零 transport 调用，以及实际 reactive compaction 期间撤销后 provider 出站 schema 消失而原生工具保留；仅交 Actions。此增量不声称实现不存在的后台 shell 或完整 idle/cache 能力。

`57ed8d1ce9e4ed2f2d5750391343d7a3056b498c` 的 Actions [40849945527](https://github.com/puzige/vega/actions/runs/40849945527) 为 177 发现／177 执行通过／0 跳过；独立静态审查发现预检事件 sink 的等待窗口仍可将旧 schema 传入 precompute，因此该版本不是 Clear。补充合同：PrimaryPreflight 回执后重新读取当前定义并重算 estimate、foreground/precompute eligibility；开发回归在该 sink 内撤销 MCP，核对实际 arm 请求的 schema 和估算一致且保留原生 read。

修复 `e09aa4f2775a280b40f4aa66517194c04ce8bc09`（tree `dd59c9fed9c3ac4e3f934b7c3ec38fcf5cb76a14`）独立静态复审 Clear，关闭该 P2。Actions [40852148640](https://github.com/puzige/vega/actions/runs/40852148640)：Clippy `123374016044` success，Nextest `123374015680` 执行 178 项、177 通过／1 失败／0 跳过，14.291s，required `123376552890` failure。失败回归重建 payload 时漏掉 runtime 的 cold ReplayFrame，历史 assistant 来源包装差 31 tokens（74531 vs 74500）；`40564b5a90efd3a994c63d6fd85f887a81cde3ef` 补相同冷投影边界，严格 schema／估算相等断言不变，待新 Actions，未将静态 Clear 当行为通过。

#### 原生 `/compact` 与摘要要求（2026-10-29）

Native composer 支持精确首 token `/compact`（区分大小写，前导空白不解释为命令）；后续空白分隔的文本为摘要要求，去除两端空白，最大 8192 UTF-8 bytes。`/compactness` 等相似普通消息不拦截，ACP 不解释此原生命令。菜单／slash 建议提供 `/compact`，选择只填入命令与现有草稿供编辑，提交（现有键盘／发送入口）才运行。要求只传 `ContextCompactionRequested.summary_requirements`，selection=None；不落普通用户／assistant 消息、不启动普通运行、不带图片／文件引用或 skill intent。

仅空闲、权限有效、来源可压缩且模型／权限／其他提交或受信动作未占用时接受；运行中不可排队或 steering 为普通消息。过长参数、附件／引用／skill intent 和不可用状态显示就近可恢复错误，草稿不丢。成功派发后清空原命令，保留 owner request id 与清空时输入版本；失败／取消只在同 owner 且草稿仍未改动时恢复原命令，不覆盖新输入；成功不加额外成功文案。现有 controller 再次验证 owner/route/admission，现有状态／取消保持。开发回归验证实际 composer 事件（无普通提交）、参数字节边界／精确识别、菜单投影、占用／重复提交拒绝、失败／取消恢复与不覆盖新草稿；复用已验证的底层摘要要求隔离与 controller 生命周期。真实窗口键盘／点击待用户手测，不把 GPUI 状态回归当原生输入验收。

`89a94de7665b2ca8689dc77cd406256d38970dd0` 的 Actions [40854577657](https://github.com/puzige/vega/actions/runs/40854577657) 全部成功，182 发现／182 执行通过／0 跳过、16.858s，包含修复后的 preflight 回归；独立审查仍发现完整命令重新激活建议和 ACP 空建议截获 Enter 两项 P2。补充键盘合同：新增 compact 建议仅在 Native 的未完成首 token（无参数）上激活；完整 `/compact`、选择后的命令和后续参数编辑均退出建议上下文，普通 Enter 提交；ACP 的相同文本不激活该原生建议。开发回归使用输入观察器与 GPUI 测试窗口按键，验证建议选择仅插入、参数编辑后 Enter 恰好发出一次压缩事件、无普通提交，以及 ACP 普通提交保留。此类进程内事件路由不冒充真实原生窗口验收。

#### 脱离前台的生成与逐阶段结算（2026-10-28）

后台准备在当前 admission 下完成有界捕获：来源、append fence、前驱及引用摘要 digest 来自同一快照，拒绝 staged model change、来源不一致与不完整响应组。返回的 prepared job 不包含 admission、前台 actor、diagnostics channel 或任何 SQLite connection；只保存冻结请求、来源快照、摘要偏好、计价及凭据投影策略。前台生成仍全程持有原 admission 和同一 600 秒期限，不改变既有安装语义。

provider 通过仅用于 compaction 的可选工厂显式声明后台能力，默认不支持。工厂在后台执行器内构造独立传输；OpenAI-compatible 实现使用新 HTTP client／连接池，保留冻结 endpoint、协议、model snapshot、重试策略和每次请求前的凭据 guard，不把前台池的 `Send + Sync` 当作跨 runtime 存活证明。OwnerCredentialProvider 必须重建完整保护包装，保留动态凭据读取及例外策略；借用式 child／观测包装不自动获得后台能力。没有此能力只跳过预计算，不影响前台压缩。

生成复用同一 planner、tool-free 摘要和 projection interpreter，但采用显式 Precompute policy：保留完整尾组，超限可退让完整组／缩小摘要批次，不得进入 summarize-all／head omission 兜底；保留前驱已有 omission，不新增 omission，不授予 reactive deeper-pass。600 秒生成期限从实际启动开始计算，不能作为 ready 消费期限。

每个实际 provider stage 返回后（包括失败／取消／未知 usage）先独立结算再进行下一阶段。ledger 只记录本阶段而非累计 usage，完成或失败均封存已结算阶段；无实际请求的取消不制造账单或 unknown stage。发送状态必须独立于 provider 方法调用：OpenAI 在请求校验、凭据 guard 和请求构造之后、首次可能执行 HTTP 的位置置位；取消先于该位置或 guard 拒绝时不置位。同一阶段重试共享只增不减的状态，已经可能发送后发生 guard 失败／取消仍保留 unknown，不根据最终错误反推未发送。Owner 凭据包装在预检完成后转发同一状态并保留出入站过滤；未提供传输证据的 provider 保守地以调用作为可能发送边界，不猜测未收费。prepared job 的执行只产生已结算 ready 或失败，不返回可被前台重复入账的累计 usage，不安装 checkpoint；发布仍受当前 generation、来源／依赖／模型及取消围栏限制，失败按精确 generation 退役，不触碰新候选。

后台诊断使用独立 compaction 有界缓冲，不复制前台 DiagnosticsContext／sender。根操作与最多 32 个 stage 的 Started／终态共最多 66 条；后台记录只保留已验证身份、固定枚举与数字，删除 provider 任意 request ID，不保存模型正文、原始错误、摘要、路径或凭据文本。结算、发布或失败退役完成后，根终态如实记录；在 worker 返回和 runtime 销毁前，由短期独立 Store 事务一次性写入，验证数据库真实身份且不持有 admission。诊断落库失败保留真实 ready／账单结果，不冒充已落库，记录无内容的失败日志；不因遥测失败删除有效候选、重新计费或改变用户结果。阶段结算／凭据读失败也必须留下该 stage 的失败终态，不能只有 Started。所有 Failed 根／stage 使用已有安全失败码映射，未识别错误回落 UnknownSafeFailure；不改变 Cancelled、存储终态形状或错误返回。摘要响应 metadata 由 ProviderStream.metadata 承载，内容流中的 ResponseMetadata 仍是协议错误，诊断测试不得用非法流冒充成功。开发回归验证成功／失败／取消的成对终态、敏感 request ID 不落库、32 阶段与根回执容量、前台 channel 未被复用及后台退出前可读取真实诊断。

开发回归必须证明 admission 在生成前释放、前台 append 不被阻塞且进入恢复后的当前来源、旧内容编辑拒绝 ready、取消／失败保留实际账单、不重复计费、unknown 保持 unknown，以及 typed PTL 不能让预计算新增 omission。provider 工厂回归静态构造／合成流验证配置与保护包装，不运行真实网络 E2E。此阶段不把 prepared job API 视为 runtime 自动触发、退出 drain 或最终消费安装已经完成。

持久化基线 `e0c3471b14fed4d693fe4b6d567d50d6e6fd1a57` 的 Actions [40718995497](https://github.com/puzige/vega/actions/runs/40718995497) 已完成：Clippy `122924425728`、Nextest `122924426530`、required `122927304871` 成功，原始汇总 90 发现、90 执行通过、0 跳过。独立增量审查无发现；该结果不覆盖本节后续实现。

后台生成提交 `e016242898cc564841b37ab18a7eab95e1b747dc` 的 Actions [40730684803](https://github.com/puzige/vega/actions/runs/40730684803) 中 Clippy `122965843440` 成功；Nextest `122965843732` 原始汇总为 99 执行、98 通过、1 失败、0 跳过，required `122968095963` 失败。失败 fixture 错把已知凭据当作可放行的格式误报，修正为分别验证已知值始终脱敏、合成格式误报例外保留、动态 reader 失败关闭，不修改生产凭据政策。独立审查另确认发送前取消／guard 拒绝可能制造 unknown stage；按上述发送边界修复，并补发送前无阶段、已发送后重试拒绝仍记 unknown 的云端回归。修复提交 `b82868543a0ede081708644289b61098c8e43053` 的 Actions [40737563571](https://github.com/puzige/vega/actions/runs/40737563571) 已成功：Clippy `122990117517`、Nextest `122990118230`、required `122992900401` 成功，原始 Nextest 启动 101 项、执行 101 项、通过 101 项、0 跳过；独立发现计数行未单独留存。非实现者增量审查确认原 P2 修复且无新增发现；静态审查不替代云端行为验证。此证据只覆盖该提交，不覆盖后续消费接线。

后台诊断增量 `dc4a499a1f4ed81d300b71d000ba166599e4074e`（tree `8ad146c7a6ff7c03420199f1b66d3dbea9bb5662`）的 Actions [40782359373](https://github.com/puzige/vega/actions/runs/40782359373) 中 Nextest `123146945943` 实际 136 执行／135 通过／1 失败／0 跳过，7.817s，独立发现数未取得。唯一失败为新诊断 fixture 将 ResponseMetadata 作为内容事件，触发既有 SummaryFormatInvalid；此前三个 runtime 预算 fixture 回归通过。Clippy `123146946411` 因诊断缓冲的 collapsible_match 失败，required `123148412695` 失败。独立静态审查另发现凭据 reader 失败的 Failed + None 违反存储校验，整批诊断回滚；本批以已有安全 fallback 修根／stage，保留 reader、账单失败与遥测失败案例的全部终态断言。修复待新提交 Actions，不能把本次 135 项通过称为整轮通过。

#### Ready 消费与原子安装（2026-10-28）

消费先取得当前数据库／线程的安装 admission；ready 不携带授权，也不复用生成时的 deadline。读取候选、当前完整来源、当前前驱和新的消费快照围栏必须位于同一读事务。旧候选围栏允许合法追加，但消费快照还须覆盖追加部分的原始正文、历史交付信息及 assistant 截图等 artifact；旧候选围栏与 legacy 完整指纹不能代替此校验。准备完成后仍保留旧候选原值，不能让调用方滑动 covered／omitted／Source 范围或替换摘要。

最终写事务依次确认当前 slot／generation／ready 原值、原候选来源与摘要依赖、消费时完整快照围栏、完整 source version／fingerprint、当前模型／前驱／取消，再通过普通 checkpoint 安装的同一事务内核心写入；成功安装和精确 generation 退役必须同事务提交。任何失败／取消／过期／来源变化都不留下半安装、不退役当前或较新候选。消费重试在候选已移除后是无候选，不重新安装或记账；普通 checkpoint API 的 AlreadyCommitted 行为保持不变。此存储边界本身不授予安装权或证明模型适用性。

conversation 使用当前系统、工具、模型及 replay frame 重建完整请求，保留合法 append 后缀；只经共享解释器及 durable equality 校验叠加仍保留的 live reasoning／bindings。兼容旧 hook 的入口没有实际 replay frame 时，不消费含当前运行 owner 的候选，仍走原前台路径；完整候选请求在安装前重新读取当前凭据并执行既有拒绝政策，不改写摘要或绕过已知凭据保护。摘要偏好必须相容，partial 或额外自定义要求不借用不匹配候选；当前 owner／task／continuation 来自消费入口，不能从旧摘要恢复权限。候选原模型不同并非硬拒绝，按当前模型的完整 framed wire 估算，结果须小于原输入且严格低于当前 auto 阈值，不得从 precompute 授予 reactive deeper-pass。已结算 usage 以独立 provenance 表达：前台不重复持久化，已知计价不因返回空 usages 被误报 unknown，确实缺失的 usage／计价继续保持未知。

开发回归覆盖：真实 SQLite 中合法追加后安装与 reload 一致且只安装一次；候选身份、前驱、当前模型、源编辑、消费后追加或追加截图／raw historical 正文变化均拒绝；取消发生在插入后时 checkpoint 与退役一起回滚；旧消费不能移除新 generation，usage 行及原始 transcript 不变。conversation 另覆盖当前模型 fit／不 fit、偏好不匹配、保留 live replay、完整请求预算和已结算计量。此阶段不把安全消费 API 视为自动 arming、registry／Stop／退出 drain 或整卡已完成。

消费超时的失败恢复仍需来源授权：安装 guard 在事务校验前超时，也必须在返回可恢复的 SummaryTimedOut 前，于新的只读事务复核原候选／generation／依赖、完整消费快照围栏、完整 source version／fingerprint、当前模型及 pending model 状态。复用安装校验，不以 legacy fingerprint 代替追加 raw 正文和截图围栏；读取失败或任一来源变化均返回不可恢复的 SourceChanged，禁止 primary 发送旧请求。显式取消保持取消。回归覆盖未变来源可恢复超时，以及来源／追加隐藏正文／截图／模型／前驱变化与安装前超时同时发生时关闭恢复，checkpoint／候选／账单无额外变化。

首版消费提交 `33fa22034e164df474cb396d40f33ad526c21c84` 的 Actions [40748830780](https://github.com/puzige/vega/actions/runs/40748830780) 中 Clippy `123029714963` 成功，Nextest `123029714446` 与 required `123032384255` 失败：112 项执行、111 通过、1 失败、0 跳过，独立发现计数行未取得。失败是偏好回退 fixture 只有一个用户轮次，却以默认 prefix 策略期待前台生成；补齐可压缩的旧轮次并覆盖 prefix／full 两种策略，不改变生产策略或原断言。非实现者审查另发现消费超时未复核失败来源的 P2；按上段修复，修复后的云端和增量审查证据另记。

超时围栏修复提交 `9e49dca0ee882082b3449238c9dcb9b83053d06e` 的 Actions [40753788063](https://github.com/puzige/vega/actions/runs/40753788063) 在开发测试编译阶段失败：两个 generation 查询断言误传临时 Transaction；未执行 Rust 回归，Clippy `123047142538`、Nextest `123047144562`、required `123049597329` 失败。`d4e8c74095116327e175235a1307766d7611ac09` 仅改为 API 所需的 Connection，独立增量审查无发现；其 Actions [40756538550](https://github.com/puzige/vega/actions/runs/40756538550) 的 Clippy `123056850951` 成功，Nextest `123056850626` 执行 113 项、112 通过、1 失败、0 跳过，required `123059526769` 失败，独立发现计数行未取得。偏好回退回归已通过；唯一超时竞争回归在变更注入前退出，fixture 将 read 改名为 ComputerScreenshot 却保留无效 `{}` 参数，不满足生产审计记录合同。改用既有桌面参数规范化函数构造合法审计及交付回执，保持七种变更、primary 请求数、完整快照与账单断言，未改变生产校验或超时恢复政策。修正后的云端结果另记；此前静态 Clear 不等于行为验证成功。

截图夹具提交 `d73b46cdafda2f254093717f7181e5a96afcb7a6` 的 Actions [40758317009](https://github.com/puzige/vega/actions/runs/40758317009) 在测试编译时发现未导出的 `vega_store::params!`：Clippy `123063099400`、Nextest `123063098976`、required `123065344322` 失败，未执行 Rust 回归、发现计数未取得。单行修复 `b7d36375cb197761f9a13be51d1f1bf80f0814e0`（tree `d88a87795707e07aa7fced184aaa35d18c43c247`）使用同内容／顺序的 `[&str; 2]` 参数绑定，未改断言或生产实现；非实现者增量审查 Clear。其 Actions [40761984680](https://github.com/puzige/vega/actions/runs/40761984680) 的 Clippy `123076036023`、Nextest `123076036469`、required `123078474093` 均成功；原始日志明确 113 项发现、113 执行／通过、0 跳过，22 binaries，12.339s。偏好回退与七场景超时围栏回归均实际通过。该结果不覆盖后续 registry 改动，也不是最终删测交付。

#### 压缩专用在途注册、失效与退出回执（2026-10-28）

注册以数据库真实身份（不是路径字符串）＋线程区分主／子 agent；每键最多一个在途 worker。前台在当前 admission 下取得不含 admission 的运行 lease，lease 固定 epoch，Stop／模型阶段变化／删除／权限重置／关闭使旧 epoch 失效；原运行不能通过旧 lease 再启动。后台不持有前台 actor、runtime、admission 或跨 await 的 SQLite connection。注册锁不跨 await、生成、结算或线程等待持有；互斥失败按不可用拒绝启动，不绕过围栏。

取消与完成分离：取消请求只撤销旧 lease 并取消 pending 的 token，不立即删除在途记录或允许同键第二个 worker。完成回执须在生成、逐阶段结算／封存、失败精确退役、后台 runtime 销毁并 join 工作线程之后产生。等待者取消或丢弃 wait future 不取消 worker；显式取消由注册入口执行。drain 只等待所捕获的真实任务，不制造成功；期限内仍未退出则保持阻塞并报告超时，不通过 abort 或丢弃 handle 伪装关闭。数据库 pause 在持有期间拒绝新 lease／启动，支持重叠 pause，释放最后一个 pause 也不恢复旧 epoch；registry close 不可重开。ready 仍由持久化／消费校验，不因单纯模型切换删除已完成候选。

precompute 的连续失败计数与前台 automatic／rapid-refill 独立；同 epoch 成功归零，三次实际失败后拒绝 re-arm。显式取消、来源失效及完整响应组不足不计 counted failure；旧 epoch 的迟到完成只释放自身槽，不改变新 epoch 的计数或任务。开发回归使用真实 SQLite、真实生成／结算 worker 与可阻塞的合成 provider，验证单在途、不同数据库隔离、旧 lease 失效、等待取消独立、drain 必须等 provider 资源释放、ready／账单终态、暂停／关闭以及失败 gate。该注册原语不单独宣称 runtime 自动触发、Stop／模型／权限重置／删除／窗口和 app quit 接线已完成；接入须继续覆盖这些具体入口。

注册原语提交 `318f71cc0e169338e3f0e47c1fe32291aa498310`（tree `e9ca2a9b14bbc1e7bbfac85bb42bad996e54c6cf`）的 Actions [40763908020](https://github.com/puzige/vega/actions/runs/40763908020) 未执行构建或测试：Clippy `123082813541`、Nextest `123082814274` 均 cancelled 且 steps 为空，annotation 明确 hosted macOS arm64 runner 容量不足；required `123090610049` 失败，Rust 发现／执行计数未取得，不是 no Rust tests。独立审查指出缺少旧 epoch 的实际 counted failure 迟到回归，生产 epoch 比较未发现确定缺陷。本批补齐真实失败已结算但 runtime 仍未退出时失效的场景，并保留新 epoch 三次失败／第四次拒绝和旧结果重观察不重复计费的断言。

#### 前台安装后的候选协调（2026-10-29）

前台自行生成（含 full／manual／partial／reactive 与不适用 Ready 的 fallback）成功安装新 checkpoint 时，在同一写事务读取当时 slot 的 generation 并精确退役，保留所有已报告账单与诊断；不在事务外按线程盲删。失败、取消、来源失效或 AlreadyCommitted 重观察均不得删除此后候选。事务提交后只请求取消该 generation 的在途 worker，不撤销当前运行的 lease／owner、不清 pending 槽或伪造 drain；同键仍须等真实退出才能启动另一 worker。消费 Ready 保留现有精确消费与幂等安装合同。

后台 begin 在同一事务核对准备时的 checkpoint 前驱与 slot，阻止“前台安装时尚未写入 slot”的迟到准备在新投影上启动。新投影的后续合法运行／steering 可使用原本仍有效的 lease 重新捕获；同一用户运行的 automatic／reactive arm suppression 保留，不把清理当作绕过 Stop 的重新授权。必要回归覆盖安装失败／取消回滚保留候选、成功原子退役、旧准备／晚发布拒绝、旧清理与 AlreadyCommitted 不误伤新候选、后台资源未释放仍占用、账单不丢失，以及更新后的投影可以重新准备。此合同不扩展通用生命周期或 fork 借用。

本批新增真实 SQLite 回归覆盖 pending／Ready 两态、retire SQL 失败与退役后取消的事务回滚、AlreadyCommitted 重观察与迟到 begin；conversation 使用真实注册／worker／runtime，在前台安装后确认旧 worker 自行结算但物理资源未释放仍不得 re-arm，释放后原有效 lease 可重新准备，旧 generation 的取消不影响新 worker。既有偏好不匹配 fallback 用例原先保留旧 slot 的断言按本节新合同改为精确退役并拒绝重发布，原有请求／用量／checkpoint 断言保留。

提交 `bd0facb38822128dfc964b4efeca2338f89b2fbf`（tree `4929eb9c868bd311619b38cdf12acade2ef920c1`）的 Actions [40796181508](https://github.com/puzige/vega/actions/runs/40796181508) 中 Clippy `123193093645` 成功，Nextest `123193093249` 与 required `123194398482` 失败；原始输出 145 执行／143 通过／2 失败／0 跳过、23 binaries、15.703s，独立 discovery 计数未取得。新增协调回归均通过，独立静态审查 Clear。两处旧消费／超时回归已先确认拒绝旧来源、禁止 primary 发送旧请求及账单保留，随后仍按旧合同读取已由竞争前台安装退役的 slot／Ready，分别报 QueryReturnedNoRows 与 Ready 数量 0≠1。修正仅针对该场景，验证精确退役、候选删除和原账单／stage 保留；其他来源变更继续保留原候选。另保留候选仍在而前驱已变化的独立 fixture，使 generation 消失不能掩盖原前驱围栏回归。此修正不放宽消费／超时来源校验，不改生产实现；本地仅格式与差异静态检查，修正后的完整行为待 Actions。

协调修正提交 `ba4122d72825309b3993aa010c07baf9c60ba1f2`（tree `ba264f23da59d31a7410c4474b1a836685f14a63`）的 Actions [40798827553](https://github.com/puzige/vega/actions/runs/40798827553) 全部成功：Nextest `123201733190` 原始输出 145 发现／145 执行通过／0 跳过、19.196s；Clippy `123201733574` 与 required `123203206478` success，增量独立审查 Clear。随后同步 master #744／#745 时，保留已正式发布的 `0047_thread_reasoning.sql` 和 `0048_native_tool_descriptions.sql`；本卡未交付的 projection／micro／precompute 三项迁移依次顺延至 0049／0050／0051，不改正式迁移或用户数据库。开发夹具适配新增工具描述字段，不放宽原有业务断言。合并后仍须重新云端验证。

#### 生命周期与自动入口接线（2026-10-28）

自动生成启用前先接破坏性边界。显式 Stop 在前台或准备阶段是否存在均撤销该线程的旧 lease；仅导航、等待取消或交接不等同 Stop。模型 set／stage／promote 在写入期间暂停该键并撤销 pending，已完成 ready 不删除；新的运行在窗口启动边界、当前 admission 下绑定一次 lease，沿四种运行配置传至 conversation；不能推迟到工作线程或 hook 首次调用时重新绑定，使 Stop 之后的迟到准备取得新 epoch。权限重置 session 同时持有数据库 admission pause 与 precompute pause，完成／恢复原语在持有 precompute pause 且真实 drain 完成后才能修改配置／权限；超时保持 Busy，不假报完成。删除同时暂停父／子线程并检查真实 drain，未退出时返回明确的“正在停止，请稍后重试”，不在 UI 同步等待、不先删除 transcript 或账单。

窗口拥有可关闭的 compaction owner，只跟踪其 lease／worker，不携带 admission 或内容；关闭撤销本 owner 的旧 lease 和 worker，不取消其他窗口后来启动的任务。app quit 关闭进程内 registry 并等待实际 drain。当前 GPUI 0.3.4 的 `on_app_quit` Future 仅等待 200ms 且不可否决退出，因此压缩专用 callback 在返回 Future 前同步等待独立 runtime 的真实退出；不将异步 hook 的超时当作完成，也不修改通用退出框架。该终止阶段可能延迟退出，不依赖 UI／前台 actor 推进结算。窗口关闭本身不宣称 worker 已退出，全局注册仍保留它直到结算／退出。错误保持失败关闭并记录未能确认退出，不通过新建 registry 绕过暂停或关闭。

runtime 在完整持久化响应／工具结果边界、micro 之后用完整 framed wire 的当前可信 usage anchor 或保守估算判断 precompute 阈值，首次单用户／无完整组、已尝试 reactive 或紧随压缩成功的迭代不启动。不在 delta、未完成工具响应或 final-turn 子 agent 处 arm。conversation 启动前重新验证来源、模型／pending、当前 lease，已有有效 ready 或 pending 时不替换。前台 threshold／manual／reactive 可等同键 pending，取消等待不取消后台，后台失败仍按当前来源走普通前台 fallback；真正安装一律复用完整消费校验，不因等待成功直接接受摘要。该入口使用独立 precompute breaker，不改变前台 automatic／rapid-refill 计数。

自动入口细化：preflight 在 micro 后、未进入 foreground auto 的区间尝试；最终正常完整响应在持久化 ack 后也可尝试，usage anchor 必须匹配完整 wire，否则重新估算。一次当前用户运行已尝试 auto／reactive（含显式与模型技能预检）时抑制后续 arm，新 steering 用户拥有者才重置。最终持久化已确认后，预计算的可选估算／准备失败只放弃后台启动，不反转主回答成功终态。捕获后核对当前 owner 的完整 durable suffix（含无工具的最终文本）与其前缀历史，随后冻结精确 source fence，不把纯文本响应当作遗漏。已有同键 pending、未耗尽计数前的 ready 与当前偏好匹配时均不重复生成；保留 ready 的检查与 expected generation 同快照，迟到准备仍由 lease／slot CAS 拒绝。普通消费最多等待同键 pending 的真实退出，取消等待只返回 Cancelled；后台失败后重新捕获当前来源再走前台生成，不以等待结果代替安装验证。注册观察失败仍返回错误，不冒充 worker 已退出；完整 owner suffix 必须匹配，不能因旧工具组匹配而接受缺失最终文本。期限届满返回不可恢复的当前操作错误，不将尚未验证的 source 当成可继续 primary 的超时。此处不把普通消费实现冒充 fork 借用或子 agent 能力恢复。

本批开发回归覆盖窗口 owner 关闭／重观察不影响其他窗口、阻塞 runtime 超过 200ms 时 shutdown drain 不提前完成、三种模型变更保留 ready、删除保留 transcript 直到退出、direct reset 与 session reset 的 Busy／真实退出后完成，以及 GPUI 中后台单独运行／标题等待／普通准备／活跃运行／trusted preparation 五条 Stop 路径与旧 stream／导航不误撤销。生命周期提交 `34fbd8b03b18d2dff811e026dd467ef4fe468a61`（tree `1025efc35319d235d891455009d0d2c74241abc9`）独立静态审查 Clear，但 Actions [40771839352](https://github.com/puzige/vega/actions/runs/40771839352) 的 Nextest `123110770734` 与 Clippy `123110771106` 均在开发测试 `PermissionResetError` 的 `?` 转换处 E0277，required `123120054298` 失败；Rust 未执行，发现数未取得。测试侧五处显式错误转换提交 `bde76953e3b61c6705d40608699059075f2346ec`（tree `e2c35ccc5d8f979785178a29fb8b9d5d2525071b`）的 Actions [40775984564](https://github.com/puzige/vega/actions/runs/40775984564) 中 Clippy `123125280857` 通过，Nextest `123125280665` 实际 125 执行／124 通过／1 失败／0 跳过、22 binaries、15.362s；独立 discovery 数未取得，required `123127924512` 失败。唯一失败是模型变更 fixture 的无效 `read_only` 值，conversation 读回报 `CorruptRow("permission_mode: read_only")`；本批将共享 fixture 改为 `PermissionMode::ReadOnly.as_str()` 的正式编码，不改生产解析或断言。

本批启用自动 arm／pending 消费并补回归：完整最终响应 ack、可信 usage 与保守 framed-wire 阈值、micro 后重新估算、初始单用户不启动、工具结果完整后启动、普通／reactive／两种技能预检尝试后抑制、steering 重置，以及真实 SQLite／后台 worker 的 pending／ready 去重、等待取消独立、失败后前台 fallback、来源变化拒绝及完整 owner suffix。提交 `678bffda48fc04996a114b74fd6db4253228e26d`（tree `de7f845f082844773e5848b4771b3f0ac1a0f4ad`）增量独立静态审查 Clear。Actions [40779807646](https://github.com/puzige/vega/actions/runs/40779807646) 的 Clippy `123138298945` 通过；Nextest `123138298498` 实际 134 执行／131 通过／3 失败／0 跳过、23 binaries、21.523s，独立 discovery 数未取得，required `123140897112` 失败。三项新 runtime fixture 忽略完整 wire 的 4/3 安全系数、schema 与 framing，导致 micro 后实际触发 foreground auto、技能路径提前普通压缩，以及预期可恢复超时实际已越硬上限；生产 estimator 与恢复判断未改。本批改用公开 framed-wire estimator 配置并断言预算区间，技能案例下调合成历史并断言实际 SkillProspect 超限、普通 preflight 低于提前阈值，保留原有时序／数量／抑制断言。修复与新后台诊断仍待云端验证；fork 借用、子 agent 等后续仍未交付，不以本批宣称整卡完成。

#### 子 agent 原生压缩接线（2026-10-28）

子 agent 继承父运行的 ContextBudget 自动策略，不再无条件关闭；这取代旧委派阶段的临时禁用，不改变冻结权限、工具集、StageHook、credential reader、turn limit 或父子执行占用。runtime 用独立的 delegated-compaction 标志识别子运行，不能把“父运行具有委派工具”当作子 agent 身份。子 agent 的估算 preflight 与最终响应均不 arm；仅正常非最终响应的完整工具组持久化 ack 后、仍有下一回合、未接收新 steering／未试 foreground compact、可信 usage／完整 wire 达提前阈值时尝试。前台 threshold 继续走该子线程的 full 策略，reactive 与技能路径保留原边界。

ChildProvider 显式重建 owned compaction provider，只有内层工厂可用才提供能力；后台保留原 child 凭据拒绝和内层保护，metadata／submission／once／recovering 路径都按原协议转发，不借用父 runtime／transport、不跳过发送前检查、不制造未发送账单。后台 usage 使用独立 Store 的同一短事务写 stage、token_usage、delegated parent 归属与已冻结 automation claim，按 usage ID 幂等；未知用量／落库失败传递现有 observer 的 missing_usage，不计入虚构用量、不在消费时重记。observer 只保存计量状态，不复制前台 actor 或诊断 sender。

每个子执行在验证 admission 后、调用 entry 前绑定固定 compaction owner／lease；取消 token 只作为该子执行显式取消边界，直接取消其后台 worker，绝不从普通等待者取消继承。正常、失败、取消和阶段 handoff 返回后均先撤销自己的 owner，暂停该子线程、等待真实 runtime／结算／诊断退出，再在同一 admission 下精确退役当前 generation；账单和诊断保留。完成上述步骤后才持久化／释放子执行终态；失败关闭并保持 quarantine，不把“请求了取消”当作已释放。丢弃 lifecycle future 也撤销自己的 worker，原执行占用继续按已有 quarantine 规则保留。其他线程／父运行／新执行不被旧 owner 清理。

开发回归验证子 agent estimate／final／turn-limit／继承 auto-off 不 arm、完整非最终工具组后 arm、steering／foreground suppression 及持久化 ack 失败；真实 SQLite＋后台 worker 验证直接取消子执行即完成计量封存与诊断、资源未释放仍不得报告退出，退出 drain 只清本子线程候选、账单／父预算归属幂等、unknown 保持未知。owned provider 回归覆盖借用 wrapper 销毁后仍可构造独立 provider、动态凭据拒绝与内层拒绝发生在发送标记之前、metadata 转发、内层输出过滤保留，以及不支持独立 transport 时不声明能力；外部 provider 合成，其余使用生产路径。本地仅格式和差异检查，行为结果待本增量 Actions；此增量不冒充 fork 借用、恢复附件或整卡交付。

后台诊断修复提交 `c0e6dfad1332fd1ee1a5f8496cd8847a8e21735b`（tree `cc7279c576df4b513d90e55f0fde883a1a79cea4`）的 Actions [40784163692](https://github.com/puzige/vega/actions/runs/40784163692) 已成功：Clippy `123153076122`、Nextest `123153075674`、required `123154894342` 均 success；原始输出 136 发现／136 执行通过／0 跳过、15.851s，含六种后台诊断终态与容量回归。该结果只覆盖修复提交，子 agent 新增量待验证。

子 agent 初版 `4afb1b76e1d1b72b379b23c1be83d19d4b3c9cb3`（tree `0a75ed1c81ca6e8874fb3689df372458e1c2b552`）普通推送后的 Actions [40792164092](https://github.com/puzige/vega/actions/runs/40792164092) 失败：Clippy `123179761648`、Nextest `123179761872`、required `123181283080` 均 failure；format 与 Python release-script 检查通过。Rust 编译阶段发现新增 child cleanup 直接引用 conversation 未依赖的 `rusqlite`、测试同样引用该 crate、计量 observer 未实现 trait 必需方法及未使用导入，未取得成功 discovery 或测试执行，不能记成 no Rust tests。修复使用现有 `ready::current_generation`、既有 `VegaError::Store` 映射，不增加依赖；observer 对不应发生的前台 dispatch/completion 回调显式失败，原有业务断言保留。修复提交 `f1dd0feb73a34571d3c5a96f635e6c0f49a0c364`（tree `3cdf2bb388c5ce741641ef9ad81a56500f4e00d5`）的 Actions [40793229259](https://github.com/puzige/vega/actions/runs/40793229259) 成功：Nextest `123183300541` 原始输出 142 发现／142 执行通过／0 跳过、6.618s，Clippy `123183300793`、required `123184220402` 均 success。子 agent 增量独立审查只发现上述未使用导入，修复增量复核 Clear；此证据不覆盖后续候选协调或整卡其他剩余机制。

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
