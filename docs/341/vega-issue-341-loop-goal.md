# 持久 Goal 与 Loop

版本：v1.1，2026-10-17。关联 [#341](https://github.com/puzige/vega/issues/341)。实现基线 `3705c687`；任务分支 `feat/341-loop-goal`。v1.1 澄清报告仅在成功终态生效、唤醒工具限 Loop tick、上下文失效边界，并显式列出所有归档接缝；产品范围不变。

Issue 的第三方静态调研是背景，不是需要复制的接口或实现。本文件冻结 Vega 自有行为。用户明确要求实现，并选择：目标和循环跨重启保存；重新打开 Vega 后手动恢复；Loop 支持固定间隔及模型安排下次唤醒。

## 1. 目标与范围

Vega 原生会话可以设置一个持续目标、一个周期任务，二者可以同时存在。Goal 在成功回合结束后继续推进，直到有证据完成、连续受阻、用户暂停或预算耗尽；Loop 首次立即执行，此后在到期且会话可运行时执行。普通消息不自动创建目标或循环。

使用原生 runtime 的现有权限、工具、provider、队列、压缩和取消链路，不另建执行器。外部 Codex backend 保持原行为，不显示可用的原生 Goal/Loop 入口；明确输入这些本地命令时说明仅适用于 Vega 原生任务并保留草稿。本卡不扩展 Codex app-server、系统后台服务、云端定时任务、cron 日历、独立 LLM 裁判、通知通道或 provider cache 优化。

## 2. 验收矩阵（先于实现计划）

自动验证均在 GitHub Actions 执行；本地只做格式、差异及静态检查。以下初始状态均为待 CI/手测，不以文档存在证明行为。

| 编号 | 前置与操作 | 可观察结果 | 层级与证据 |
|---|---|---|---|
| G1 | 普通原生对话发送文本 | 不创建 Goal/Loop，不新增相关状态行，不改变现有运行语义 | conversation/app 回归 |
| G2 | 新草稿或已有会话提交 `/goal 完成任务` | 持久保存目标，只启动一个正常权限下的回合；新草稿正常 materialize | store + app controller |
| G3 | 模型报告 progress，回合成功结束 | 空闲后自动续跑，目标内容与身份不变；不要求再次发送 | mock provider + controller |
| G4 | 模型调用完成工具且给出证据 | 状态 Complete，保留目标、累计用量和结果；不再续跑 | runtime + conversation |
| G5 | 同一 blocker 连续出现 | 前两轮继续，第三轮 Blocked；恢复后重新计数；不同 blocker/实际进展清零 | 纯状态机 + conversation |
| G6 | 模型没有报告进展或反复空回答 | 连续三轮无报告/无进展后停止自动续跑并显示可恢复原因；不以重复计划作完成 | conversation |
| G7 | 正预算创建，多个回合产生真实 usage | input+output 跨回合累计且每 run 只计一次；达到预算停止新回合；单次在途请求可以越过预算，UI 不承诺硬计费上限 | store + mock usage |
| G8 | 负数/零/溢出预算、空或超长目标 | 明确就近错误、保留草稿，不启动、不部分落库 | parser + controller |
| L1 | `/loop 5m 检查任务` | 首次立即运行；完成后等待到期；忙时不打断、不积压多个漏过 tick | fake clock + controller |
| L2 | `/loop 检查任务`，模型安排延迟 | 动态 Loop 使用模型设置的下一次时间；仅该 Loop 允许安排唤醒，默认 300 秒，边界 60–3600 秒 | runtime + scheduler |
| L3 | 固定 Loop 中调用动态安排、外线程/旧 generation 的调用 | 拒绝且不改变任何任务、时间或授权 | runtime + conversation |
| L4 | 模型给出 `stop:true` / 用户 clear / 到达 7 天有效期 | 停止后续 tick；状态真实反映结束或到期；恢复到期 Loop 需要用户主动操作 | conversation + store |
| L5 | noop tick | 仍保留真实历史与 usage，不新增独立成功通知；下次唤醒仍可到期 | controller/UI |
| S1 | Goal 与 Loop 同时到期、已有消息排队、正在输入或待权限/计划 | 一次最多一个 run；用户排队优先，其次到期 Loop，再 Goal；有草稿或 pending decision 时让位 | controller |
| S2 | 用户 Stop / pause，已有回合或 timer 未结束 | Stop 暂停该线程所有自动任务并取消回合；单项 pause 只撤销对应自动续跑（若该项拥有当前自动回合则取消）；旧 timer/result 不能重新激活 | controller + race tests |
| S3 | clear 后重建、重复终态、迟到工具/worker | generation/revision + run identity 防重入，旧结果不覆盖新任务，用量不重复计 | store + controller |
| S4 | 运行失败、凭据/权限/上下文受阻、用户取消 | 保留原因并暂停自动任务；不靠新循环绕过现有有界重试、权限或预算 | mock provider + controller |
| S5 | 重启、切换会话、归档或删除 | 重启后 Active 全部变为 Paused；不会追赶漏过 tick；同一打开的窗口切换会话不破坏运行；归档/删除取消自动任务并使旧回调失效 | store + app |
| S6 | 压缩/模型切换/重开 | 目标与调度从持久状态恢复，与 transcript 摘要独立；沿用新的模型/权限快照；不复活旧 revision | conversation + app |
| U1 | + 菜单及 `/goal`、`/loop` 输入 | 两入口可发现；鼠标、键盘、Esc 与既有 composer 行为一致；命令错误不发给模型 | GPUI/controller + 用户手测 |
| U2 | active/paused/blocked/complete/budget/expired/error | 紧凑状态行有目标/循环身份及相应动作；完整指令按需展开；错误和恢复入口直接可见 | GPUI + 用户浅深主题/窄窗手测 |
| R1 | 老库迁移、外键级联、重复恢复 | 只增新表，旧会话数据不变；迁移及恢复幂等；不可将 Store::open 当每次全局恢复触发点 | store |
| R2 | 工具注册/调度贯穿正常执行 | 使用现有工具生命周期/audit；不依赖 GPUI；凭据 redaction、取消、上下文预算、测试断言保留 | runtime integration + 静态 diff |

## 3. 用户入口和显示

- `+` 菜单提供「持续目标」「循环任务」，选择后向 composer 放入 `/goal ` 或 `/loop ` 并聚焦，保留已有文本作为目标内容；不立即执行。
- `/goal <objective>`；`/goal --budget <positive integer> <objective>`；`/goal pause|resume|clear`。无参数仅显示当前状态/简短用法，不调用 provider。
- `/loop [interval] <prompt>`；`/loop pause|resume|clear`。没有 interval 为动态模式。interval 精确支持正整数 `s/m/h/d`，范围 60 秒–24 小时；不静默四舍五入。命令词需完整 token 匹配，`/goals` 等保持普通消息。控制词作为目标可用显式 `--` 分隔符。
- 每项指令上限 16 KiB UTF-8，空值、非法预算/间隔给出局部错误且保留原草稿。图片/skill intent 不能静默丢弃；命令携带附件应拒绝并保留，普通提交路径不改。
- 一线程每类最多一个。已有未结束项时拒绝隐式替换，提示先清除；Complete/Stopped 可用新目标替换。用户 clear 仅删自动任务，不删会话历史。
- 仅在有 Goal/Loop 时，在 composer 邻近显示紧凑状态与 Pause/Resume/Clear 文本动作。两者分别控制；完整 objective/prompt 放按需详情。状态词使用用户任务语言，时间显示下一次执行，预算显示实际累计/上限。缺失 usage 显示未知，不伪造零。
- 已完成结果持续可查看但不叠加成功 toast；错误和待处理原因不能藏进 hover。沿用 ThemeColors、Typography、Layout 与共享 button/interaction，不引入新装饰、常驻说明或图标噪声。

## 4. 持久化与状态

新增递增 migration（以集成时最新编号为准，当前下一个为 0026），独立存储线程 Goal、Loop 及幂等 run 记账/claim。外键绑定 thread 并支持删除。Store 只存储，领域共享类型经 `vega_conversation::types` 对外。

Goal 保存 identity、revision、objective、状态、可选 token budget、已知 usage 总量/完整性、累计活动耗时、连续 no-progress/blocker、最近报告/原因及时间。状态至少 Active/Paused/Blocked/BudgetLimited/Complete。Loop 保存 identity/revision、prompt、固定/动态模式、interval、下一次到期、7 天 expiry、Active/Paused/Stopped/Expired、最近结果/原因。用户恢复必须显式发生：保留计量，清零受阻 streak；预算已经耗尽不能靠 resume 绕过预算，需 clear 后新建或显式提高预算（本版不另建编辑表单）。

在应用启动恢复入口将持久 Active 变为 Paused（原因「应用重启后已暂停」），清除在途 claim；不能在每次 Store::open/migrate 或每个窗口创建时全局暂停，不能把第二个窗口当应用重启。睡眠恢复只处理一次到期，不补发漏掉的 tick。归档停止该任务的自动运行，恢复归档不自动开始；删除依赖既有外键/取消链。

计量从该目标 revision 所属 run 的真实 provider usage 获取 input+output（cache 子项不重复加），持续回合含工具内部的 provider round trip。run identity 幂等；压缩如已有真实同 run 用量应按现有归属记账，不另行估算。无 usage 的失败不算免费；有预算且 usage 不完整时暂停并显示无法核实用量。预算在每个新自动回合前检查，允许已提交的 provider 请求越限，不更改现有请求预算系统。累计耗时仅加实际运行时间，不把 Loop 等待时间当工作耗时。

## 5. 调度与运行所有权

App controller 拥有计时器与 stream 连接，conversation 层提供纯决策/持久服务。后台 worker 处理新增 DB IO，UI 渲染/键盘热路径不做同步磁盘读写；不用通用新调度框架，不引入依赖。

复用 `AgentController` 的 per-thread active registry、queue generation、trusted action/preflight、permission queue 和取消机制。自动触发是显式来源，不能伪装成新的用户授权；短触发消息可进入历史，详细控制策略来自当前持久配置。用户指令、文件和 tool output 都是数据，不可通过包装字符串提升优先级。

触发顺序：真实用户待发送/queued/steer → 到期 Loop → 可续跑 Goal。任何 active run/preflight、未空 composer draft/IME、pending permission/plan/credential recovery、model selection/config mutation、更新安装中均推迟，不打断。所有条件满足后也需原子 claim 和现有 run registry 最终校验，至多一个自动 run。只维护一个待执行 tick，不为忙碌时间构造队列。后台会话保留可运行 stream 与绑定，不依赖当前选中页面；多窗口不得各自复制同一任务的调度所有权。

Goal 成功且仍 Active 后以短的可取消让步（至少 1 秒）续跑；Loop 首次/用户恢复后立即执行，之后以本轮完成时间加间隔确定下一次时间。动态 Loop 默认 300 秒，`schedule_wakeup` 可覆盖为 60–3600 秒；固定 Loop 拒绝此工具改间隔。7 天 expiry 从创建/用户 resume 计算，模型不能延长 expiry。过期保留记录并可显式恢复。

失败处理复用本轮 provider 重试策略，耗尽后暂停自动任务并显示原因；本卡不增第二套网络重试计数。Stop 暂停本线程所有自动任务并取消当前 run；单项 pause/clear 取消其拥有的自动 run，但不取消独立用户发起的 run。所有 timer、异步读回、tool mutation 和终态均校验 thread + identity + revision/run claim；撤销后结果只能用于历史审计，不能恢复调度。

## 6. 模型契约

原生运行已有显式 Goal/Loop 时才注入受限能力和当前自动任务数据，不从普通请求推断自动模式。用 `RuntimeToolConfig` 上专用 headless hook 将持久服务接到已有工具注册/prepare/execute/audit；runtime 不依赖 conversation/store/UI，领域结构仅从 conversation types 给 app/UI。

- `get_goal {}`：读取本线程已启用的目标、真实计量、状态和剩余预算。
- `update_goal {status, progress?, reason, blocker?}`：`status` 为 `active|complete|blocked`。每个 run 最多接受一次进展报告，重复调用不能累加 streak；Active 的 `progress` 为 `progress|waiting|no_progress`。reason 必须有非空证据。waiting 必须给出本轮核实仍活跃的具体等待对象/证据，不把计划或空转视为等待。complete 必须陈述逐项完成证据，prompt 要求先检查当前事实；这是模型自审，不冒充独立裁判。blocked 仅同一 blocker 连续三轮时生效；前两轮可记录该次报告但不得停止。progress/waiting 清零 no-progress；不同 blocker 重新计数。模型不能 pause/resume/替换目标、提高预算或给别的线程改状态。
- `schedule_wakeup {delay_seconds?, reason?, noop?, stop?}`：仅本次 Loop tick 暴露并接受该工具，只有当前动态 Loop revision 可调下一次，秒数整数、60–3600 越界明确拒绝。`stop:true` 对当前 Loop 停止后续调度（固定 Loop 也允许停止，但不可调间隔）。普通消息/Goal 回合不得修改尚未触发的 Loop；这些回合也不把待执行的 Loop prompt 当作当前任务注入。每次只保存一个下一次时间；历史回放/旧 run 不可改变它。noop 只记录未变，不删除真实输出或计量。
- 没有有效 Goal 报告的成功回合按 no-progress 处理，连续三轮进入 Blocked 并给出可恢复原因；不同失败由上一节停止，不混淆成业务完成。

完整 objective/prompt 始终作为 JSON data 边界注入，与当前模型上下文预算共用现有装配/压缩流程；不自行截断、不创建第二套文件附件或伪装 system authority。实际注入的任务 identity/revision/data 必须进入既有 replay/context fingerprint，不能仅以工具 schema 相同重用旧目标上下文。工具报告绑定本 run 先暂存，只有成功终态才应用进展、完成和下一次调度；本轮后续失败时不能显示目标已完成。无论工具调用是否成功，本轮结束都只从持久状态决定续跑，不读自然语言中的“完成”字样。Goal 与 Loop 同时存在时，本轮可同时报告目标进展，但 Loop 的停止不自动清除 Goal，Goal 完成也不自动删除 Loop。

## 7. 实现顺序与文件归属

同一专职实现 agent 是唯一代码写入者，Codex 主会话只改规格/审查/集成。

1. `vega_store/src/automation.rs`（可拆分）、`migrations/`、store `lib.rs`：持久状态、原子 claim、幂等记账、恢复/级联；新增迁移可精确更新既有表数断言，不能删除原断言。
2. `vega_conversation/src/automation/`、`types/automation.rs`、模块导出：命令解析、领域服务、纯时间决策、报告/预算/恢复。对接 `agent/entry.rs`、`pipeline.rs`、`events.rs`/`persistence.rs` 的现有正常回合链；`threads.rs` 的 `set_thread_status` 与 `update_thread` 两个归档入口均需撤销自动任务，删除依赖现有事务及外键清理。
3. `vega_runtime/src/agent/automation.rs`、`mod.rs`、`mcp_registry.rs`、`tools_exec.rs`、必要的 `loop_.rs`：专用受限 hook 与工具生命周期，不绕过现有审计/取消/credential redaction。
4. `vega/src/automation_controller.rs`、`window/automation.rs`、`window/agent.rs`、`pending_messages.rs`、`session.rs`/`navigation.rs`、`app_agent.rs`、启动/模块导出、`thread_reload.rs`：命令、持久 worker、调度所有权、停止/删除/恢复、状态投影；更改限制在这些功能接缝。
5. `vega_ui/src/conversation_stream/automation.rs`、composer/actions/render/mod 相关接缝、必要的 theme token：菜单、命令与状态动作及键盘行为；相关单元/进程内/GPUI 测试。
6. 交付报告 `docs/341/vega-issue-341-loop-goal-delivery.md`：实际文件/静态验证/待 CI、风险、规格偏离（应为无）。不要自行改规格；契约冲突先报告主 agent。

不改仓库工作流、依赖、无关 UI、既有 provider 实现或用户运行时配置。不新增代码注释或 issue 编号测试名。所有外部进程/真实网络测试以 mock 边界替代，仅 Actions 执行测试。

## 8. 验证、手测与回滚

本地：`git diff --check`、`cargo fmt --all -- --check`、针对新增代码的静态红线检查。不得运行 Vega、cargo check、Clippy、test/nextest、构建或打包。

Actions：`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`（覆盖 store/conversation/runtime/app/UI 新增用例），required check `check (fmt, clippy, test)` 对最新待合并 head 成功才合并。保留首次失败日志并修复实现，不降低测试。

用户手测：设置一个可验证的 Goal，确认至少一次续跑与完成；暂停/恢复/Stop；用 `/loop 1m` 验证立即一次与下次执行，输入期间让位；动态 Loop 安排唤醒/停止；切换会话、退出重开后确认暂停且保留内容；清除后无再次执行。浅深主题与窄窗口检查状态动作、详情、键盘可达和错误恢复。

回滚通过 revert 本卡 squash commit；保留新增表及用户记录，不降级或删除数据，旧版本不识别的自动任务不会被本版定时器执行。合并后 Issue 保持开放并转 In review，保留本卡分支/worktree，等用户手测。
