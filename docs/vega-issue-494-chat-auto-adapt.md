# Chat Completions 内部适配与思考设置收敛

## 授权、基线与范围

- Issue：[#494](https://github.com/puzige/vega/issues/494)，Feature；Codex 组织专职子 agent 实现。
- 2026-10-19 用户要求修复 `step-5-preview` 工具调用，Chat Completions 差异内部自动适配，移除多套用户协议模板；追加要求参考 pi 的独立模型适配包，梳理能力事实与用户偏好的边界。
- 生产基线：`39b706e2e646c53fcfb25d010b3eecd19b323eb3`；分支 `feat/494-chat-auto-adapt`。当前版本 v2.0，配置契约按用户追加裁决重开先测后修。
- 2026-10-19 用户追加：不兼容读取旧配置。取消 v1.0 的 v1/v2 codec、旧模板导入与能力字段保留要求；旧 `reasoning.toml` 完全不读取、不迁移、不改写。使用独立 `reasoning-preferences.toml`，version=1，仅保存 provider/model/preference；已有思考偏好需重新选择。
- 本文替代 #435 中“未知 Chat 返回推理与工具时拒绝，并要求用户选择高级模板”的产品规则。#435 的原始状态内存边界、精确身份、权限、预算、压缩及持久化约束保留。
- 不改用户运行中的配置、凭据和历史，不进行真实模型付费探测，不升级依赖，不拆分新的 workspace crate，不增加新 transport 或重写整个模型目录。

## 已核对根因与参考

旧 `cpa / step-5-preview` 配置使用 Chat Completions、OpenAI controls、`standard_chat`、high。两次用户失败记录均为 HTTP 200、stop=tool_use，之后被本地 runtime/protocol/response 拒绝。在 `validate_replay_response` 中，Unknown/StandardChat 返回文本推理并提出工具调用会触发 ReasoningReplayUnsupported。仅去掉检查还不够：工具及最终 Assistant 的构建路径依赖显式 profile，缺省 profile 仍会丢回传状态。

设置把支持范围、关闭能力、档位集合、协议和回传策略都当作用户可编辑配置，还保留对应键盘动作；因此仅隐藏模板行不能完成本轮要求。旧 OpenAI 模板给 Step 声明了六档与关闭能力，也不能作为真实模型能力继续呈现。

本机静态核对 pi `@earendil-works/pi-ai` 0.84.1：统一消息/工具/流事件接口；provider/model 元数据与内部 compatibility 规则生成实际 wire；文本推理保留可回传来源。公开 [pi-ai README](https://github.com/earendil-works/pi/blob/main/packages/ai/README.md) 与 [package](https://github.com/earendil-works/pi/blob/main/packages/ai/package.json)说明独立包职责与开发者 compat 覆写。参考其分层，不复制 URL 子串识别、不因此新增 JavaScript sidecar，也不声称 pi 的 Step 已实测。

[Step 工具示例](https://platform.stepfun.com/cookbook/step-5-tool-calling)保留实际返回的 `reasoning_content`；[推理档位示例](https://platform.stepfun.com/cookbook/step-5-reasoning)列出 low/medium/high。未据此宣称可关闭思考。

## 冻结验收矩阵（先于实现计划）

以下为 v2.0 冻结目标，取代旧配置兼容目标。原生产实现的未提交草稿先归档并恢复至测试基线，再写齐新周期测试。全部新周期证据待 Actions；进程内替身仅隔离 provider/network，不 mock 适配决策或执行结果。保留与本次裁决无关的真实 runtime/错误/UI 先行用例，不批量恢复历史测试。

| 用例 | 前置与操作 | 可观察结果与证据边界 |
|---|---|---|
| A | 自定义 Chat proxy、精确 step-5-preview、内部 standard_chat 策略、合法 high 偏好；两次顺序工具调用、最终答复 | 真实 runtime 完成；工具各执行一次；下一实际 wire 保留原文思考、call-id、参数与真实结果；不含 GLM/DeepSeek 控制参数；high 真实编码 |
| B | Chat 无模型 profile，及内部 unknown/standard_chat 策略；分别返回 absent、empty、nonempty 思考与工具 | 不因未选模板失败；缺失不补，空串与原文按响应归属保留；普通无思考 Chat 可用；默认不猜发控制字段 |
| C | 同一路由 warm 后继续一用户轮；完整最终答复带思考；再冷投影或切路由 | 适用的文本状态在完整工具周期与有效 warm 前缀中保持；冷历史不伪造/重执行原始状态；不串模型身份；无 tools 的可选思考不强建 warm 回放状态 |
| D | 已知官方 DeepSeek/GLM 的必需思考缺失；Responses layout 缺失；泛型 Chat 混入 foreign Responses 状态 | 原有完整性失败仍在该响应任一工具审批/执行前发生，零工具副作用；不降级为丢字段成功 |
| E | malformed reasoning 类型，或已有工具提议但流未完整结束 | 生产 SSE/parser/runtime 如实失败；不执行部分工具、不伪造终态、不封存残缺回放 |
| F | 官方已知端点、近似/伪似 URL、未知模型和代理 | 精确端点规则保留；未知服务默认仅原样回传收到的 Chat 文本状态；不靠 URL 子串/品牌词猜发专属参数；Responses 仍由真实 API 决定 |
| G | 精确 Step、无偏好文件；以及新文件仅保存 high 偏好 | 内部能力为 low/medium/high，未声明关闭；high 正常叠加；界面与运行使用相同事实；不要求用户创建模板；任何传入投影均不能覆盖内部能力 |
| H | 新偏好文件选择 Step 不支持的 effort 或 disabled，加载设置后选择 default/合法档 | 原偏好明确显示需调整且保留合法选项；运行在 provider 请求前拒绝；用户可修复并保存；不静默改成默认，不凭空添加档位 |
| I | 鼠标与真实 GPUI 键盘遍历模型思考分区，切页并重新加载 | 无高级协议/模板/协议/支持范围/允许关闭/允许档位控件、占位或隐藏快捷动作；只保留当前偏好及真实 reload/错误/冲突反馈；每个合法偏好有实际可聚焦控件，Enter/Space 选择正确值 |
| J | 仅旧 reasoning.toml 存在，内容为合法 v1/v2、损坏或含注入能力字段；加载模型/设置，然后首次保存新偏好；随后并发编辑新文件 | 旧文件完全不参与行为且字节不变；新文件缺失时按内部事实和默认偏好运行，不在加载时创建文件；显式保存仅写 version/provider/model/preference；新文件中的旧能力字段、重复身份与不支持版本被拒绝；不同模型并发编辑保留，同模型偏好冲突和外部删除拒绝，原子保存/权限边界保留 |
| K | 失败实时显示及重开历史恢复 | 不再提示选择高级模板；仍清楚表述真实无法续接/缺失问题与可执行下一步；原始推理/opaque 不新增 SQLite/TOML/日志/Debug/UI 暴露 |

验收数据/目标冻结后不按实现补测或修改断言；若发现新边界，先记录独立变化与新 RED 基线，重开该周期。已有权限、credential fencing、原参数绑定、预算和取消路径不得放宽。

测试可达性说明：当前公开 AgentRequest 的主循环在各模式都携带内建工具定义，无 tools 的主模型生命周期不可直接构造。C 的工具/warm/cold 链由真实 run_agent 验证；无 tools 的边界用现有公开策略契约与实际 Provider ChatRequest 编码验证不合成字段/控制参数，不把孤立 bool 断言算作完整 runtime 生命周期证据，不为测试扩大生产 API。

## 目标职责与决策

### 用户设置

提供方仍选择真实 API 与连接信息；模型思考分区只选择当前偏好（提供方默认、能力允许的关闭/档位）。模型/服务事实不由 UI 修改；移除高级协议入口和所有模板及能力编辑动作，不把旧控件搬到另一个常驻帮助区。未知能力维持提供方默认，不为展示伪造六档。

### 内部模型适配

在现有 headless runtime 新增 `chat_compat` 内部模块，集中 Chat 的 protocol、replay policy、支持档位/关闭 wire/默认偏好等能力事实；通过既有 conversation/types 边界给 App 与 UI 投影。App 负责配置身份与偏好叠加，不再维护第二份品牌能力表。此阶段无需新 crate/npm 依赖。

精确服务注册表的既有 DeepSeek/GLM 规则迁入该边界，保留 HTTPS、端点完整身份与有限规范化；不能复制 pi 的宽泛 URL 子串 heuristic。精确 Chat 模型 ID `step-5-preview` 在官方及自定义代理上采用 OpenAI effort wire、low/medium/high、未声明关闭与可选文本回传。它是已知模型的明确规则，不使用 model substring 或泛化到未核对的 Step 版本。

模型能力只来自内部规则，任何配置或 UI 投影都不能覆盖。新文件中的合法偏好叠加在内部事实之上。某模型不支持的偏好在投影中保留，运行 freeze 拒绝；设置同时显示简短错误和合法偏好，使用户能修复。不能返回清空 authority 的通用无配置错误导致无法保存。

使用独立 `reasoning-preferences.toml`，version=1，条目仅含 provider/model/preference。删除旧 v1/v2 codec、模板字段转换、能力字段 patch 及导入合并路径；旧文件既不作为 fallback，也不作为读取错误或启动阻塞来源。不保留已知模型的第二份磁盘能力表。新文件 grammar 校验偏好形状，实际可用性由内部模型事实判断。沿用原子写入和 CAS，不同身份的偏好可合并，同身份冲突或外部删除不得被陈旧快照覆盖。

```toml
version = 1

[[preferences]]
provider = "proxy"
model = "step-5-preview"
preference = "high"
```

不读取旧文件意味着旧配置中的自定义厂商协议声明和偏好均不生效；已知服务按内部注册表解析，未知服务采用通用 Chat 行为与提供方默认。新文件可按 UI 明确选择创建，运行与设置的加载路径均使用同一个文件名常量。

### 原样 Chat 回传

Unknown/StandardChat 默认支持工具请求周期内实际收到的 `reasoning_content`。Absent 不创建字段，empty 与 nonempty 保留原文及 Assistant 归属；工具回复与最终回复均使用同一策略，`AgentRequest.reasoning=None` 也覆盖。无工具请求的可选思考不据此扩展回放生命周期。

只接受已有 parser 验证过的 Chat 文本字段；不把 summary/可见文字重新拼成原始状态，不把 Responses opaque/layout 当作 Chat 文本。已知严格策略与完整流前置校验保留。回放能力与思考控制分离：收到文本状态不授权向未知端点增加 thinking/clear_thinking/禁用参数、重试工具副作用或猜测其他协议。

有效 warm/cold/route identity 与压缩的既有边界保留，实际 wire 预算继续计入所需原始状态。原始推理不新增落盘，可信缓存用量不伪造。

## 具体实现顺序与文件归属

1. 专职子 agent 在生产未改的基线上写齐矩阵所需最小行为测试；可新增独立测试模块及必要 test-only wiring，不能整目录恢复已删除的历史集合。允许现有 workspace 版本的 tempfile 与 gpui-kit test-support 作为开发期支持；不新增生产依赖，不升级 package，锁文件变动逐项审查。
2. Codex 审查测试补丁与纯测试基线，建立同一卡 draft PR，由现有 Actions 取得有效 RED，记录原始失败。未获 RED 前禁止生产实现。
3. `vega_runtime/src/chat_compat.rs`、`lib.rs`：集中 headless 能力事实，既有共享类型通过 `vega_conversation/src/types/reasoning.rs` 接线；App `reasoning_capabilities.rs` 只协调 API/配置与能力，Step 和精确服务在统一入口解析。
4. `vega_runtime/src/provider.rs`、`agent/loop_.rs`：可选 Chat 回传策略及两条 Assistant 生产路径，覆盖无 profile；保留 strict/foreign Responses 校验。 本轮 E 的有效 RED 还确认了 provider 流未发 Done 时误记成功的缺口：在收集结束、任何工具副作用之前补最小缺终态失败 guard。保留实际 OpenAI wire 的 finalize 语义，不额外要求服务发送另一种结束标记。`openai/mod.rs`、`openai/sse.rs`、`tool_context.rs` 仅在确有必要的统一序列化/投影接线处修改，不扩大 parser 支持或删除完整性保护。
5. `vega_store/src/reasoning.rs`、`vega/src/window/{reasoning,session,context}.rs`、`app_agent.rs`、共享 projection：改为独立的新偏好文件；移除旧 codec/模板导入与能力 patch，运行和设置共同使用内部事实加偏好。保留无效新偏好的可修复状态及原子/CAS 边界，禁止新 DDL、读取或迁移旧文件。
6. `vega_ui/src/settings/{reasoning_render,reasoning_state,mod,state,provider_management,model_catalog}.rs` 及实际持有相关 state/focus 的文件：移除模板/协议/能力编辑器及隐藏动作，移除目录的“应用思考档位”能力写入口，保留目录其余只读数据。保留并修正当前偏好的真实焦点/键盘/错误/冲突生命周期。不新增无关常驻说明。
7. `vega_conversation/src/types/events.rs` 及实际 settings 错误消息：移除模板指令，按真实错误给下一步；不新增原文诊断。更新本卡交付记录。
8. 同一冻结测试云端 GREEN 后在 PR 固定源码/补丁、完整 SHA、RED/GREEN run/job 和计数。独立提交删去本轮 Rust 测试及独占 fixture/feature/dev dependency，再审查生产差异与最终空集 head 完整门禁。

生产代码无新增注释、非测试 unwrap/expect、依赖或架构反向引用。主 agent 只写方案、审查和集成，子 agent 为唯一实现写入者。

## 验证、交付与回滚

本地只执行快速 `cargo fmt --all -- --check`、`git diff --check` 和低 CPU 静态检查，不运行 Vega、编译、Clippy 或测试。RED/GREEN 与最终门禁均使用现有 PR Actions：fmt、全目标 Clippy、三组保留 Python 检查、Nextest 清点及运行；required check 为 `check (fmt, clippy, test)`。CI 每 5 分钟查询，完成通知/用户即时请求除外，不自动重试。

带测试 GREEN 与最终树 0 发现/0 执行分别记录，后者写 `no Rust tests`，不能代替 GREEN。最终 gate 通过后转正式 PR 并 squash merge，卡片 In review，等待用户真实 Step/代理与 UI 手测；不声称已安装，不提前关闭或清理任务资源。

用户手测：更新云端发布版本；原 cpa/Step 连接配置可继续用，旧思考偏好失效，在新界面重选 high 后连续两次文件任务再发一轮；确认工具结果可继续使用且无重复执行。模型设置只显示合法偏好，无协议/能力编辑器；切换偏好、Tab/Enter/Space、重载及重开应用有效。新文件中的无效偏好能就近修复。真实网络/模型结果由用户验收，自动替身不替代它。

回滚通过仅 revert 本卡的审查 PR，不改用户配置/凭据/历史；旧文件保持原样，但本轮新版本不读取。原始验证与开发测试固定在 PR，不依赖临时 artifact。


## v1.0 历史周期（已被用户配置裁决替代）

- 测试初稿提交 `de112c562715806ca4a61e27797ae21f95136fa4`；[首次 Actions](https://github.com/puzige/vega/actions/runs/39237364347) 为 GPUI fixture owned/reference 借用接线编译错误，行为断言未执行，不算 RED。
- 仅修正 ElementId 的引用借用，提交 `973e3857a4983852bf4ec809856c2ecdab2f95e1`；原数据/断言不变，原提交与失败保留。
- [有效 RED run](https://github.com/puzige/vega/actions/runs/39238140388) / [Nextest job](https://github.com/puzige/vega/actions/runs/39238140388/job/118012146061)：22 个执行、8 通过、14 行为失败、0 跳过；格式与全目标 Clippy 成功。
- 失败包括 Step/泛型文本回传、warm 续接、Step 能力归一化、偏好独立保存、能力不可编辑、无效偏好修复、模板文案和实际控件/焦点。E 缺 Provider Done 时误记成功同样由既定断言检出，按原验收补 guard，不改变测试或增加 wire 结束标记要求。
- E 的传输证据限于 malformed reasoning 与缺少 wire finish 的片段/终态；不声称已单独测试 foreign Provider 在完整 finish 之后故意省略 Done 的抽象契约。无 tools 主循环的可达性限制见矩阵后注。
- 主 agent 于 2026-10-19 18:38 UTC+8 审查有效 RED 后授权专职子 agent 生产实现；此后冻结测试不得修改。
- 用户随后取消旧配置兼容。立即停止未提交生产草稿，原测试与上述 RED 留作历史，不能证明 v2.0。新周期先归档草稿并恢复生产未改状态，再按 v2.0 写齐测试、取得新的有效 RED，之后才恢复必要生产实现。修改验收对应的测试属于明确重开周期，不计作原冻结测试 GREEN。
- 旧生产草稿已归档，补丁 SHA256 `c697bc9da408ce16ea02e49b69fd51a94b1c4a51705b2dd31ce1d4010ad096fc`；13 个源码快照另有逐文件哈希。恢复至 `973e3857a4983852bf4ec809856c2ecdab2f95e1` 后，工作树仅保留本次文档修订；生产源与该提交一致，新周期没有在旧草稿上改断言。

## v2.0 新周期

新配置目标对应测试在恢复后的生产基线上先行编写，复用真实应用加载、store codec/CAS、控制器保存入口，不增加测试专用生产决策。共 27 个测试定义：runtime 8、能力及应用加载 6、偏好 codec/控制器/CAS 7、GPUI 4、设置错误 1、运行失败投影 1。v1 的旧配置导入/保留目标由本次用户明确取消；工具、流完整性、warm/cold、严格厂商和错误目标继续保留。

新测试验证旧文件合法、损坏与伪造字段均不影响模型加载；新文件独立加载、只含偏好、首次显式保存、严格 grammar、并发合并/冲突/删除、无效模型偏好修复。测试先行提交、哈希、有效 RED 与后续同一测试 GREEN 在 PR 固定；未取得新 RED 前不恢复生产草稿。

- 先行提交：`eb079d6018b8176041b864ad82f162aa1da8b1d0`。除三份测试源和两份文档外，与 `973e3857` 无差异；生产未改。保留原透明接线及独占开发依赖，GREEN 后独立清理。
- [v2.0 有效 RED run](https://github.com/puzige/vega/actions/runs/39240808870) / [Nextest job](https://github.com/puzige/vega/actions/runs/39240808870/job/118020881294)：27 发现/执行、6 通过、21 行为失败、0 跳过。格式、全目标 Clippy 与 Python 发布检查成功，无编译或环境失败。
- 原始失败明确包括：旧文件仍影响实际加载、新格式无法解析、旧能力字段仍可覆盖协议、控制器仍接受能力修改、Step/泛型工具与 warm 续接失败、缺终态误记成功、模板控件/错误指令和真实偏好焦点缺失。
- 主 agent 于 2026-10-19 19:00 UTC+8 审查后授权专职子 agent 生产实现；本周期 27 项数据与断言自此冻结，同一组测试取得 GREEN 后方可独立删除。
- 生产实现提交 `0139e8d0f7d79383476824b696bad810a37f41e8`；[带测试 GREEN run](https://github.com/puzige/vega/actions/runs/39242802623) / [Nextest job](https://github.com/puzige/vega/actions/runs/39242802623/job/118027425612)：27 发现/执行、27 通过、0 跳过，格式、全目标 Clippy、Python 5/15/14 项及 required check 成功。主 agent 复核全部冻结源码/inline 段哈希一致，在 [PR 固定完整证据](https://github.com/puzige/vega/pull/495#issuecomment-6265953287) 后才授权独立删测；最终门禁另记，尚未以空集替代 GREEN。
- 随本文本次独立提交删除开发期 Rust 测试及独占支持，保留全部已验证生产行为；依赖/通用 helpers 恢复生产基线。最终 head 的 0 发现/0 执行日志、required check 与合并状态在 PR 后续固定，再交用户手测，不用静态清点冒充云端结果。
