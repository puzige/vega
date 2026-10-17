# Issue #300 — 通用工具接口与模型历史隔离

2026-10-17 后续契约：[Issue #435 自动推理回传](vega-issue-435-automatic-reasoning-replay.md) 仅将下文 live 原始参数的 run 内存寿命扩展为本应用内有界、经完整身份校验的 warm 会话内存，以支持跨用户轮协议回放；不新增原文落盘/日志，不改变审计、权限、幂等或凭据围栏。cold/压缩后的历史观察和 source 比较按该卡明确边界接线，不能借缓存恢复丢失的可执行参数。

## 授权、证据和范围

2026-10-16 用户选择以 pi 为参考的一套跨模型通用 JSON 工具接口，并明确要求「你直接基于这个方案 开始 修 + 改」。本卡由 Codex 组织专用实现子 agent，主 agent 负责规格、审查、集成与 CI。基线为 `8c69ef64`，分支为 `feat/300-universal-tools`。

已确认的代码链路：`PreparedRuntimeCall::model_call` 除 computer-use 外复用审计 `input_json`；Write/Edit、无效 bash、MCP 和部分 skill 参数已在准备阶段变成审计摘要。`agent/loop_.rs` 将其加入后续模型请求；`agent/pipeline.rs` 重开时再次将审计作为参数回放。实际请求与后续模型输出也出现 `audit_version/content_bytes/fingerprint_v1` 等字段，而非声明的参数。不能把全部失败归因于此，也不能在没有对照测试时承诺成功率提升百分比。

Vega 已发送工具 schema 和 strict 标记。当前代理移除 strict 的行为另由代理卡处理；本卡不修改用户配置、代理服务或默认模型。通用表示支持工具调用的模型使用相同业务工具契约，不表示所有服务商都有严格生成保证。

本规格替代 #90 对 `command` 的否决和 #112/#85 的模型可见字段、工具名与可选项表达。其权限、凭据保护、检查点、审计脱敏、历史兼容、文件版本和幂等安全要求继续适用。旧行为测试仅在本规格明确变更的接口/反馈/投影处更新，安全断言保留。

## 验收矩阵（先于实现计划）

全部自动执行均为 GitHub Actions；本地仅静态阅读、差异和格式检查。

| 用例 | 前置与操作 | 可观察结果 | 层级与证据 | 初始状态 |
|---|---|---|---|---|
| A | 支持工具调用的不同模型/两种 OpenAI 协议请求工具定义 | 核心名称、字段、语义相同；示例符合实际 schema；无按模型替换编辑工具 | registry 与 provider wire 回归 | 待 CI |
| B | 执行 write/edit/MCP/skill，随后发起下一轮 | 模型收到本次真实可执行参数或明确的历史结果，绝不把审计字段放在可执行参数位置；落库、事件、Debug 仍不保存敏感原文 | MockProvider 完整 loop 与事件回归 | 待 CI |
| C | 无效参数含敏感值、未知字段、audit 对象 | 审批与执行前拒绝；短错误含固定原因和正确 shape，不回显原始值/任意字段名；可纠正后仅执行一次 | parser、runtime、安全投影 | 待 CI |
| D | 重开旧/新 Write/Edit、无效 bash、MCP、skill 审计历史及混合调用组 | 审计不可伪装成函数参数；结果、先后顺序及真实状态保留；无孤立 tool result；不从当前文件猜恢复原始参数 | store → history → provider 回归 | 待 CI |
| E | 长回合工具轮次后自动/手动压缩；steering 后 completed group | 双投影差异不误报 SourceChanged；用户/工具结果/审计身份篡改仍拒绝；call-id、覆盖边界、CAS 保持有效 | conversation compaction 回归 | 待 CI |
| F | 相对/绝对路径 read 后 write/edit，项目外文件与 symlink | 同一目标身份；现有权限、凭据、Git/checkpoint 防护和 read-before-write 保留；bash 的 cd 不改工具根 | tools/runtime 进程内回归 | 待 CI |
| G | edit 多项作用于同一文件原文 | 唯一且不重叠时一次审批、检查点与原子写入；结果计数准确，原有编码/BOM/换行保持 | mutation 文件回归 | 待 CI |
| H | 批量一项无匹配/歧义/重叠/无变化，或审批后文件变化 | 零部分写入；重新 read/增加原文上下文等固定指导；不猜改文本、不逐项循环写文件 | mutation 失败与竞争回归 | 待 CI |
| I | 旧 Read/Write/Edit、file_path、旧单编辑形状、cmd/timeout_ms | 有限迁移可调用/回放；新旧重复或混合字段拒绝；旧审计/检查点可读；同 ID 同身份复用，异身份冲突 | codec/runtime/store/UI | 待 CI |
| J | timeout 省略/null/正整数秒及 legacy 毫秒；类型/范围/单位冲突 | 默认仍 120 秒；转换检查溢出，正整数秒转毫秒；无效输入零执行 | bash parser + injected executor | 待 CI |
| K | grep 使用项目内相对或绝对目标；越界/symlink/Git/凭据 | 项目内绝对目标可归一化；外部范围不放开；glob 说明仍为项目相对 pattern | 搜索边界回归 | 待 CI |
| L | 新/旧成功、拒绝和失败工具卡恢复 | 正确工具、状态和可纠正摘要；篡改仍拒绝；原文不因新字段暴露在卡片 | conversation projection/UI 回归 | 待 CI |

## 统一接口

所有支持工具调用的模型看到同一套核心函数；现有按权限/运行模式可见性不变。保留 grep/glob、skills、MCP、computer-use 的能力，不把它们强行改成文件工具。

| 名称 | 业务参数 | 语义 |
|---|---|---|
| `read` | `path: string, offset?: integer, limit?: integer` | 路径相对项目根或绝对；offset 从 1 起，limit 保留已有范围语义 |
| `write` | `path: string, content: string` | 新文件或完整重写；已有文件须先 read，保留权限与版本检查 |
| `edit` | `path: string, edits: [{oldText: string, newText: string}]` | 非空数组，每项同一原文中的唯一、不重叠替换，empty newText 表示删除 |
| `bash` | `command: string, timeout?: integer` | timeout 单位秒，正整数；省略/null 用既有 120 秒默认；命令非空 |

1. 模型可见名称仅小写 read/write/edit/bash。旧名称和字段作为显式迁移入口接受，不能再给模型并列公布两套定义。
2. 逻辑可选项在 strict wire 采用 nullable 且 required 的表示；所有对象 `additionalProperties:false`，包括 edits 项。描述明确 null 的默认语义，完整示例必须匹配对应 wire。非 strict 调用省略仍按默认处理。不要把“strict:true”当作本地校验的替代。
3. 合法例子：`read({"path":"src/lib.rs","offset":null,"limit":null})`；`write({"path":"notes.txt","content":"hello\n"})`；`edit({"path":"src/lib.rs","edits":[{"oldText":"old","newText":"new"}]})`；`bash({"command":"pwd","timeout":null})`。
4. 工具定义集中维护 schema、参数说明和固定纠正示例；生成请求、token 估算、压缩上下文使用同一权威定义。同步运行提示中的旧参数指导，避免与 schema 相互矛盾。模型差异仅由现有 provider transport 处理，不加入模型白名单和模型专用编辑接口。
5. 相对文件路径统一以当前工具实例的项目根解析；外部绝对文件沿用 #112 权限。grep 可接受项目内绝对目标并归一化后再沿用围栏；不会扩大为外部搜索。glob 的 pattern 明确仍相对项目根。bash 内 cd 不影响别的调用。

## 校验、迁移和反馈

迁移仅接受有确定含义的旧形状：Read/Write/Edit 名称、file_path、旧单项 old_string/new_string/replace_all、bash 的 cmd/timeout_ms。legacy path 继续兼容。新旧字段同时出现、canonical edits 与旧单项混用、command 与 cmd 混用、秒与毫秒混用一律拒绝，不挑一个字段执行。旧 replace_all 和 empty-old 的行为只在旧形状保留；canonical edits 每项要求非空 oldText 和唯一匹配。

不自动执行猜测的 command，不修补截断 JSON，不把任意字符串/布尔/null强转为数字，不把缺失 content/oldText 从文件或摘要里补出来。规范化只做一次，执行只走一次原有授权/检查点路径。空 edit 数组、未知字段、错误类型、溢出和不合法 JSON 均在执行前拒绝。

严格输入的验收包括 bash 新旧字段的重复键，以及 write/edit 顶层和 edits 项内的重复键；不能经普通 JSON map 解析后悄悄保留最后一个值。此类拒绝仍使用固定错误，不回显键值，不进入审批、检查点或执行。

bash 增加有界原因枚举：malformed_json、input_not_object、missing_command、wrong_command_type、empty_command、unexpected_field、conflicting_fields、invalid_timeout。Edit 增加批量数组/项/重叠相关分类。反馈只用固定字段名、原因和对应工具的合法示例；匹配失败提示 read 最新原文，重复匹配提示增加上下文，无变化提示检查是否已经完成。旧持久化反馈和旧审计的严格恢复仍有效。

沿用 run/attempt/tool 的现有诊断事件记录固定原因及执行/拒绝状态，不增加原文日志和第二套遥测服务。准确区分提议、校验拒绝、实际执行失败、执行结果未知。跨回合的无进展预算与强制收尾属于 #411，不在本卡提前实现。

## 审计与模型历史

2026-10-17 [#434 历史工具观察规格](vega-issue-434-historical-tool-observations.md)取代本节中把历史工具拼成 Assistant 正文及提前生成 primary 历史文本的具体表示：内部保留 stored 标记的 rich 审计身份/真实状态，只在模型边界生成独立、无指令权威的 User JSON 观察。live 参数侧车、整组配对、脱敏、幂等与 source/CAS 保护继续有效；不恢复缺失参数，不清洗用户/模型原文。该修订尚待实现与 CI，不宣称已生效。

模型协议历史和持久化审计必须分离。`RuntimeToolCall`、事件、数据库、卡片、幂等账本继续使用已有脱敏审计。有效调用的真实业务参数只在本次运行内存中保留，沿用凭据检查；不得新增原文持久化字段、原文日志或诊断。

在模型请求投影中，live 有合法完整参数的调用使用真实参数与配对结果；仅有审计或被安全策略移除参数的调用变成带工具名、call id、真实结果的明确历史文本，不再发成 function call。不能伪造 placeholder content 让其表面符合 schema。无效参数同样不回显可能包含凭据的 raw JSON，保留固定失败指导。MCP、skill 等也遵守这一规则，不能只修 Write/Edit。

投影作用于完整调用组，保留结果顺序和 pairing；混合 live/审计组可以整体转历史文本以避免孤立结果。历史文本明确是过去工具活动，不提升为 system 指令。保留现有输出截断、图片/CUA 临时字段脱敏与安全过滤。用户内容、普通工具输出中的同名字符串不得被正则误删。

压缩、一致性检查和幂等使用可信内部身份，不能用模型可见格式相等替代审计身份，也不能简单移除 guard。具体采用 `ChatMessage` 私有内存参数侧车：内部 tool_calls 保持审计，侧车仅绑定相同 id/name 的 live 调用且不进入 Debug/事件/数据库；没有原文的调用不创建侧车。所有 primary/summary provider 请求和 wire token 估算使用同一个模型投影。投影检查绑定，发现不一致失败关闭，不能把未验证侧车应用到别的调用。

压缩 guard 对比前仅去除内存参数侧车，继续校验原 call id、工具身份、审计指纹、结果和顺序；覆盖边界、完整分组及 CAS 仍基于内部历史。现有 primary 历史文本移除审计 input，rich 内部历史仍保留审计供 guard 使用。credential redactor 如果改变参数任一字节，整条改为历史结果投影，不以被替换的字符串伪装原调用。任何由对比产生的参数原文不得写入错误。

本卡只调整工具参数双投影造成的比较差异；不顺手处理 #409 的 reasoning 历史一致性问题。现有 reasoning、source_version、source fingerprint、checkpoint predecessor 和最终 CAS 保护保留。

## 批量编辑原子性与审计

在现有 mutation lock 和文件版本保护下，以同一原始 TextFile 求全部匹配范围；逐项唯一、范围不重叠后，构造一次完整目标字节，再走原有权限、检查点、重验证和原子替换。保留有限引号兼容和编码/换行保护，不扩展为 NFKC/任意模糊搜索。canonical 不支持隐式全替换。

批量审计沿用现有 WriteEditAudit::Edit 的总 old/new 字节计数，扩展可选的 `edit_count`（只允许大于 1 的整数）；它标识多项输入，使用独立 batch 指纹 domain 并包含路径、项数、顺序和各项字段的长度边界。不保存原文。单项 edit 保持旧 v1 指纹和合法形状；旧 v1 解析、幂等和检查点读取保持。批量成功仍返回实际替换数、写入字节数和原检查点引用，不绕过工具卡的真实性检查。重复调用的身份必须绑定真实 batch，不可把多项审计降成一个假单项。

bash 的旧/新形状重放按解析后的 command 与有效 timeout_ms 比较；超时变化仍冲突。Read/Write/Edit 仅允许已列出的名称别名等价，其它工具名严格比较。旧固定反馈保持精确白名单兼容，不用宽松前缀匹配把篡改记录误判为合法。

## 实现顺序与文件边界

1. 先补针对审计参数污染、批量原子性、旧历史和压缩的回归断言，再修改生产路径。
2. `vega_tools` 的 mutation/codec/error/bash/read/grep/fence：统一输入迁移、批量编辑、固定错误和安全审计；尽量复用既有能力。
3. `vega_runtime` 的 agent/tools_exec、loop、provider 与 OpenAI 请求投影、context/token 估算：集中定义，区分内存参数和审计，所有实际模型请求走相同投影；清理旧指导。仅按实际调用链增加小型模块，不引入新依赖。
4. `vega_conversation` 的 history pipeline/compaction/tool 类型投影及必要 UI 工具识别：适配历史、双投影比较、旧反馈和新 batch 审计，安全判断保留。
5. 更新明确被替代的 #90/#112/#85 规格入口，保留原始证据。新增测试覆盖本矩阵；现有不变安全断言不得删除、ignore 或放宽。

代码只由指定实现 agent 写，主 agent 审查和集成。同卡不并发修改相同实现文件。无新依赖、无 DDL 变更，除非先回报主 agent 修订规格。新增代码不含注释，`unsafe` 邻接 SAFETY 例外按执行总纲。

## 验证、交付与回滚

本地允许 `git diff --check`、`cargo fmt --all -- --check` 等快速静态检查；禁止运行 Vega、编译、check、Clippy、测试或打包。

Actions 执行既有 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。仅最新 head 的 `check (fmt, clippy, test)` 成功后 squash merge；CI 每 5 分钟查询，通知/用户即时请求例外。记录实际结果，不以静态审查冒充测试。

用户手测：更新云端发布版本后，在一次性样例项目分别使用两种已配置工具调用模型完成 read→批量 edit→write→bash；在权限模式下验证外部文件；重开旧失败会话继续；长会话触发压缩。确认工具历史不出现审计参数模板，错误能纠正，工具卡状态正确。真实模型成功率仍需手测/后续对照；本卡不触发付费批量模型实验。

合并后 Issue 保持开放并置 In review，保留分支/worktree等待用户验收。回滚以回退本卡 squash 提交为主；新批量审计在旧二进制中可能不可显示，不能声称降级完全透明，不删除新记录或修改用户数据。

## 参考（行为研究，不引入上游实现）

- [pi 核心工具](https://github.com/earendil-works/pi/tree/main/packages/coding-agent/src/core/tools)：read/write/edit/bash、参数靠近定义、编辑数组。
- [pi-ai](https://github.com/earendil-works/pi/blob/main/packages/ai/README.md)：稳定业务工具接口与 provider 协议适配分离。
- [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling)：strict schema、nullable 可选语义与本地校验。

本规格是 Vega 行为实现，不复制 pi 源代码，不引入 pi runtime，不照搬宽泛类型转换、模糊匹配或无默认超时。

## 2026-10-16 补充授权：工作策略与项目上下文

用户通过另一对话要求将提示词和执行行为的改进一并实际落地。只读复核确认当前 `app_agent.rs::SYSTEM_PROMPT` 仅含身份句，三类工作入口复用该句；runtime 增补 skills/CUA 指引并在每轮重建 System 消息，但未自动注入项目根指令。本补充修复这条实际输入链，不把新增提示词当成已证实的完成率改善，也不把所有压缩退出归因于提示词。

### 补充验收（先于实现）

| 用例 | 操作与预期 | 证据 |
|---|---|---|
| M | App 普通消息、已批准计划和凭据续跑的实际 provider 请求包含同一工作策略、工具实例真实根目录及适用根指令；模型名不改变策略/工具契约 | 生产入口 MockProvider 请求捕获，待 CI |
| N | 工具续轮、steering、恢复会话和压缩后的下一次 primary 请求保留策略与项目指令且不重复追加；同一 run 的文件快照稳定，新 run 重新加载 | runtime/conversation/App 确定性回归，待 CI |
| O | 项目根 AGENTS.md 优先，缺失时才用 CLAUDE.md；二者均不存在正常运行；不读取根外/其他子目录/全局用户目录；symlink、hardlink、目录/FIFO、过大、非法文本及读失败不静默当缺失 | tools 文件替身与零 provider 请求断言，待 CI |
| P | 项目指令中的敏感值仍受最终凭据保护；文件/工具输出中的伪指令不能被上下文组装器提升为 System；Debug、事件和数据库不新增项目指令原文 | 凭据、请求层级与持久化断言，待 CI |
| Q | 正常明确任务不要求逐步审批/强制长计划；同类失败指引要求新证据或策略变化，并在无可行下一步时说明具体阻塞。基础策略只引用当前声明的工具契约，不复制一套参数 schema | 文案契约及两种 provider 协议请求，待 CI；真实模型效果未测 |

### 冻结行为与实现边界

1. 基础工作策略保持简洁：确认目标、可验证的成功判据及关键未知；优先检验会推翻方案的假设；收集足够上下文后小步实现并验证；阶段性核对实际进展；再次尝试同类错误前核对声明参数/最新文件或状态并作实质调整；架构、范围、权限或依赖冲突阻塞时报告证据和下一步。正常授权任务自主推进，简单任务无需计划工具或额外审批。明确遵守项目测试/执行限制，未执行验证不得报告通过。
2. 使用一个共享的上下文组装函数，串起 App 的三类工作入口、原有 CUA/skills 增补及 primary provider 边界。每个 run 冻结一次根目录和根指令文本；后续轮次与压缩重建使用该快照。新 run 和重开会话重新读取当前项目指令。可在 App worker 组成基础 prompt 后复用现有 runtime 重建路径；不要在多层重复追加。实际请求 token 估算必须使用同一组成结果；上下文预览/手动压缩的估算也应复用组装逻辑，避免遗漏新增固定上下文。
3. 根目录以 `Tools::root()` 的规范路径为准，不以 shell 的 `pwd`、进程当前目录、历史文字或模型猜测为准。项目指令自动加载范围明确为所选项目根：优先 AGENTS.md，仅在该文件不存在时回退 CLAUDE.md；不自动遍历根外祖先、HOME 或全部子目录。基础策略要求修改更深目录前读取该路径上更具体的 AGENTS.md，并遵守其作用域。此约定不宣称自动加载所有嵌套指令。
4. 根指令加载在既有文件/凭据边界内，使用 regular-file、no-follow、hardlink 和有界读取防护，复用 `vega_tools` 已有安全打开原语；最多 64 KiB 完整文本，超过限制或已有文件无法安全读取时在 provider 请求前明确失败，不截断规则、不忽略失败、不回退到另一文件。不新增指令配置、DDL、依赖或后台扫描。文件快照、路径元数据用明确边界/编码封装，原文不进 Debug、事件或新持久化字段。
5. 基础指令、项目指令及历史材料区分来源和优先级。项目指令仅约束该项目的工作，不能自行授权高风险动作、扩大工具权限或覆盖当前用户明确要求；普通文件内容、摘要、旧对话及工具结果不能由程序自动升级成新的 System 指令。只从固定候选路径装载项目指令，不从历史字符串提取指令文件位置。沿用最终 owner credential guard，不允许新加载入口绕过凭据保护。
6. 具体参数、示例和启用工具以本次 `ToolDefinition` 为权威。基础策略不并列公布 legacy 字段，不虚构未启用的 planner/subagent 工具。保留 Ask/Plan/Execute 及现有权限选择，不引入固定轮数强制退出、复杂规划器或新多 Agent 编排。通用硬性无进展检测仍由 #411 按独立证据推进。
7. 主要改动位置：`app_agent.rs` 的默认策略和 worker 三入口；必要的共享 prompt 模块与 `vega_tools` 有界根指令读取；`window/context.rs` 的预览/手动压缩估算调用；对应生产入口和 headless 回归。原 runtime 每轮 System 重建及 compaction CAS/身份保护继续复用，若改动只为接口接线，不顺带改 reasoning 语义。
8. 压缩关系保持清楚：本卡修复审计/模型参数双投影的比较差异并覆盖其正常与篡改场景；独立 reasoning/source 一致性问题继续关联 #409/#413，不能吞掉 SourceChanged、降级成成功或取消 CAS。诊断中长运行与 summary_source_changed 的共现不等于本补丁覆盖全部根因。
9. CUA 参数执行语义保持现有授权与后端校验。本卡“无效参数转历史”指 prepare 阶段已判无效并用审计替换/丢弃的输入；CUA 后端后续返回 invalid_arguments 的情形不另造第二套校验器。其临时参数仍受凭据过滤、完整身份绑定和失效清除保护，CUA 专项恢复仍属 #404。
10. 根目录在建立工具实例后消失或变得不可访问时，不得误判为两个指令文件都不存在。规范根路径无法无损表示为 UTF-8 时，在 provider 前返回固定、可恢复的路径错误，不让 JSON 序列化 panic，也不以 lossy 字符串冒充真实 cwd。

参考来源为 [OpenAI Codex Prompting Guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide) 的自主推进与避免无进展重复原则，以及本机 pi 0.84.1 的 prompt 组合方式（工具规则、项目指令、skills、cwd）。只提取与 Vega 契约匹配的行为，不整份照搬某模型提示词。真实效果对照只有在相同模型、任务快照、工具和预算下实际运行后才能报告收益；本轮不运行付费模型评测或 Vega 本地测试。

## 执行证据（2026-10-16）

- 第一阶段提交 `15af87df` 的 [Actions 38874792128](https://github.com/puzige/vega/actions/runs/38874792128)：Clippy 通过；Nextest 原始汇总为 `2762 tests run: 2748 passed, 14 failed, 5 skipped`。19 个新增 Issue #300 用例通过，但该 head 的 required check 失败，未合并。
- 失败暴露 bash 重复字段被普通 JSON map 接受的回归；修复恢复严格拒绝，并覆盖批量编辑内的重复字段和零执行。其余修订按新契约核对名称、真实调用与历史文本、summary 因果顺序和实际上下文预算；权限、凭据、内部审计指纹及单次执行断言保留。
- 补充实现覆盖普通发送、批准计划、凭据恢复、工具续轮、新 run 重载、steering、自动/手动压缩、上下文预览，以及加载失败和凭据检查；新增生产入口请求捕获回归。该完整提交尚待新一轮 Actions，不能沿用第一阶段通过项作为最终验收。
- 本地仅执行格式、差异与静态审查。最终 PR/Issue 将记录待合并最新 head 的检查结果；真实模型效果和应用系统集成仍待用户手测。
