# ✦ Vega — Phase 1 工程规划 · First Light（Agent Window MVP）

**版本** v0.2 · 2026-08-28 · 关联文档：[vega-prd.md](vega-prd.md)（v0.3，Notion 为准）

> Phase 1 目标（PRD v0.3 锁定）：**Month 1-4，Agent Window MVP——自研 Vega Runtime 在真实仓库完成一个任务（改代码 → diff 审阅 → commit），token 成本全程可见，dogfood 一周。**
> 范围：A1 外壳 + A2 会话流 + A3 Runtime(v1) + A5 Diff(v1) + A10 Token(v1) + A11 数据层。
> v0.2 变更：定位从 IDE 转为 Agent 工作台，编辑器内核/LSP 全部移除，核心难点变为「流式会话渲染 + 自研 agentic 循环」。

---

## 0. 关键工程决策

### E1 · GPUI 引入方式（沿用已确认方案）
crates.io 锁定版（`gpui` + `gpui_platform`，`font-kit` feature）+ 提交 `Cargo.lock`。首次需改框架内部时再 vendor fork。验证：`cargo tree | grep gpui` 全图单一来源。

### E2 · 仓库形态
独立 cargo workspace `vega`（不放 loom）。建议 `github.com/peanut996/vega`（private 起步）。

### E3 · Phase 1 只做单 Provider
Vega Runtime v1 只接 **OpenAI 兼容端点**（可同时覆盖 OpenAI / DeepSeek / 你的 CPA / B.AI 等一切 OpenAI 兼容服务）。Anthropic 原生 SDK 留 Phase 2。理由：agentic 循环的架构质量比 provider 数量重要。

### E4 · 性能测量从 Day 1
KPI：冷启动 <50ms / 空闲内存 <100MB / 万行滚动 120fps / token 上屏 <16ms。S1 建 `xtask bench`，CI 回归告警。

### E5 · 安全默认
权限模式默认「变更前确认」（write/edit/bash 需人工批准）；危险命令硬拦截清单（`rm -rf`、`git push --force` 等）即使全自动模式也需确认。这是 agent 产品的生命线。

---

## 1. Workspace 结构（crate 分解）

```
vega/
├── Cargo.toml               # workspace 根
├── rust-toolchain.toml      # 锁 stable（GPUI 要求最新 stable）
├── crates/
│   ├── vega/                # 应用入口：窗口创建、App 启动、路由
│   ├── vega_ui/             # GPUI 视图：侧边栏、会话流、composer、工具卡片、diff 视图
│   ├── vega_conversation/   # Thread/Message 模型、流式状态机、上下文组装
│   ├── vega_runtime/        # 自研 agent：agentic 循环、provider 抽象、工具执行、权限门禁
│   ├── vega_tools/          # 内置工具：bash/read/write/edit/glob/grep（可被 runtime/ACP 复用）
│   ├── vega_markdown/       # 流式 markdown 增量解析 → 渲染指令（代码块 tree-sitter 高亮）
│   ├── vega_store/          # SQLite：threads/messages/tool_calls/token_usage/projects
│   ├── vega_token/          # token 计量、定价表、成本引擎
│   └── vega_theme/          # Light/Dark 主题
├── xtask/                   # bench / run / package
├── assets/                  # 字体、图标、主题
└── .github/workflows/
```

**依赖方向**（单向）：`vega → vega_ui → {vega_conversation, vega_markdown, vega_theme}`；`vega_conversation → {vega_runtime, vega_store, vega_token}`；`vega_runtime → {vega_tools, vega_token}`。UI 通过事件订阅会话状态（GPUI Entity 模型天然契合）。

**关键边界**：`vega_runtime` 不依赖 GPUI——agent 核心纯 headless，单测不需要窗口。这为 Phase 2 ACP 适配层和未来 CLI 形态留路。

---

## 2. Sprint 分解（8 × 双周 = 4 个月）

| Sprint | 目标 | 交付物 | 验收 |
|---|---|---|---|
| **S1** (W1-2) | 脚手架 & 外壳骨架 | workspace；GPUI 窗口；CI（fmt/clippy/test/build）；`xtask bench`；SQLite schema v1；设置页（API key 存储 Keychain） | CI 绿；冷启动计时上报；key 不落明文 |
| **S2** (W3-4) | 侧边栏 & 项目模型 | 侧边栏（新建任务/项目/会话历史）；项目注册（选文件夹→识别 git repo→分支感知）；多项目多线程数据流 | 建 2 个项目 × 各 3 个 thread，重启后状态完整恢复 |
| **S3** (W5-6) | ⚠️ 流式会话渲染（最高风险，前置 3 天 spike） | `vega_markdown`：流式增量解析；代码块 tree-sitter 高亮；虚拟化长列表 | 10k 行会话滚动 120fps；流式追加不跳变、不重排已渲染区 |
| **S4** (W7-8) | Vega Runtime 核心 | provider 抽象（OpenAI 兼容 + SSE 流式）；agentic 循环；read/glob/grep 只读工具 | headless 单测：给任务「找出 repo 里所有 TODO」，agent 自主调用工具完成并输出 |
| **S5** (W9-10) | 写操作工具 + 权限门禁 + 三模式 | write/edit/bash 工具；权限状态机（只读/变更前确认/全自动）；权限确认 UI；危险命令拦截；**Ask/Plan/Execute 三模式 + Plan 产物审批流** | 默认模式下每次写操作弹确认；拦截清单生效；Plan 模式出计划待批准后才执行；全部操作落库可审计 |
| **S6** (W11-12) | Diff 审阅 & 产物 | git 工作区 diff 视图（高亮、hunk 导航）；产物卡片；Open in…（VS Code/Cursor/Zed/Terminal）；commit 辅助 | agent 改完代码 → diff 视图审阅 → Open in 外部编辑器 → 生成 commit message 并提交 |
| **S7** (W13-14) | Token 经济 v1 | API usage 回收 + tiktoken 预估；实时计数器（composer 旁）；每任务成本标注；定价表（内置主流模型，可自定义） | 跑一个真实任务，Vega 显示成本 vs API 账单误差 <5% |
| **S8** (W15-16) | 打磨 & 里程碑 | 主题完善；中断/恢复；内存与渲染调优；dogfood | **里程碑：自研 Runtime 在真实仓库完成任务（改码→diff→commit），成本全程可见；dogfood 一周** |

**缓冲策略**：S3（流式渲染）是最高风险，若 spike 证明增量解析不可行 → 降级为「按 block 分段全量重渲染」（体验略损，不 block 里程碑）。S7 可压缩：定价表先做 5 个模型，后续加。

---

## 3. 关键技术要点

### 3.1 流式 markdown 渲染（S3 核心难点）
- 问题：token 流是任意切分的，markdown 结构（代码块/列表）跨 chunk。朴素做法每帧全量 reparse + 全量重渲染 → 长会话必然掉帧。
- 方案：**两阶段**——流式期间只对「当前未完成 block」做行内解析（纯文本+行内样式），block 闭合后才做完整结构解析并冻结渲染结果。已冻结 block 不再重排。
- spike 验证项（3 天）：① pulldown-cmark 增量可行性；② 10k 行虚拟化滚动帧率；③ 代码块未闭合时的高亮降级（先纯文本，闭合后高亮）。
- 参考：Zed 的 markdown 渲染、gpui-component 的 CodeEditor（只读高亮可借鉴）。

### 3.2 Vega Runtime 架构（S4-S5）
```
loop {
    let stream = provider.chat(messages, tools).await;   // SSE
    for event in stream {                                  // text_delta / tool_use / usage
        emit(ConversationEvent);                           // → UI 订阅
        if tool_use { check_permission() → execute() → append_result }
    }
    if !has_tool_calls { break }                           // 收敛
    if token_budget.exceeded() { compact_context() }
}
```
- 每个事件同时写 SQLite（tool_calls 表）→ Harness Trace 和成本审计的数据基础，Phase 3 直接复用。
- 中断：tokio CancellationToken，<1s 停手（KPI）。
- 上下文压缩 v1：朴素截断（保留 system + 最近 N 轮 + 工具结果摘要），v2 再做智能摘要。

### 3.3 Token 计量（S7）
- 三层：① API 返回 `usage`（准，以此为准）；② tiktoken-rs 预估（流式期间实时显示）；③ 定价表 JSON（内置 claude/gpt/deepseek 主流模型，支持自定义模型+价格，兼容你的 CPA/B.AI 渠道）。
- cache token 单列：prompt caching 命中是成本优化关键指标。

### 3.4 数据 Schema（S1 定稿，后面不加表只加列）
`projects / threads / messages / tool_calls / token_usage / permissions` 六张表。token_usage 每 API 调用一行（thread_id, model, input, output, cache_read, cache_write, cost_microcents, ts）。

### 3.5 CI
`macos-14` runner；fmt → clippy(-D warnings) → test（含 headless runtime 测试）→ release build → bench 回归（告警不 block）。`Swatinem/rust-cache`。
> 2026-08-29 修订：S1 起先落地**本地 git hooks 门禁**（pre-commit/pre-push，见 vega-s1-tasks T03 v0.2），云端 CI 延后至产品稳定——防 macOS runner 费用。届时按本节原案上云。

---

## 4. 风险与对策（Phase 1 特有）

| 风险 | 对策 |
|---|---|
| 流式 markdown 渲染复杂（最高） | S3 前置 3 天 spike；降级方案备好 |
| 自研 Runtime 质量差 | Phase 1 里程碑只要求「能完成真实任务」，不追求聪明；ACP 兜底在 Phase 2 |
| agent 误操作文件 | E5 安全默认 + 危险命令拦截 + 全程落库审计 |
| API key 泄露 | macOS Keychain 存储，绝不写明文配置 |
| GPUI breaking change | 锁版本，月升级一次 |

---

## 5. 开工清单（确认后执行，按 AGENTS.md 委托 subagent 实施）

1. [ ] 建 repo `vega`（GitHub private？还是本地先行——**待确认**）
2. [ ] rustup stable + Xcode CLT 检查
3. [ ] S1：workspace 脚手架 + CI + SQLite schema + bench 骨架
4. [ ] S3 前置：流式渲染 spike（3 天时间框）

**待确认**：repo 形态（GitHub private / 本地）。确认后进 S1。
