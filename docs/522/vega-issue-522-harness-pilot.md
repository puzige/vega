# Harness² 原生 Runtime 离线试点

## 状态、授权与基线

- Issue：[#522](https://github.com/puzige/vega/issues/522)。规格 v2，2026-10-21；先行测试阶段，尚未实现或运行真实实验。
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
- 任务根使用新建目录，不复用/覆盖既有内容；controller保存的请求、oracle和日志在其外。seed不得包含符号链接或目录逃逸；固定项目AGENTS与技能不在可写范围。工具实例加载项目指令并加入固定提示上下文，记录内容hash。
- 真正调用 `AgentRequest` + `run_agent_with_sink` + `vega_tools::Tools`。不用 mock runtime 替代；只允许 CI 隔离 Provider/network。
- 结果JSON包括 `schema_version=1`、`backend=vega_native_runtime`、task_id/run_id、terminal、final_text、artifacts（路径到文本）、metrics、trace、hashes。hashes绑定task/seed/policy/harness/model/instructions/request/config；config包含实际provider route/profile/pricing/限额/工具定义快照但不包含凭据。metrics记录主模型与工具计数、input/output tokens、cost_microcents、pricing_known和wall time，缺失计价时cost为null。原生技能trace使用 `skill_activation`、成功status=`loaded`。没有任务success字段或oracle。
- 工具轨迹记录原生调用和结果；重复提议用规范化工具名+JSON输入、同一已知文件变更epoch内的重复定义，是描述指标，不自动等同无效调用。不记录私有推理全文。终态与任务评分分离。
- 限额要求非零turn_limit、max_output_tokens、max_total_tokens及deadline；观测usage达到token预算后取消并记 `budget_exhausted`。这属于协作式已观测用量限制，单次在途请求可能超过阈值，不能冒充严格货币上限。其他终态为end/length/turn_limit/cancelled/timeout/failed；即使end也须另做任务判定。货币上限无法从缺失价格保证时 preflight/报告明确未知，不声称已限制美元开销。
- 超时先取消CancellationToken，再等待原生运行清理；不会把仍有工作进行中的状态记为已完成。Provider必须遵守取消契约，外部研究环境可以配置独立进程监督，但不新增CI E2E门禁。

## Controller、任务集与报告

- 新增标准库 Python 控制层 `scripts/harness_pilot.py` 和可选上游桥 `scripts/harness2_vega_adapter.py`；上游导入只在显式研究入口进行。不新增Python包依赖或更改现有workflows。
- 固定24个原创脱敏任务，检索理解、受限文件修改、错误恢复、Skills使用各6个；每类4个优化、2个留出，即16/8。任务输入/初始文件、独立预期产物与判定、来源/域均版本化；不包含真实历史对话内容。
- 每例必需明确产物路径，按文件内容/结构和必要不变量判定；Skills用例结合真实激活事件和产物。包含正确/错误产物对照，不能只测“存在文件”或由mock返回成功。
- Task公开暂存目录只含公开材料；私有预期只由controller保存且不投影到原生请求。上游 task_files 只返回该公开目录。
- H2 adapter实现 tasks/domain_of/task_prompt/task_files/execute/delivered/trajectory/deliverable_files/deliverable_text/base_harness/validate_tree/score 及 Substrate 接口，固定上游版本，参数/返回值按实际源码核对；live优化不另造算法。
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

## 规格变更

- v2：独立方案审查指出技能审批来源不完整；补充受信任精确hash批准、过期拒绝及首次搜索限制。补清worker JSON/私有CLI边界、有效模型profile、未知计价和观测token预算终态。尚未冻结测试或开始生产实现。

