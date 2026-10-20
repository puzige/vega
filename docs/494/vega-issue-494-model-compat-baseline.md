# 模型兼容基线与模型能力编辑 v4

## 当前阶段与用户裁决

继续 [#494](https://github.com/puzige/vega/issues/494)。本轮从 `8c57f7efb3d886c1c10429edb2bc1219a7b7a753`（v0.1.182、PR #503）开始，分支 `feat/494-pi-compat-baseline`。主会话负责来源、契约、测试审查与集成，专职子 agent 编写测试及后续实现。本地只做源码、差异、格式和静态检查，自动测试只交现有 Actions。

2026-10-20 用户在 DeepSeek/Step 手测失败后要求先移植 pi 的模型兼容测试，完整基线通过后才算成功。范围由单个 Chat 流解析器扩展到模型兼容侧整体；设置编辑模型允许的思考等级、图片输入等能力，会话选择本次使用的等级。仅修改一个空终态判断、只修几个模型 ID 或仅改变设置文案均不足以验收。

用户另外明确：沿用现行 Rust 测试生命周期，测试只在开发阶段运行，GREEN 后在 PR 固定可恢复的源码、完整 SHA 和原始结果，再独立删测和验证最终空集。此基线不是永久保留 Rust 测试的例外。后续兼容修改从固定来源恢复本基线，不能只引用本轮最终空集结果。

本轮替代 v3 中“设置只保存默认偏好、能力全部只读”的产品契约。协议方言继续由内部处理；不恢复协议模板、不读取/迁移/改写旧 `reasoning.toml` 的用户裁决继续有效。v2/v3 历史证据保持原状，不追认为 v4 验收。

## 已定位失败

已安装 v0.1.182 的三个相关主请求均收到 HTTP 200；既有诊断记录标为 provider/protocol/response，工具阶段尚未发生。经只读检查对应网关日志的脱敏结构，DeepSeek 两个档位及 Step 的首个流式 choice 均含 `finish_reason: ""`，后续还有非终态增量。`openai/sse.rs::SseAssembler::absorb` 只排除 null，将空字符串映射为 Unknown 后立即拒绝。固定版 pi 先判断 finish_reason 非空，继续读取中间帧，并在末尾单独验证真正结束标记。

本轮测试必须包含去除身份、正文、请求头和凭据的合成等价首帧、正文/思考、工具与最终结束序列。它是实际反馈驱动的新回归，不伪称来自 pi 原有测试。原始请求或响应全文不提交到公开仓库。当前先冻结完整基线，不能先改生产再补这个断言。

## 固定上游与来源证明

来源为 `@earendil-works/pi-ai@0.84.4`，对应 Git `b79e4cc834970cca69daebffab7df1da7d1e52c4`；同时审查该提交的 coding-agent 模型配置、registry/runtime 和相关测试。

- [模型类型](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/types.ts)：reasoning、thinkingLevelMap、input、contextWindow、maxTokens 及 API compat 各有独立语义。
- [可用等级](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/models.ts)：reasoning=false 时仅 off；显式 null 排除等级，缺键与 null 不等价，xhigh/max 需显式映射。
- [模型配置](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/coding-agent/src/core/model-config.ts)和[组合逻辑](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/coding-agent/src/core/provider-composer.ts)：模型定义与用户覆盖独立，逐模型覆盖在组合后应用，thinkingLevelMap 按键合并，input 替换对应能力。
- [Chat codec](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/api/openai-completions.ts)、[Responses codec](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/api/openai-responses-shared.ts)及对应上游单测，覆盖实际消息、工具、思考和流式终态转换。

取得的固定 Git 源归档 SHA256 为 `d64407e044d71ff38d752ca5344ac372213d740be43c99e21de804f4a926075d`。685 个选定源文件已逐个核对 Git API tree 的 blob 身份，包含 ai 源码/测试与 coding-agent core/测试及 MIT 许可；源清单 SHA256 为 `87ab7e1f44d054140d34d73528eee1a8370658f259be38326ab914766f48df2a`。源归档与 npm 分发数据是不同资料，分别保留来源；生成模型目录仍按现有[固定分发包证明](vega-issue-494-chat-catalog-source.md)核验，不能把 Git 树中不存在的生成 JSON 当源码。

补取同一归档的 7 个 ai/scripts 生成器与模型数据校验源，逐 blob 核对，独立清单 SHA256 为 `241ce7f4e5c813b069b09d68c5fbc82a38e23dadc4efb167dac23c1cbd45d4f7`，不改原 685 文件清单。静态 AST 清点全部 383 个测试文件、3538 个定义（ai 137文件/1535定义；coding-agent 246文件/2003定义）；其中 ai 全部及 coding-agent 58 个相关文件/354 定义进入重点映射，共 1889 定义。383 个测试文件的 SHA256 与 Git blob 经主会话再次独立核对，0 parse diagnostics。未执行上游测试，参数行/循环不冒充独立执行数。

[逐源用例清单](vega-issue-494-pi-source-cases.json)保留全部定义身份、重点用例分类、原断言/参数表及 G01–G21 目标。当前两个 API 与公共模型层的必须候选为 89 文件/447 个源定义：352 现有语义、81 模型/快照/存储缺口、9 compat/codec 缺口、5 普通 ToolChoice 输入缺口。另 195 项新请求/产品输入与 1247 项外部/live/auth/扩展/其他原生 API 明确不计已移植；余下 1649 个 coding 定义仅为 discovery。447 不是 Rust 测试数，也不是通过数；混合上游用例只认领明确的子断言，最终需对应实际 Rust 测试/表行后才算移植。

同版本官方 npm 归档重新下载并核对既有 SHA1/integrity，归档 SHA256 为 `dfd3c929cee5a7387199a0a24dfc1be2096f1ea8f59ffb8285198a0ed01ebf93`。固定数据中当前两 API 是 758 原始条目、28 提供方、750 个 API+入口+模型身份；图片输入标记为 Chat 331/Responses 101 条。这些是来源元数据，不能称已真实调用相应服务。原包文件逐项 SHA256 与分类保存在逐源清单，后续不依赖机器上可变的全局安装。派生测试/数据沿用已收录的 [pi MIT 许可](../../crates/vega_runtime/src/chat_compat/PI-LICENSE)。

## 上游测试移植规则

逐用例清点整个 ai 测试目录及 coding-agent 模型配置、覆盖、registry/runtime、可选等级、模型选择和图片能力相关测试，保留路径、标题、行号、内容摘要、参数行、循环与 skip 条件。不能只按 `openai-completions-*` 文件名选少数样例。

每个兼容目标应明确属于以下哪一类，并在清单中给 Vega 测试目标或具体原因：

| 类别 | 处理与可声称的证据 |
| --- | --- |
| 可直接移植 | 保留输入、操作与原断言的业务含义，调用 Vega 真实 resolver/encoder/parser/store/controller |
| 需要语义适配 | 固定上游原断言、Vega 不同的语义和变换理由；保留同一可观察不变量，不伪称逐字照搬 |
| 当前能力缺口 | 纳入本轮能力/请求链验收所必需的缺口进入先行测试；缺失 API 只能记录接口 RED，不能声称行为已执行 |
| 其他产品或外部依赖 | 逐项列清其 API、认证/扩展/调度或真实服务前提及与本轮关系，不计为移植成功、测试通过或静默忽略 |

现有原生接口为 Chat Completions 与 Responses；基线覆盖两者及共享模型能力，不把其他原生 API 的认证/SDK 接入混成模型覆盖。其他 API 模型的通用能力定义仍可用于纯能力逻辑测试。真实网络、账号、外部进程由测试替身隔离，原始上游“没有 key 就跳过”的条件不作为 Vega CI 通过证据。

移植测试不得执行 pi 生产算法来生成 Vega 的期望，也不得用 test-only 业务镜像替代真实能力覆盖。AST/哈希/目录清点只证明来源与范围，不算行为测试。测试源码和模块名用行为命名；任务编号只进入文档与溯源清单。

需要显式记录的语义差异包括：Vega Usage.input 是含缓存的总输入，pi 是扣除 cacheRead/cacheWrite 的输入；当前用户选档不应在能力变化后被静默 clamp；工具参数本地校验、批准身份、凭据保护和回放真实性继续有效。上游可配置 EOF 成功、部分 JSON 修补、工具 ID 变换、未支持图片的投影等场景，必须审查其适用条件及对 Vega 的影响后再冻结，不直接弱化断言或整体排除所有消息/工具测试。

## 验收矩阵（先于实现计划）

以下为验收组；逐用例、表行与实际测试的对应已在[冻结测试清单](vega-issue-494-frozen-model-compat-tests.json)固定。数量只表示静态定义，云端发现、执行和通过数以原始 Actions 输出为准。

| 组 | 前置与操作 | 可观察结果 |
| --- | --- | --- |
| M1 模型事实 | 解析固定模型目录，分别解析 Chat/Responses 与未知服务模型；同名模型来自不同入口 | 有效模型能力有明确 API/入口/模型归属；已知事实按实际记录，未知不借同名原厂能力；目录错误不伪装未知继续发送 |
| M2 可选思考等级 | pi 原有 reasoning/level-map 的缺键、null、映射值、off、xhigh/max；非思考/布尔/固定思考模型 | 可选等级与请求控制能力分型，支持子集准确；未知仍可默认六档，用户可以覆盖为子集；不将“可选”误存成“默认选择” |
| M3 用户能力覆盖 | 同提供方两模型、同 ID 两提供方；新增/修改/删除一个覆盖；重载与恢复自动 | 只影响目标模型，内置事实不被原地修改，未覆盖能力沿用来源；删除覆盖恢复自动事实；新请求采用新快照 |
| M4 设置编辑 | 在模型思考页多选可用等级；基本信息编辑图片输入；保存、取消、失败、冲突、恢复自动 | 设置表现能力编辑状态，不显示默认等级单选器；已选可用集合与保存一致，偏好文件不因编辑能力而改写；键盘、焦点和重载有效 |
| M5 会话选择 | 能力仅允许 low/high，尝试其他档位；保存一档后缩小允许集合 | 输入框只提供当前允许选项，实际请求编码所选档位；已保存的无效选择可见且可重新选择，不静默改级或吞参数 |
| M6 图片能力 | 文本模型、视觉模型及显式覆盖；用户图片、历史图片和工具图片进入当前请求 | 支持时保留真实图片编码；不支持时在发送前给出可恢复结果，不把图片悄悄丢掉当成功，不从展示名称猜视觉能力 |
| M7 持久化 | 能力文件不存在/合法/非法/冲突/部分字段；删除模型/提供方；旧配置存在/损坏 | 新能力覆盖独立存储，偏好与能力分离；原子保存与 CAS/身份检查有效，失败不破坏旧值；不读旧 reasoning.toml，不接触真实用户数据 |
| M8 请求适配 | 上游兼容请求与消息用例：角色、工具、思考格式、上限字段、严格工具和流用量 | 对真实 Chat/Responses wire 断言，服务/模型规则统一，所有请求消费者使用同一有效模型事实；不以 UI 选中态代替发送证明 |
| M9 流式响应 | 上游正文/思考/details、工具碎片、用量、终态、取消和失败；本次空 finish 首帧 | 非终态帧继续处理；真正终态收敛；错误/残缺响应不会假装成功；工具提议与执行保持明确边界 |
| M10 回放与模型切换 | 上游消息/工具 ID/思考回放用例；模型、API、入口、能力快照变化 | 有效 warm 续接；身份或有效能力变化使旧候选失效，cold 不恢复已去权威内容；同工具不重复执行 |
| M11 工具与数据真实性 | 上游可移植工具与图片结果用例；schema 错误、身份歧义、原始状态和受保护凭据 | 兼容转换不替代业务权限或参数校验；真实工具结果、来源与调用匹配保留；raw 不新增落盘、日志或可见正文 |
| M12 辅助请求/回归 | 主 Agent、标题、摘要、连接检查及恢复；容量/定价等已有设置 | 各入口消费当前模型快照，未被本轮编辑的容量/定价与既有权威来源保持一致；两种 API 不互相误发专属参数 |

## 数据与交互契约

内置模型事实、用户能力覆盖、当前请求选择是三个不同输入。内部先按实际 API、规范化入口和模型解析事实，再应用精确提供方/模型的能力覆盖，最后校验并冻结本次选择。Agent/UI 不各自增加品牌或模型名单。

用户能力覆盖至少表达“可选思考等级集合”和“是否接收图片”；未覆盖与明确禁用必须可区分。空等级集合表示不提供可调档位，不凭空制造关闭操作。已有布尔开关/不可关闭等协议事实不得被一个空集合误改。用户打开某档时优先沿用有来源的 wire 映射；缺少对应实际控制方式的情况需明确处理，不能 UI 允许而编码时省略。能力覆盖不能改认证、API 路由、工具权限或原始状态回放策略。

沿用模型配置的基本信息/思考/定价层次。思考页以多选表达允许等级；图片输入放基本信息；恢复自动仅作用当前模型的覆盖。当前请求等级仍在会话输入框选择。普通成功以实际保存后的状态反馈，不加常驻技术说明；保存冲突、无效选择、禁用原因和图片发送阻塞须就近可见并有恢复动作。控件沿用现有 token、焦点链和键盘行为。

新能力记录使用独立有版本的 typed schema 和原子/CAS 保存，不能把能力塞回只允许 provider/model/preference 的新偏好文件，也不能借此恢复旧协议配置。有效模型快照进入运行冻结、上下文与回放身份；配置或能力变更后的下一请求不能复用旧快照。已有目录刷新、容量和价格的权威来源须显式整合，避免建立第二套互相覆盖的模型事实。

### 核心接口与语义

运行层新增纯模型组合边界 `model_compat`，不依赖 store、conversation 或 UI。`ModelApi` 区分当前两个原生 API，`ModelIdentity` 是精确配置提供方和模型，`ModelRoute` 包含 API 与规范化入口。`lookup_model_definition(route, identity)` 解析固定目录事实或明确未知事实，`resolve_model(definition, override)` 是唯一能力组合决策。目录损坏是错误，不能退化成未知。

`ModelDefinition` 保留 reasoning 的已知/未知、原始 `thinking_level_map`、文本/图片输入、目录上下文/输出元数据、与 API 对应的 typed compat 和来源标识。等级表使用 map 的缺键、null、字符串三态；不把它先折叠成 UI 数组。`ResolvedModel` 同时保留源可用等级、实际可编码档位、用户允许档位、真实开启/关闭操作、图片输入以及身份/规则/来源的有效指纹。`freeze_choice` 校验本次选择；实际请求校验模型、API、入口、选择及图片能力的一致性。默认与无思考请求也携带有效模型快照，不能只在显式选档时绑定它。

| 输入/操作 | 冻结行为 |
| --- | --- |
| 源 map 缺少 ordinary 档位 | 按 pi 默认语义处理；xhigh/max 不因缺键自动启用 |
| 源 map 显式 null | 此档不支持；与未覆盖不同 |
| 源 map 有非空字符串 | 保留实际 wire 映射，不要求字符串等于 UI 档名 |
| 未知模型 | 六档 generic wire 均可选，图片默认关闭；用户可改档位子集和图片能力 |
| 已知布尔/固定思考模型 | 不伪造档位；实际开启/关闭按 codec 能力显示 |
| 用户允许档位 | 只能来自实际有编码方式的六档集合；无 wire 项不可选并可获取原因，save/freeze 同样拒绝非法值 |
| 用户允许集合为空 | 不提供可调档位；不制造 off，也不改变既有布尔操作 |
| 已保存的本次偏好失效 | 显示可恢复错误，等待用户在会话重新选择；不采用 pi 的静默 clamp |
| 另一提供方或另一模型的覆盖 | 不改变当前模型事实；孤儿记录不参与请求，不创建新模型 |

按实际 API、规范化入口、模型选择目录条目；配置提供方名字不能冒充原厂来源。同名模型位于未知入口仍用通用回退。删除原有 `model == "step-5-preview"` 的跨入口特例，Step 使用同一查表/回退/覆盖/编码与工具链。目录今后若声明该入口和模型，才采用其已知事实。

模型能力事实与服务传输规则分别归属。当前固定数据中 OpenAI 模型登记为 Responses，不代表显式选择官方 Chat 接口时退化为未知服务：官方 `https://api.openai.com/v1` 的 Chat 请求沿 pi 服务规则使用 max_completion_tokens；未知/相似伪装入口仍用既有 generic max_tokens，None 均省略。此规则不跨 API 借用模型的 reasoning/vision/off 声明，也不按模型 ID 前缀猜传输。精确已证实的入口别名沿现有规则处理，实际路由仍进入运行身份。

技术核心类型由 `vega_conversation::types` 向应用/UI 提供投影或重导出，沿用现有 runtime 技术类型的单向依赖方式。产品请求与设置事件不在多个 crate 复制品牌判定。目录 contextWindow/maxTokens 与 Vega 输入容量/output_reserve 含义不同：保留来源元数据，已有容量/价格设置保持各自权威，不按 pi 数字盲目覆盖既有设置。models.dev 刷新及显式映射不是认证或协议权限来源。

### 新能力文件

`vega_store::model_capabilities` 管理 `model-capabilities.toml`，版本为 1。根只有 version 与 models；每条 `[[models]]` 只有 provider、model、可选 allowed_efforts 与可选 image_input。能力文件不保存当前 preference、raw 等级映射、协议模板、任意 headers 或认证。

Store 使用严格字符串 DTO（allowed_efforts 为可选字符串数组）校验文件 grammar，由 App/conversation 边界转换为 runtime 等级类型；不新增 store→runtime 业务依赖，不在 store 复制品牌适配决策。

- 字段缺失表示继承；`allowed_efforts = []` 明确清空可调档位；`image_input = false` 明确禁图。
- 拒绝未知字段、未知/重复档位、重复身份、空身份和不支持版本。值的词法校验归 store；实际可编码集合校验归 resolver/controller，不能只验证 TOML 可解析。
- Upsert 更新当前模型的整条覆盖；Reset 删除当前模型记录，恢复自动。读、快照、编码、保存/回读使用真实路径参数与原子/CAS 边界。
- CAS 按目标记录比较，允许保留其他模型的外部更新；目标冲突保留外部状态与当前草稿，重载后再操作。保存失败不得宣称成功或破坏旧文件。
- 删除 provider/model 时不做隐式跨文件删除；孤儿覆盖不参与运行，目标已被配置删除时拒绝保存。保存其他模型不得顺带清除孤儿记录。
- `reasoning-preferences.toml` 仍只保存 provider/model/preference，能力编辑不写它。不存在、合法或损坏的旧 `reasoning.toml` 对本轮读写结果都没有影响。

### 设置与会话

思考页提供六档多选，编辑“可选档位”；不再编辑“当前偏好”。基本信息提供图片输入开关。用明确草稿、保存、取消、恢复自动操作表示变化；保存中阻止重复提交，失败保留草稿，过期 generation/实体/模型的异步结果不覆盖当前页面。恢复自动仅删除当前模型能力覆盖，不改偏好。六档覆盖不等于 pi 动态配置的任意 `thinkingLevelMap` 修改；本轮不开放 off 或 wire-map 编辑。

保存只更新实际编辑的覆盖字段。只改档位时，未触碰的 image_input 缺失继承不能被 UI 显示的默认值固化；只改图片同理。原本保存 high、后来允许集合改为空时，会话仍能真实选择并持久化 ProviderDefault 恢复发送；只断言 core 接受默认值不够，须经过实际 composer/controller 和下一请求。

实际设置后台共用 `ModelCapabilitySettingsService::new(config_path)` 的 load/save 边界，完成目标模型存在性、精确记录 CAS、可编码子集校验、保存回读与重算；此服务不读写本次选择偏好。服务测试不能替代真实 SettingsView/VegaWindow/composer 路径的草稿、事件、代际 fence 与偏好恢复测试。GPUI 开发接线若需独立路径，必须在任何真实 HOME/配置访问或外部 worker 启动之前注入；只抑制与被测行为无关的更新器等启动活动，并在证据中列出，不用 test-only 实现替代被测判断。

会话沿用真实开启/关闭和可用档位选择，提交前再次核对完整快照。图片能力校验覆盖用户图片、历史图片和工具结果图片；不支持时给出重新选模型/修改能力的恢复路径，不能以丢图得到成功。图片来源、实际解码、大小和数量限制仍有效。所有模型事实/覆盖变化在下一请求生效，已有运行保持冻结事实；旧 warm 候选不能越过新的有效模型指纹。

### 上游语义差异的具体裁决

1. `finish_reason` 缺失、null、空字符串均为中间态；只有合法非空终态或对应 API 真实终态能成功结束。当前没有启用 `supportsFinishReason=false` 的产品输入，不引入通用 EOF 成功开关；其上游成功断言记录为有意不同，Vega 断言不完整流失败。真正未知非空终态仍失败。
2. pi `openai-completions-tool-choice.test.ts:734` 的 ID 漂移发生在批准前，且保留首个非空 ID。按稳定 index 合并这些碎片，保留首 ID、原 name、完整参数并只产生一个调用；后续 null/missing name 不覆盖。跨 index 重复身份、真实 name 冲突、终态后或批准后变更仍拒绝，执行使用冻结批准身份和原参数。不能沿用 v3 过严拒绝把该正向兼容用例改成负例。
3. 上游修复部分 JSON、参数强制转换、optional-null 删除不移植为可执行参数修补。Vega 对最终原始 JSON 和真实工具 schema 校验，错误不产生工具效果。
4. raw 思考需要转换为 assistant 文本块的已知兼容行为，仅可在对应 wire 发生；不升级成可见回答、持久化正文或日志。cold 历史、来源身份与凭据保护继续有效。明确服务的来源字段转换须绑定有效 profile，未知服务不得猜测/补造缺失 raw。
5. pi 的 Usage.input 扣除缓存，Vega 是含缓存总输入。通过明确的总量/缓存/输出会计不变量适配，不照抄数字造成二次扣减。目录总上下文与 Vega 可输入容量同样不能混用。
6. 上游假图片 fixture 改为真实可解码小图，保留数量、顺序、角色和像素来源断言。pi 静默移除不支持图片的成功投影不作为 Vega 成功；改为真实发送前阻塞断言并注明差异。
7. `ChatRequest.max_tokens = None` 保持已有提供方默认语义，wire 不凭空补目录最大值；Some(n) 保留已冻结的请求上限并选择对应 API/compat 字段。pi empty-tools 中默认 model.maxTokens 和按剩余上下文估算 clamp 的数值断言不是 Vega 同型行为；分别翻译为 None 省略、显式值保留以及现有容量预算链的独立校验。源元数据不越过已保存的 Vega 输入容量/输出预留权威，不能把目录 maxTokens 再当输入容量或悄悄覆盖用户设置。上游生成器的 raw metadata 转换与 Vega runtime 解析/组合也分开标注，不能声称运行了 pi generator。
8. Responses 的 ProviderDefault 省略整个 reasoning 对象；已知非思考模型也不请求 reasoning.encrypted_content。显式 Effort 才发送实际映射的 effort 与 summary:auto；Disabled 走真实 off wire，不能把默认选择改成关闭。已知思考/已有真实 raw 的加密回放请求仍按真实布局处理，不给非思考模型普遍添加只适用于思考的参数。Chat/Responses 对支持 strict 的工具分别明确 true/false；明确不支持 strict 时才省略，不让普通 MCP/function 工具被 API 默认升级 strict。
9. 源 SDK 支持但固定目录和上游测试未启用的 assistant/tool-result bridge，不在本轮为了覆盖数量新增伪造助手历史。现有空工具/真实历史仍要保持；原有 false fixture 不转成一个未经冻结的 true 正向功能。内部 string-thinking 方言若覆盖，单列为依据固定源 codec 的 Vega 补充测试，不能伪称源测试已存在。图片顺序使用两张可区分且真实解码的 fixture，对 typed wire 的逐项像素 data URL 断言；重复同一张图的计数不证明顺序。
10. 没有图片且正文为空的真实工具结果，在两 API 的 wire 中使用 `(no tool output)`，保留调用关联及顺序；实际结果/存储值仍为空，不生成虚假图片占位或工具效果。此与 pi `openai-responses-empty-tool-result` 原断言一致，也适用于含原生回放布局的消息。
11. 真实 length 终态仍为输出达到上限，不仅凭输入/缓存/输出用量启发式制造上下文超限。显式提供方上下文错误的代码/文本继续识别，普通限流和非上下文错误不能误触发；pi 的零输出/剩余上下文启发式不计为 Vega 已移植行为。

本轮为普通函数工具新增真实请求输入 `Option<ToolChoice>`：缺省省略，Some(Auto/None/Required/NamedFunction) 在两种 API 编码对应 wire；None 操作即使没有工具也编码 `tool_choice: "none"`，空工具字段仍按真实历史规则处理。named 必须指向已声明的 function 工具；Required 没有工具时本地拒绝，不伪造工具。上游 tool-choice:153 的无工具用例实际选择 none，并不是 required，按原断言正向移植。此输入不增加协议模板或用户设置，也不能只在测试中构造一个实际请求从不读取的字段。

图片阻塞的副作用边界按真实阶段断言：用户/历史图片不支持时不发请求；已被合法批准并执行的工具若产生不支持图片，阻止的是下一次 provider 请求，不能宣称撤回已经发生的合法工具效果。未完成/非法提议的零工具效果断言仍适用。

pi 的 prompt-cache/session-affinity、sampling/service-tier、thinking-token-budget、grammar/deferred-tool-search、任意请求 headers、SDK/账户 transport 和动态认证/扩展产品有各自真实输入，本轮不为移植数量虚构这些产品。逐用例记录其缺口或外部前提，不算已移植或已通过。现有普通工具、模型能力与请求链需要的 compat 字段不能因旧 projection 漏了就按上述范围排除；原生 Anthropic/Gemini/Bedrock 等 SDK 也不包含在当前两 API 通过结论中。

### 生产文件计划

| 层 | 文件/边界与动作 |
| --- | --- |
| runtime 模型层 | 新 `model_compat` 纯核心与固定两 API 的目录事实；保留完整等级/input/来源语义，组合精确覆盖，产生有效指纹；现有 chat_compat 作为兼容 codec 消费层，移除 Step 跨入口特例 |
| runtime 请求与响应 | `provider.rs`、`openai/{mod,chat,responses,sse}.rs` 和 `images.rs`：携带完整模型快照、ToolChoice、真实 wire/图片校验；按冻结用例修正终态、碎片、用量和消息转换，不改工具批准权威 |
| store | 新 `model_capabilities.rs` 及必要导出；独立 typed 文件、目标 CAS、reset、原子回读；现有 reasoning 偏好文件只处理本次选择 |
| conversation | `types` 中的模型能力投影与事件；真实 agent/摘要/标题/凭据检查入口携带同一模型事实，不把 raw 数据放进产品能力投影 |
| app | `reasoning_capabilities.rs`、`app_agent.rs`、window 模型读取/能力保存/reasoning/设置事件：后台读取、代际与身份 fence、保存回读后重算并失效旧 warm；容量和定价权威保持 |
| UI | settings 的能力草稿/思考多选/基本信息图片/保存恢复/键盘焦点；conversation_stream 的允许集合、本次选择与提交校验；控件和错误表达真实状态 |

不增加 Node/sidecar 运行时、业务 crate 依赖、测试调度或 CI 基础设施。开发 GPUI 测试若需现有上游 test-support，仅限同一锁定依赖的 dev 接线，并在 GREEN 后独立清除。主会话只修改规格与证据；实际测试和实现由专职子 agent 单写。

### 辅助请求的实际边界

M12 穷举当前构造点：runtime `agent/loop_.rs`、`provider_check.rs`；conversation `automatic_titles.rs`、`composer_predictions.rs`、`agent/compaction.rs`、`git_workspace/trusted_git/service.rs`；实际 Provider factory 是 app `app_agent.rs` 与 `window/context.rs`。`AgentRequest.model_snapshot` 和 `OpenAiProvider::with_model_snapshot(ResolvedModel)` 是供真实调用链使用的目标接口，不只是测试赋值字段。默认/无思考请求也要在真实 preflight/encode 前绑定同一有效事实。

Actions 可用有界同进程 127.0.0.1 临时 HTTP fixture 替代模型网络，驱动真实 Provider/encoder/parser 和实际辅助逻辑并检查真实结果、终态及必要存储效果。此类证据是网络替身，不能标真实服务或 E2E；不访问外网、不启动 sidecar/外部进程，不以观察 guard 返回错误制造正向成功。

零请求用例的网络替身需保持监听并观察非法接入，直到显式有界收尾；不能先关闭 listener，再用连接被拒绝与空接收数组冒充发送前拦截。观察真实 pre-attempt/encoder 和真实结果配合使用，正常路径继续委托真实网络实现。

Trusted Git 的完整 open_checklist/prepare/draft 正向需要真实 Git 权威链，按仓库限制不恢复外部 Git E2E，也不能伪造 authority。模型兼容自动测试覆盖生产实际使用的私有 draft 请求构造边界与 collect_draft_with_deadline、真实绑定模型的 Provider、wire 和返回语法；实际 commit_provider 的配置/模型绑定单独验证。外层 Git 权威/完整工作流只静态审查与用户手测，明确未自动执行，不计为全流程通过。不为此扩大 production public API。

## 实现与证据顺序

1. 固定上游来源，完成 AST 用例清单、适用性和差异审查；冻结全部本轮测试目标、预期和来源映射。
2. 专职子 agent 在上述未改生产基线上先写齐实际 Rust 行为测试与必要开发接线。主会话逐项审查，发布可访问源码/完整 SHA 和原始清单，通过新 draft PR 的现有 Actions 获取有效 RED。接口缺失与行为失败分别记录。
3. 在有效 RED 后才委派实现：统一能力解析与覆盖/store、设置与会话投影、图片前置校验、两个 API 的请求/响应兼容。先前已改的 v3 不冒充本轮 test-first 的生产未改基线。
4. 同一冻结基线完整 GREEN 才能声称本轮兼容目标通过。用例接线错误先留证审查，修正不得改变验收含义；新遗漏重新冻结新周期，不追改期望换绿。
5. 按用户答复固定 PR 中的源/补丁、摘要、RED/GREEN run/job 和准确计数后，再独立删 Rust 测试/专用支持，审查生产差异，并确认最终 head 完整门禁和 Rust 0发现/0执行、no Rust tests。
6. 最新 head 门禁成功后转正式 PR、按仓库流程 squash merge，再交用户手测。整套自动测试通过不冒充真实模型服务逐模型已通过，不以空集结果代表兼容成功。

### 首次 Actions 前冻结记录

专职子 agent 已停止测试写入；主会话完成来源、真实消费者、网络替身和开发接线静态审查。冻结 **133 个 Rust 定义 / 19 个测试文件**：runtime 77、conversation 23、store 11、UI 12、App 10。447 个源候选均有记录，其中 302 个定义有具体语义子集映射；含 helper 的 1275 条断言记录中 771 条有映射、504 条未认领或仅相关。不声称 447 整例移植、上游测试执行或任何行为断言通过。

[冻结测试清单](vega-issue-494-frozen-model-compat-tests.json)逐项保存测试名、文件/定义哈希、原子断言/表行、M1–M12 对应、真实消费者、接口契约、758 条独立数据来源及明确未认领边界。公开清单 SHA256 为 `59105217ffc4096445de39a03976ea00ad8d2f106f73df616294a9cddcfedef7`；其内容不依赖本机临时目录。测试源码与开发接线随同本次先行提交公开，完整提交 SHA、原始补丁及后续 run/job 链接由 draft PR 固定，不 force-push 丢失来源。

生产兼容行为、目录、用户文件仍未改。仅有私有 `draft_request` 的等价提取，以及 cfg(test)/dev-feature 编码观察、构造前临时路径注入、真实 GPUI 控件观察。锁定开发支持增加 10 个包，旧包版本/来源/checksum 和五个 crate 的生产依赖不变；静态审计随清单收录。本地 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --locked --format-version 1` 通过；未编译、未运行 Vega/Clippy/Rust/Python 测试。

下一步只提交测试与规格并通过现有 draft PR 取得 Actions RED。`model_compat`、能力 store/controller 和请求快照等真实目标接口当前缺失；若云端因此失败，只认定接口缺失 RED，并明确行为断言尚未执行。尚未取得有效 RED，主会话尚未授权任何生产实现。


### 首次云端检查与测试接线修正

先行提交 `80f4fe45b8f8f960d2f582e2b9768c729de64f3f` 的 [Actions 39360808726](https://github.com/puzige/vega/actions/runs/39360808726) 在测试目标编译阶段同时报告预定的真实目标接口缺失，以及测试自身的 rusqlite `FromSql<u64>` 错误。后者不是有效 RED；本轮未据此进入实现。Format 与 Python 5/15/14 通过，Rust 发现数未取得、执行 0，行为断言尚未执行。

主会话[先公开留证审查](https://github.com/puzige/vega/pull/504#issuecomment-6282232482)，再授权专职子 agent 只将三个测试 SQL 用量 tuple 的宿主类型改为受支持的 i64。仅三行类型变化，输入、真实调用、SQL、精确期望值及133个定义/属性均不变；不加测试、不改断言、不改生产或依赖。原始冻结清单与源映射保持原样，[独立接线更正清单](vega-issue-494-test-wiring-corrections.json)记录旧/新源码哈希、原始 job 与审查证明。修正后 fmt/diff/offline locked metadata 静态成功，生产仍暂停，下一次现有 Actions 须确认不夹带已知测试错误的接口 RED。
