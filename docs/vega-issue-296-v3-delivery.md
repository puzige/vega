# Issue #296 · v3 实施与交付记录

状态：实现与本卡组合定向验证完成，提交云端门禁；Issue流转与真实交付进度以[#296](https://github.com/puzige/vega/issues/296)为准。2026-10-11 用户批准按 [18点 spec](vega-issue-296-ui-redesign-v3.md)实施，用户负责最终验收。不是用户已验收的记录。

## Freeze

- 基线：#298已合并，#295底栏无分支修复已在基线。
- 当前spec的18点及追加Codex轻量活动行/工具详情参考均在范围内。
- 本轮沿用原生GPUI、当前依赖与共享token，不更改真实会话、权限或模型设置，不重开#291。
- 主agent负责设计裁决、审查、集成、CI/合并；专职实现agent各自隔离worktree。
- 后续精确事件/API契约在本文件补充冻结，先写契约再实现。未知final和分类数据按spec保守降级，不造数据。

## 测试用例（实现前）

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| A1 / V01 | 侧栏尾槽与footer | 运行/未读任务，短长标题 | hover/focus/菜单开闭 | footer48、动作槽52+外距8、标题不跳、状态动作替换 | GPUI几何/状态 | A、D2定向证据 | 自动验证通过，像素待交付 |
| A2 / V02,V12 | 锚点与窄窗 | 长历史/流式/折叠 | 点刻度、滚动、resize | ID定位正确、保留全目标、正文Composer同轴、无挡字 | GPUI交互/测量 | A、D2及最终组合证据 | 自动验证通过 |
| A3 / V06,V07,V15 | 阅读密度与工具详情 | 长思考/表格/代码/空输出 | 展开、滚动、选择、折叠 | 无整组灰底、仅详情有表面、可读限高、时长邻接 | GPUI/Markdown | A、D2定向证据 | 自动验证通过，像素待交付 |
| A4 / V11,V13,V14 | 共享视觉与既有能力 | 浅深/ReduceMotion/菜单/输入法 | 状态切换与既有动作 | 图标同族、焦点可辨、无branch恢复、既有权限选择与发送安全不变 | GPUI/生产行为 | A、C、D2及最终组合证据 | 自动验证通过 |
| B1 / V08 | 模型配置导航 | provider/model/孤立价格、脏表单 | 切页、编辑、保存取消/冲突 | 六项导航、所有能力可达、价格共享作用域明确、来源正确 | GPUI/controller | B定向证据与三项独立审查修复 | 自动验证通过 |
| C1 / V03,V04 | final阶段归属 | Native/ACP多工具轮、失败、无phase历史 | 事件回放、进入final、手动展开 | 真实信号驱动，过程仅默认收起一次，无信号保留全部有效正文 | runtime/conversation/GPUI | C后端、UI及最终组合证据 | 自动验证通过 |
| C2 / V05,V13 | run时间与shimmer | 运行/等待/后台/终态/历史缺值 | 阶段切换、恢复、终态 | 同run不重置、终态持久、缺值不造、等待/ReduceMotion停动画 | 时钟状态/GPUI | C后端、UI及最终组合证据 | 自动验证通过 |
| D1 / V09 | 目录匹配与覆盖 | 精确/歧义/离线/坏数据/自定义 | 获取缓存、选择映射、刷新、应用默认 | 只补齐metadata、明确来源、不覆盖自定义、不触发模型调用 | fixture/service/controller | D1定向证据及失败恢复复审 | 自动验证通过 |
| D2 / V10 | 当前上下文与分类 | 配置缺失、估算、超限、模型/版本变更 | 打开浮层、更新projection | 分母口径不偷换、缺值不为0、有分类才分段并对账 | accounting/GPUI | D2定向证据 | 按批准总量降级自动验证通过 |

## 实施计划与所有权

1. A共享视觉/阅读：conversation_stream、sidebar、Markdown渲染、tool card/group、theme/icons。A为共享theme唯一编辑者。先不改run阶段/底层事件；为C保留清晰接入点。
2. B设置结构：settings目录与其application wiring；复用旧controller，不改共享theme/icons，所需图标/token由A添加。目录接入UI在B完成后串行接入。
3. C阶段与时钟：先只读拟定typed run/segment/final和持久化方案，主agent冻结后改runtime/conversation/ACP adapter；stream接入等A结束再由单一agent完成。
4. D目录与上下文：C/B接口稳定后实现目录数据源、匹配/缓存/覆盖及浮层，分类缺口明确补齐或按spec降级。输入上限B、输出预留O、runtime(B+O)-O口径保留。
5. 定向测试及独立审查后，集成同一#296 PR，云端全量门禁绿后主动合并，正式交付供用户验收。不要把任何子片完成当成本轮全18点完成。

### C 阶段/时间接口冻结（2026-10-11）

- Native 的 `assistant_message_id` 为 run 身份，primary `attempt_id` 为 response 身份。每轮 provider stream 结束发出 typed `AssistantResponseCompleted`，包含 response 身份、原文 UTF-8 字节起止、`Commentary | Final`。
- 只有明确 `StopReason::End`、没有待调用工具且正文非空才标 Final；有工具轮标 Commentary。缺少 stop、Length、TurnLimit、失败和取消均不推断 Final。当前协议不提供可信 final 首 token 标记，因此按主 spec 的保守路径，在明确完成时执行默认收起。
- Conversation 使用共享 response metadata 类型和事件；只增 response span 表，history 恢复同一 run/response 与边界。原始正文、工具 offset、事件次序保持不变；缺元数据的历史保留全部正文。无效/越界/非 UTF-8 边界元数据不得导致丢失文本。
- ACP 当前无已确认的正文 channel 合约，保留全部助手正文，不把 prompt End 当作 final。将已确认的 thought chunk 独立转译为 ThinkingDelta。
- Native/ACP 的 `RunStarted` 向 UI 传递同一起点 Instant，持续时长包含服务端、工具和用户等待。ACP 成功/失败/取消统一携带 elapsed 毫秒，复用现有终态 duration 表。进程重启后未知终态不推造时长，不新增续跑功能。
- UI 接入由 C 在 A 交付后串行进行；完成收起不得干扰正在选取、回看、聚焦或手动展开的内容，权限/提问/错误继续可见。响应元数据不得直接驱动不受校验的字符串切片。
- 必测：多工具轮与 final；缺 stop/Length/失败/取消；UTF-8 span、历史重播与旧数据；ACP 三条终态路径 elapsed 与 thought/正文隔离；UI 后续生产测试覆盖一次收起和用户阅读优先。
- 独立审查补充：不能把旧SSE映射归一后的End当作可信来源。`content_filter`及未知wire结束原因必须保留非正常/未知身份，只有已确认正常stop或Responses completed允许Final。可增Unknown/Other枚举保持旧正文可读终态，但不得产生Final或可信usage anchor；新增SSE assembler→runtime合成回归。ACP计时除helper测试外，扩充现有进程内duplex runner的IPC失败、取消/权限等待路径断言。

### D 数据与展示接口冻结（2026-10-11）

- U09本轮按主spec总量级交付，使用现有完整owner/model投影的输入估算与保存输入上限；不增加未具备账本依据的分类比例。环28/16、点击保持的320宽浮层、显式估算/配置输入上限、超限饱和和未知状态必须落地。未知估算仍有入口，显示尚无数据；不把未知画成0%。Esc/外点回焦有生产测试。
- U08使用公开 `https://models.dev/api.json` 的provider offering和明确canonical身份；官方schema的可选字段原样区分不存在、false、0。只读公开目录，不上传任何本地模型、配置、端点或用户内容。网络适配层放已有reqwest的app crate，共享安全投影走conversation/types；无新依赖。
- 缓存24小时，相关设置首次进入/过期才后台读，手动刷新显式触发；固定HTTPS主机、请求超时、响应体有界，完整校验后原子更新最后有效缓存。获取失败仍保留有效缓存和用户草稿；过期/失败状态可见，无缓存可手填。
- provider/model精确匹配仅限该provider与目录身份已确认；自定义Provider名称不能凭显示名冒充官方Provider。其他情况进入显式映射选择，映射键为本地(provider,model)，目标为目录(provider,model)或明确canonical；不使用短名/大小写/子串猜测。
- 目录默认层独立于手动配置，记录匹配身份、字段来源、获取时间、schema版本和内容摘要。模型详情自动展示已匹配资料，用户可选择应用支持的字段；已存在的配置/用户草稿优先，刷新不执行配置写入。恢复默认是明确动作，不自动新增/启用/测试模型。
- 价格保持现有全局model作用域；导入前明确共享影响，且经过原有验证/冲突/原子保存路径。canonical能力不携带中转价格。只导入本产品可表达且语义一致的价格字段；阶梯等不能完整映射时显示资料并保留手工配置，不降格为统一单价。
- `limit.context`、`limit.input`、`limit.output`分别保留。只有显式input可作为输入上限候选；context不转换成input，output不从已有input再次扣除。未提供input时仅展示总容量资料。
- reasoning支持/effort等作为资料和可用默认候选；目录flag不能自动确定wire协议、关闭操作和工具回传；不改用户默认档位或已冻结run。仅在用户已有明确兼容协议且映射完整时通过原控制器应用支持的档位，其他情况维持资料展示与手动编辑。
- 目录异步投影携带选择身份和generation，关闭/切换Provider/模型后旧结果不得覆盖新页面。缓存损坏、超限、超时、未知字段、缺字段、歧义、离线、有手动覆盖、共享价格、过期generation均纳入fixture/controller测试。
- 本机Python urllib首轮读取返回HTTP403；随后curl固定官方URL获得200，5,389,868字节，226个provider/8,468个model。该只读schema核验不等于app网络适配已通过。样本SHA256为`6a4f51406db6365ff31d74a5986892ee37931d2d9cfd3a94155c412ac85824eb`；实现采用合成fixture与固定16MiB响应上限，合理Vega User-Agent，保留失败/缓存路径。部分model没有cost，input字段可缺，非聊天模型可有0上限，不得一律解析成默认配置。
- D1落地细化：目录缓存及显式映射位于当前config_path父目录的`model-catalog/`，不引入store schema；缓存记录schema/获取时间/SHA256并原子替换，映射与配置独立。连接10秒/总30秒超时，禁止跨域重定向。模型资料折叠区显示当前offering身份与时间，输入/输出默认值和可表达的单层text价格只填入现有草稿，仍须原保存动作。思考沿用原即时保存控制器，以明确“应用目录档位”操作仅改变完整可映射的efforts；原preference不在新集合时禁用应用，协议/关闭/回传/偏好不自动变更。复杂定价与缺失字段只作资料，不补0、不借用其他offering价格。

## Results

### A 阅读与共享视觉定向证据

- 实现提交：共享`8bb549ccfc0e140829cd5834b6f6cd33ecda76cf`及阅读`07782cb858130dba92a558728480342301dc4b53`，已由主agent集成。上下文浮层及运行阶段展示另批实施。
- 命令：`cargo nextest run -p vega_ui -p vega_theme -E 'test(issue296) or test(issue72) or test(thinking) or test(sidebar::threads_block::organization::tests) or test(issue70_e70_long_bash) or test(model_markdown) or test(issue147) or package(vega_theme)'`，退出0。
- 原始footer：`Summary [6.411s] 234 tests run: 234 passed, 409 skipped`；日志SHA256 `7dada09aae4dcbca7d54a31bee0ca8ef8260bf1f6815d05d74a2b5257090c6ed`，主agent已复核。筛选包含部分既有设置回归，不与B数量相加宣称唯一测试总数。
- 覆盖：紧凑footer、状态与动作替换、独立锚点窗口、无整组灰底、相邻时长、箭头按钮、长思考回看守位、Markdown混排字号行高/相邻间距取max/列表缩进/代码内距横滚/表格/跨块复制。
- 首轮定向检出隐藏菜单宽度0、resize旧高度、思考自动跳尾；新增长代码测试检出父宽限制横滚，修复后通过。仅更新被本spec明确替代的灰底/按钮/锚点视觉期望，保留业务与权限断言。
- SVG资产浅深样本为生产资产的独立排版检查，不是GPUI framebuffer；真实App像素另行验收。主审要求当前锚点按真实logical row身份选择，避免估算高度中点导致长消息提前切换；后续D2提交已修复并验证。

### C 后端定向证据

- 后端实现提交：`7c1f25d54478147c8b5c602ef4ea65ba94343331`，2026-10-11。以下为后端阶段证据，UI及ACP后续证据另见C信息流小节。
- 命令：`cargo nextest run -p vega_runtime -p vega_conversation -p vega_store -E 'test(issue296) or test(text_only_preserves_events_usage_and_visible_content) or test(issue146) or test(fast_text_deltas_coalesce_and_each_displayed_delta_is_durable) or test(migrate_creates_exactly) or test(migrated_store_is_wal) or test(schema_has_twenty_nine) or test(two_call_tool_journey)'`
- 退出0，原始footer：`Summary [0.126s] 15 tests run: 15 passed, 981 skipped`。skipped来自定向筛选，不代表全量运行。
- 原始日志SHA256：`12e8f8376db62e85124b17d1a2d655b331e310f694a4e8c2d29fe5ff530e79dd`。
- 覆盖：明确End/缺stop/Length/空白/失败/取消的Final规则，UTF-8边界，元数据重播/无效输入/旧记录保守行为，生产stream→持久→history一致，原16ms批处理与权限等待持续计时，17版/29表schema，现有费用链。
- 首次编译退出101：旧history分页测试未处理新增枚举；补足分支后定向通过，未放宽原业务断言。
- 此后端阶段未运行ACP新测试及UI折叠/时钟/shimmer，后续已在C信息流小节补齐。ACP thought与Native原始思考一样仅实时呈现，本轮未新增思考历史持久化。
- 独立审查后的修复提交：`a6bd1dcf7baa150b1a8c03804fbb9a1a02a8eb23`。先以生产SSE assembler→runtime回放复现`content_filter`误标Final（1 passed / 1 failed）；新增Unknown保留未知终止，不产生Final/可信usage anchor，明确stop与Responses completed仍可生成Final。
- 修后命令：`cargo nextest run -p vega_runtime -p vega_conversation -E 'test(issue296) or test(finish_reason_maps_to_minimal_stop_reasons) or test(i71_responses_summary_parts_snapshots_and_refusal) or test(issue91_valid_primary_usage) or test(summary)'`，退出0，`Summary [0.976s] 62 tests run: 62 passed, 780 skipped`，日志SHA256 `95147cbf8ab03cb41cbc290325422af35e8a29d06b0ee64fc37f05352fdaf1e0`。
- 补充Responses命令：`cargo nextest run -p vega_runtime -E 'test(issue296_responses_completed_snapshot)'`，退出0，`Summary [0.015s] 1 test run: 1 passed, 270 skipped`，日志SHA256 `7d31c077c1474bcf0625b8006900b0c2551648f448c64ec8b61e3c1610d374ae`。主agent已复核以上两个footer/hash。ACP duplex新增用例不计入这些后端测试数量，后续独立验证见C信息流小节。

### B 设置结构定向证据

- 实现提交：`a9cf38f6c21d19a1e44590646efb73ca7f1eea2e`，13个settings文件；目录数据接入另批实施。
- 命令：`cargo nextest run -p vega_ui -E 'test(settings::)'`，退出0。
- 原始footer：`Summary [5.291s] 96 tests run: 96 passed, 523 skipped`；日志SHA256 `c6118fee1ef346b4dd3a2085fedb5d9259ed8c49e7ecbbcdeacd487dc9f1246a`，主agent已复核。
- 包含六项导航、Provider主从/窄屏、对象菜单、模型三个配置页、共享及孤立价格、脏草稿保存取消/离开保护、帮助点击保持与Esc焦点。首次90项有6项旧导航/菜单路径假设失败，保留业务断言改用新入口；新增菜单Esc测试抓到无全局binding时handler不一致，修复后通过。
- NOT RUN：真实安装后的像素与系统集成，不能把GPUI测试当作像素验收。
- 独立审查发现并修复三项P2：当前协议重复点击重置配置、模型删除确认跨Provider串对象、容量保存未ACK时仍允许放弃。修复提交`7d3c3959d7f52521a8da800fd0a48087279013ba`绑定完整删除身份并执行时复核、同值选择无操作、统一容量保存busy与ACK后导航。
- 修复后命令`cargo nextest run -p vega_ui -E 'test(settings::)'`退出0：`Summary [4.779s] 99 tests run: 99 passed, 523 skipped`，日志SHA256 `3e0e9bf99ed520a175c45e43de9c1c9910fdd288e73cfb56b2136bb0c992973b`。补全离开对象清理后的最终命令`cargo nextest run -p vega_ui -E 'test(issue296_v3_)'`退出0：`Summary [0.473s] 10 tests run: 10 passed, 612 skipped`，日志SHA256 `2f22feb4043118be0c42bf09864a830248067c90bbd76552d095e42d7062375e`。主agent已复核两个footer/hash。
- 独立复审确认上述三项已闭环；C未知wire stop修复同样完成独立复审，无新增P1/P2。

### D2 总量浮层与阅读边界定向证据

- 实现提交`06f315afad232c0154b85e9b30ce02c2d7fe738d`，主集成`7bd8394`；28静态环/320总量浮层、点击保持、未知/超限、Esc/外点真实回焦，预算与数据契约未改。分类数据按已批准总量降级，不伪造分类比例。
- 命令：`cargo nextest run -p vega_ui -p vega_theme -E 'test(issue64_context_usage) or test(context_control) or test(composer_actions) or test(r68_popup_dismiss) or test(issue296) or test(issue72) or test(thinking) or test(sidebar::threads_block::organization::tests) or test(issue70_e70_long_bash) or test(model_markdown) or test(issue147) or package(vega_theme)'`。
- 退出0，原始footer：`Summary [6.369s] 278 tests run: 278 passed, 368 skipped`；日志SHA256 `8728417675bc73fd6ac5ce226def7a4209d88c34caccd9c3a5e08f8a4e65ee7b`，主agent已复核。此轮包含A测试，不与A数字相加。
- 主审/独立审查两项阅读修复：当前锚点以logical row稳定ID选取；运行思考短内容首次超过240高继续追尾，用户回看保持offset，初次展开历史不被误带到底部。生产几何回归均通过。
- 首轮浮层交互测试检出点击capture阻断pin及外点回焦后继续冒泡，两者修复；旧常驻容量说明期望按新help契约更新。保留所有初次失败日志及原因，未以重试掩盖失败。
- 独立复审确认总量数据分母/unknown/超限、浮层真实焦点、锚点身份与首次溢出跟随修复闭环，无新增P1/P2。

### D1 模型目录定向证据

- 实现提交`3a9caf08c54b2ff4e15e13858e8e31acf142b752`，主集成`8dc4846`；固定公开目录、24小时有界缓存、独立身份映射和显式应用，不自动写配置或触发模型调用。
- 命令：`cargo nextest run -p vega -p vega_ui -E 'test(settings::) or test(issue296_catalog)'`，退出0。
- 原始footer：`Summary [6.377s] 114 tests run: 114 passed, 763 skipped`；日志SHA256 `c29a3c43165f47027de4f08eda8a45842f46c57d3ac9063b699bba4588d56275`，主agent已复核。包含旧设置回归，不与B数字相加。
- 覆盖真实Settings点击→草稿→原保存worker→隔离DB、刷新不写配置、失效身份/generation丢弃、同名模型映射隔离、失败保留缓存、复杂价格和不兼容思考选项不冒充已应用。公开目录样本通过生产Rust parser离线探针；探针和样本未入库，不宣称真实网络E2E通过。
- 独立审查发现思考目录值从基本信息页保存失败后，错误隐藏且离开保护错误重试容量保存；交由原实现agent修复并补失败→重试→导航交互证据。此项未修复复审前D1不标通过。
- 修复提交`e3609342f66c17473009da64f3844f9328b51329`，主集成`6662e13`：目录思考操作先进入所属思考页，再走原即时保存控制器；失败留在所属页、离开保护重试正确服务、成功ACK后恢复原导航目的地，其它草稿优先保护。schema/hash与纯解释收进帮助详情，常态保留来源、获取时间与必要共享价格提示。
- 修后命令`cargo nextest run -p vega_ui -E 'test(settings::)'`退出0：`Summary [7.455s] 107 tests run: 107 passed, 523 skipped`；日志SHA256 `a6aca19918fa696ffde8fabefc683aee896575ae397e4b2f96962b6de7bcb5b8`，主agent已复核。新增GPUI失败→导航保护→重试（没有容量保存事件）→ACK后原定价目的地、其它草稿保护、帮助Esc回焦测试。独立复审确认原P2关闭，无新增P1/P2。

### C 信息流阶段、时间与动效定向证据

- UI实现提交`cb1df9e`，主集成`26bdae6`。typed Final校验后一次收起过程，未知/无效元数据保留正文；手动展开、回看、选择与focus优先；锚点重开过程。思考和工具详情默认折叠，Working for沿同一起点计时，终态使用持久时长；仅当前思考摘要/Thinking有1.8秒文字扫光，失活/不可见/Reduce Motion/等待/终态停动画。
- 命令：`cargo nextest run -p vega_ui -p vega -E 'test(issue296_phase) or test(issue296_run_clock) or test(issue296_acp) or test(issue287_first_codex_prompt) or test(issue287_codex_tool_permission_and_lifecycle) or test(issue287_stop_during_permission) or test(issue146) or test(issue151) or test(i61) or test(issue103_thinking) or test(conversation_stream::tests::thinking::i71) or test(issue296_v3_thinking) or test(conversation_stream::tests::issue70) or test(conversation_stream::tests::permissions_cards) or test(conversation_stream::tests::hydration) or test(conversation_stream::tests::issue72)'`。
- 固定UI提交运行退出0：`Summary [1.002s] 93 tests run: 93 passed, 792 skipped`，日志SHA256 `8f6d3636f60cad98a25e7b5e0b9398bdd926a6e8a48b1e72ff79c679b95aa597`。加入D1两提交后同一命令退出0：`Summary [1.691s] 93 tests run: 93 passed, 805 skipped`，日志SHA256 `ee702ec828d871774728167a7019afbbb4e0c41890eacd8445022270e16ffc57`。主agent复核footer/hash，并核对owned HEAD `9130c22`与主集成crate树完全一致。
- 包含ACP进程内duplex的成功、权限等待取消、IPC EOF失败与持久elapsed；无真实模型/外部进程验收。生产GPUI验证历史元数据与UTF-8/冲突降级、Final一次收起/阅读保护、失败工具及权限常显、时钟不重置、实际paint文字颜色扫光及停调度。
- 首次编译处理`ToolCard::is_expanded`从test-only变为生产读取；旧自动展开测试改为显式展开继续验证时长、滚动、脱敏和fail-closed，未删除业务断言。controller Git fixture只改与重命名测试一致的key，捕获序列不变。
- 独立审查检出并修复并行工具事件错误解除等待状态；等待改独立于实际阶段的状态，待确认时普通Running/Finished/正文/摘要不恢复shimmer，真正决议后才恢复最近阶段。新增真实权限决议测试，固定提交独立复审无新增P1/P2。组合验证另发现计时canvas占布局5px，见下节修复记录。

不得沿用#298测试结果冒充本轮通过；分片绿不替代以下组合与云端门禁。

### 首次组合定向验证

- 全部生产改动集成到`26bdae6a87309b4678327f56528adc5b6d10323a`后扩大本卡定向；无依赖或工作流改动，无新增代码注释。与C合并D1后的crate树一致。
- 命令：`cargo nextest run -p vega -p vega_ui -p vega_theme -E 'test(issue296) or test(settings::) or test(issue64_context_usage) or test(context_control) or test(composer_actions) or test(r68_popup_dismiss) or test(issue72) or test(thinking) or test(issue70) or test(issue147) or test(issue146) or test(issue151) or test(sidebar::threads_block::organization::tests) or test(issue287_first_codex_prompt) or test(issue287_codex_tool_permission_and_lifecycle) or test(issue287_stop_during_permission) or package(vega_theme)'`。
- 退出100，原始footer：`Summary [8.575s] 437 tests run: 416 passed, 21 failed, 488 skipped`；日志SHA256 `41406e61a3a2d083f631e525fe2181f643cba24e954b4f8a368678ed4100fe8d`。这是失败证据，不以已有分片绿替代。
- 按所有权分派：B三个应用设置helper入口；C六个旧展开/几何/失败状态测试；阅读agent十二个unknown入口、设置往返浮层、代码复制滚动及loaded锚点。先区分旧展示契约与真实回归，保留原业务/安全/焦点/复制断言，修后重跑相同组合。
- B修复`a080cc8`（主集成`e42a883`）仅更新应用测试helper到独立模型页viewport、等待真实ready、使用compact容量投影；无生产改动。三个原用例定向退出0：`Summary [2.311s] 3 tests run: 3 passed, 248 skipped`，日志SHA256 `7f487c4857e61b504dced9c8cc9f1d848ad85d39a9bdef4fb335d96cc966cac2`；持久化/草稿/非法输入/旧ACK/零provider调用断言保留。
- C修复`db5ac773`（主集成`2558358`）：真实clock canvas占布局5px问题改为标题内部绝对定位，实际绘制与锚点估高共用是否显示Thinking的判断（28/60px）；旧测试显式点击展开后继续验证交互，失败过程维持可见。六个失败加phase/clock受影响用例定向退出0：`Summary [0.236s] 16 tests run: 16 passed, 631 skipped`，日志SHA256 `09288f854c682c2994c9d93f58a1b32cb1b44b368cf1e5148ea0038b21bbeca1`。
- 上述B/C修复主agent已复核原始footer/hash，独立复审无新增P1/P2。阅读组与最终组合待完成。
- 阅读修复`a11d8eae`（主集成`325a798`）：修复Settings回返后context dismissed未清理，保留Esc/外点关闭；unknown入口、48行可实际滚动复制夹具、stable ID刻度与邻近预览按新契约验证，完整复制/选区/一次定位请求断言保留。命令`cargo nextest run -p vega -p vega_ui -E 'test(issue64_production_root_tab_reaches_context_ring_after_attachment) or test(issue64_context_settings_) or test(issue64_context_usage) or test(issue296_v3_context_popover) or test(issue147) or test(issue72)'`退出0，`Summary [36.761s] 52 tests run: 52 passed, 846 skipped`，日志SHA256 `81d26580465d1f4baa329d8a64c0b3037235f19cdf8d6dae3e80049126454922`。精确首轮12/12失败证据保留。主agent核验且独立复审无新增P1/P2。
- 最终主集成在`325a798`重跑上方同一组合命令，退出0：`Summary [41.603s] 437 tests run: 437 passed, 488 skipped`；日志SHA256 `036f319f67eb6fbd14d2901c16af76a9e7a5169e4c23190783cd5b1fe7708fff`。21项组合失败全部闭环，未加ignore、删除业务/安全断言或修改重试配置。
- 本地只做本卡定向，workspace全量fmt/Clippy/nextest留云端。与批准spec偏离：无；未知Final保留正文、上下文仅总量、复杂目录字段资料展示均为已批准的保守路径。

## 回滚与兼容

UI通过revert本轮合并回退；新增元数据缓存不覆盖用户配置；如需要新持久化字段，schema只增不删并保持旧记录可读。活动展示重排不改原始事件顺序或历史内容。不同提供方独立价格不在UI重排中暗改；先保留共享价格语义。

## Residuals

- 本卡组合437项已通过并完成独立审查；云端门禁、正式发布、安装身份与真实界面检查由后续Issue/PR交付记录跟踪，不以本地通过冒充云端或真实模型验收。
- 用户主观视觉与真实系统手测待最终交付后完成；Issue保持OPEN直到用户认可。
