# ✦ Vega (北极星) — Native AI Agent Desktop PRD

**✦ Vega PRD v0.3.1** · 7 项关键决策已锁定 · 2026-08-28
*Rust + GPU 原生 Agent 工作台 · 性能碾压 Electron，成本与质量透明可控*

> Notion 源页面：https://app.notion.com/p/3caae0ca542b8056b086ee2e990d9b8a
> 本文件为本地同步副本，以 Notion 为准。
> v0.3 变更：定位从「AI Coding IDE」修正为「Agent 工作台（Agent Window）」，对标 WorkBuddy / Codex Desktop / Antigravity / ZCode。
> v0.3.1 变更：基于五家竞品功能拆解（[vega-feature-teardown.md](vega-feature-teardown.md)）——Composer 补 Ask/Plan/Execute 三模式；A5 补 Checkpoint 回退；A7 补闲时任务；新增项目记忆；A10 差异化基准上调为「必须超越 ZCode 用量统计页」。

---

## 0. 已锁定决策 · Locked Decisions

> 🔒 **D1 · GUI 框架 → GPUI**
> 起步用官方 crates.io 发布版（`gpui` + `gpui_platform`，锁定版本 + 提交 Cargo.lock），获得 Metal 原生渲染能力。首次需要修改框架内部时再 vendor fork，逐步演化为 Vega 自有 UI 层。全依赖图禁止混用不同 GPUI 发行版。

> 🔒 **D2 · 平台策略 → macOS First**
> Phase 1-3 仅支持 macOS，充分利用 Metal 后端成熟度和 Apple Silicon 性能优势。Windows/Linux 在 Phase 3+ 通过 wgpu 跨平台层适配。

> 🔒 **D3 · 差异化 → Harness 质量引擎 + Token 经济仪表盘**
> 内置 Agent Trace 可视化、Golden Set 评估、Bad Case RCA、安全护栏。同时构建完整的 Token 经济系统：实时用量追踪、成本仪表盘、预算控制、模型性价比分析——让 AI 成本透明可控。

> 🔒 **D4 · 产品名 → Vega（北极星）**
> Vega — 天琴座最亮的恒星，夜空中第五亮的星。寓意：为开发者在 AI 编程时代的导航之星。

> 🔒 **D5 · 产品形态 → Agent 工作台，不是 IDE（v0.3 新增）**
> 核心循环 = 任务 → Agent 执行 → 产物审阅。代码能力只做 **viewer + diff 审阅**，编辑交给外部 IDE（"Open in VS Code/Cursor/Zed/Terminal..." 交接，参考 Codex Desktop）。不自研编辑器内核、不做 LSP。

> 🔒 **D6 · Agent 执行模型 → 自研 Runtime 优先 + ACP 编排并存（v0.3 新增）**
> **B 优先**：Vega Runtime 自研 agent 循环，直连模型 API——token 计量最精确、Harness 可控性最强、是护城河。**A 并存**：通过 ACP（Agent Client Protocol）编排外部 CLI agents（codex / claude-code 等），Devin Desktop 已有参考实现，保证模型与 agent 生态无关。

> 🔒 **D7 · Remote/SSH → 后续 Roadmap（v0.3 新增）**
> Phase 1-2 只做本地执行。SSH 远程环境（对标 Codex 的 Local/Remote 切换）进入 Phase 4+ roadmap。

## 1. 产品愿景 · Vision

### 市场空缺

Agent 工作台品类正在爆发（WorkBuddy / Codex Desktop / Antigravity / ZCode / Devin Desktop），但存在一个明显的结构性空缺：

- **🎯 目标象限（空缺）** — Native 性能 + 自研 Agent Runtime + Token/质量双透明 → **Vega**
- **当前领导者** — Electron/Web 套壳 + 深度 Agent → WorkBuddy / Codex Desktop / Antigravity / ZCode
- **云端 Agent** — Web 端异步任务 → Devin / Cursor Agents / Codex Cloud
- **CLI 形态** — 终端内 Agent → Claude Code / Codex CLI / Aider

**关键观察**：该品类产品**全部是 Electron 或 Web 技术栈**——长对话流 + 工具调用卡片 + 产物预览是渲染重度场景，滚动卡顿、内存膨胀是普遍痛点。Native GPU 渲染在此品类的体验红利比 IDE 品类更直接。

### 一句话愿景

> **✦ Vega — The agent command center that flies: native speed, any model, every token accounted for.**

三根支柱：

- **🏎️ 原生性能** — GPUI + Metal 渲染，万行对话流滚动 120fps，内存仅 Electron 的 1/10
- **🧠 Runtime 自主 + 生态开放** — 自研 Vega Runtime（直连模型）+ ACP 编排外部 agents，不被任何一家绑定
- **💎 Token 透明 + Harness 质量** — 每个任务、每次工具调用的 token 用量、成本、质量评分 — 清清楚楚

## 2. 竞品格局 · Competitive Landscape

| 产品 | 技术栈 | Agent 模型 | 外部编辑器交接 | Token 透明 | 自动化 | 定价 |
|---|---|---|---|---|---|---|
| WorkBuddy | Electron | 自研编排 | ✅ | ⚠️ 仅消耗数字 | ✅ | 订阅 |
| Codex Desktop | Electron | 自家模型绑定 | ✅（VS Code/Cursor/Zed/Terminal…） | ⚠️ 基础 | ✅ Scheduled | $20+/mo |
| Antigravity | Electron | Gemini 绑定 | ✅ Open IDE | ❌ 黑箱 | ✅ | 订阅 |
| ZCode | Electron | GLM 绑定 | ✅ | ❌ 黑箱 | ✅ | 订阅 |
| Devin Desktop | Electron | 自研 + **ACP 开放** | ✅ | ⚠️ 基础 | ✅ | 订阅 |
| **✦ Vega** | Rust+GPUI | **自研 Runtime + ACP 双模** | ✅ | ✅✅ 全维度 | ✅ | TBD |

> 💡 **关键洞察：** ① 全员 Electron，Native 是空白；② 全员模型/agent 绑定，只有 Devin 开始用 ACP 开放——Vega 从 Day 1 双模（自研 + ACP）是最开放的；③ Token 成本是这类产品的最大用户焦虑（agent 一跑几十万 token），没有一家做到真正的成本透明——这是 Vega 最锋利的差异点。DeepSeek 等将基座成本降低 95%，使「自研 Runtime + 高频 agentic 循环」经济上可行。

## 3. 产品定位 · Positioning

### 🏎️ 原生性能
GPUI + Metal 直接渲染，万行对话流 120fps 无卡顿，内存是 Electron 的 1/10，冷启动 <50ms。长任务挂一整天不膨胀。

### 🧠 Vega Runtime（自研 Agent 核心）
自研 agentic 循环：流式输出、工具调用（bash/文件读写/搜索）、权限门禁、中断/恢复、多轮上下文管理。直连 Anthropic/OpenAI/DeepSeek/本地模型，token 计量精确到每次 API 调用。

### 🌐 ACP 开放编排
原生实现 Agent Client Protocol，可编排 codex / claude-code / Gemini CLI 等外部 agent——用户已有的 agent 订阅不浪费，Vega 做统一的工作台与度量层。

### 💎 Token 经济透明（Vega 独有）
实时 Token 用量追踪（input/output/cache）、成本仪表盘、按模型/任务/项目维度分析、预算上限告警、模型性价比对比。知道每一分钱花在哪里。

### 🔧 Harness 质量引擎
Agent Trace 可视化、Golden Set 回归、Bad Case RCA、安全护栏与 HITL 断点。Agent 行为可审计、可复现、可回滚。

### 🍎 macOS Native 体验
macOS 优先，深度适配 Apple Silicon + Metal + ProMotion 120Hz。

## 4. 技术架构 · Architecture

### 分层架构

**🎨 UI 层 · GPUI (crates.io → vendor fork 演化)**
- 会话流渲染（虚拟化长列表）
- 工具调用卡片 / 权限确认交互
- 产物预览 / Diff 视图
- 侧边栏导航 / 命令面板
- 主题 & 布局

**💬 会话引擎 · Conversation Core**
- Thread/Message 数据模型（SQLite 持久化）
- 流式 markdown 增量渲染
- 工具调用生命周期状态机
- 上下文组装（@文件/@符号/历史压缩）

**🧠 Agent 层 · Agent Layer**
- **Vega Runtime**（自研）：agentic 循环、工具执行、权限门禁、中断/恢复
- **ACP Adapter**：外部 agent 进程编排（codex / claude-code…）
- Provider 抽象：Anthropic / OpenAI / DeepSeek / Ollama
- Harness Evaluator

**💎 Token 经济层 · Token Economy**
- Token 计量器（API 返回校准 + tiktoken-rs 预估）
- 成本计算引擎（模型定价表）
- 用量仪表盘 / 预算控制器 / 性价比分析

**🔌 平台层 · Platform Services**
- MCP Client/Server
- Git 集成（分支感知、commit 辅助）
- 产物存储与预览
- PTY 终端
- 外部编辑器交接（Open in…）

**🖥️ 基础设施 · macOS / Metal**
- Metal (macOS) · Tokio · SQLite (rusqlite) · FS Watcher

## 5. 核心模块 · Product Modules

### A1 · 应用外壳 & 导航 [Phase 1]
- 侧边栏（新建任务/搜索/自动化/插件/项目列表/会话历史）
- 多项目 × 多线程模型
- 命令面板 & 快捷键
- Light/Dark 主题
- 设置（Provider 密钥、权限默认值）

### A2 · 会话流引擎 [Phase 1]
- 流式 markdown 增量渲染（代码块 tree-sitter 只读高亮）
- 万行虚拟化滚动
- 工具调用卡片（bash/read/write/search… 可折叠）
- 权限确认交互（允许一次/总是/拒绝）
- **Ask / Plan / Execute 三模式**（Codex/Devin/WorkBuddy 标配；Plan 生成可批准的计划产物，批准后才进执行）
- Composer（@上下文、/命令、模型选择器、权限模式、分支选择器）

### A3 · Vega Runtime（自研 Agent 核心）[Phase 1-2]
- Agentic 循环（think → tool call → observe → continue）
- Provider 抽象 + 流式 API（Anthropic / OpenAI 兼容先行）
- 内置工具集：bash / read / write / edit / glob / grep / web_fetch
- 权限门禁系统（分级：只读 / 变更前确认 / 全自动）
- 中断 / 恢复 / 上下文压缩
- 子 agent 派发（Phase 2）

### A4 · ACP 编排层 [Phase 2]
- ACP client 实现（Rust SDK）
- 外部 agent 进程生命周期管理（spawn/monitor/kill）
- 统一会话模型映射（外部 agent 消息 → Vega thread）
- 外部 agent 的 token/成本归集（尽力而为，API 返回为准）

### A5 · 产物 & Diff 审阅 [Phase 1-2]
- Git 变更感知（工作区 diff 实时视图）
- Diff viewer（语法高亮、hunk 导航、行内评论）
- 产物卡片（文件/报告/图片，present & preview）
- **Open in…** 外部编辑器交接（VS Code/Cursor/Zed/Terminal/默认 app）
- Commit / PR 辅助（生成 commit message）
- **Checkpoint 回退**（Phase 2：任务内自动检查点，一键回退到任意 checkpoint，对齐 ZCode Goal-checkpoint）

### A6 · 终端视图 [Phase 2]
- PTY 集成（alacritty_terminal）
- Agent 命令执行可视化（命令/输出/退出码卡片化）
- 交互式终端标签页

### A7 · 自动化 [Phase 3]
- 定时任务（cron/一次性提醒）
- 任务模板 / 快捷指令（周报总结、报错修复…）
- **闲时任务**（非紧急任务排队，用低峰/低价模型调度执行，与 Token 经济联动，对齐并超越 ZCode 闲时模式）
- 后台执行 + 完成通知

### A8 · 连接器 & 插件 [Phase 3-4]
- MCP 原生（client 先行，server 后续）
- 连接器市场（第三方服务接入）
- WASI 插件沙箱（Phase 4）

### A9 · Harness 质量引擎 [Phase 3]
- Agent Trace 可视化（每步思考/工具/耗时/成本）
- Golden Set & 回归评估
- Bad Case RCA 分类
- 安全护栏 & Forbidden Actions
- HITL 人工介入断点

### A10 · Token 经济系统 ✦ Vega 独有 [Phase 1-3]
- 实时 Token 用量计数器（input/output/cache，API 返回校准）
- 每次交互/每个任务成本标注
- 按模型/项目/天 成本仪表盘
- 月度预算上限 & 告警
- 模型性价比对比（DeepSeek vs Claude vs GPT）
- Token 优化建议（上下文裁剪、缓存命中率）
- 团队用量报告（Enterprise）
- **差异化基准：必须明显超越 ZCode 用量统计页**——API 调用级真实成本（非配额/积分）、跨模型性价比、预算硬控制（竞品全部停留在配额/倍率层）

### A11 · 数据层 [Phase 1]
- SQLite 本地存储（threads/messages/tool_calls/token_usage）
- 项目注册表（git repo 发现、分支感知）
- 导出/备份
- **项目记忆**（Phase 3：项目级记忆页，沉淀项目约定、偏好与历史决策，对齐 ZCode/WorkBuddy）

### A12 · Remote 环境 [Phase 4+ · D7 延后]
- SSH 远程执行环境
- 远程工作区透明代理
- 对标 Codex Local/Remote 切换

## 6. 路线图 · Roadmap

### Phase 1 · First Light
**🕐 Month 1-4** · Agent Window MVP — 自研 Runtime 跑通真实任务

**核心交付：** A1 外壳 + A2 会话流 + A3 Runtime(v1) + A5 Diff(v1) + A10 Token(v1) + A11 数据层

- GPUI 脚手架 & CI & 性能测量骨架
- 侧边栏 + 多项目多线程 + SQLite 持久化
- 流式 markdown 会话流（虚拟化）
- Vega Runtime v1：单 provider（OpenAI 兼容）+ agentic 循环 + bash/read/write 工具
- 权限门禁（变更前确认模式）
- 工具调用卡片 + diff viewer（只读）
- 💎 Token 实时计数 + 每次任务成本标注

> ✅ 里程碑：用 Vega 自研 Runtime 在真实仓库上完成一个任务（改代码 → diff 审阅 → commit），token 成本全程可见；内部 dogfood 一周

### Phase 2 · Open Agent
**🕐 Month 5-8** · ACP 编排 + 终端 + 产物完善

**核心交付：** A4 ACP + A6 终端 + A5 Diff(v2) + A3 Runtime(v2)

- ACP client：编排 codex / claude-code
- 统一会话模型（自研/外部 agent 同视图）
- PTY 终端 + 命令卡片
- Open in… 外部编辑器交接 + commit 辅助
- Checkpoint 回退（任务内检查点）
- 多 Provider（Anthropic 原生 + DeepSeek + Ollama）
- 子 agent 派发
- 💎 成本仪表盘（按模型/项目/天）

> ✅ 里程碑：同一任务分别用自研 Runtime 和 ACP（codex）跑，成本/质量可对比； dogfood 不回退其他工具

### Phase 3 · Trust & Automation
**🕐 Month 9-14** · Harness + 自动化 + 公开 Beta

**核心交付：** A7 自动化 + A8 MCP + A9 Harness + A10 Token(v2)

- 定时任务 & 快捷指令
- 闲时任务（低价模型调度，Token 经济联动）
- 项目记忆（项目级约定与偏好沉淀）
- MCP client 连接器
- Harness Trace 可视化面板
- Golden Set & 安全护栏 & HITL 断点
- 💎 预算上限 & 性价比对比 & 优化建议

> ✅ 里程碑：公开 Beta · Token 仪表盘 + Harness 面板成为差异化卖点

### Phase 4 · Ecosystem & Remote
**🕐 Month 15-18** · 插件生态 + 远程 + 跨平台

**核心交付：** A8 插件(v2) + A12 Remote + 跨平台

- WASI 插件系统 & 市场 MVP
- SSH 远程环境（D7）
- Windows + Linux（via wgpu）
- 团队协作基础

### Phase 5 · Go To Market
**🕐 Month 18+** · 商业化

- 定价 & 计费（Free / Pro / Enterprise）
- 云端同步 & 团队用量报告
- 自托管 / VPC 部署

## 7. 技术选型 · Technology Stack

| 项 | 选型 | 状态 | 理由 |
|---|---|---|---|
| 语言 | Rust | 🔒 锁定 | 内存安全、零成本抽象 |
| GUI | GPUI (crates.io → vendor fork) | 🔒 锁定 | <4ms 延迟已验证，Metal 后端 |
| GPU 后端 | Metal (macOS) → wgpu (跨平台) | 🔒 锁定 | macOS First 策略 |
| 异步运行时 | Tokio | ✅ 确定 | 事实标准 |
| 本地存储 | SQLite (rusqlite / sqlx) | ✅ 确定 | 会话/产物/token 数据，零依赖 |
| 语法高亮 | Tree-sitter（只读） | ✅ 确定 | 代码块/diff 高亮，增量解析 |
| 终端 | alacritty_terminal | ✅ 确定 | 久经验证 |
| Agent 协议 | ACP (agent-client-protocol Rust SDK) | ✅ 确定 | Zed 系开放协议，Devin Desktop 已验证 |
| 模型 API | async-openai / anthropic-sdk / reqwest+sse | ✅ 确定 | 流式 SSE |
| Token 计量 | tiktoken-rs + API usage 校准 | ✅ 确定 | 跨模型以 API 返回为准 |
| Markdown | pulldown-cmark + 自定义渲染 | ⏳ 待验证 | 流式增量解析需 spike |
| 插件沙箱 | Wasmtime (WASI) | ⏳ Phase 4 | 安全隔离 |

## 8. 风险矩阵 · Risk Matrix

| 风险 | 可能性 | 影响 | 缓解 |
|---|---|---|---|
| 自研 Runtime 质量追不上成熟 agent | 高 | 高 | ACP 并存兜底：自研不行用户还能用外部 agent；Golden Set 持续评估 |
| 流式 markdown 增量渲染复杂度 | 中 | 中 | Phase 1 前置 spike；降级方案：分段全量重渲染 |
| GPUI pre-1.0 breaking change | 中 | 中 | 锁版本 + 每月只升一次；vendor fork 后自主演化 |
| 权限/安全模型出事故（agent 误删文件） | 中 | 高 | 默认「变更前确认」；危险命令拦截清单；操作可审计可回滚 |
| 单人/小团队资源不足 | 高 | 高 | 极致 MVP；大量复用开源；AI 辅助开发 |
| ACP 协议演进不稳定 | 中 | 低 | 适配层隔离；先支持 codex/claude-code 两个实现验证 |
| Token 计量跨模型不一致 | 中 | 低 | 以各 API 返回 usage 为准，tiktoken 仅作预估 |
| WorkBuddy/Codex 快速做 Token 透明 | 中 | 中 | 窗口期执行速度；Harness 质量维度做深护城河 |

## 9. 成功指标 · KPIs

- **<50ms** — 冷启动
- **<100MB** — 空闲内存（无任务时）
- **120fps** — 万行会话流滚动帧率
- **<16ms** — 流式 token 上屏延迟（收到 → 渲染）
- **±5%** — Token 计量误差（vs API 账单）
- **<1s** — 任务中断响应（stop → agent 停手）

### 验收标准

- **P1 (M4)** — 自研 Runtime 完成真实任务（改代码+commit）；token 成本可见；dogfood 一周。
- **P2 (M8)** — ACP 编排 codex 可用；Open in… 交接顺畅；成本仪表盘上线。
- **P3 (M14)** — Harness 面板 + 自动化完整；公开 Beta。
- **P4 (M18)** — SSH 远程可用；Win/Linux 支持；≥10 插件。

## 10. 明确不做 · Out of Scope

- **🚫 自研编辑器内核 / IDE**（D5）— 只做 viewer + diff 审阅，编辑交接外部 IDE。不做 LSP、不做多光标、不做 Vim。
- **🚫 使用 Electron / WebView** — 产品核心命题是 Native 性能。
- **🚫 自训练基座模型** — 聚焦 agent 编排与度量层，模型留给 OpenAI/Anthropic/DeepSeek。
- **🚫 Phase 1-2 的 SSH/Remote**（D7）— 本地先行，远程进 Phase 4。
- **🚫 移动端 / Web 版** — 只做桌面原生。
- **🚫 Phase 1-2 的 Win/Linux** — macOS First，跨平台延后。
- **🚫 云端 Agent 托管（前期）** — 先本地执行，云端同步属 Phase 5。

---

*✦ Vega PRD v0.3.1 · Agent Window · GPUI · macOS First · Runtime+ACP 双模 · Harness + Token 透明 · 2026-08-28*
