# Harness² 原生 Runtime 离线试点

## 状态、授权与基线

- Issue：[#522](https://github.com/puzige/vega/issues/522)。规格 v2，2026-10-21；原生worker与controller已实现并取得独立审查、带冻结测试的云端GREEN。独立删测、最终head门禁与合并以[PR530](https://github.com/puzige/vega/pull/530)后续记录为准；真实实验未运行。
- 用户明确要求接手本卡；实验模型固定为 `gpt-6.1-sol`，思考强度 `max`。真实调用费用上限、可用原生 API route/凭据与隔离实验环境未明确，不启动付费模型搜索或跑批。
- 已 fetch 并从最新 `origin/master` 建立 `feat/522-harness-pilot`；生产未改基线 `9be433a147b5aad9848e32e30afaa9fa23c5915e`。主 Agent 已完成基线同步，子 Agent 不再就此询问用户。
- 主 Agent 写规格、管理 Git/PR/集成；Native 实现者仅写本规格指定 Rust 范围；Controller 实现者仅写指定 Python/研究数据范围；非实现者独立审查。禁止并发写同范围。
- 遵守 [执行总纲](../vega-exec-guide.md)、[测试生命周期](../vega-rust-test-delivery-lifecycle.md)和[交付 skill](../../.agents/skills/vega-kanban-delivery/SKILL.md)。本地仅低 CPU 静态检查；不运行 Vega、编译或 Rust/Python 测试。现有云端 PR 门禁、五分钟查询节奏保持不变。

## 交付行为与验收矩阵

先验证评估入口本身，真实收益只能由随后明确标注的研究实验支持。通过模拟 Provider 的 CI 不代表模型优化收益。

| 场景 / 前置条件 | 操作 | 必须可观察的结果 | 验证层级 |
| --- | --- | --- | --- |
| 合法公开任务、非空限额、明确输出路径 | 原生 Provider 经 Runtime 提议 Read、Write/Edit 并结束 | 真实原生工具产出目标文件；种子来源不变；终态、事件和内容可供外部判定 | CI：仅替换 Provider/network |
| 相同任务重复采样 / 相同 run_id 重试 | 从同一只读种子启动两次；再重用已用 run_id | 不同试验从独立初始副本开始；相同完整输入可读取已固定结果，不重新执行；输入/hash冲突或不完整目录拒绝，不重放副作用 | CI |
| 模型提出 Bash、外部 Read/Grep、未授权 Write/Edit、修改 AGENTS/skill | 使用真实 Runtime 权限逻辑 | 拒绝；外部 sentinel 和固定文件字节不变；明确输出的正向写入仍成功，不能“全部拒绝”假通过 | CI |
| 任务产物错误 / 截断 / 回合耗尽 / Provider 故障 | 执行或判定 | 自然 End 仅表示执行终态；独立 oracle 决定任务完成。Length、TurnLimit、错误和缺失产物不能记成功 | CI |
| Provider 等待、用户取消或截止时间到达 | 触发 CancellationToken | 原生运行清理结束后固定 cancelled/timeout 终态与部分轨迹；不用 abort 或丢弃 future 后冒充终止 | CI |
| usage / pricing 缺失或混合 | 汇总原生事件及研究账本 | 未知费用输出 null/unknown，不能把 Runtime 或上游占位0算免费；失败与搜索成本不被剔除 | CI |
| 候选树含脚本、hook、配置、子 Agent、路径逃逸或符号链接 | 校验 / materialise | 先拒绝，不触发 provider，不静默忽略越界文件 | CI |
| 优化集与留出集、私有 oracle | 暂存 task_files / 原生请求 | 只含公开输入；oracle、评分答案、其他split不进入执行者工作区或轨迹；split不重叠且版本固定 | CI |
| 冻结配置与经验库后评估留出集 | baseline/frozen 各自独立采样 | 不运行 proposer/selector/reflection，不修改策略/经验库；报告相同模型/限额及全部样本、成功率、耗时、费用缺失与失败 | CI 结构验证；真实模型由明确研究运行验证 |
| 原生及上游配置不可用 | preflight | 在花费前给出缺失项；当前 Codex 模型设置不冒充 API route/凭据或 OpenCode 通道配置 | CI / 操作者真实验收 |

## 研究范围与结论边界

- 只评估原生 Runtime 的受限工具、工作策略及 Skills 微任务；不测 GPUI，不重放实际用户工作区，不评估桌面主控委派或整套 PR/CI 交付耗时。
- 试点 harness 有自己的固定 policy 与可编辑 guidance，版本与字节哈希显式记录。桌面 SYSTEM_PROMPT 当前位于应用 crate 的私有模块；本卡不改它，也不声称新入口与桌面编排完全相同。
- Harness² 上游固定为 `1cfe254e8d25b15f37048c3730134686abe60335`。实现其 Substrate / BenchmarkAdapter 接缝，不复制或重写递归算法，不自动安装/运行外部框架。
- 上游 SurfaceSpec 默认还允许脚本、skill scripts、sub_agents；本卡必须额外逐路径拒绝。仅允许 `strategy.md` 与 `skills/<slug>/SKILL.md`，无脚本/插件/hooks/自动化/子 Agent。
- 固定 policy、工具参数契约、原生权限逻辑、项目指令、凭据保护、模型/思考设置及限额不属于搜索空间。技能激活使用原生已审查 project skill catalog；`trusted_skill_approvals` 是独立于可编辑树的操作方/可信controller输入，逐个绑定技能slug与实际SKILL字节SHA256，缺失或过期须在Provider启动前拒绝。禁止仅根据模型生成正文构造 `SkillApproval::reviewed`、自动更新批准hash或接受候选自带审批。
- 首次真实搜索可只改strategy与触发指导，保持已有批准的技能正文不变；改动技能正文时须取得独立审阅后的精确hash批准。CI显式注入可信测试批准fixture只验证接线，不代表真实用户批准。必须有原生 `load_skill` 成功激活对照及正文变更后旧批准失效对照。
- 原生工具采用 Execute + Confirm，只为任务明示输出配置原生精确 Write/Edit 规则；不提供 Bash规则、MCP、自动化、子 Agent或用户交互 hook。其余请求由默认拒绝 hook 立即拒绝；不修改核心工具或权限实现。
- 独立目录不是 OS 沙箱。原生外部读/写必须由既有权限挡住；真实外部研究通道的隔离由操作者明确配置，不能把提示词或环境变量声明当作沙箱证据。不得访问本机真实用户配置、凭据或任务内容。
- 固定模型/思考强度须通过实际模型兼容性 profile 验证；不支持或未知就 preflight 失败，不降级为 provider default。
- 上游 proposer/selector 仅支持 opencode / claude_cli，独立于 solver substrate。本卡研究配置使用单独配置的 OpenCode 同模型与 max variant；不可将 Codex 当前会话的订阅/身份自动用于原生 API。费用未知或未获本轮预算时不运行真实链。
- 上游 run_sweep 在每个任务继续递归/反思，不用于留出评估。优化结束先固定一个候选树及经验库内容/hash，留出只调用原生执行入口。
- 上游 RunView/ledger 可能将费用缺失记0；本卡费用和终态从原生结果与各研究角色证据重算。无法取得 proposer/selector/reflection 计费时，搜索总成本为未知，不能声称更便宜。

## 原生 worker 协议 v1

新增 opt-in 二进制 `harness_eval`，私有模块位于 `crates/vega_runtime/src/bin/harness_eval/`；不新增跨 crate 领域类型，不改默认启动入口/Runtime loop。CLI JSON 编码、参数拼写可由实现者据现有风格决定，语义冻结如下：

- 公开任务请求有 `schema_version=1`、`task_id`、`task_prompt`、`seed_files`（相对路径到UTF-8文本）、`writable_paths`、`fixed_policy`、`harness`（strategy与skills正文）、`model`（id与reasoning_effort）、`limits`（timeout_ms、turn_limit、max_output_tokens、max_total_tokens），另有受信任的 `trusted_skill_approvals`。CLI的trial-root/run-id/provider-config单独作为控制参数，provider-config包含显式route/profile及可选pricing，凭据仅环境注入。禁止 oracle/expected/scoring 字段进入请求；严格校验未知字段、空限额和逃逸路径。
- 秘密只经专用环境变量或已有受控凭据接口传入，不出现在请求JSON、任务目录、输出、日志或CLI参数；无自动读取用户已登录配置。Provider错误只输出脱敏类别，不保留秘密响应。
- CLI的 `--trial-root` 是本次试验的精确新建叶目录，workspace位于该目录下；run_id仅作身份元数据，不再隐式拼一层路径。不复用/覆盖既有内容；controller保存的请求、oracle和日志在其外。seed不得包含符号链接或目录逃逸；固定项目AGENTS与技能不在可写范围。工具实例加载项目指令并加入固定提示上下文，记录内容hash。
- 真正调用 `AgentRequest` + `run_agent_with_sink` + `vega_tools::Tools`。不用 mock runtime 替代；只允许 CI 隔离 Provider/network。
- 结果JSON包括 `schema_version=1`、`backend=vega_native_runtime`、task_id/run_id、terminal、final_text、artifacts（路径到文本）、metrics、trace、hashes及wall_time_ms。hashes绑定task/seed/policy/harness/model/instructions/request/config；config包含实际provider route/profile/pricing/限额、公开原生基础工具定义、启用能力事实与worker版本，但不包含凭据或每任务路径/正文。完整实际工具集在首个真实模型请求处另记 `hashes.tools`；不得复制私有skill schema或新增Runtime公共API来伪造快照。metrics记录主模型与工具计数、input/output tokens、cost_microcents、pricing_known、usage_complete，缺失计价时cost为null。沿用原生legacy字段名但明示 `cost_units_per_usd=1_000_000`（以vega_token实际协议为准），不按字段名推成1亿或擅自缩放原始quote。原生技能trace使用 `skill_activation`、成功status=`loaded`。没有任务success字段或oracle。
- 原生 `--preflight` 对完整请求和显式provider-config做结构/profile/限额/批准校验，输出config等hash，不读取秘密、不发模型请求、不创建工作区；标明 `transport_verified=false` 与 `live_calls=0`，ready只代表结构可用，不代表已验证服务可达。
- 工具轨迹记录原生调用和结果；重复提议用规范化工具名+JSON输入、同一已知文件变更epoch内的重复定义，是描述指标，不自动等同无效调用。不记录私有推理全文。终态与任务评分分离。
- 限额要求非零turn_limit、max_output_tokens、max_total_tokens及deadline；观测usage达到token预算后取消并记 `budget_exhausted`。这属于协作式已观测用量限制，单次在途请求可能超过阈值，不能冒充严格货币上限。其他终态为end/length/turn_limit/cancelled/timeout/failed；即使end也须另做任务判定。货币上限无法从缺失价格保证时 preflight/报告明确未知，不声称已限制美元开销。
- 超时先取消CancellationToken，再等待原生运行清理；不会把仍有工作进行中的状态记为已完成。Provider必须遵守取消契约，外部研究环境可以配置独立进程监督，但不新增CI E2E门禁。
- Python研究入口以独立session启动原生worker，收到中断后转发信号、等待真实退出并保存已返回的取消结果，再停止后续试验；普通取消不kill子进程。尚未进入Runtime且未返回JSON的启动中断如实保留不完整/未知证据。上游OpenCode在途session按其原有退出/超时机制等待，不能声称已即时取消外部通道。

## Controller、任务集与报告

- 新增标准库 Python 控制层 `scripts/harness_pilot.py` 和可选上游桥 `scripts/harness2_vega_adapter.py`；上游导入只在显式研究入口进行。不新增Python包依赖或更改现有workflows。
- 固定24个原创脱敏任务，检索理解、受限文件修改、错误恢复、Skills使用各6个；每类4个优化、2个留出，即16/8。任务输入/初始文件、独立预期产物与判定、来源/域均版本化；不包含真实历史对话内容。
- 每例必需明确产物路径，按文件内容/结构和必要不变量判定；Skills用例结合真实激活事件和产物。包含正确/错误产物对照，不能只测“存在文件”或由mock返回成功。
- Task公开暂存目录只含公开材料；私有预期只由controller保存且不投影到原生请求。上游 task_files 只返回该公开目录。
- H2 adapter实现 tasks/domain_of/task_prompt/task_files/execute/delivered/trajectory/deliverable_files/deliverable_text/base_harness/validate_tree/score 及 Substrate 接口，固定上游版本，参数/返回值按实际源码核对；live优化不另造算法。上游run_id可含斜线，桥接层确定性映射为安全hash身份，原alias仅作私有控制元数据。选择阶段 `delivered` 只判断真实终态及认证的在盘明确产物就绪/可解析，不比较私有预期；精确oracle由controller/post-selection的score使用，不提前泄漏通过位。
- 研究计划区分优化搜索与冻结留出。留出默认每例每臂3次独立采样，可显式配置但必须两臂相同；使用同一task/source/model/effort/限额/pricing与配对计划。实际运行前输出预计rollout数与预算未知项。
- 比较报告记录各任务/臂/重复的结果及paired样本、质量、无效/重复提议、端到端耗时、每成功任务总成本、失败和权限事件；全无成功或费用未知返回null。失败的执行成本计入分母对应总账；搜索费用单列并包含所有改进/选择/反思/子角色，没有则标未知。
- 冻结树与经验库在留出前后核对hash；发现改变或混入不同模型/数据/限额拒绝合并报告。未执行给 planned/not_run，绝不生成虚构提升率或自动采用配置。

## 实现顺序与文件归属

1. 主 Agent 完成此规格并提交；独立审查方案的可执行性与完整性。
2. Rust写入者仅写 `crates/vega_runtime/src/bin/harness_eval/` 的先行测试与必要测试接线；不修改其他生产逻辑。Python写入者仅写 `scripts/tests/test_harness_pilot.py` 与本轮研究fixture数据；先不实现控制器/上游桥。
3. 写入停止、测试就绪后，非实现者独立审查实际API用法、正常对照、失败判别、固定断言。主 Agent 提交并开 Draft PR，现有 Actions 确认有效RED。新目标接口不存在仅记接口缺失RED，明确行为断言尚未执行；其他编译/接线错误不算RED。
4. 有效RED后冻结测试与预期。两个实现者在互不重叠范围实现：Rust worker；Python controller、24任务/基础harness与上游桥。只改上列文件；不新增依赖、改workflow或放宽工具/权限。
5. 完整实现停止写入后独立审查，并验证同一冻结测试云端GREEN；保存原始源码/full SHA/run/job/准确计数。
6. 仅在带测试GREEN后，独立提交删除Rust测试与独占支持；Python测试保留。非实现者审查生产不变及最终空集head，完整required check成功后转正式PR并合并。
7. 合并/CI不是实际研究收益；用户手测与明确的付费研究运行另记真实证据。试点不自动改日常策略。若实验预算/环境仍未明确，准确报告该阶段待定，不把卡片标Done或声称优化已完成。

## 回滚及待定事项

回滚本卡新增入口/研究文件即可，不涉及数据库或用户配置。既有主程序、权限、工具契约、CI和默认提示均不变。

真实研究启动前仍需操作方明确可用原生route/凭据注入、上游OpenCode同模型配置、环境隔离方式、调用预算上限；本规格和实现准备可独立推进。不得因研究尚待配置而跳过已授权的实现/审查/CI工作。

## 隔离研究环境操作入口

以下命令仅供操作方在明确配置的研究环境中执行；本卡开发机未运行这些入口。`harness_eval` 使用云端构建产物，不在本地编译。路径均为占位示例，不能直接当作可用环境。

不提供执行参数时，两个Python入口仅输出计划和缺失项，不启动上游或模型：

```sh
python3 scripts/harness_pilot.py plan
python3 scripts/harness2_vega_adapter.py
```

`provider.json` 必须是独立、无凭据的显式route及兼容性快照。以下仅示范结构，`example.invalid` 不可连接，`pricing: null` 不满足付费留出执行要求。实际context/output上限、max映射及计价须由操作方按真实服务配置核实，不能照抄为已验证能力。

```json
{
  "schema_version": 1,
  "provider": "pilot",
  "api": "openai-completions",
  "base_url": "https://example.invalid/v1",
  "model_catalog": {
    "version": 1,
    "models": [{
      "provider": "pilot",
      "id": "gpt-6.1-sol",
      "api": "openai-completions",
      "baseUrl": "https://example.invalid/v1",
      "reasoning": true,
      "thinkingLevelMap": {"max": "max"},
      "input": ["text"],
      "contextWindow": 32768,
      "maxTokens": 16384,
      "compat": {"thinkingFormat": "openai", "supportsReasoningEffort": true}
    }]
  },
  "pricing": null
}
```

原生结构预检从stdin读取完整公开请求，不创建工作区、不取凭据、不验证传输：

```sh
/operator/bin/harness_eval --preflight --provider-config /operator/provider.json < /operator/public-request.json
```

真实单次调用额外使用 `--trial-root /isolated/new-trial-leaf --run-id sample-1`；凭据只通过专用 `VEGA_HARNESS_API_KEY` 注入，不能写入上述JSON、命令行或研究树。stdout为单个协议JSON，stderr仅错误类别。

操作方独立审阅基础和候选Skills后，将 `slug → 正文SHA256` 批准映射保存到可编辑树之外的 `/operator/skill-approvals.json`。工具不会自动批准生成的Skill。上游优化入口还要求显式的OpenCode配置、`PREFIX/gpt-6.1-sol`模型与max variant、空隔离配置目录，以及覆盖solver/proposer/selector/reflection的外部强制配额记录；入口参数为 `--external-spend-control`，具体记录字段见 `scripts/harness2_vega_adapter.py` 的 `_spend_control`。该记录是操作方声明，桥接层不验证计费系统，也不把它称为内部硬限额。

搜索结束后人工选择候选，并固定候选目录与经验库，再生成配对留出计划：

```sh
python3 scripts/harness_pilot.py plan \
  --frozen /isolated/frozen-harness --library /isolated/frozen-library \
  --skill-approvals /operator/skill-approvals.json \
  --native-binary /operator/bin/harness_eval --provider-config /operator/provider.json \
  --output /isolated/holdout-plan.json
```

该计划默认8个留出任务、两臂各3次，共48次原生rollout。执行留出需使用相同来源参数，另加 `holdout --plan /isolated/holdout-plan.json --runs-root /isolated/holdout-runs --execute --budget-usd <明确上限> --isolation-label <真实环境标识>`。金额检查基于已观测费用，单次在途请求可能超出阈值，`currency_cap_is_hard=false`；需要严格支出上限时由操作方在服务侧配置。留出不导入上游、不继续学习；结束后检查所有样本、失败、未知费用及冻结hash，再讨论收益。

## 规格变更

- v2：独立方案审查指出技能审批来源不完整；补充受信任精确hash批准、过期拒绝及首次搜索限制。补清worker JSON/私有CLI边界、有效模型profile、未知计价和观测token预算终态。尚未冻结测试或开始生产实现。
- v2协议拼写澄清：实现准备时确认trial-root叶目录、wall_time_ms位置、结构性preflight、基础config与完整实际工具hash分开，以及盲选delivered与私有score分开；冻结验收及测试断言不变。

## 验证记录

- 先行测试/规格提交：`99fb0d7c0a69d4268357e4470c5b67f96b9c5d75`；8项Rust、21项Python和24项任务fixture经非实现者独立静态审查。主Agent复核精确hash、rustfmt、AST/JSON及差异检查通过；未本地执行。
- 最小接口缺失RED：[Actions 39597872628](https://github.com/puzige/vega/actions/runs/39597872628)。Format无法解析尚未实现的runner.rs；Python无法导入尚未实现的harness_pilot。新增行为断言尚未执行，Rust未运行；已有Python5+15+14项通过，discover另有1个新模块导入错误。不是行为RED或完整GREEN。
- 确认后才进入原生worker与controller/上游桥实现；任务fixture保持不变。该阶段尚待带测试GREEN、独立删测和最终空集门禁；真实研究未运行，后续结果逐项记于下文与PR。
- 首次GREEN前独立审查并留证的fixture单位修正：[PR记录](https://github.com/puzige/vega/pull/530#issuecomment-6316021958)。基线 `vega_token/src/catalog.rs:11` 实际每美元1,000,000原始单位，Runtime直接转发。10输入+5输出、每百万$1/$2原生报价应为20而非2000；Python相同1e6/2e6原始值对应总$3、搜索$5、合计$8、每成功$3。仅纠正5个数值，其他验收、原始输入与断言保持不变；不是从新实现输出反推预期，不改原生计价逻辑。
- 修正后冻结Rust测试SHA256：`65e1510eb8dbd17e27a342de8fe8ba93619d78b6b893abe4da4ceb2fa5323ec7`（旧 `ffea8e44603079fe7c5570009cc9d775c7b8d3823f291b658d4d7af3df8df61d`）；Python：`329936c4be162310208008c0ab534c660feddc7fd03f86b5fc67e20ebb6683a1`（旧 `ab41067ddfffcb20e6ff210cd8c1fad8e23fc380b37d756b47000aec0289eace`）。主Agent逐行复核精确变更和新hash，任务集hash不变。
- 首版实现停止写入后，非实现者独立静态审查通过；修正了原生Read/Write/Edit别名的重复epoch统计与Python启动器的协作取消路径。该次审查文件SHA256：runner `e570f5e8bc9329a1c9644cb64c974655e86b42af72934dae9878b40359b1a656`、protocol `5c9048df477b682ac06fd5b8d002fb690214e55943c85aca1aa603af423ca1c3`、trace `ae320e4bb1de1832d94a085ee39b7a22c2dcd6ef1e60dfd5128ec4c518e2342a`、controller `57c7cde7d9f0e0db055a4e38dcfc1ee87966d0f647eaf9be6c9dd329d4b8222d`、upstream bridge `2b9ef4534d0c000806007feecb70f02495be28cc8dd1ee7073595a66130e836c`。所有新增代码仅做静态检查，未本地执行、编译、测试或调用模型。
- 首轮实现CI：[Actions39603657413](https://github.com/puzige/vega/actions/runs/39603657413)，head `16b852b3e3f03b17fe9f4591a8944a6166eee758`。Format/全目标Clippy成功；Python55执行、54通过、1error，Rust步骤跳过。配对/冻结测试的共享替身无条件返回answer.json，违反部分留出任务的声明路径；不是合法worker行为。保留生产fatal校验，按[编辑前独立审查记录](https://github.com/puzige/vega/pull/530#issuecomment-6316795380)仅在该用例设置空产物，原48次/配对/冻结断言不变。修正后Python测试SHA256 `0b24a4d022688d4d3c697ac28919d2972edebb0fefc6ebb29139683505e74ad7`；Rust/任务集和全部生产字节不变。该修正纳入尚未GREEN的实现提交，保留先行来源及批准补丁后再云端重验，不重写已GREEN来源。
- 原生行为首轮：[Actions39605400418](https://github.com/puzige/vega/actions/runs/39605400418)，head `d9a0809fdd05344e750da141fb324afefe61bc4c`。Format/Clippy和三组Python成功；Rust8发现/执行、4通过、4失败、0跳过（15 binaries）。独立确认两处生产根因：缺少checkpoint根目录使mutation准备中止，连带编辑脚本第二次采样读到未消费步骤；workspace启用serde_json preserve_order，使不同对象键序的等价输入哈希不同。仅补新trial的0700 checkpoints目录及Value递归排序对象键（保留数组顺序），独立增量审查通过。新runner SHA256 `20582a0afb6fc90be299baf6ca6d0d357e091c34dfc89b15069a349da2402639`、protocol `4ca263b200d03c2493b1ea42eb4ed5403e18db49d616c76c9e9592101cf55b0d`；测试/权限/schema守卫无变。相同冻结测试GREEN仍待云端重验。
- 带相同冻结测试GREEN：[Actions39607398233](https://github.com/puzige/vega/actions/runs/39607398233)，head `9f59218b90e6d406e6e540b88cd8efbb53805ca4`。Rust8发现/8执行、8通过、0失败、0跳过（15 binaries）；Python5+15+35=55执行通过。Format/全目标Clippy及required check全部success。非实现者核对完整head、生产/测试hash、原始日志及来源链；[PR固定记录](https://github.com/puzige/vega/pull/530#issuecomment-6317299633)已读回，再批准独立删测。
- 可恢复的[GREEN Rust源码](https://github.com/puzige/vega/blob/9f59218b90e6d406e6e540b88cd8efbb53805ca4/crates/vega_runtime/src/bin/harness_eval/tests.rs)、[Python源码](https://github.com/puzige/vega/blob/9f59218b90e6d406e6e540b88cd8efbb53805ca4/scripts/tests/test_harness_pilot.py)与[全部批准测试修正原始patch](https://github.com/puzige/vega/pull/530#issuecomment-6316849308)不依赖临时文件或会过期artifact。开发测试最终按规则删除，生产与Python测试保留；最新空集门禁不能替代本次8项行为GREEN。没有真实优化收益、外部通道或OS隔离的已验证结论。

