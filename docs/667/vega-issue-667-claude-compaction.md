# #667 — Claude Code 上下文压缩全路径对齐

## 状态、目标与来源边界

- Issue：[#667](https://github.com/puzige/vega/issues/667)。阶段：参考契约核实 / spec；尚未实现、验证或合并。
- Vega 基线：`fb1e1dc96946e35970d6634fd2ab0e8cd01d6e70`；任务分支 `feat/667-claude-compaction`。初查基线 `aa6bb8f74c9cf96fb413ea2b6a079f987115ae44` → 当前基线只新增 SSE timeout 分类与 shell 按钮选中修复，不改变已核实压缩合同。
- 参考：本机 Claude Code **2.1.292** 的静态可验证客户端实现。版本指纹、调用链及默认/实验/平台差异见 [参考矩阵](claude-2.1.292-reference.md) 与 [脱敏证据索引](claude-2.1.292-evidence.json)；未完成核实的项目不得标成已对齐。
- 用户要求是重写完整压缩机制，不是只改续接提示词，也不是沿用旧规格排除 micro、partial、reactive、precompute、hooks 后宣称全量完成。
- 对齐行为契约，以 Vega 原生实现适配 SQLite、可取消运行、多 provider 与 GPUI；不复制整段专有实现，不假装复现不可观察的服务端算法或私有实验分配。
- 本规格先列验收，再定义实现。主要客户端路径已由 62 项有界证据固定，参考矩阵已完成静态整理；涉及 provider/插件平台的取舍尚未冻结，不能靠空接口或未经确认的替代语义宣布对齐。

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
| A15 | PreCompact、PostCompact、SessionStart(compact) 与 provider/插件能力组合；hook 失败/取消/修改源 | 明确阶段、顺序、输入/输出权威与安装时机；不可用 hook 有可观察状态；hook 文本不自带权限；本地原生契约覆盖，不以空 trait 冒充完成 | CLOUD + MANUAL |
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
| hooks/provider | 现有 `ContextCompactionHook` 是内部 conversation 服务，不等于用户扩展 hooks；ModelCompat 无 context edit 能力 | 参考扩展生命周期的真实实现和 capability gates；不得把 Anthropic-only 字段发给所有兼容 provider |
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

### 4.5 已核实的具体参考合同

- **预算**：令 `R=min(maxOutputTokens,20000)`、`W=policyWindow-R`、`H=hardContextWindow-R`。通常 compact=`W-13000`，warn=`compact-20000`，block=`H-3000`；auto 关闭时 warning 基准为 W。合法 pct override 取其阈值与 `W-13000` 的较小值；block override 独立。precompute=`min(W-round(W*f),compact)`，f=0.2 只是 scalar fallback，可被窗口/REPL/SDK 表覆盖。例：policy=200000、hard=1000000、maxOutput=32000，则 R=20000、W=180000、H=980000、compact=167000、warn=147000、block=977000、fallback arm=144000。该例仅验算公式，不代表任一模型实际配置。小窗口在 Vega 必须有边界检查，不以整数下溢模拟参考 JS 负数。
- **partial 选点**：`up_to` 摘要 `[0, selected)`，保留 selected 起的后缀，安装 summary→kept；`from` 摘要 selected 起、保留之前的前缀，安装 kept→summary。后者摘要请求可见前缀作为上下文，但不能把保留区重复标成 covered。选点不是工具组内部任意字节。
- **分组/反应式**：以 assistant response 身份区分回合（不是只看可见文本偏移），不足两组失败；初始保留一组，token gap 可增加保留组数以缩小摘要输入。last-resort 可改为 summarize-all/完整组头截断；最多三次头截断不等于最多三次总 API 请求。precompute 禁此 last-resort；reactive 的一次受限 deeper-pass 与单次 fork `maxTurns=1` 分开。
- **后台所有权**：pending 结果只可由同 controller/generation 发布；正常消费移除候选，fork 借用不移除；只取消前台等待时后台可继续，必须由明确的生命周期取消控制，不能把导航/等待取消与用户 stop 一概合并。模型切换取消 pending，ready 的适用性延后复核；Vega 需要在当前权限下重新取得安装权。
- **后台恢复**：参考 sidecar schema=3、上限 8000000 bytes、最长 7 天，包含 session、边界及保留 UUID。Vega 用自己的版本化 SQLite 候选实现同等生命周期，不要求照搬文件名或 JSON 作为数据库。模型/CLI 版本匹配在参考 rehydrate 处仅遥测。
- **空闲会话压缩**：独立 off/log/compact 状态，参考 fallback off；1h cache 活动到过期时间的 90% 处调度，fraction 限 0.5–0.95；默认至少 200000 token、配置下限 100000、迟到超过 60s 不执行。还要验证 prefix、活动、owner、额度与缓存仍 warm。没有可信 cache expiry 不得凭空调度；这不是 3900s idle micro。
- **摘要变体**：control/lean/short/capped 的选择先 env 后 feature，非法值回 control；reactive 使用该选择器。capped 的 2000 words 是提示词而非确定性截断。Vega 不展示或持久化分析草稿，保留任务所需摘要；该安全适配与参考 control 的输出形状差别明确记录。

## 5. 分阶段实现与验证顺序

所有阶段属于同一卡完整范围，任何单阶段完成都不能宣称全量对齐。

1. **参考合同**：完成版本绑定矩阵、默认/gate/外部能力分类；补齐本规格的数值与方向语义，先消除会改变数据模型的未知。固定可恢复文档版本。
2. **核心投影/预算/续接**：版本化 plan 与共用 reload、策略预算、任务来源、full/manual/partial、安全安装和摘要变体；用开发回归验证任务元污染、完整工具组、旧 checkpoint、取消/CAS。
3. **轻量与恢复**：micro 各路径、只读历史回读、reactive/last-resort、状态恢复、独立熔断；复核 wire 与 usage。
4. **后台与扩展**：precompute 的准备/生成/消费/失效/取消、hook 与 provider capability 适配；覆盖 append-vs-edit race、model/owner 变化、重启和停机。
5. **产品入口与交付**：只接压缩所需 `/compact`/partial/状态；独立审查绑定停写版本；现有 Actions 验证开发回归；固定源码/完整 SHA/原始结果后删除开发 Rust 测试及独占接线；最终 head required check 与 Rust 空集清点；然后合并，保持 Issue OPEN / In review 等用户手测。

一次卡默认不超过三次提交：规格基线；实现与必要开发测试；留痕后的最终测试清理。实际验证失败必须修责任层，不因提交数量而放宽断言或忽略失败；若确需超出提交策略，先记录原因并按项目规则处理，不继承其他卡的特殊许可。

## 6. 当前证据、未知与下一步

已完成静态核实：当前预算、partial-assistant 覆盖、summary 请求/包装、实时安装/reload、工具预览、manual owner、SQLite CAS、模型能力字段。参考取证共 62 项有界片段，原研究执行在报告整理阶段失败，已确认释放写入权并保存产物；后续只整理既有证据，不重复全量调查。未进行模型调用、本地 Vega 运行、编译或测试。

已消除的重要误推：手动 `/compact` 不等于 ordinary full；自动/错误恢复可发生在用户任务内部；reactive 也保留尾部；micro 的 hint/idle/server 不共享全部门槛；precompute 支持校验恢复且前台等待取消不必取消后台；model switch 的 pending/ready 不同；rapid-refill 的计数来自 query/API 回合推进而非人类消息。

真实实现前提：Vega 的 `ModelApi` 和 `ProviderApi` 目前只有 ChatCompletions / Responses，没有 Anthropic Messages、首方 beta/context-hint/server edits/缓存寿命来源；也没有可承载 `session.compact` 消息替换的外部插件平台。移植客户端选择/生成/恢复逻辑可以在现有路由实现，但不能证明 Anthropic 私有服务或远端实验分配同效。完整“功能行为对齐”与“原协议/插件兼容”必须明确区分，不能只加 capability=false 的空接口就声称后者完成。

下一步需明确的产品边界：是以 Vega 原生能力实现所有客户端压缩路径、为专属服务保留真实 capability 和等价降级，还是同时扩建 Anthropic Messages provider 与 Claude 外部插件兼容平台。两者都不等于只修续接；后者也无法复制私有服务端算法或证明未公开 rollout。选择前不自行收缩“全量”的验收定义。

当前交付仅为可恢复规格/证据基线；本地静态检查包括 Markdown 链接、21 项验收编号、62 项证据编号与 JSON 结构、脱敏路径和空白检查。无 Rust 改动、无运行测试、无 PR/CI/独立实现审查/合并结论。卡片保持 OPEN / In progress；不标 Ready、In review 或 Done。
