# Chat Completions 统一适配层 v3

## 来源、范围与本轮基线

本规格继续 [Issue #494](https://github.com/puzige/vega/issues/494)。用户在 #495 合并后反馈：原报错已经消失，但只覆盖几种模型，未达到参考 pi 统一处理模型差异的预期。本轮为原 Feature 的覆盖修订，不能把上一轮 27 项 GREEN 当作本轮证据。

生产基线从 `aefa2ee7` 同步到仅重排文档的 `dbc8136b8913bcfe22bcd73e1eae339faf6b99d7`（#502），生产代码未变。本卡保留分支 `feat/494-chat-auto-adapt` 已同步，工作 head `da1fe0f79ca0696dc60dbf9e4d20a369a37f66dc` 与该主线全树相同。v2 的配置裁决继续有效：不读取、迁移或改写旧 `reasoning.toml`；新 `reasoning-preferences.toml` 只保存偏好；不暴露协议模板或能力编辑。本文扩展 v2 原先仅覆盖少数模型、仅回传 reasoning_content 的范围。

本轮覆盖当前 ChatRequest 的模型、消息、函数工具、图像、输出上限和思考偏好。参考 pi 的固定 Chat Completions 分发目录及实际使用的适配维度，建立目录、规则解析、编码和响应规范化的共同边界。Responses 仍按真实 ProviderApi 走现有实现；不把 pi 的其他 API、认证、任意采样或缓存配置移植为新产品入口。

2026-10-19 测试冻结前用户追加：cpa 下 `gpt-6-luna`、`gpt-6.1-sol`、`gpt-6-astra` 都只剩提供方默认，要求无法识别时也提供完整思考档位。已只读核对 cpa 的真实 API 为 Chat Completions。这是本轮验收的一部分：已知能力按事实限制，未知能力允许默认及六档人工选择；实际发送选中档位，服务拒绝时明确失败，不静默忽略、降档或改默认。不写三个 GPT ID 的局部补丁，也不读取旧配置。本裁决替代本文早期“未知只能默认”的保守策略。

参考固定为 `@earendil-works/pi-ai` **0.84.4**，npm `gitHead=b79e4cc834970cca69daebffab7df1da7d1e52c4`。已只读校验官方 tarball integrity 及本机分发目录：26 个 provider、653 条 `openai-completions` 记录。这个数字表示元数据范围，不代表逐模型真实服务验证。

- [npm 发布元数据](https://registry.npmjs.org/@earendil-works%2Fpi-ai/0.84.4)
- [固定版 Chat codec](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/api/openai-completions.ts)
- [固定版分发包及其 dist/providers/data 目录](https://registry.npmjs.org/@earendil-works/pi-ai/-/pi-ai-0.84.4.tgz)（生成的 JSON 不在该 Git 树中）
- [MIT 许可](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/LICENSE)
- [Step 工具回传](https://platform.stepfun.com/cookbook/step-5-tool-calling)与[档位](https://platform.stepfun.com/cookbook/step-5-reasoning)

## 验收矩阵（先于实现）

测试先在上述未改生产上写齐。使用真实 resolver、编码器、SSE、run_agent、偏好加载/保存和 GPUI；mock 仅隔离网络、外部进程与服务。数据来源清点与源码扫描是静态证据，不冒充行为测试。每组需要对实际结果作断言，不能只断言内部旗标或把目录读取算法再写一遍。

| 组 | 操作 / 前置 | 可观察结果 |
|---|---|---|
| A 目录契约 | 读取固定目录；另向同一 decoder 输入非法格式、重复冲突身份或未实现映射 | 目录能被实际请求使用；非法条目明确失败，不吞字段或任选第一条。静态记录 26/653、来源和投影摘要 |
| B 身份优先级 | 同一 GLM/Kimi/DeepSeek 经原厂、Baseten、Together、Qwen plan；未知代理与伪似 URL；真实 Responses API | 服务专属 wire 按实际端点解析；别名不冒充服务；歧义代理使用明确的通用档位兜底；Chat 数据不覆盖 Responses |
| C 八种思考编码 | 每种实际分发格式选默认、允许的开/关和档位 | 产生约定 wire，默认不主动开/关；专属字段不串服务；无效偏好在请求前拒绝 |
| D 用户偏好语义 | 布尔 Qwen/Together/Baseten、嵌套档位 Ant-Ling、已知无控制模型、Step、未知 GPT/任意新模型 | 布尔模型仅真实开关；档位有实际 wire 映射；已知无控制只默认；Step low/medium/high；未知默认加完整六档。设置与 composer 使用同一解析结果，键盘可选择 |
| E 普通请求 | 输出 cap、system、strict 工具、GLM 流开关 | 两种 cap 字段只发其一；developer/system、stream_options、strict、tool_stream 按服务和模型事实；schema 内容与本地验证不改 |
| F 工具历史 | 当前 tools 为空但仍有完整调用/结果；工具图片观察；空 assistant | 必要时发 tools:[]；顺序、调用匹配和图像不可信来源标记保留；不制造工具结果或提升历史权威 |
| G 三种文本源 | reasoning_content / reasoning / reasoning_text，null、空、非空，与正文同 chunk，重复别名和途中冲突 | 实际来源与原文续接；可见思考不重复；空与缺失可区分；有歧义的非空来源在工具前失败 |
| H Chat details | text/summary 碎片、多个 encrypted 项、metadata、空数组及畸形输入 | 顺序/metadata/opaque 原样保持；只有可合并文本项合并；下次 Chat wire 使用 details；不混入 Responses，不把 opaque 显示成回答 |
| I 工具槽 | 两工具交错，无 index 有 ID，迟到 ID/index，冲突/歧义/缺身份/非法参数 | 正确聚合并各执行一次；不能把全部无 index 调用塞到槽 0；无从确定的调用在副作用前失败，不修补 JSON 来执行 |
| J wire ID | 需要规范化的长/pipe ID，公共前缀不同调用，同一路由 live 调用 | 映射可重复、无碰撞，结果同步匹配；原执行/权限身份和参数不变；opaque 绑定不被擅改；cold 不复活调用 |
| K 完整续接 | 八种方言各一个完整工具周期；代表性文本源/details 两次工具后最终答复 | 观察下一真实请求和真实只读工具结果；各工具只执行一次；不依赖用户模板 |
| L 终态 | 正常 finish+EOF/[DONE]；缺 finish/Done、坏 SSE、未知/content_filter/network_error 终态 | 正常完成；错误终态在任一工具副作用前失败，不从 EOF 推断成功，不封存残缺回放 |
| M 用量 | 顶层/choice.usage、标准/DeepSeek/顶层 cached_tokens、cache_write_tokens、冲突字段 | 使用一个一致来源；缓存计数合法、冲突拒绝；Vega input 的原有语义不变，reasoning tokens 不重复计入 output |
| N 回放隔离 | 文本新来源/details 的 warm；模型/API/路由/凭据/工具/规则变化；cold/压缩 | 有效 warm 续接；任何身份变化使候选失效；cold/压缩不带原始状态、不重做历史工具 |
| O 预算与凭据 | 新 raw/detail 超限、带受保护秘密，取消与重试，摘要/辅助请求 | 新数据计入 turn/run/retention/context；凭据检查覆盖可发送内容；opaque 不经篡改后伪称有效；失败/取消无工具副作用；辅助 Chat 同一编码入口 |
| P 存储/UI 回归 | 保存 enabled 和真实档位，旧文件存在/损坏，新文件非法偏好，冲突/删除 | 仅新三字段文件变化；旧文件不读不改；无效偏好可见且可修复；CAS/删除保护、无模板、真实焦点与重载保留 |
| Q 原始数据边界 | 新来源和 opaque 经过 Debug、错误、持久化、显示/摘要路径；foreign Responses | 新 raw 不落 SQLite/TOML/日志/Debug/可见回答；Chat 与 Responses 分型及严格检查保留 |
| R 未知能力兜底 | 三个 cpa GPT 和任意未知模型；已知服务的未知模型；Chat/Responses；选择档位后服务拒绝 | 设置/composer 可选六档并保存重载；真实 wire 保留所选档位，使用当前 API/已知服务的编码；不补关闭/开启开关，不自动降档、不吞服务错误，失败前无工具副作用 |

矩阵中 H/I/L/O 的 malformed 场景必须经过真实 parser/runtime 的失败终态，结合零工具副作用验证；不能仅构造内部 error 或检查函数被调用。A 的完整目录清点不替代 C/K 的代表性行为验证；真实外部服务和原生整机交互仍由用户手测。

## 内部目录与解析规则

### 固定目录

导入固定发布物中的 Chat 子集，只保留 provider、API、模型 ID、baseUrl、reasoning、thinkingLevelMap 与本轮消费的 compat 字段。附来源、版本、原文件摘要、投影摘要、MIT 许可和可复现导入步骤。运行时只用随应用打包的数据，不读本机 pi/npm、任意配置中的 compat，也不在线下载规则。

目录按服务与模型组织，不在 App/Agent/UI 新增品牌匹配。26 个 source provider 包括 ant-ling、baseten、cerebras、Cloudflare 两种入口、deepseek、fireworks、github-copilot、groq、huggingface、Moonshot 两地区、nvidia、opencode/go、openrouter、三种 Qwen plan、together、Xiaomi 及三种 plan、zai/coding-cn。同 endpoint 的重复目录只有行为完全一致时可合并；冲突不能由文件顺序决定。

Vega 覆写单独列来源。保留已核实的 Step 精确 ID low/medium/high、无关闭和既有严格 DeepSeek/GLM 规则；目录对当前已支持模型的差异必须逐项说明，不静默覆盖。新增规则不读取用户旧能力配置。

### 解析优先级

1. 真实 API 先决定 Chat 或 Responses。
2. Chat 用规范化 URL 的 scheme/host/port/path 精确匹配服务。拒绝 userinfo/query/fragment 带来的伪似身份；允许一个末尾斜杠等价。已知别名路径由数据显式列出。Cloudflare 账户/网关占位只匹配对应位置的非空单段，不放宽 host 或其他路径。
3. 选该服务的模型记录及有来源的内部覆写；服务缺模型记录时应用该服务的通用 transport 规则，并将模型控制明确标为未知档位兜底。
4. 完全未知代理走通用六档兜底，不仅凭同名模型复用原厂的能力子集、关闭操作或严格回放策略；即使目录中的同名记录一致，也不能证明代理使用同一协议契约。按响应实际字段回传。Step 的公开 OpenAI effort 契约作为有来源的精确覆写保留。
5. 自定义 provider 名只是配置身份，不充当品牌或认证证据。未知模型默认请求不主动添加思考参数，用户选档后才发送该档位；未知不等于禁用工具或禁用思考选择。

完全未知的入口保持 Vega 既有通用请求基线（system、max_tokens、流用量请求和既有函数 strict 选择），不因为缺少目录记录就全局切换为某个服务的参数。已识别服务使用其 transport 默认值和模型覆写；Step 的精确覆写保留已经工作的输出上限 wire。源目录的 653 条记录含 8 条行为等价的重复端点/模型身份，去重后为 645 个端点/模型身份；它们不应被报告成 653 个不同模型。

适配规则版本/有效 profile 身份进入已有 replay environment 绑定；不能只靠 provider alias 维持 warm 复用。

### 控制与用户选择

规范选择为 ProviderDefault、Enabled、Disabled、Effort。Enabled 仅用于实际有开启 wire 的布尔控制；存储字符串 `enabled` 属新文件 version=1 的偏好值，仍只有 provider/model/preference。模型只有不可控的固有推理时，只提供默认。

未知能力有独立的内部来源标记，不能伪称模型已验证支持。其用户选项为 ProviderDefault 和 minimal/low/medium/high/xhigh/max，不推测 Enabled/Disabled。完全未知 Chat 服务发送顶层 reasoning_effort；Responses 使用既有 reasoning.effort 编码。已识别服务的未知模型使用服务默认方言：OpenRouter/Ant-Ling 发送 reasoning.effort；DeepSeek/Zai/Qwen/Together 在对应开启字段之外发送用户选择的 reasoning_effort；Baseten 未知模型没有经过确认的模板变量，使用服务通用 OpenAI effort 编码。未识别模型的六档不受服务默认 supportsReasoningEffort=false 的静默丢弃逻辑影响；是否接受由服务真实响应决定。已知模型的明确能力限制仍生效。

这项兜底同时进入设置、composer、偏好保存/重载和运行冻结。选档后不得只让 UI 看起来选中而 wire 省略，不在收到拒绝后自动更换档位或 API；保留现有可恢复错误反馈，不新增协议模板、能力编辑器或常驻技术说明。

有效档位来自格式与 thinkingLevelMap 的共同契约：null 不支持，映射值决定 wire；顶层 supportsReasoningEffort 不能关闭 OpenRouter/Ant-Ling 的嵌套 effort。没有档位作用的布尔模型不展示假档位。默认选择始终不主动开启/关闭思考；已保存的无效选择不得 clamp、偷偷改默认或发出未生效的请求。

固定目录中缺失映射键与显式 null 不等价。模型声明 reasoning 且实际格式支持 effort 时，沿用 pi 的 minimal/low/medium/high 缺键默认映射；xhigh/max 要求显式映射。Ant-Ling 的嵌套 effort 只接受显式字符串映射；OpenAI 顶层关闭只接受明确 off 字符串。格式只有布尔开关或没有控制字段时，不能从这些默认档位反推出并不存在的调节能力。

| 格式 | 非默认选择的 wire | 额外约束 |
|---|---|---|
| openai | reasoning_effort | 仅实际支持顶层 effort 的模型；关闭须有明确 off 值 |
| deepseek | thinking.type；有支持才带 reasoning_effort | 严格回传要求独立于控制；必需状态缺失不能用空字符串掩盖 |
| zai | thinking.type、clear_thinking:false；有支持才带 reasoning_effort | 有 tools 时按 metadata 发 tool_stream；保留既有已验证 GLM 事实 |
| qwen | enable_thinking；有支持才带 reasoning_effort | 同一服务承载其他品牌也用服务契约 |
| together | reasoning.enabled；有支持才带 reasoning_effort | GPT-OSS 等具体记录可以覆盖为 openai |
| baseten | chat_template_args 的已知 thinking.enabled 变量；可选 effort | 只支持分发数据中出现且已实现的变量；任意模板输入不开放 |
| openrouter | reasoning.effort | 不把网关方言移植给所有同名原厂模型 |
| ant-ling | reasoning.effort 的显式映射 | 不因顶层 effort=false 丢失真实档位 |

## 请求、响应和原始状态

现有 runtime 内形成统一目录 resolver 和 headless Chat encoder/decoder。业务内部角色和工具身份保持权威，兼容转换作用于发送/接收边界。标题、摘要、连接检查、主 Agent 等现有 Chat 请求都消费同一 transport 解析，不能只修主聊天。

请求按 resolved profile 选择 token 字段、system/developer、stream_options、strict 和 tool_stream。函数 schema 不能因为服务不支持 strict 就丢弃或弱化本地校验。历史有完整调用而当前无工具时发送 tools:[]。图像继续沿用已有不可信工具观察表示。协议变换不能制造执行成功、工具结果或用户授权。

三种文本思考字段保存实际来源。每轮选择首个非空来源；同 chunk 多个非空字段内容相同只显示一次，内容不同拒绝。尚无非空内容时保存第一种明确空字段；null 不视为 raw。来源确定后若其他来源给出不同非空增量且原来源缺席，不猜测拼接，按协议歧义失败；同 chunk 相同别名可去重。已知 OpenCode Go 的 reasoning→reasoning_content 覆写只作用于该服务。

reasoning_details 与 Responses opaque/layout 分型。接受 text、summary、encrypted 的结构化记录，保留字段、顺序和身份元数据；仅连续且身份/非载荷元数据一致的 text/summary 项合并文本，encrypted 项离散保留，不解密、重排或改写。details 存在时作为回传表示优先于重复的原始文本，不把结构化 summary 当作原始全文。畸形类型、冲突身份和预算超限先失败。

工具流按 index 和 ID 共同解析，允许唯一可确定的迟到身份。无 index 的多个不同 ID 不合并到 0。一个 index 对应冲突 ID、一个 ID 对应冲突槽或无身份增量无法唯一归属时拒绝；最终缺 ID/name 或不完整 JSON 不执行。禁止为了兼容修补模型参数。需要的 wire ID 映射必须稳定且无碰撞，同步 assistant 与 tool results，不能改变原始执行/权限绑定；有 opaque 绑定而无法安全转换时明确失败。

ID 规范化遵守固定版 pi 的适用范围：非本 route live 绑定的 pipe ID 在 Chat wire 投影中转换为合法字符且最多 40 字符；不含 pipe 的长 ID 只在精确识别的官方 `https://api.openai.com/v1` Chat 入口受 40 字符限制，不扩散到其他服务。截断、字符替换或共享前缀不得造成两个调用重名，需对完整 ID 建立可重复且无碰撞的映射。当前有效 live 绑定保持原始 ID；先执行已有冷历史去权威投影，不能为了规范化恢复已移除的工具调用。

正常 finish_reason 才允许结束；EOF 不补成功。未知、content_filter、network_error 等错误终态在任何工具审批/执行前失败。Pi 的可配置无 finish 降级在固定目录中没有使用，本轮不启用。保留现有取消与无副作用重试边界。

用量可来自顶层或 choice.usage，按标准/DeepSeek/顶层 cached_tokens 和嵌套 cache_write_tokens 读取。多来源不一致、非法数值或缓存超过输入时拒绝；input 保持 Vega 原有“包含缓存的输入总量”语义，不能直接照搬 pi 扣减后的 input；completion 已含 reasoning，不再累加。

新增原始状态进入现有预算、context estimate、输入 fingerprint、retention 内存计量与 credential guard。raw 只在有效 route/model/API/profile 的内存回放中保留；cold、压缩和身份变化使用现有去权威流程。opaque 中命中受保护秘密须阻止发送，不能脱敏修改签名后继续伪称保真。原始状态不新增持久化、日志或 UI 展示，不从可见文本反造状态。

## 对照 pi 的明确边界

下列没有当前 Vega 输入或未被固定目录使用，不为“全适配”添加新用户配置；目录导入须记录排除原因，未来使用新格式必须先增加契约与验证，不能静默吞掉。

| pi 维度 | 本轮处理 |
|---|---|
| supportsStore / store | Vega 不发送 store，无新存储意图 |
| cacheControlFormat / supportsLongCacheRetention | 无 cacheRetention/TTL 输入，不新增缓存 wire |
| sendSessionAffinityHeaders / sessionAffinityFormat | 不把已有 thread 或凭据身份冒充服务 affinity ID |
| deferredToolsMode | 当前始终发送已授权的实际函数集合；不新增 Kimi deferred 声明时间线 |
| openRouterRouting / vercelGatewayRouting / headers | 无对应路由偏好或认证产品输入，不新增 |
| temperature / samplingParams / thinkingBudgets / priority | 无输入，不按输出 cap 猜造 |
| grammar/custom tools | 当前函数 schema 工具照常，不新增工具类别 |
| qwen-chat-template / chat-template / string-thinking | 固定 653 条目录未使用；不可据此声称适配任意本地模型服务 |
| requiresToolResultName / requiresAssistantAfterToolResult / requiresThinkingAsText / supportsFinishReason=false | 固定目录无启用项；不虚构助手消息、把跨模型思考变成正文或放松终态 |
| 其他原生 API / Copilot 认证 | 不属于本轮 Chat codec，真实 API 路由保持明确 |

## 实现顺序、文件和验证

1. 专职子 agent 先写齐上述 A–R 行为测试和必要 cfg(test) 接线；生产行为未改。仅允许两项已静态核对的透明准备：把现有 Chat/Responses 构造分支原样委托给 `OpenAiProvider::request_body`；把现有能力到 UI projection 的逐字段映射原样移到共享方法并由 App 调用。不得提前增加适配行为、造接受一切的目录 decoder stub，或整目录恢复历史测试。可复用既有 workspace 版本 tempfile、GPUI test-support 作为开发支持，不新增生产依赖。
2. Codex 审查测试、源码来源及生产基线，在同卡新 draft PR 触发现有 Actions；取得有效行为 RED 才允许生产实现。缺失最小接口只标接口 RED；接线/编译/runner 故障不算行为 RED。
3. `vega_runtime/src/chat_compat` 组织固定数据、resolver、事实与版本身份；允许把原单文件拆为该模块。跨 crate 类型经 `vega_conversation::types` 共用；不新增 workspace crate/Node sidecar。
4. `openai/{mod,sse,recovery}.rs` 与 `provider_check.rs` 接入统一编码、响应规范化；`provider.rs` 承载有类型且 Debug 脱敏的 Chat raw；保持 Responses 分型。
5. `agent/{mod,loop_,context_accounting}.rs`、`context.rs`、conversation 的 credential/replay/compaction/辅助消费者接入必要的新状态边界；Agent 不增加厂商条件。更新全部 exhaustive matches，不能漏新事件处理。
6. `reasoning_capabilities.rs`、projection、store 新偏好 grammar、设置/composer 和保存链路支持真实 Enabled/档位选择，保留无效状态恢复。新增可见文字仅必要的“开启思考”，不添加技术说明栏。
7. 同一冻结测试在云端 GREEN，固定源码和原始 RED/GREEN 证据；独立删除 Rust 测试及专用支持；最终 head 完整门禁确认 Rust 0发现/0执行、no Rust tests 后再正式交付。Python 通用检查保留。

本地只做低 CPU 格式/差异/静态数据检查，禁止运行 Vega、编译、构建、Clippy 或测试。云端沿用现有 fmt、全目标 Clippy、三组 Python 与 Nextest，不增建流程；检查每 5 分钟查询。主线漂移先同步并按实际影响重验，不挪用上一轮 GREEN。

跨 crate 行为测试可通过临时 `vega_runtime/test-support` 暴露真实 SSE parser 和请求 encoder 的直接委托。该 feature 仅由测试所需 dev-dependencies 启用，不补造状态、finish、profile 或兼容决策；测试仍走真实 controller、credential 和 store。它属于开发支持，GREEN 后连同专用依赖边删除；与保留的两项行为等价生产重构分别审查。

手测覆盖：Step 连续工具与续问；至少一个新增服务的思考偏好和工具；只支持开关的模型设置/composer；真实代理及重开应用。自动测试仅证明隔离外部依赖后的行为，元数据覆盖不等于真实服务全通过。回滚按本轮 PR 整体回退，保持 v2 的旧配置不读取裁决；不改用户凭据和历史。

## 进度与证据

2026-10-19：来源核验及规格完成，49 个先行测试定义已编写并静态审查（runtime 25、App 13、conversation 5、UI 6）。本地 `cargo fmt --all -- --check`、`git diff --check` 和 `cargo metadata --no-deps --locked` 成功；未执行 Vega、编译或测试。预期最小缺失接口 `decode_catalog` 为接口 RED，行为断言尚未执行；须由云端确认后才授权生产实现。RED、GREEN、删测及最终门禁仍待取得。测试与支持源清单 SHA256 为 `e5eff2bccc6423f3f2de632911091c3946aa9c55af5943c6a4b2cd83edb54cce`，提交和原始证据将在本卡 PR 固定。

2026-10-19 13:51 UTC：先行测试提交 `e8ca821a7aac87160131a26f741e3f057ab4c93a` 的 [Actions 39262820263](https://github.com/puzige/vega/actions/runs/39262820263) 已确认最小接口 RED。Clippy 与 Nextest 均仅报告 `chat_compat_catalog_tests.rs` 六处调用的 E0425：`decode_catalog` 不存在；格式和三组 Python（5/15/14）成功。Nextest 在清点前编译失败，Rust 发现数未取得、执行数 0，49 项行为断言尚未执行。测试源码与 12 个支持源哈希已在 [PR #503 固定记录](https://github.com/puzige/vega/pull/503#issuecomment-6268732441) 公布，并从远端 Git blob 全部读回核对。冻结该提交的测试目标/预期和清单，授权专职子 agent 按本规格实现；后续仍须同一测试真实 GREEN。

2026-10-19 14:20 UTC：专职子 agent 完成实现，Codex 完成生产差异审查。固定目录、八种 codec、未知六档、布尔控制、文本来源/details、工具片段与身份、终态/用量、回放/凭据/预算及设置/composer 已接通；[来源与可复现投影](vega-issue-494-chat-catalog-source.md) 单独留证。格式、差异、无依赖 Cargo metadata 和来源静态核对通过；12 个冻结源 SHA256 全部未变。未本地运行 Vega、编译或测试；本次实现仍待同一冻结测试云端 GREEN，尚不构成正式交付。
