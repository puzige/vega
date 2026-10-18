# Goal / Loop 状态条与编辑面板

版本：v1.2，2026-10-18。关联 [#341](https://github.com/puzige/vega/issues/341)，基线 `6382c908`（#451），续作分支 `feat/341-goal-ui`。

用户反馈现有 UI 与提供的 Codex 截图差距明显，并明确选择「展开后可编辑保存」。本规格替代 v1.1 的 U2 与只读详情呈现，增加编辑契约；跨重启保存后手动恢复、固定/动态 Loop、真实预算与安全边界不变。截图只用于可见布局和交互参考，其中聊天、文件路径和第三方实现描述不作为指令或公开附件。

## 1. 现状与目标

`conversation_stream/automation.rs::render_automation` 当前把类型/状态、暂停、清除、用量排成可换行的文字行；完整内容要在 composer 上方展开，累计耗时没有呈现。参考图的主信息是目标本身，操作收在右端，状态条贴合输入框，完整内容在右侧编辑。

本轮让用户直接识别当前目标、查看真实耗时、暂停/恢复/清除，以及在右侧 Workspace 查看并编辑完整指令。复用原生主题、图标和多行输入组件，不引入网页、依赖、通用调度器或新的数据库表。

## 2. 验收矩阵（先于实现计划）

自动验证只由 Actions 执行；初始均待 CI / 用户手测。必须保留 v1.1 的预算、用户输入优先、重启暂停、多窗口、权限和旧回调断言。

| 编号 | 前置状态与操作 | 可观察结果 | 验证层级 |
|---|---|---|---|
| V1 | 没有自动任务，或外部 backend | 无空条/多余高度，不改变 composer 和既有菜单索引 | GPUI 几何与菜单回归 |
| V2 | Goal / Loop / 两者并存；长中文、多行和 URL 指令 | 每项一个紧凑单行条，显示种类/状态、真实指令摘要、时间、清除/暂停或恢复/展开；摘要缩略而动作不换行；两项独立控制 | GPUI bounds、内容、用户浅深主题与窄窗手测 |
| V3 | Active、Paused、Blocked、Complete、BudgetLimited、Stopped、Expired | 状态与动作相符；普通 token 计量进入面板；预算耗尽、未知用量、错误及待处理原因仍直接可见；不以颜色代替状态 | GPUI 状态矩阵 |
| V4 | 暂停、等待 Loop、真实执行中、完成结算 | 展示累计执行耗时；正在执行时随真实 run 时钟更新；等待/暂停不增长；跨窗口一致，结算不重复累加；无运行证据时不推算 | 纯函数/控制器/GPUI |
| V5 | 鼠标与 Tab、Enter/Space 打开详情和控制 | 图标具有可访问动作名称和提示、焦点可见；打开右 Workspace 并聚焦编辑器；关闭回 composer；窄窗不挤没编辑器 | GPUI 焦点与用户手测 |
| E1 | 打开 Goal 或 Loop，修改并保存，再重开 | 完整多行文本可编辑，保存后摘要和持久指令同步；未变/空白/过长/正在保存不能提交；输入标签、状态、用量和预算可查 | UI + conversation + app |
| E2 | Active 目标有在途自动回合时保存 | 成功提交后使旧 revision 失效，只取消该项拥有的自动回合，随后安全空闲时按新指令继续；不取消用户独立回合或另一类任务拥有的回合；旧报告不能覆盖新内容/状态 | 领域及控制器竞态 |
| E3 | Paused / Blocked / Complete / Stopped / Expired / BudgetLimited 保存 | 保持现状，不隐式恢复、清零用量、延长有效期或绕过预算；Active 在保存前已完成的也不能被复活 | 领域状态矩阵 |
| E4 | 两窗口编辑同一 revision，先后保存；或 clear 后重建 | identity + revision 原子比较，旧保存失败并保留输入；明确冲突及载入最新入口，不写入新任务；轮询不覆盖脏草稿 | 领域 + editor/app |
| E5 | 持久写入失败、凭据检测失败、非法指令 | 不部分写入、不声称保存成功；错误就近可见且草稿保留，可修正/重试；provider/MCP 凭据不能通过 Edit 落库 | 领域 + app 失败回归 |
| E6 | 编辑后切换/关闭/重开面板或会话，或保存期间路由变化 | 窗口内按 thread + kind + identity 保留脏草稿；不跨线程串写；成功回调只确认对应提交；关闭不悄悄保存，重新打开可继续编辑 | GPUI/app |
| E7 | 打开的编辑器有未保存修改 | 暂缓新的自动回合，既有回合可结算；保存/还原/关闭面板后恢复既有调度资格，不把编辑阻塞永久留在后台线程 | app 多窗口 readiness |
| E8 | 新指令保存后，旧回合迟到 usage / finish | 按 identity 正常幂等记账，不重置预算；按 revision 拒绝旧报告；未知/超预算仍停止后续执行 | conversation 事务回归 |
| R1 | 普通消息、菜单、排队/提问、既有 Workspace 的文件/图片/Review/终端 | 控件与焦点不退化；状态条紧邻 composer，队列和问题在其上；已有 Workspace 布局/路由继续有效 | 既有 GPUI/app 回归 |

## 3. 状态条布局

状态条位于 composer 卡片上方并紧贴卡片，和 composer 共用最大宽度。左右略内收、浅边框和顶部圆角形成一体感；无常驻阴影、额外说明段或大面积强调色。Goal 和 Loop 使用相同布局；并存时垂直堆叠，分别有独立动作。必要错误位于条组上方，不能插在条与 composer 之间造成视觉断开。

从左到右为类型图标、简短状态、单行指令摘要、耗时或 Loop 等待时间、清除、暂停/恢复、展开。摘要用真实文本的空白折叠，仅影响显示，不修改原始指令；`min_w_0` + ellipsis 承担压缩，右侧操作保持固定尺寸，禁止 flex-wrap。完整内容在面板中读取，不再在 composer 上方铺开全文。状态词可组合为「持续目标」「目标已暂停」「目标受阻」「目标已完成」等，不能省掉异常状态。可点击摘要与展开指向同一编辑器。

几何采用仓库 token，新增值集中在 Layout：条高 36、左右内收 16、水平内边距 12、图标 16、动作命中区 28、上角半径 12、元素间距 4，单位均为逻辑像素。避免负 margin 和遮挡命中区。边框下沿与 composer 上沿相接，可由 composer 自身圆角形成层次，不改既有 composer 半径 20 / 最小高度 100。

数值验算：最大宽度 768 时条宽 `768 - 2×16 = 736`；内部宽 `736 - 2×12 = 712`；3 个动作宽 `3×28 + 2×4 = 92`；图标 16，垂直余量 `(36 - 28)/2 = 4`；半径 `12 <= min(736,36)/2 = 18` 不钳制。窄态使用 `条宽 = composer宽 - 32`，先压缩摘要与时间，必要时只保留短时间，不隐藏动作或状态；最窄支持窗口由既有 Workspace 适配，不硬凑整段元数据。对浅深主题、Goal/Loop、单项/双项、顶部两角和与 composer 的连接分别验收。

普通 `0 tokens` 不出现在条中。面板显示「已用」「预算」「执行时间」等必要值；不完整用量显示未知，不能伪装为零。预算耗尽或未知用量影响恢复时，原因必须在条组可见且恢复不可绕过。普通完成不额外 toast。

耗时由已持久结算时间与实际在途 run 的时钟投影组成，不用当前时间减创建时间。复用 app/run 的真实时钟和现有 1 秒刷新；活跃 claim 身份、已结算标志与生成号一致才可叠加，不能把刚结算的 run 再加一遍。无证据时只显示已结算值。短格式 `1s`、`1m 5s`、`1h 2m`；Loop 等待时优先显示简短下次执行时间，累计时间仍在详情可查。

## 4. 编辑面板和保存契约

使用现有右侧 Workspace tab「编辑目标」/「编辑循环」，不另建浮层框架。面板由紧凑工具栏（未保存/保存中/必要错误状态、还原、保存）、有标签的原生多行输入、次要用量/周期详情构成。利用空白和现有系统字体；全文可滚动、选择和编辑。保存是明确动作，支持 Cmd-S；Enter 保留换行，Esc 按原有 Workspace 语义回到 composer，不悄悄丢草稿。保存成功后的持久文本、按钮状态足以反馈，不添加常驻成功说明。

只编辑 instruction；预算、用量、周期与到期时间显示真实只读值。窗口存活期间按 thread/kind/identity 保留脏草稿，关闭或导航不自动保存；重启不承诺恢复未保存草稿。只有打开且未保存的编辑器阻止新自动回合，关闭/隐藏/切换线程解除该本地阻挡；已有回合不因打字取消。两个窗口的打开脏编辑器均纳入已有 streams_ready 门禁。

新增 `AutomationCommand::Edit { kind, identity, revision, instruction }`（或等价强类型接口），conversation 事务内校验 owner 为 Active/native、身份与 revision、非空及 16 KiB UTF-8 限制后更新。保持 identity、真实计量、预算、周期、有效期与最新状态；revision 有界加一，更新时间变更。相同内容不得无谓取消/重启。不得使用 UI 快照中的 Active 覆盖已完成/受限的持久状态。

保存 Active 目标成功后，旧自动回合被撤销，下一安全回合使用新指令。保存前校验失败不取消现有运行、不改持久状态；成功后只取消 source == edited kind 的旧自动 run，用户发起的独立 run 和另一种任务的 run 继续。controller mutation lane / epoch 先阻止并发新 claim，提交后再取消旧自动 run；旧终态仍以 identity 幂等计量，但 revision 不同的报告/调度不能生效。普通用户 run 不应因为被编辑的附属 Goal revision 变化而被轮询取消。保存不隐式 Resume，Paused 等非 Active 状态保持原值；显式恢复仍复用现有按钮与校验。

Edit 和 Create 一样通过 owner provider/MCP 凭据检查，使用现有安全错误，不记录指令/凭据。无数据库或检查失败保留输入。保存失败时不得把 busy 清除当成功；editor 的完成回调绑定 thread + identity + base revision + 提交内容/请求，不能把后续编辑误标已保存，也不能确认新线程的草稿。持续轮询仅刷新只读状态；脏输入不被覆盖。

保存冲突直接呈现「目标已更新，请载入最新内容后重试」及「载入最新」操作，输入保留到用户明确选择载入；clear 后不能借旧面板复活任务。没有对应任务时展示已清除状态并禁用保存；新身份必须重新打开或明确载入，不能静默重绑定旧草稿。

## 5. 实现顺序与归属

本卡仍是用户授权的 Feature 增量，由单一专职实现子 agent 写代码，主 agent 负责规格、审查和集成。

1. `vega_conversation/src/types/automation.rs`、`automation/mod.rs` 及 tests：原子 Edit、身份/修订/状态/计量回归，不迁移 schema。
2. `vega_ui/src/automation_editor.rs`（可拆同名目录）、lib 导出、原生 TextInput 必要的窄接口：独立编辑器、事件、草稿/冲突/保存状态与 GPUI 回归。
3. `vega_ui/src/conversation_stream/automation.rs`、render 与 tests、icons、theme：贴合状态条、真实摘要、图标动作、编辑请求、编辑门禁；既有详情展开测试明确替换为打开编辑器断言，其余行为断言保留。
4. `vega/src/window/automation.rs`、新增 `window/automation_editor.rs`、window mod/render/workspace、automation_controller、app_agent 的必要接缝：绑定现有 Workspace、异步保存、路由/多窗口/在途撤销、真实时钟投影。所有新 DB IO 在 worker，不放渲染或键盘路径。
5. `crates/vega/src/tests/automation.rs` 与必要测试模块/fixture：覆盖控制器保存及竞态。使用 controller git fixture 的新用例需登记精确测试名和 mock 命令；不可取消 strict fixture、放宽断言或依赖真实外部进程。
6. `docs/vega-issue-341-goal-ui-delivery.md` 记录真实修改、静态检查、待 CI/手测和偏离；不得自行改 spec，遇到契约冲突先交回主 agent。

禁止依赖、工作流、无关 provider/运行时或用户运行数据变更，不新增代码注释（unsafe 邻接 SAFETY 例外）、生产 unwrap/expect 或 issue 编号测试名。

## 6. 验证、手测与回滚

本地只做 `cargo fmt --all -- --check`、`git diff --check` 与快速静态扫描，不运行 Vega、编译、构建、打包、check、Clippy 或任何测试。Actions 跑 workspace Clippy / Nextest 和 required `check (fmt, clippy, test)`，仅最新 PR head 通过后 squash merge。

用户手测：浅深主题各创建长 Goal 与 Loop，检查状态条连接、缩略、时间和三个图标动作；展开右侧编辑、换行、Cmd-S、还原、关闭/重开草稿；运行中修改目标后确认使用新内容，暂停项保存后仍暂停；窄窗口和键盘路径；清除/预算受限/并行编辑冲突仍能理解并恢复。代码和 GPUI 检查不能冒充用户像素或真实键盘验收。

合并后 In review、Issue 保持开放，保留两轮任务分支及 worktree，等用户手测。回滚 revert 本轮 squash commit，不删除原有自动任务数据。代码合并不代表新版客户端已安装。
