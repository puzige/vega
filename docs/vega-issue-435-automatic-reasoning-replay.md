# Issue #435 — 按模型协议自动回传推理状态

## 冻结与授权

- 版本：v1.0，2026-10-17。Issue：[自动适配模型思考回传并移除普通设置开关](https://github.com/puzige/vega/issues/435)。
- 用户明确要求补齐方案、转 Ready、交 Vega 实现。本 Feature 适用明确委派例外；Codex 完成需求审查、后续代码审查、PR/CI 与集成，Vega 按此契约实施。
- 代码基线：`4eb22a1d776155be00a2e940889f8da15d125f55`；任务分支：`feat/435-automatic-reasoning-replay`。交接提交及固定版本链接以 Issue 最新交接为准。
- 本文交付时只有方案，无运行代码、PR、自动测试或手测通过结论。所有自动回归仅由 GitHub Actions 执行，本地只做快速静态检查。

## 现状、证据与改后行为

当前 `vega_store::reasoning::ReasoningProfile`、`ReasoningProfileProjection`、`FrozenReasoning` 用 `preserve_reasoning_content` 布尔值同时表达能力和偏好，普通设置可编辑“工具轮回传”。布尔值无法表达文本与 opaque items、当前工具链与跨用户轮的差异。模型目录的 `reasoning: true` 也不提供这些协议保证。

`app_agent.rs::reasoning_for_provider_model` 只读取精确 provider/model 的用户配置，缺失即 Unknown。`agent/loop_.rs` 在工具响应路径按布尔值保留文本推理；最终无工具响应路径却清空 Responses replay，并只创建普通 Assistant 文本，遗漏最终响应自己的推理状态。`agent/entry.rs` 完成持久化后丢弃 `AgentOutcome.messages`，下一次用户发送只能从持久化可见历史重建。重开历史同样没有原始推理和 live 工具参数。

`openai/responses.rs` 已有独立的 reasoning items 回传、`store:false` 和 `include:["reasoning.encrypted_content"]`；这不是 Chat 的 `reasoning_content`。现有 source/CAS、完整工具组、凭据过滤、CUA 临时参数失效、协议单响应限制都须保留。

改后：普通用户只选择模型及已有思考档位；回传由冻结的协议能力决定。相同路由且上下文完整时，在应用内连续发送保留模型需要的原始状态；无法验证的恢复走新的回传周期，用已保存的真实任务、回复与工具结果继续。模型变化、压缩和冷恢复允许缓存失效，不能假称完整恢复了已丢失的推理。

## 验收矩阵（先于实现计划）

每个用例须在生产调用链用进程内替身捕获真实请求/状态，不能只验证枚举映射或新增 helper。初始状态均为待 CI；手测项待合并后用户验证。

| 用例 | 前置及操作 | 可观察结果 | 层级与证据 |
|---|---|---|---|
| A | 官方 DeepSeek Chat 已知模型，无配置及旧开关 false；声明 tools 后多次工具调用，再正常结束并发第二条用户消息 | 自动完整回传该周期所有 Assistant 的原始 `reasoning_content`，含没有调用工具的最终答复；身份/顺序准确，工具实际只执行一次 | App → conversation → runtime → Chat wire 捕获 |
| B | 同模型请求不声明 tools；以及服务实际无推理字段的普通 Chat | 按协议省略无须回传的推理；不存在空造思考或额外协议参数 | provider wire 与预算 |
| C | GLM 官方标准 API 和 Coding Plan，已知支持 preserved thinking 的模型；多个用户轮及混合文本/工具 | 自动发送 `thinking.clear_thinking:false`，逐字、按原序保留完整推理；不强行覆盖用户合法档位，不给其他协议发送该字段 | App 解析、Chat wire、原有设置回归 |
| D | Responses 返回多段/加密 reasoning items、函数调用及最终无工具回复；第二用户轮 | 完整校验后的原始 items 与相应响应关联、有序回传；保持 stateless，未声明新参数不硬发 `reasoning.context`；不把 summary 文本冒充 opaque 状态 | Responses parser → loop → wire |
| E | 相同模型别名但代理不同、伪似官方域名、未知接口；选择明确兼容模板后重试 | 不靠名称/URL 子串或目录 reasoning 位猜协议；未知 Chat 返回未声明的推理加工具提议时，在执行该响应任何工具前给固定兼容错误；已声明后正常自动处理 | resolver、零执行、错误归属 |
| F | 老 v1 配置 true/false/缺失，新 v2 配置、重复键/非法值及保存竞争 | 已知协议不再被旧 false 关闭；明确旧自定义 true 有有限迁移；坏配置不静默丢弃；CAS 不覆盖其他模型/并发修改；普通设置无回传开关、动作或焦点残留 | codec/store/设置生产控制器/GPUI |
| G | 第一轮成功落库，原路由、指令、工具和来源不变，再发新消息；切去其他聊天再切回 | 命中有效内存会话，追加新用户消息而非重建旧 Assistant；前缀除正常协议处理外稳定，无重复调用/消息；这不是服务端 cache hit 证据 | App 两次 worker、wire 前缀与执行计数 |
| H | 退出再启动、无内存状态、超出 idle 容量、旧/新历史及含图片历史 | 从 durable 源冷恢复；旧 Assistant/Tool 只成为有界的非权威历史观察，真实 User/图片/结果顺序保留；不造原始思考、参数和可执行调用，不写 DDL | file-backed Store → prepare → wire |
| I | warm 状态下改模型、provider、API、endpoint、凭据版本、指令、权限或工具定义，再切回原模型 | 新请求使用目标能力及预算，旧状态不串线；切回不复活已失效状态；相同名称不代表兼容，不承诺跨模型缓存 | App 配置与 registry 生命周期 |
| J | 多轮长工具链触发自动压缩；手动压缩；上下文溢出恢复；重复压缩 | 仅完整、安全且最终 CAS 接受的边界建立新周期；目标/修正/进展保留，覆盖外真实数据不丢失；下一请求预算按实际 wire 重算 | runtime + conversation compaction |
| K | 压缩来源真实被改、checkpoint 前驱错、取消/超时/无进展；未完成/未知结果工具组 | guard/CAS 仍拒绝；失败不发布新周期或改变前缀，未知工具结果不伪造、重执行；无压缩自旋 | source/CAS/错误与零重复执行断言 |
| L | streaming 中取消/失败、持久化 ACK/close 失败、steering、批准计划、凭据续跑、并发迟到结果 | 不封存残缺状态；steering 按真实接受顺序；三入口同一能力解析；旧 lease 不能覆盖新 generation；已有恢复/审批/幂等语义保留 | persistence actor、queue、App 入口 |
| M | 推理/opaque/live 参数增加实际输入，展示尾部被裁剪，warm→cold/压缩后变化 | 所有实际请求、预览/预算/估算及 usage anchor 使用同一投影；必需字段计费一次，展示裁剪不删模型状态；不恢复累计 1 MiB 任务硬限制 | context/accounting/保留回归 |
| N | provider usage 含/不含 cached tokens，DeepSeek hit/miss；发生 cold 或模型切换 | 原样按已有可信字段汇总；缺失为未知，不用内存 cache 命中、估算或前缀相似度伪造服务端命中/省钱 | parser → usage → 已有 UI |
| O | 合成敏感哨兵、被 redactor 改写的参数、失效 CUA 目标、恶意历史字符串 | 不为回放新增 SQLite/TOML/日志/事件/Debug/UI 的原始 payload；现有经过滤的有界思考展示保留；改写使精确回放不再成立时整段降为历史观察或阻止未完成链；权限/凭据围栏不放开 | 安全、投影、持久化回读 |
| P | 普通设置鼠标/键盘操作；运行中、保存冲突、兼容失败 | 无“工具轮回传”及同义第二开关；合法思考偏好仍可操作；错误归于所属对话/运行，有可操作下一步；不在 composer 增添常驻协议说明或成功横幅 | GPUI 生产测试及用户手测 |

## 能力模型与解析规则

### 独立的协议能力

新增有界 `ReasoningReplayPolicy`，由 App 边界解析一次，随本次 `FrozenReasoning` 冻结，runtime 不读设置文件或通过输出文字猜策略。字段表达 encoding、保留范围、tools 条件、兼容协议版本及明确支持的额外 wire 参数；不能用新名字继续承载用户 on/off。思考开关/effort 是另一个已有偏好，用户看见的思考摘要也不是本策略的输入。

| 能力配置 | 输入/保留策略 | wire 行为 |
|---|---|---|
| `standard_chat` | 没有声明 Chat 推理回传扩展 | 不发送 `reasoning_content`，沿用普通消息及工具协议 |
| `deepseek_tools` | 原始文本；请求声明 tools 时，保留本周期所有 Assistant 响应，含最终无工具响应 | 有 tools 才回传完整 `reasoning_content`；判断是请求的 tools，不是本次响应有没有 ToolUse |
| `glm_preserved` | 原始文本；本周期所有响应 | 合并 `thinking.clear_thinking:false`；已有合法 type/effort 仍由思考偏好决定，forced-thinking 禁用约束保留 |
| `glm_interleaved` | 原始文本；至少当前用户轮内完整工具链 | 仅明确兼容声明可选；不额外发送 preserved 参数；下一真实用户轮按此声明开始新的推理保留范围 |
| `responses_items` | 校验后的原始 reasoning items，绑定原始响应及顺序；相同精确路由/模型 | 保持现有 `store:false`、加密 include、手工完整 input；省略未明确声明的 `reasoning.context`，由服务按对应模型默认处理 |
| `legacy_text_tools` | 仅从旧自定义配置显式 true 迁移的有限兼容能力；有 tools 时保留所有文本响应 | 原样文本回传，不据此添加 GLM/DeepSeek 的其他参数；不作为新 UI 模板 |
| `unknown` | 无足够协议声明 | 不发送猜测的扩展；见下面错误处理 |

V1 不引入 Anthropic 等新 transport、`previous_response_id`/服务端 conversation、自动付费探测或请求失败后轮流试参数。Responses API 的现有明确选择本身足以采用 `responses_items`；它不意味着支持 GLM 字段或任意新 OpenAI 参数。

已知服务的默认能力不能仍依赖用户先创建 reasoning profile：注册表先组成合法协议能力，再合并已有用户偏好。DeepSeek 增加独立 `ReasoningProtocol::DeepSeekChatCompletions`（store 值 `deepseek_chat_completions`），使用已声明的 thinking type/effort wire，禁用仅允许 `thinking.type:disabled`，不能继承 OpenAI 的 `reasoning_effort:none`；新模板声明 `low/high/max`，已有官方接受的档位值映射按合法配置保留。GLM 5.3 的强制思考及仅 low/high/max 规则继续适用；不能通过回传模板改变合法禁用/档位。明确未知 API/非法偏好仍在请求前失败，不把失败静默改成提供方默认。

完整性与当前思考模式分别判断：保留解析得到的原始 absent/empty/nonempty 状态，不能用展示层收到的非空 delta 推测字段是否存在。协议允许的空思考原样保留，禁用时不凭空造思考；一个开启思考且按声明必须有回放状态的完整工具响应缺失原字段时，返回 typed `ReasoningReplayIncomplete` 并在执行该响应任何工具之前停止。不能在有工具副作用后才发现下一请求缺状态。

Responses 还须保留同一响应内部 reasoning、message、function_call 的原始相对顺序，不能把所有 reasoning items 一律挪到全部 calls 前面当作“完整回放”。在现有 validated reasoning items 上增加仅内存的 `ResponseReplayLayout`：原始顺序的有界 item slots，引用已校验的 reasoning item、对应 assistant message/text 及绑定的 call-id/name/live 参数；必要的原始 envelope/响应 id 随 slot 保留，未知或不合法 item 仍按现有协议校验拒绝。它不是第二份审计/执行提议，Debug 只计数。序列化先验证 slot 与实际消息/参数绑定，再按 slot 输出；绑定失效的已完成组整体成为观察，不能留下孤立 function output。opaque 本身不解码、不修改。

### 自动识别与自定义接口

解析顺序固定为：已验证的 v2 精确兼容声明 → 随代码发布的精确服务注册表 → 有效 v1 声明的有限迁移 → Unknown。若 API 与声明不兼容，在发请求前失败。官方已知端点不能通过旧 false 或自定义 `standard_chat` 关闭其必需回传；冲突声明须报配置冲突，不静默覆盖。

注册表是有来源、可测试的能力声明，不是字符串猜测。键为 API + 规范端点精确值 + 模型精确值；提供方显示名称、仅含 glm/deepseek 的别名或路径、目录 `reasoning`/`tool_call` 布尔值不能替代它。端点比较只做 URL 解析允许的 host 大小写、默认 HTTPS 端口及末尾 `/` 归一化；不跟随重定向识别服务，不解码任意路径、不忽略 query/userinfo/额外后缀。未知模型不得用前缀通配。

本版种子注册表：

- `https://api.deepseek.com`、`https://api.deepseek.com/v1` 的 Chat，模型 `deepseek-flash`、`deepseek-v4-pro`：`deepseek_tools`。这两项来自核对日官方示例/说明；其他旧/代理别名须有精确兼容声明，不猜版本。
- `https://api.z.ai/api/paas/v4`、`https://api.z.ai/api/coding/paas/v4`，以及官方对应 `https://open.bigmodel.cn/api/paas/v4`、`https://open.bigmodel.cn/api/coding/paas/v4` 的 Chat，模型 `glm-4.7`、`glm-5.3`、`glm-5.3-flash`：`glm_preserved`。本产品为已明确支持的 agent 模型主动启用 preserved；不能误称标准 API 服务端本来默认开启。
- API=Responses：采用现有 transport 的 `responses_items` 契约；未知兼容端点仍保留现有 parser/字段校验，遇不支持的返回就诚实失败，不降为文本思考。

自定义代理通过**已有的协议/能力模板编辑入口**声明服务兼容性；在该入口增加 DeepSeek、GLM preserved、GLM interleaved 的有限协议模板，选模板后自动决定回传，不增加“是否回传”的替代开关。协议模板属于高级配置，收于模型详情的“高级协议”展开区，普通偏好只留已有档位/提供方默认。注册表可识别的模型不用选择模板；代理无法凭模型名字证明它支持哪套协议，这是明确的支持边界。保留现有 OpenAI/Zhipu 模板入口的兼容映射。

Unknown Chat 在完整响应里出现原始推理字段且提出工具调用，而没有可验证的回传能力时，返回 typed `ReasoningReplayUnsupported`，**在该响应任何工具审批/执行之前**停止；不丢字段后继续产生 400，也不把此错误报网络/压缩故障。仅返回最终可见答复时可正常显示，但不将无法完整续接的状态封存为 warm；下一用户轮冷恢复。没有推理扩展的普通未知 Chat 工具使用仍可正常工作。错误提供“选择匹配的高级协议模板后重试”，诊断只有有界原因和对象身份，不回显思考或 provider 原始正文。

### 配置迁移与普通设置

`reasoning.toml` 格式升为 v2，现有 profile 增加 `replay_profile` 有界字符串（缺省 `auto`），只表示高级兼容声明。保留现有 effort/support/preference 及精确 provider/model 键。不要把远程模型目录写入当前配置，也不在启动时主动改写用户文件。

读取接受 v1/v2。v1 先在内存迁移：官方精确注册表或 Responses 自动能力优先；旧 `zhipu_chat_completions` 明确声明映射到 GLM 完整文本回传能力（跨轮保留，不凭旧声明强发新增 preserved 参数；用 `legacy_text_tools` 编码保留其已有 wire）；其他未知自定义 true 映射 `legacy_text_tools`；false/缺失是旧默认，不作为否决已知协议的能力。不能把 Unknown 变成通用 OpenAI 思考档位支持。

仅当用户正常保存配置时写 v2，去除 deprecated bool；保持现有原子保存、expected snapshot 和按字段合并 CAS。旧值只留在 v1 decoder，不能继续进入可编辑 Projection/Patch、焦点路由或覆盖运行策略。保存 effort 不得顺带把 auto 固化成错误模板、清空无关模型或以旧快照覆盖外部变化。非法版本/值/冲突按已有设置错误保留未提交编辑，提供重新读取。回滚到旧二进制可能不接受 v2，不承诺透明降级。

## 原始状态与会话生命周期

### 存储边界

原始 `reasoning_content`、opaque/encrypted items 和原始工具参数只用于协议内存，不作为普通消息正文、审计字段或新磁盘缓存。保留已有经过滤、独立有界的思考展示，不把原始回放对象再作为 UI 摘要来源或回灌数据。保留已有凭据过滤；不新增原文 SQLite 列、TOML 内容、日志、导出或 DDL，不解析/转写 encrypted_content。不从模型可见文本识别“历史观察”或抽取思考来补状态。

本卡仅将 #300 的“live 参数只留本次 run”扩展为**本应用内有界、经身份校验的 warm 会话内存**，以保留跨用户轮协议完整性；不改变审计脱敏和不落盘要求。不会恢复独立累计思考字节的任务硬上限。单响应/parser 的现有限制及真实模型上下文预算继续生效。

### warm 内存状态

App 初始化一个应用级 `Arc<ReplaySessionRegistry>`，各 `AppAgentController` 共享该实例，注册表在 conversation 层实现；通过 `PersistenceActorConfig` 的生产构造参数传到准备/结束路径，不能经 ConversationEvent、Store 或 UI 投影传递 payload。可复用 `main.rs` 已有 AppMcpSettings 一类的 app-scoped 注入方式，不能每个 worker/window 新建私有缓存来绕过总量限制。runtime 不依赖 Store，也不保存权限授权对象或复用工具 executor。

一条封存记录只持有原始 runtime messages（含完整 response 绑定与原始 live 侧车）、ReplayFrame、可信 durable 源 fence、冻结的协议/请求环境指纹；不持有完整 RuntimeEvent/展示思考副本。最多 **4 条 idle 会话，合计 32 MiB**，按 LRU 淘汰，大小计入文本、图片、opaque、原始参数和元数据。单条超额就不封存。借出的 active lease 不参与淘汰；这不是运行失败、强制压缩或累计 thinking 限额。

冻结身份至少包含：线程+数据库/项目身份、provider 配置身份/API/规范 endpoint/精确 model、策略版本/偏好、credential revision、系统/项目指令快照、实际工具 schema 与 enabled aliases、权限模式、skills/MCP/CUA 能力快照、模型预算策略、source_version/fingerprint、checkpoint 前驱/覆盖范围及 generation。凭据只用 owner 提供的本地不透明版本身份，不记录/打印密钥或把 secret hash 当公开标识；当前输入快照无法验证就 miss。任何身份改变立即丢弃旧 idle 状态，切回原配置也不复活旧记录。

生命周期顺序固定：

1. 单线程同一时间只发一个带 generation 的 lease。普通新消息在 `prepare_run_with_images_and_reasoning` 的现有 transaction 内，**新 User/streaming Assistant 插入之前**读取 base source/checkpoint，比较上次封存的 fence；recover/legacy cleanup 改了来源则 miss。不把刚插入的新用户消息误当成历史篡改。批准计划/凭据续跑等特殊入口无法证明同一 base 时直接 cold，不能剥掉任意最后一条 User 绕过校验。
2. 当前 run 照常执行准备事务和 ACK，读出最新真实来源/幂等结果。候选 warm messages 不能代替 durable 校验。等 skills/MCP/CUA、工具 schema 和固定 System 最终冻结后，runtime 在第一请求前再核对请求环境指纹；不符就丢弃候选，使用已准备的冷历史。
3. warm 仅复用验证过的旧 messages 与 ReplayFrame，追加本次真正新用户消息/图片；System 使用本次已验证相同的快照。live 工具 args 仅回传协议，不作为执行提议；新执行只来自当前 provider 新响应，仍经原审批/幂等/凭据检查。
4. 完整收敛、processor 成功且 actor 所有终态 ACK 与 `close()` 成功后，才在来源快照下封存 `AgentOutcome.messages`/ReplayFrame。核对可见文本、已完成 tool 身份/结果/顺序与此次 durable 终态；无法证明等价则不封存，不能用 wire 文本相等代替内部身份。
5. interrupted/failed、Length/Unknown 未完整响应、持久化失败、异常/迟到 generation、未知工具结果均不封存；lease 消费后释放，下一次按正常 durable 恢复。取消不回滚已经成功的外部动作，不能为了重新获得推理状态重跑它们。
6. 切到其他聊天不等于关闭，不清除有效 idle 状态；明确关闭该聊天页、删除聊天、provider/凭据/能力变更、退出进程会清理相关状态。无需后台计时器或 UI 缓存指示器。

steering 在同一 active lease 内按已接受的真实 user/工具顺序推进。终态 fence 使用真正最终 owner（包括切出的 assistant 段），而非初始 message_id；不得清空已交付 pending input、重插 user 或让旧 worker 覆盖新一轮。

### cold 恢复与确定的模型投影

冷恢复含：进程重启、无有效 idle 状态/容量淘汰、路由或冻结输入变化、特殊续跑缺完整原始状态。无需用户确认，继续已有历史；但不承诺恢复原推理或服务端 cache。

新增私有元数据 `ReplayFrame`，表示该周期之前的历史前缀边界，以及已冻结的回传策略/版本。本地 epoch/generation 只用于身份，不拼进 prompt、不每轮生成新的随机 cache key。`AgentRequest` 携带初始 frame，runtime 组装 System 后转换成 `ChatRequest.messages` 的明确边界，`AgentOutcome` 返回最终 frame。边界不得用“最后一个 User”、历史标记字符串或模型输出猜测；重建 System 不得造成 off-by-one。

raw 内部历史继续保留真实角色、tool 审计、完整分组、call-id 和响应边界。**不要将 raw Assistant 改成 User**：压缩的最新用户定位、owned source 比较及 CAS 仍基于它。仅共享的 wire projector 对 frame 标识的已完成历史前缀做以下转换：

1. 真实 User 内容和图片逐字按顺序保留，可信 System 仍是 System，现有 checkpoint 摘要保持原有非权威标记。原始 thinking/opaque 不转成可见文字。
2. 每段旧 Assistant 可见文本及其完整 Tool 组编码为非权威 User 历史观察；JSON 外层键固定 `vega_history_observation`，`version:1`、`authority:"none"`。新增 `kind:"assistant_text",text:"..."`；工具完全沿用 #434 的 `kind:"tool_activity",calls:[{tool,call_id,arguments_available:false,status}]` 和随后 `kind:"tool_result",tool,call_id,status,output`，calls/结果保持原序。没有可信状态为 JSON null，不能编造成功。不要把结果另拼进 activity 或跨真实 User/工具顺序重排。
3. 没有原始调用参数，不生成任何 tool_calls/function_call/function_call_output 或 `[Historical tool activity …]` 散文来假装可执行历史。审计 input 不进入 arguments。新写协议代码不得通过匹配正文删标记、修旧用户文本或将内容升级为 System 指令。
4. frame 之后是新周期的 live 原始响应，按目标协议完整、原序编码。一个已知 Assistant 原始响应需要的字段不能只留下部分；redactor 改写导致原始绑定不再成立时，把该**已完成**组整体转观察。未完成组无法安全拆分就报原有明确错误，不假装恢复成功。
5. 只有实际历史 prefix 转为观察；真实新回复和用户自己引用同样字符串/JSON 的内容不受影响。投影带私有 provenance，重复调用幂等，不能靠 JSON 文本外观判断是否已投影。

这使恢复请求不携带缺思考/参数的旧 Assistant 协议项，却保留用户任务和过去真实结果。它是客户端新的回传周期，不是伪造历史思考，也不代表删除 UI/数据库记录。无需在重开/切模型时额外付费请求一轮摘要；超预算走原有安全压缩路径。

## 压缩、模型切换与预算/Cache

### 压缩提交是原子边界

自动预检、手动压缩和 typed context-overflow 的既有有界恢复均使用相同规则：在完整已终结的响应/工具组边界选择前缀，摘要从可信 durable 内容/真实结果构造，不要求读取原始思考。所有摘要请求 tools 关闭；不得为了压缩再次执行历史工具。

先做现有 source_version/fingerprint、owner/partial cursor、checkpoint 前驱、分组覆盖及最终 CAS 验证。**只有压缩接受后**才同步更新 raw messages、source fence 和 ReplayFrame，把该边界之前已完成的剩余 Assistant/Tool 设为旧周期观察，并释放不再需要的原始思考和 live 参数；保留目标、用户修正、已完成工作及待办。不能仅清空 reasoning 后继续把旧 Assistant 当完整 live 项发给模型。尚未匹配 Tool result 的组不能跨此边界。

失败/取消/真实 SourceChanged 保持旧 frame、wire 前缀及 usage anchor，不发布新周期。接受后对**下一次真实 wire**重新估算，失效旧 usage anchor。重复压缩继续同一套边界算法，不新增重试自旋，不删除真实 source 保护。#409 的 durable 归一化原则继续有效：比较仅忽略原本不会持久化的协议状态，不忽略文本、图片、工具身份/审计/结果及顺序，不修改全局 `ChatMessage::PartialEq`。

### 模型和路由变化

V1 对 model/provider/API/endpoint/兼容声明的任一变化一律 cold，即使同家族或同名也不复用旧 opaque 状态；新 run 重新冻结目标 policy/预算。切回也建立新周期。只复用可信持久化任务上下文和已有 model-switch/checkpoint 语义，不并列持有可复活的多模型隐状态。运行中设置变更不突变当前冻结请求；现有取消/下一轮路由边界照常处理，后续 run 使用新配置。

### 一个投影、真实估算与缓存证据

扩展现有 `project_model_messages` 为接收冻结 frame/policy 的共享入口（可增 `project_model_request` 包装），Chat/Responses 序列化、primary/summary 请求、`estimate_chat_context` / `estimate_wire_context`、`agent/context_accounting.rs::fingerprint`、手动压缩和上下文预览使用同一结果。不要各自实现一份历史包装；preview 无 active lease 时明确按 cold 估算，不能偷用某个旧模型内存数据或伪装成 provider 实测。

模型可见的推理、opaque、工具 schema、历史观察 wrapper、图片都计入实际输入，一次且完整；仅展示的思考不造成虚假输入压力。frame/policy/投影内容变化必须失效对应 usage anchor；本地 epoch 随机号本身不成为 token/缓存输入。已有 per-response parser 防护不删，#375/#393 已删除的累计任务 thinking 硬限制不恢复。

相同 warm 周期应尽可能只追加正常对话；保持原始推理及稳定 System/工具定义，不改写旧前缀换取表面简洁。cold、摘要替换/重包历史、模型/路由/工具或指令变化会改变前缀，允许 cache miss；已有更短前缀可能复用，但不是保证。删除推理可能少发 token，却也可能丢协议必要状态或后半段 cache，不能统一宣传为省钱。

只沿用 provider 实际返回的 cached/input/cache hit/miss usage，缺失标未知。registry hit 是本地恢复优化，不等于 provider KV cache hit。不得用静态单测制造“命中率提升”或“降低错误率百分比”，不新增缓存开关、第二套遥测、后台预热、原文缓存持久化或新 UI 状态条。

## 失败反馈与并行卡片边界

新增兼容/回放完整性错误使用有界类型，从 runtime error 经 conversation 现有 run failure/diagnostic 通道到所属 Assistant 的错误区，不用任意字符串匹配分类，不把错误挂到当前 composer。无消息 owner 的准备失败沿用既有发送前错误通道；如 #427 已集成，则使用其对话内失败投影。不能为本卡重做全局错误系统或把配置错误归成 Network/Compaction。

与相关 Ready 卡的关系：

- [#409](https://github.com/puzige/vega/issues/409)：保留其 owned durable source 比较规则。本卡 warm/冷 frame 接线需要同样的局部归一化；若 #409 尚未进入实施基线，本卡须完成此必需接线并保留其正/负例，不等待它再运行。归一化仅在 durable guard 的两侧对称剥离协议内存数据；不是全局放宽 PartialEq。若已合入则复用，禁止再实现第二份比较器。
- [#413](https://github.com/puzige/vega/issues/413)：继续负责摘要失败分类/恢复。本卡只增加自己兼容错误的类型及既有 UI 路由，不复制或改写其历史失败迁移规则；其是否合入不影响本卡执行。
- [#434](https://github.com/puzige/vega/issues/434)：工具观察格式、终态私有元数据、rich 内部历史和基础非权威规则在本卡是冷恢复的必要基础。未合入时按已发布契约在本卡实施这些接线；已合入则复用，只新增旧 Assistant 文本观察及 frame 范围。不能延用当前基线的 Persisted Assistant 散文作为完成态。
- #300/#375/#393 的审计/预算/协议限制继续有效；仅 #300 的 run 内存寿命按本文明确扩展。#416 的最终答复/过程展示不改，UI 展示裁剪不能影响本卡内存协议状态。

因此本卡没有等待其他卡合并的隐性前置条件，也不宣称这些卡已经完成。共享文件必须单一写入者、串行实施/集成；后续其他卡领取前核对已实现接线及测试，不能覆盖本卡或并发重写同一文件。如果实际 API/安全约束与此冻结契约无法同时成立，交回 Codex 修订，而非自行删除 guard 或缩减范围。

## 实施顺序、文件与接口

不新增依赖、数据库迁移、通用 agent 框架、外部服务、自动真实 E2E 或代码注释；新增小模块按业务命名，不用 Issue 编号命名。已有结构可做最小接线，禁止借本卡重构 tools 业务实现或权限/幂等算法。以下名称表达明确职责；可按现有模块可见性落位，但不得拆出第二套策略/投影器。

1. **先写真实入口的回归用例。** 在 runtime `provider.rs`/`openai/{mod,responses}.rs` 现有测试模块、`agent/tests` 增加自动协议、最终响应回传、cold frame、token/usage 断言；在 conversation 和 App 测试覆盖两次用户发送与重启，不只给新类型写镜像测试。保持所有原有来源篡改、参数绑定、凭据、图片、预算、持久化失败与零重复执行断言。
2. **冻结能力。** `vega_runtime/src/provider.rs` 增加 `ReasoningReplayPolicy` 与固定能力变体，`FrozenReasoning` 去除用户 bool 权威；`vega_conversation/src/types/reasoning.rs` 做合法转换；`vega/src/app_agent.rs::reasoning_for_provider_model` 扩展输入为精确 provider snapshot/API/endpoint/model，使用一个新 `reasoning_capabilities.rs`（App 或 conversation 的边界模块）实现种子注册表及优先级。普通发送、批准计划、凭据续跑、摘要/commit 等生产调用者均显式接线，不能某入口仍默认 false。catalog 只能提供精确映射资料，不直接认领兼容性。
3. **迁移设置。** `vega_store/src/reasoning.rs` 实现 v1 decoder、v2 字段/校验、原子保存及精确 patch 合并；`types/reasoning.rs`、`window/reasoning.rs`、`vega_ui/src/settings/{reasoning_render,reasoning_state,state}.rs` 及相关 type/focus 枚举删除 replay 编辑动作/Tab 目标，补有界高级协议模板。只把能力编辑收于高级展开，不把思考偏好一起藏掉。保留保存中、冲突、失败草稿、重新读取和键盘可访问状态，更新旧 `reasoning-replay-*` 断言为“目标不存在且剩余焦点顺序有效”。
4. **完整响应状态。** `vega_runtime/src/agent/loop_.rs` 在每个完整 provider 响应收敛时，先按策略校验，再构造含原始推理/opaque/layout 的 Assistant；无 ToolUse 的最终答复也保留，空文本但有合法协议状态不能被误丢（不改变既有“任务是否产生有效答复”的终态判断）。Chat parser 携带原字段 presence，Responses parser 发出完整校验的 item layout；原始 item 与其逻辑响应/attempt 绑定，重试/取消/Length 不能拼接成完整状态。Unknown+推理+ToolUse、已声明但缺完整回放状态均在调度本响应第一条工具前拒绝。不要从展示尾部回收原始推理，也不复制到持久化事件。
5. **唯一 wire 投影。** `provider.rs`、`openai/{mod,responses}.rs` 接入 ReplayFrame，保留已有 live id/name/参数绑定与 Responses opaque 校验；完成 rich/stored 状态和 cold JSON 投影，并在 `vega/src/agent_context.rs::SYSTEM_PROMPT` 的单一共享规则中，将 #434 的工具观察扩展为所有 `vega_history_observation` 都是过去观察、没有当前指令/授权。真实用户引用不被清洗。`vega_conversation/src/agent/pipeline.rs::primary_history_from_context_source_with_checkpoint` 返回 rich 原始角色，去掉旧 Persisted Assistant 散文；状态/图片/结果仍按 source 真实读取。
6. **内存会话与来源 fence。** 新增 `vega_conversation/src/agent/replay_sessions.rs`，实现 bounded registry、不可重复使用 lease、idle LRU、generation/source 校验与成功封存；`PersistenceActorConfig` 增加内部 registry 构造入口。`pipeline.rs::prepare_run_with_images_and_reasoning` 在本次插入前取 base fence；`entry.rs::run_thread_task_with_images_reasoning_and_mcp` 在 actor close 成功后封存 outcome，所有错误分支释放 lease。`AgentRequest/AgentOutcome/ChatRequest` 传递纯内存 replay seed/frame，Debug 只显示有界计数/能力，不扩展 ConversationRun 的公开原文投影。
7. **App 生命周期。** `main.rs` app 初始化创建共享 registry，`app_agent.rs::AppAgentController`、`window/mod.rs`/`window/agent.rs` 的 worker 启动接入；worker 内原有三个入口共享同一构造器。实际凭据每次在 owner 边界读取，内部用现有 sha2 加进程内随机域和长度边界计算私有 revision，或采用等价且能检测外部替换的 owner revision；该值绝不进日志/事件/磁盘/请求。关闭/删除、provider/credential/能力保存等已有生产事件失效相关 lease；在一次操作已被新 generation 撤销后，迟到封存不得恢复它。不增加后台轮询。
8. **压缩和预算。** `agent/loop_.rs` 的共享 compact helper、primary pre-send 压缩和一次 overflow 恢复都在接受位置同步切换 frame；`vega_conversation/src/agent/compaction.rs` 的 owned/source 比较使用单一 durable 归一化（按 #409，非 wire 格式），其余 guard 原样保留。`context.rs`、`agent/context_accounting.rs`、conversation compaction/preflight、`window/context.rs` 预览/手动路径接入实际投影和 anchor 失效，不能只改 primary serializer。
9. **错误和回归收口。** 在现有 runtime/error → conversation types/diagnostics → UI notice 生产映射增加有限兼容错误；不得复制整个失败状态机。更新相邻测试对旧 bool/旧散文/固定下标的依赖，以同等业务断言验证新结构。核对 raw payload 没有出现在任何 persistence/Debug/diagnostics/UI/cache 导出中，保留原有安全负例。提交实现和测试后交回 Codex，不自行 PR/合并。

### 状态类型的必要语义

- `ReplayFrame` 属于原始历史元数据，携带明确定义的 prefix 边界、策略版本；wire projection 和估算读取同一实例。默认的独立测试/旧调用若没有 warm seed，应显式建立 cold frame，而不是把所有旧 Assistant 默认为 live。已有系统消息 prepend/重建必须由单一 builder 计算边界。
- `ReplaySessionRegistry::acquire` 只取走候选和 generation，不在持锁时等待数据库/provider。source 证明与冻结请求环境证明分两阶段；任一步不符，放弃 warm 优化并使用合法冷源，不跳过正常 prepare 事务。
- `seal` 只有当前 lease generation、成功 actor close、等价 durable 终态与当前配置 revision 全部有效时才写 idle 项。`invalidate` 同时撤销在途旧 generation，防止 A→B→A 复活过时状态。不能 clone lease 让两个 worker 同时复用。
- Raw history、ReplayFrame、注册表 candidate 及 wire 观察格式各有生产类型；不把字段塞进 `input_json`、model content 或 runtime diagnostics 来传递。模型收到 JSON 字样不等于它有可信 provenance。

## 自动回归、手测与回滚

新增业务名用例如 `automatic_replay_final_response_survives_next_user`、`automatic_replay_cold_history_has_no_executable_calls`、`automatic_replay_compaction_rebases_only_after_cas`、`automatic_replay_stale_generation_cannot_seal`。在现有 file-backed Store、MockProvider/request collector、注入 executor 和 GPUI TestAppContext 路径验证矩阵；不得使用真实网络、外部应用、Keychain 或付费模型自动测试。credential revision 用进程内 owner 替身，实际仍走生产解析/检查链。

完整请求测试至少使用两次 run：第一 run 有两个工具响应及最终无工具响应，第二 run 新 User；分别捕获 warm、重启 cold、模型切换和已接受压缩后的真实 Chat/Responses body。断言顺序、payload 完整性、工具执行计数、source/checkpoint 和实际输入估算；不要只断言字段存在、旧错误字符串消失或请求非空。增加响应仅有思考/仅有 opaque、tools 已声明但一次工具也没调用、多个原响应、多个工具混合历史、图片、投影重复调用、恶意同名 JSON、redactor 改写和 actor 失败的负例。

以下命令仅供**实现后的 Actions**，本轮没有执行或通过结论：

```sh
cargo nextest run -p vega_runtime -E 'test(automatic_replay_) | test(reasoning_budget) | test(context_accounting) | test(universal_tools)'
cargo nextest run -p vega_conversation -E 'test(automatic_replay_) | test(continuous_compaction) | test(compaction_timeouts) | test(history_permissions)'
cargo nextest run -p vega -E 'test(automatic_replay_) | test(reasoning) | test(project_context)'
cargo nextest run -p vega_ui -E 'test(automatic_replay_) | test(reasoning)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
python3 scripts/release_tests.py
python3 scripts/prepare_computer_use_tests.py
python3 -m unittest discover -s scripts/tests -v
```

沿用 `.github/workflows/pr-check.yml`，required check 为 `check (fmt, clippy, test)`。只认最终待合并 head 的完整结果，CI 查询每 5 分钟；通知或用户即时请求除外。不 ignore/删安全断言、不自动重试换绿、不新增门禁拓扑。Vega 本地仅静态差异/格式，不运行代码、编译、测试或应用。

用户手测在合并、云端发布后进行：

1. 普通模型设置确认没有回传开关，思考档位仍能正常选择；键盘遍历无悬空控件。已知服务不需手配回传，代理可在高级协议模板中声明对应服务。
2. 用一次性普通文件任务连续发送两轮，观察进度、最终答复和真实文件结果；再切其他聊天后返回继续。不应因最终无工具答复丢状态，不应重复已有改动。
3. 重开应用后继续该聊天，切换一次模型/接口后继续，再切回；既有任务与结果仍在，不要求保留丢失的原始思考。自动/手动压缩后仍继续完成任务，真实错误出现在所属对话。
4. 在已有 usage 显示/脱敏诊断核对服务端提供的缓存统计；缺失保持未知，切换/重开后的 cache miss 本身不是失败。只记录实际观察，不进行付费批量压测或宣称缓存命中保证。

回滚走实现提交的 checked revert PR，不删聊天、工具审计或 checkpoint。没有 DDL/原始状态文件需要迁移；内存状态退出即释放。v2 TOML 被旧二进制拒绝时应清楚报配置版本错误；确需回退配置，由用户使用其自己保留的旧配置恢复，不能自动重写、抹掉高级声明或声称透明兼容。

## 交接与完成条件

方案提交、push 并核对固定提交上的 spec 可读后，将 #435 置 Ready 并读回。阶段：等待 Vega；当前执行者：待领取；下一执行者：Vega。用户已明确将此 Feature 交 Vega，无待产品裁决或隐藏的前置合并依赖。

Vega 领取前 fetch，复用 `~/Workspace/worktrees/vega-435-automatic-reasoning-replay` 的任务分支或在其他机器以同一远端分支建立独立 worktree，保护已有改动并更新基线；不得在 master 或其他卡 worktree 实施。只有本卡单一写入者；核对相关卡已合入情况，复用已有契约接线。

完成时同卡回报方案版本、基线/实现分支/commit、修改文件、矩阵逐项证据、静态命令/退出码、待 Actions/手测项、真实耗时、限制和规格偏离（应为无）。列保持 In progress，阶段等待 Codex 审查，当前待领取、下一 Codex，然后停止写入。Vega 不自行开 PR、合并、关闭或置 Done。Codex 审查后再负责 PR/CI/合并，绿灯合并后 In review 等用户手测。

若原始状态无法安全验证、第三方协议与冻结声明冲突、历史/预算 API 需要超出本范围的架构或新依赖，或无法保留 source/CAS/凭据/幂等断言，按执行总纲 §6 在 #435 用 `[BLOCKED]` 提交脱敏证据、已改文件/commit、具体冲突和所需修订；仍 In progress，当前待领取、下一 Codex，停止实施，不擅自改 spec 或降级成“完成”。

## 官方依据与结论边界

以下为 2026-10-17 核对的协议依据；种子注册表按本文精确元组适用，不把某模型版本规则推广为所有 OpenAI-compatible 服务。

- [DeepSeek thinking mode](https://api-docs.deepseek.com/guides/thinking_mode/)：带 tools 的请求需要历次完整推理回传，包含未实际调用工具的答复；不带 tools 的旧推理不会作为上下文。本文连续周期保留、cold 观察重建是 Vega 的实现选择。
- [DeepSeek 官方集成示例](https://api-docs.deepseek.com/quick_start/agent_integrations/nanobot/)：声明 `/v1` base URL；根端点见 thinking 示例。注册表分别记录两个精确入口，不任意替用户加/删路径。
- [GLM thinking mode](https://docs.z.ai/guides/capabilities/thinking-mode)：工具链保留原始思考；preserved 要求顺序/内容完整，标准 API 与 Coding Plan 默认不同。本文对已知兼容 agent 模型主动使用 preserved，不凭通用协议名强发参数。
- [GLM 官方端点](https://zcode.z.ai/en/docs/configuration)：标准与 Coding Plan 的国内/国际端点分开；本卡不会替用户改路由或账户计费方式。
- [OpenAI reasoning](https://developers.openai.com/api/docs/guides/reasoning)：Responses reasoning item、加密无状态回传及模型默认保留范围有专门语义；本文保留原始项和精确路由，不将 Chat 文本字段套过去。
- [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)、[DeepSeek context cache](https://api-docs.deepseek.com/guides/kv_cache/)：稳定前缀是复用条件，缓存由服务端管理、受实际缓存边界影响；本文没有真实缓存收益实验，不作命中率/费用保证。

本轮不复制 SDK/pi 上游实现、不引入新 runtime。可见思考、协议状态、本地 warm registry 和服务端 KV cache 是不同对象，不能以其中一个的存在证明另一个已恢复或命中。
