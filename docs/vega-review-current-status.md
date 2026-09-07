# Vega 当前本地交付状态

## R17 正式Logo接入（2026-09-08，已完成）

用户定稿01蓝色单星笑脸作为Vega Logo，Luna max实现已集成本地分支。正式浅深/单色SVG及生产ICNS引用完成，root完整门禁988/0/0、fmt、strict lint、build通过；已安装与当前验收副本图标更新并严格签名验证，功能代码不变。Finder与实际Dock截图确认新图标，Launchpad缓存已刷新且176条布局不变。保持832/1024底板、单星和微笑光标。详见[R17验收](vega-r17-acceptance.md)。

并行工作树清理已完成：`~/Workspace/` 与 `~/Workspace/worktrees/` 共移除42个闲置工作副本（目录原分配空间合计约1.37GiB），保留全部分支头和恢复命令，修复12处旧账号路径链接；8个未整合或原仓库缺失的副本保留，主仓库/当前集成树保留；本轮Logo执行树集成后另行移除并保留分支。清单与恢复说明在外部验收目录 `cleanup-20260908/README.md`。此前各轮状态均为历史交付。

## R16 标识重新设计（2026-09-06，B方向及macOS精修交付）

追加[02修长星形切口预览](../assets/logo/explorations/r16/vega-blue-02-slender-spark.png)已交付，用户要求02也使用修长星形。主图保持整体 `>`、单颗负空间星孔、蓝色/柔光；完整提示词保存，缩略几何仍需生产矢量统一。未推定替代此前01选择，未改安装资源。

当前用户选择01：[单星微笑精修稿](../assets/logo/explorations/r16/vega-blue-01-single-spark-smile.png)已交付。用户明确只要一颗星，采用修长四角星、轻微上扬光标与终端 `>`；主Agent目视确认两枚主图和四枚缩略图均为单星。蓝色/柔光保持，完整内置imagegen提示词已记录纠正。下述02推荐及尚未选择状态是历史，当前以01为准，生产资源未替换。

最新形状探索：[蓝色三轮廓对比](../assets/logo/explorations/r16/vega-blue-shape-studies.png)已交付。用户认可蓝色后要求换形状；01紧凑终端星标、02星形切口提示符、03终端折线V，均含浅深和缩略构图。主Agent推荐02，用户尚未定稿；完整内置imagegen提示词已保存。下述B蓝色稿作为材质/配色基线，生产资源未替换。

当前配色：[蓝色精修稿](../assets/logo/explorations/r16/vega-logo-b-macos-terminal-blue.png)已交付。用户要求从绿色切换为合适的蓝色；浅色宝石蓝、深色冰蓝，保留 `>_`、导航星与已认可高光/柔和材质。主Agent目视确认双外观及四枚缩略图；完整内置imagegen提示词已保存。下述绿色版本是历史迭代，生产矢量、安装资源和UI主题代码尚未改变。

最新：[终端识别精修稿](../assets/logo/explorations/r16/vega-logo-b-macos-terminal.png)已完成。用户认可高光/柔色，要求强化左侧终端；新稿将斜线连成完整 `>` 并增加短横光标，保留右侧导航星及材质。主Agent已目视检查，完整内置imagegen提示词和上一版均保存；本次仍是设计预览。

依据仓库Logo理念及PRD的导航/执行定位，完成三方向品牌探索。用户明确选择第二版B（终端折角与星芒），随后要求结合macOS设计风格。[B精修稿](../assets/logo/explorations/r16/vega-logo-b-macos.png)已交付：浅/深底板、祖母绿/薄荷绿图形、细高光和轻微浮起层次。[设计方案与取舍](../assets/logo/explorations/r16/README.md)、[规格](vega-r16-logo-redesign.md)及完整内置imagegen提示词已保存；初轮A推荐仅为历史记录。

本次是新品牌标识的设计稿，主Agent已目视检查两种外观与缩略构图，尚未进行生产矢量制作/安装，不将生成的缩略图视为原生小尺寸验收。当前安装图标、运行副本和完整软件门禁仍以R15交付为准。下一步沿已选B方向制作统一SVG/ICNS，再做Dock/Launchpad验证。

## R15 本地交付（2026-09-06）

macOS Dock/Launchpad的图标偏大已修复。图标/打包代码 `9b8bb5e`，最新集成验证HEAD `4e8e61d`；当前唯一原生副本为 `/Users/puzige/Workspace/vega-review-20260905/native-r15/Vega.app`，沿用R14功能。直接AppKit导出透明画布，底板832/1024、各边96px留白，10档表示正确；实际Dock和启动台搜索均显示正常比例。

安装版 `/Applications/Vega.app` 已备份并仅更新图标/重新签名，程序代码段与UUID保持；启动台现存入口和175条布局不变，缓存已自动刷新。**最终完整workspace988/0/0，fmt/strict all-target clippy/build通过。** 首轮发现旧HTTP测试fixture同步join与无写超时死锁，按窄规格仅修测试收尾、保留生产超时/断言，红灯与sample保留。[R15验收](vega-r15-acceptance.md)记录系统截图、签名、回退和完整门禁。

下一批仍为外观字号/代码显示与通知；无Keychain或真实provider请求，性能延期。未push/master合并；安装副本功能版本保持原样。以下R14及更早章节为历史交付。

## R14 本地交付（2026-09-06）

最新应用代码 **0d7056a** 已整合到 `codex/vega-review-integration`，签名副本为 `/Users/puzige/Workspace/vega-review-20260905/native-r14/Vega.app`。三个 Astra medium 实现，主 Agent 完成审查、集成和真实单实例验收。当前1280×750浅色工作区，打开自有文件夹任务直接显示实际 `main`。以下R13及更早章节为历史快照。

用户新报的三项问题已修复：侧栏隐藏后有常驻恢复按钮；打开文件夹自动显露、展开原项目组且重开去重；输入区自动读取当前分支，隐藏侧栏也跟随实际 HEAD。模型设置增加供应商启停/排序、候选发现与明确导入、逐模型有界真实连接检查，并完成供应商列表/详情、小窗口滚动和异步状态清理。

**最终 workspace 988/0/0，fmt、strict all-target clippy、build通过。** 原生验证成功/401/15秒超时/取消的 loopback HTTP，模型导入、键盘鼠标、两个文件夹任务归属、重开及重启保持。原7项目/19任务全字段、消息/工具/用量和组数据保留，UI仅新增2自有项目/2任务，0消息。详见 [R14验收](vega-r14-acceptance.md) 与 [103项对照的R14增量](vega-r11-zcode-button-matrix.md)。

首次982/1的canonical路径测试修正、原生焦点/URL收缩/旧状态问题均有前后证据。R12 Git时序根因未声明修复。下一轮优先外观与通知设置，再补真正的上下文/视觉能力；不访问Keychain、不恢复对话成本指标，性能延期。当前使用自有测试配置，真实收费供应商未验；未push/master合并/安装替换。

## 2026-09-06 R13 最新本地交付

最新应用代码 **60bd44a** 已整合到 `codex/vega-review-integration`，签名原生副本为外部验收目录 `native-r13/Vega.app`。两名Astra medium实现，主Agent完成参考端点验、契约审查、集成与实际单实例验收。当前1280×750浅色三栏，右栏为真实README预览。详见[R13验收](vega-r13-acceptance.md)与[103项对照的R13增量](vega-r11-zcode-button-matrix.md)。

新增项目/分组/时间线视图、创建/更新时间排序、5行及更多、全局/单项折叠、项目顺序、七色跨项目任务组、组内排序、拖入/拖出及菜单替代、原子组内新建、解散保留任务、归档恢复原组和跨重启组织状态。原生发现并修正未分组落点与Escape菜单关闭；审查修正晚到创建抢设置页面。

**最终完整workspace：964 passed / 0 failed / 0 ignored**；fmt、strict workspace all-target clippy、build通过。首次962/2/0由两处旧六表清单断言导致，已按规格精确更新，原日志保留。R12 Git stdout-overflow/TimedOut本轮未复现，相关生产路径未改，不能标记根因已解决。

原生12组检查通过；8个自有任务、0消息，原有11任务全字段和14消息/24工具/15用量记录与迁移前备份完全相同。没有Keychain/真实凭据访问或真实provider调用，性能继续延期。下一轮优先模型发现/连接检查、外观与通知设置；其余功能仍按矩阵排队。未push/master合并/安装版替换。以下R12及更早为历史快照。


## 2026-09-06：R12 任务操作、导航与实时分支

最新代码 **f2340d2** 已整合到 `codex/vega-review-integration`，外部原生副本为 `native-r12/Vega.app`。三个Astra medium实现，主Agent审查、集成和真实单实例验收。当前展示1280×750浅色三栏与真实README预览；详情见[R12验收](vega-r12-acceptance.md)。以下R11及更早章节为历史快照。

任务更多菜单补齐重命名、未读、项目Finder/路径、会话ID和设置；240px菜单修复鼠标穿透。新增前进/后退、连续Cmd[/]、设置返回、失效历史跳过及窗口内草稿恢复。项目后缀读取实时HEAD，独立注册worktree、外部checkout和detached经实窗验证。R10集中统计、无Keychain和去对话指标保留。

**最终完整workspace为948通过/1失败/0忽略。** 唯一失败在未改动Git故障注入测试：stdout-overflow预期OutputTooLarge、实际TimedOut；单独一次通过不替代全量失败。此前c53752e完整948/0/0。fmt、strict all-target clippy、build通过；最终菜单指针回归与实窗复验通过。原生发现的焦点丢失、标题栏覆盖和菜单穿透均有修复证据，Git时序原因继续跟踪。

性能延期，真实provider/系统IME未验收；未push、未合并master、未替换安装版。下一批优先项目分组/筛选/排序/收起全部及模型连接测试/发现，再补外观与通知设置。

## 2026-09-06：R11 逐项按钮对照与首批功能

最新应用代码 **4a10b74** 已整合到 `codex/vega-review-integration`，可运行副本为 `/Users/puzige/Workspace/vega-review-20260905/native-r11/Vega.app`。旧 Vega 实例已逐次退出并核对，当前只运行此新版。三组 Astra medium 实现，主 Agent 完成参考端点验、审查、集成及实际 macOS 键盘/鼠标验收。详见 [103 项 ZCode 对照](vega-r11-zcode-button-matrix.md) 和 [R11 验收](vega-r11-acceptance.md)。下列 R10 及更早内容均为历史快照。

本轮补齐 Cmd+K 四分类搜索、真实任务/项目文件查询与只读预览；Cmd+O 打开并激活项目，修复 Cmd+N；真实 PTY 终端、多会话/项目隔离、右/底栏移动、显隐、Ctrl+C、resize 与关闭回收；输入区加号、@file、模式命令及准备/流式 Stop。原生反馈中的候选框遮挡、窄栏标签、Markdown 换行、搜索面板高度与分支弹层裁切均已修正。R10 设置集中统计、去对话指标和无 Keychain 本地凭据继续保留。

960×600 浅/深色及 1280×750 原生验收已覆盖搜索/任务切换/草稿、目录选择器新建/重开/取消、只读预览及明确错误、真实终端状态保持/打断/尺寸/回收、真实 Git 分支切换并恢复 main。最终布局修正的 8 app + 4 UI 回归、完整 all-target strict clippy、fmt 和 build 通过。Stop 的生产入口测试仅在供应商网络边界使用 MockProvider；缺少重新输入的 API Key，真实远端原生回复/取消未验证，中文 IME 候选窗和真实用户 shell 配置未验收，性能继续延期。

**最新完整 workspace（f0ea6e5）：925 passed / 3 failed / 0 ignored。** Diff retry GitFailed、descendant pid 未出现、InitialDrain process_control_failed 的失败完整保留。相关 Git 生产路径未在 R11 改动，三个 exact 各一次通过，不足以证明根因或将全量标绿。后续单独跟踪 Git 时序诊断。3782102 较早的退出 UI 引用失败另以 preflight worker 仅传数据、前台保留精确 fence 修正并验证，但不冒称原始偶发竞态已完整复现。

对照表不是 103 项全部完成。下一批优先任务更多菜单/导航/分组和侧栏分支缓存，再做模型连接测试/发现、外观/通知设置；附件/编辑分叉/队列、辅助对话、浏览器、MCP/技能/插件、自动化和远程工作区仍有明确缺口。未 push、未合并 master、未替换安装版。

## 2026-09-06：R10 集中使用统计与本地凭据

最新应用代码 `46e76b3` 已整合到 `codex/vega-review-integration`，可运行验收副本为外部验收目录中的 `native-r10-usage/Vega.app`。当前仅此实例运行，停留在设置的使用统计页。详细证据见 [R10 主 Agent 验收](vega-r10-acceptance.md)。下列 R9 及更早章节保留为历史快照。

按实查 ZCode 使用统计页实现总览、年度活动热力图、每日/每周/累计切换、近7/30日趋势、点击查看具体日期用量、模型占比及刷新。使用真实持久化记录，当前26.7k Tokens、估算US$0.016625与数据库核对一致；未计价记录单独标明。对话顶部、输入区和回复尾部的成本/Token统计均已隐藏，计量与失败/中断提示保留。

macOS Keychain 凭据后端已移除，改为显式配置根下独立明文凭据文件，Unix目录0700/文件0600。不会读取或迁移旧Keychain项，需要在设置中手动重新填写Key。真实客户端已验证缺失提示、草稿保留、零新增消息，以及非真实测试值保存/重启恢复；没有宣称真实模型请求已通过。

整合全工作区913通过、0失败、0忽略；后续小范围显示修正的专项UI回归、strict clippy和build通过。主Agent实窗检查960×600和1280×750、浅深色、统计按钮、具体日期数值、刷新、设置返回和历史对话去统计。性能继续延期，未push、未合并master、未替换安装应用。

## 2026-09-06：R9 三栏工作区与底部面板

当前本地代码 `436a8eb` 已整合到 `codex/vega-review-integration`。可运行副本为 `/Users/puzige/Workspace/vega-review-20260905/native-r9-panels/Vega.app`。以 ZCode 为主参考，采用用户提供 Codex 截图补充三栏及底部布局；不吸收 Antigravity 功能。

对话保留在中央，Review 与现有 artifact 预览进入可移动工作区标签；支持右侧/底部移动、隐藏重开、关闭标签、拖拽尺寸、最大化还原、菜单及标签键盘激活。窄窗口使用模式/权限/推理图标，Diff 工具栏和文件名截断已打磨；默认设置新增可保存主题和侧栏控制。打开设置返回后 Review 经真实控制器恢复，失效预览关闭，任务切换保留原有路由隔离。

主 Agent 实窗核对紧凑/大窗口、右侧/底部 Review、拖拽、隐藏/重开、放大/还原、草稿保留、权限菜单、主题、设置返回和最终菜单 Tab+Enter。独立复跑新增生产根视图回归1/1通过。最终全量901通过/1失败/1忽略；唯一失败仍为既有 Diff retry `GitFailed`，保留内容1文件+1，聚焦原有Diff5/5通过不替代失败。fmt、strict workspace clippy和workspace build通过。

交互式PTY终端尚未实现；浏览器和辅助对话能力也未在本轮添加。面板尺寸/标签状态仅当前窗口内保留，不声称跨重启恢复。多预览同时原生验收、真实IME、真实provider与性能尚未验证；性能按用户要求延期。未push、未合并master、未替换安装应用。R8以下为历史交付。

## 2026-09-06：ZCode 视觉对照版

最新本地整合代码为 `f737d58`，R8 已 fast-forward 到 `codex/vega-review-integration`。可运行副本为 `/Users/puzige/Workspace/vega-review-20260905/native-zcode-parity/Vega.app`；[R8 规格、验证与差异](vega-r8-zcode-parity.md) 为此轮权威记录。以下 44b87f3/native-delivery 是上一轮快照。

本轮按本机 ZCode 实窗调整330px侧栏、透明标题栏、圆角主面板、居中新任务输入区、紧凑模式/权限菜单、会话底部输入区和设置分区。原生对照验证960×600及960×750、浅/深色、文件引用、菜单关闭和设置返回。修复了全局 Escape 绑定抢占局部菜单及设置初始焦点问题。

最终UI136、theme6、Settings生产E2E1通过；fmt、strict workspace clippy、workspace build通过。本轮较早完整workspace901通过/0失败/1忽略，不冒充最终HEAD全量结果；后续app包59通过/1失败，既有Diff retry `GitFailed`间歇问题保留，孤立通过不算根因修复。性能继续延期。

此版是现有Vega功能范围内的视觉复刻，不是整套ZCode逐像素一致：Vega身份、工具安全摘要卡、无线程创建入口及未实现的ZCode服务功能存在明确差异。未push、未合并master、未替换已安装应用。

2026-09-05 · 主 Agent：Codex · 实现：早期原生 Luna / max；按用户最新指令切换 Astra / medium

## 当前本地交付（最终代码44b87f3）

本地联合分支 `codex/vega-review-integration` 已完成本轮工程交付。最终代码44b87f3；完整workspace快照a662dfc；详细Thinking原生验收来源fccca86。最新可运行副本为 `/Users/puzige/Workspace/vega-review-20260905/native-delivery/Vega.app`，包含44b87f3，已签名校验并通过启动、独立配置与Thinking档位显示检查。没有 push、master 合并、发布或替换已安装应用。

最新代码 `44b87f3` 已合入 Astra medium 的确定性排空竞态修复：原逻辑在 grace 到期后会遗漏已排队的 reader 结果；先非阻塞收取最多两路，再按剩余 grace 等待。红灯2条失败、修复后Git相关149/149通过，包含缺失pipe、取消、超时与回收；strict crate all-target clippy、workspace fmt与最终workspace build通过。日志 `/private/tmp/vega-drain-{red,green,clippy}.log` 与 `/private/tmp/vega-main-review-20260905/astra-final-build.log`。这是已证明的代码缺陷；它与历史Artifact/Diff失败的关联仍未证实。最后一次完整workspace为下述a662dfc的898/0/1；44b87f3使用受影响范围回归，不虚称在该HEAD重跑了全部测试。

最新用户裁决：**先功能与 UI，T48 性能测试延期**。本地 Zcode 为主要 UI 参考；不运行 bench/soak，不把延期写成性能通过。

最新完整 workspace（诊断版 `a662dfc`）：**898 passed / 0 failed / 1 ignored**，exit 0。root 清理 vega/vega_conversation 后运行同一 `cargo test --workspace --locked --no-fail-fast`，原始日志 `/private/tmp/vega-main-review-20260905/combined-diagnostic-workspace-1.log`。随后 workspace all-targets clippy `-D warnings`、workspace build与fmt均通过，日志分别为同目录 `combined-final-clippy.log`、`combined-final-build.log`、`combined-final-fmt.log`。唯一忽略项是需真实macOS Keychain的roundtrip，不冒充通过。`combined-verification-manifest.json`记录各轮源码、数量与原始日志SHA256。

**剩余风险**：下面第二轮的Diff等待超时与Artifact process_control_failed，在本轮未复现；本轮只新增安全诊断，没有证明两项根因已修复。不得用这次绿替换旧失败，也不得把间歇性问题标记resolved。已完成的功能/UI可本地review，稳定性归因仍需后续证据。性能按用户裁决延期，T50外部验收不在此轮绿的范围内。

历史第二轮：

完整 workspace（第二轮，`ab2a17b`）：**896 passed / 2 failed / 1 ignored**，exit 101；日志 `/private/tmp/vega-main-review-20260905/final-combined-workspace-2.log`。独立 worker 计数修复已进入本轮并通过。剩余失败：`tests::diff::diff_refresh_intents_keep_content_during_background_and_retry` 的成功状态等待超时，以及 `artifact::tests::capture_reconcile::artifact_provenance_downgrades_once_and_aba_does_not_upgrade` 返回 `process_control_failed`。当前补最小安全诊断以区分失败阶段；不放宽超时或安全断言，不以重跑偶然通过替代归因。

以下保留第一轮快照：

第一轮完整 workspace 结果：**897 passed / 1 failed / 1 ignored**，exit 101。root 清理相关包后，在联合树运行 `cargo test --workspace --locked --no-fail-fast`；原始日志 `/private/tmp/vega-main-review-20260905/final-combined-workspace-1.log`。唯一失败为 `tests::pricing::pricing_settings_and_agent_preflight_production_e2e` 的全局 `AGENT_WORKER_STARTS` 断言（left 16/right 15），跨并行测试共享计数。该轮随后已迁移为真实 worker entry 的独立测试计数，保留拒绝零启动/零请求与成功一次断言；最终复跑尚待，不能宣称全量通过。此前 R0a 的 836/3/1 是历史结果，其 raw `/private/tmp/vega-r0a-luna-20260905/test-workspace-01.log` 保留。

| 范围 | 当前结果与证据 |
|---|---|
| R0a 受信 Git | 来源/版本诊断、identity 检查、环境清洗已接入；[交付](vega-r0-trusted-git-delivery.md) |
| R0b fixture readiness | `050d94a` 固定验证：四单例通过，Artifact13/13、caps13/13、mutation8/8、Git workspace146/146；固定8轮、最多2轮并行、共32次 exact invocation 全通过，无追加样本/重试。完整汇总 `/private/tmp/vega-r0b-validation-20260905/SUMMARY.md`。相关测试在第一轮全量通过；固定验证不替代上方最新 workspace gate |
| R1 模型选择 | durable ack、路由/owner 守卫与当前任务持久化保留；既有原生切换/重启证据见下方历史验收及外部原生记录 |
| R2 Thinking | exact provider/model frozen request、独立能力文件、保存/刷新 generation 协调已实现。两处 stale Saving 竞态已修，gated回归8/8；整合 app60/60、UI136/136、严格clippy/check/fmt通过；[交付](vega-r2-thinking-delivery.md)。后续全量首败按上文保留 |
| R4b 权限 | 真实 CmdEnter→submit→start_agent_run→PermissionCard→Enter允许一次→write→下一轮Mock tool-result通过；[入口补验](vega-r4b-permission-e2e-delivery.md)。覆盖写入最终字节不被当成 syscall exactly-once 计数 |
| R5 文件引用 | 实际 @ 候选、解析和请求链及失败恢复接线通过；[交付](vega-r5-file-reference-delivery.md)。c09ded4原生选中README只插入一次 |
| R6 Diff | 后台刷新保留内容/展开/统计，错误守卫和显式重试保留；[交付](vega-r6-diff-refresh-delivery.md)。原生 owned README跨轮询保持+2/-0 |
| R7 Provider/models | 表单校验、持久化成功事件与失败草稿保留；Models 2–4行viewport原生修正；[交付](vega-r7-provider-models-delivery.md) |
| R3 交付入口 | 当前记录与README入口已完成审计并更新；历史/冻结SDD及旧交付raw保留 |

最新原生副本 `/Users/puzige/Workspace/vega-review-20260905/native-review-r2-owned/Vega.app`，来源 fccca86，ad-hoc严格签名校验通过。独立 XDG_CONFIG_HOME 在设置中实查为 owned-ui-review/provider.invalid；真实点击GLM模板、high→max保存，独立reasoning.toml读回，退出重启后max保持。960×628浅色与CmdShiftL深色均能显示完整声明卡片；同目录provenance.json/native-findings.md记录证据。没有真实密钥或provider发送，没有改用户Provider配置。启动主题当前来自系统外观，深色验证不等于ui.theme持久化。完整原生键盘矩阵、真实IME和原生权限卡仍未全部验证；已有GPUI相应自动链路不冒充实窗操作。

T50 的真实 provider/账单、硬件与7天dogfood仍需外部证据。当前不是Phase 1完成或Phase 2放行；交付入口审计已完成；最新workspace已通过；两项间歇失败根因仍未确认。

## R1/R4 历史联合验收快照（8c0120d）

以下保留原始结果与当时的下一步，不代表 R0a 集成后的现状。

## 结论

R1 当前任务模型选择和 R4 原生 UI 翻新已完成本地实现、独立 review 与下述验收。联合分支为 `codex/vega-review-integration`，基线为 `c888054`。生产代码验收快照为 `8c0120dd52ad416e741d3372ab599ba46b0ff272`；后续交付记录更新只涉及文档。

这不是发布放行：最终 workspace 全量为 **816 passed / 13 failed / 1 ignored**。没有 push、master 合并、发布或替换 `/Applications/Vega.app`。

## 已交付

- R1：模型选择经 durable ack 后更新当前任务；下一次请求和重建 controller 读取持久化模型。独立模型保存 owner、路由/请求校验、pending 操作守卫和 Settings 延迟刷新均已补齐；模型刷新只拥有 model 字段，不覆盖较新的标题。
- R4：统一 Light / Dark、260px 侧栏、820px 居中正文与输入区、紧凑会话菜单、分组 composer、设置和 Diff。原生走查发现的标题不同步、多行输入增长滞后、菜单错位、正文重复内边距、设置按钮缺文字和价格行挤压均已修正。
- 无新增依赖、DDL 或 Cargo.lock 变更。225 个 Rust 文件均不超过 1000 行，最大仍为既有 996 行。

## 验证与边界

| 项目 | 结果 | 证据 |
|---|---|---|
| 修正 Settings 事件顺序后的模型选择 | 4/4 PASS；最终全量也全部通过 | `07-model-selection-after-native.log`；最终 workspace log |
| UI 测试 | 115/115 PASS | `23-vega-ui-menu-zero-origin2.log`；最终 workspace log |
| fmt / clippy / workspace build | PASS | `22-fmt-menu-zero-origin2-check.log`、`24-clippy-menu-zero-origin2.log`、`25-build-menu-zero-origin2.log` |
| 最终全量 | FAIL：816/13/1，exit 101 | `/private/tmp/vega-main-review-20260905/integration-final-workspace-test.log` |
| 原生窗口 | 960×600 content、1280×750 outer 实窗验证；菜单/标题/多行输入/设置/正文/工具展开通过 | 本轮 CUA 截图与外部 `UI-ACCEPTANCE.md` |
| 原生模型持久化 | owned sandbox 由 GLM 切至 Luna；数据库读回成功；重启保持；配置哈希不变 | 外部 `native-review/model-selection-native.json` |

上述编号日志位于 `/private/tmp/vega-ui-integration-20260905/`。原生 debug 副本位于 `/Users/puzige/Workspace/vega-review-20260905/native-review/Vega.app`，来源与签名后摘要见同目录 `provenance.json`。

最终 13 个失败中，12 项为已确认的 R0：production 固定 `/usr/bin/git`，本机 Apple Git 2.39.5 不支持 `check-attr --source`。另一项为 Artifact preflight 测试，预期 GitFailed、实得 TimedOut，与首次 master 全量的失败形式一致；不以历史孤立通过代替总门禁。

上一轮联合全量同为 816/13/1，但第 13 项是 F3 `spawn race persisted across retries (attempt 4)`，那轮 Artifact 通过。F3 联合树独跑和强制重编 master 对照均 1/1 通过，最终全量也通过。两轮原始失败都保留，F3/Artifact 时序问题仍需分诊；历史文档的 F3 resolved 不应被当作当前稳定性证明。master 对照先清理并重编 vega_store / vega_conversation，避免共享 target 缓存混淆。

CUA 对 Vega 的权限已恢复。Codex 自身窗口有独立工具安全限制，没有绕过；设计采用此前读取的样式标量与原创实现。中文粘贴/重命名已测，真实 IME 候选组合、完整 Tab 遍历和 live provider 发送未测。MockProvider 证明实际 app/controller 请求边界，不证明真实网络、账单或 T50。T48 性能与 T50 硬件/7天 dogfood 仍未完成。

## 下一步顺序

1. R0：冻结受信 Git 来源与 capability 预检/诊断方案，使 production executable 与真实分支 E2E 一致；不能仅改 PATH 或删除安全校验。
2. R2：补 thinking 的 provider 能力声明、参数映射与实际请求接线；当前 UI 档位尚未进入请求。
3. R5：补 FileIndexRequested 应用层 worker、路由守卫与真实候选/请求链；当前 app 无订阅，apply_file_index 无 production caller。R4 已将 placeholder 改为“描述任务”。
4. R3：继续整理仓库旧入口与历史报告勘误；再推进 T48、T50。当前记录不改写历史 raw 数字。

后续仍由主 Agent 冻结范围、独立 review 与验收，Luna / max 实现；不再使用 pi。
