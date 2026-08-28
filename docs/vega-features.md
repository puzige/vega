# ✦ Vega — 功能点全表（Feature Backlog）

**版本** v0.1 · 2026-08-28 · 关联：[vega-prd.md](vega-prd.md) v0.3.1 · [vega-feature-teardown.md](vega-feature-teardown.md) · [vega-phase1-plan.md](vega-phase1-plan.md)

> 粒度说明：功能点 = 可实现、可验收的最小单元。Phase 1 模块细到工程任务级；Phase 2+ 保持 backlog 级。
> 优先级：**P0** = 里程碑必须，**P1** = 该 Phase 内应做，**P2** = 可顺延。
> 对标来源：CD=Codex Desktop · DV=Devin · AG=Antigravity · ZC=ZCode · WB=WorkBuddy · ✦=Vega 独有

---

## A1 · 应用外壳 & 导航 [Phase 1]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A1-01 | GPUI 窗口骨架 | 原生窗口、macOS 菜单、Cmd+Q/W 等系统快捷键 | P0 | 全员 |
| A1-02 | 侧边栏-新建任务 | 一键在当前项目建新 thread | P0 | 全员 |
| A1-03 | 侧边栏-项目列表 | 添加本地文件夹为项目、移除、排序 | P0 | 全员 |
| A1-04 | 项目下会话列表 | 时间倒序、未读标记、点击进入 | P0 | ZC/WB |
| A1-05 | 会话管理 | 重命名、归档、删除、pin 置顶 | P1 | CD |
| A1-06 | 全局搜索 | 搜索会话标题/内容/项目，键盘可达 | P1 | CD/ZC |
| A1-07 | 命令面板 | Cmd+K：跳会话、切项目、执行动作 | P1 | 全员 |
| A1-08 | 全局快捷键体系 | Cmd+N 新任务、Cmd+, 设置、 Cmd+1..9 切会话 | P1 | 全员 |
| A1-09 | 主题系统 | Light/Dark + 跟随系统 | P0 | 全员 |
| A1-10 | 设置-Provider 配置 | base_url + key，多 provider 配置档；key 存 macOS Keychain 不落明文 | P0 | 全员 |
| A1-11 | 设置-默认行为 | 默认模型、默认权限模式、默认项目 | P1 | WB |
| A1-12 | 设置-定价表 | 自定义模型单价（每百万 token input/output/cache 价） | P1 | ✦ |
| A1-13 | 自动化入口占位 | 侧边栏入口，Phase 3 前灰显 | P2 | 全员 |
| A1-14 | 多窗口 | 同项目多窗口并行 | P2 | WB/CD |

## A2 · 会话流引擎 [Phase 1]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A2-01 | 消息流渲染 | user/assistant 气泡、时间戳、头像 | P0 | 全员 |
| A2-02 | 流式 markdown 增量渲染 | 未闭合 block 行内解析、闭合后冻结不重排（spike 验证） | P0 | ✦技术难点 |
| A2-03 | 代码块高亮 | tree-sitter 只读高亮、语言标签、一键复制 | P0 | 全员 |
| A2-04 | 虚拟化长滚动 | 万行会话 120fps；底部锚定，用户上翻时不打断阅读 | P0 | ✦ |
| A2-05 | 工具调用卡片-通用 | 类型图标、状态（执行中/成功/失败）、耗时、可折叠 | P0 | 全员 |
| A2-06 | bash 卡片 | 命令全文、输出（可展开）、退出码、耗时 | P0 | 全员 |
| A2-07 | 文件变更卡片 | 路径、+x/-y、点击展开 diff | P0 | CD |
| A2-08 | 权限确认卡片 | 展示将执行的操作 → 允许一次/总是允许/拒绝（可附言） | P0 | CD/ZC |
| A2-09 | **Ask/Plan/Execute 三模式** | composer 切换；Ask 只答不动、Plan 出计划待批、Execute 直接跑 | P0 | CD/DV/WB |
| A2-10 | Plan 产物审批 | 计划文档渲染，批准→执行 / 要求修改 / 放弃 | P0 | CD/DV |
| A2-11 | Composer 输入 | 多行、Shift+Enter 换行、历史消息 ↑ 召回 | P0 | 全员 |
| A2-12 | @文件引用 | fuzzy 搜索项目文件，注入上下文 | P0 | 全员 |
| A2-13 | 附件 | 图片粘贴/拖拽（多模态模型） | P1 | CD/ZC |
| A2-14 | 模型选择器 | 切换 provider/模型 + 思考档位 | P0 | 全员 |
| A2-15 | 权限模式切换 | 只读 / 变更前确认 / 全自动，会话级 | P0 | 全员 |
| A2-16 | 分支选择器 | 显示/切换项目当前 git 分支 | P1 | CD/ZC |
| A2-17 | 中断按钮 | Stop，<1s agent 停手（KPI） | P0 | 全员 |
| A2-18 | 排队消息 | agent 忙时消息排队，可编辑/取消/调序 | P2 | CD/ZC |
| A2-19 | 错误态 | API 失败/限流，自动重试+可见提示 | P1 | 全员 |
| A2-20 | 思考流展示 | reasoning delta 折叠区（模型支持时） | P1 | ZC/AG |
| A2-21 | /命令 | /model /plan /permissions /clear 等快捷指令 | P2 | CD/AG |

## A3 · Vega Runtime（自研 Agent 核心）[Phase 1-2]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A3-01 | Provider 抽象层 | trait 统一 chat_stream；新增 provider 不改循环 | P0 | DV(多模型) |
| A3-02 | OpenAI 兼容 provider | SSE 流式、base_url 可配（天然覆盖 CPA/DeepSeek/B.AI） | P0 | ✦ |
| A3-03 | Agentic 循环 | think→tool_use→execute→observe→continue；无工具调用则收敛 | P0 | 全员 |
| A3-04 | 工具注册机制 | JSON schema 定义、运行时注册 | P0 | 全员 |
| A3-05 | bash 工具 | cwd 锁项目根、超时、输出截断（头尾保留） | P0 | 全员 |
| A3-06 | read 工具 | 行范围读取、大文件分页 | P0 | 全员 |
| A3-07 | write/edit 工具 | edit=精确串替换，失败返回上下文引导重试 | P0 | 全员 |
| A3-08 | glob/grep 工具 | 基于 ignore crate，尊重 .gitignore | P0 | 全员 |
| A3-09 | 权限门禁引擎 | 模式×工具×路径规则矩阵；危险命令硬拦截清单（rm -rf / push --force 等，全自动模式也需确认） | P0 | CD 沙盒/ZC 确认 |
| A3-10 | 中断/恢复 | CancellationToken <1s；会话状态落库可断点续跑 | P0 | CD/ZC |
| A3-11 | 上下文管理 v1 | system prompt 模板、滑动窗口、超限朴素截断 | P0 | 全员 |
| A3-12 | Trace 全量落库 | 每事件（思考/工具/结果/usage）写 SQLite，Harness 数据基础 | P0 | ✦ |
| A3-13 | 上下文压缩 v2 | 摘要压缩长会话 | P1 (Ph2) | CD |
| A3-14 | 子 agent 派发 | 主 agent 可 spawn 子任务并行，结果汇总 | P1 (Ph2) | 全员 |
| A3-15 | 多 Provider | Anthropic 原生、DeepSeek、Ollama | P1 (Ph2) | AG/ZC |
| A3-16 | web_fetch 工具 | 抓取网页转 markdown 注入 | P1 (Ph2) | 全员 |
| A3-17 | 闲时执行接口 | 任务可标记「闲时跑」，由 A7 调度低价模型 | P2 (Ph3) | ZC |

## A5 · 产物 & Diff 审阅 [Phase 1-2]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A5-01 | 工作区变更检测 | git status 轮询/事件，agent 改动实时进 diff 面板 | P0 | 全员 |
| A5-02 | Diff viewer | 统一视图、语法高亮、hunk 导航、逐文件折叠 | P0 | 全员 |
| A5-03 | 变更统计条 | 文件数、+x/-y、未跟踪文件 | P0 | CD |
| A5-04 | 产物卡片 | agent 产出文件卡片化，可预览/在 Finder 显示 | P0 | WB/AG |
| A5-05 | Open in… | VS Code/Cursor/Zed/Terminal/自定义/默认 app/Finder，可配置列表 | P0 | CD |
| A5-06 | Commit 辅助 | 生成 commit message → 确认后提交 | P1 | ZC/CD |
| A5-07 | **Checkpoint 自动打点** | 每轮写操作前对工作区打快照（git stash 机制或文件级） | P1 (Ph2) | ZC/WB |
| A5-08 | Checkpoint 回退 | 检查点列表、一键回退到任一点 | P1 (Ph2) | ZC/WB |
| A5-09 | PR 创建 | gh 集成，生成 PR title/body | P2 (Ph2) | CD |
| A5-10 | Diff 行内评论 | 评论注入会话让 agent 修改 | P2 (Ph3) | CD/AG |

## A10 · Token 经济系统 ✦ [Phase 1-3]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A10-01 | usage 精确回收 | 每 API 调用记录 input/output/cache_read/cache_write（以此为准） | P0 | ✦ |
| A10-02 | 流式 token 预估 | tiktoken-rs 实时估算，usage 到达后校准替换 | P0 | ✦ |
| A10-03 | 定价表 | 内置主流模型价格 JSON，支持自定义模型/渠道（CPA/B.AI） | P0 | ✦ |
| A10-04 | 成本引擎 | microcents 精度，按调用实时计价 | P0 | ✦ |
| A10-05 | 会话实时计数器 | composer 旁常驻：本次会话 token + 成本 | P0 | ✦超越 ZC |
| A10-06 | 任务成本汇总卡 | 任务结束：token 明细、成本、耗时、工具调用数、缓存命中率 | P0 | ✦ |
| A10-07 | 成本仪表盘 | 按天/周/月 × 项目 × 模型聚合图表 | P1 (Ph2) | ✦超越 ZC 统计页 |
| A10-08 | 预算硬控制 | 月预算上限，达到告警/阻断（非仅展示） | P1 (Ph3) | ✦无竞品 |
| A10-09 | 跨模型性价比对比 | 同类任务在不同模型的成本/效果对比报告 | P1 (Ph3) | ✦无竞品 |
| A10-10 | 优化建议 | 缓存命中率分析、上下文裁剪建议、模型降级建议 | P2 (Ph3) | ✦ |
| A10-11 | 闲时调度联动 | 闲时任务自动选低价模型，省费可见 | P2 (Ph3) | ZC 闲时→✦深化 |
| A10-12 | 用量导出 | CSV/报告导出 | P2 (Ph3) | DV |

## A11 · 数据层 [Phase 1]

| ID | 功能点 | 说明 / 验收 | 优先级 | 对标 |
|---|---|---|---|---|
| A11-01 | SQLite schema v1 | projects/threads/messages/tool_calls/token_usage/permissions 六表 + 迁移机制 | P0 | — |
| A11-02 | 项目注册表 | 文件夹选择、git repo 检测、默认分支、最近打开 | P0 | 全员 |
| A11-03 | 会话分页加载 | 长会话分段加载，首次加载 <100ms | P0 | ZC |
| A11-04 | tool_calls 全量记录 | 输入/输出/耗时/审批结果/操作人 | P0 | ✦ |
| A11-05 | Keychain 密钥存储 | API key 零明文 | P0 | — |
| A11-06 | 数据导出/备份 | 整库导出 | P2 (Ph2) | — |
| A11-07 | **项目记忆** | 项目级记忆页：约定/偏好/历史决策，agent 可读写 | P1 (Ph3) | ZC/WB |

## Phase 2+ 模块（backlog 级）

### A4 · ACP 编排层 [Phase 2]
- A4-01 ACP client（agent-client-protocol Rust SDK）
- A4-02 外部 agent 进程管理（spawn/monitor/kill：codex / claude-code）
- A4-03 统一会话模型（外部 agent 消息映射进 Vega thread，同视图）
- A4-04 外部 agent token/成本归集（API 返回为准，尽力而为）
- A4-05 agent 来源标识（自研/外部徽标 + 成本对比）

### A6 · 终端视图 [Phase 2]
- A6-01 PTY 集成（alacritty_terminal，GPU 渲染）
- A6-02 交互式终端 tab（多会话）
- A6-03 命令输出 → 会话反馈回路（终端报错一键发给 agent）

### A7 · 自动化 [Phase 3]
- A7-01 定时任务（cron + 一次性）
- A7-02 任务模板/快捷指令（周报总结、报错修复…）
- A7-03 **闲时任务**（排队 + 低价模型调度 + 省费统计）
- A7-04 运行记录 + 完成通知（macOS 通知）

### A8 · 连接器 & 插件 [Phase 3-4]
- A8-01 MCP client（stdio/SSE，授权管理）
- A8-02 连接器配置 UI（启用/禁用/凭据）
- A8-03 WASI 插件沙箱 + 插件 API（Ph4）
- A8-04 插件市场 MVP（Ph4）

### A9 · Harness 质量引擎 [Phase 3]
- A9-01 Trace 可视化（时间轴：思考/工具/耗时/成本逐步回放）
- A9-02 Golden Set 定义与回归跑分
- A9-03 Bad Case RCA 分类标注
- A9-04 安全护栏（forbidden actions 清单）
- A9-05 HITL 断点（关键步骤强制人工确认）

### A12 · Remote 环境 [Phase 4，D7]
- A12-01 SSH 远程执行环境（远程跑 agent 工具）
- A12-02 远程工作区文件透明代理
- A12-03 Local/Remote 环境切换器（对标 CD）

---

## 统计

| 范围 | 功能点数 | P0 |
|---|---|---|
| Phase 1（A1/A2/A3/A5/A10/A11） | 61 | 38 |
| Phase 2（含 A4/A6 及 v2 项） | ~15 | — |
| Phase 3+（A7/A8/A9/A12） | ~20 | — |
| **合计** | **~96** | — |

> 下一步：本表 P0 项即 S1-S8 的任务池。repo 形态确认后，S1 开工可直接按 A1-01/A1-09/A1-10/A11-01/A11-05 + 脚手架任务拆 issue。
